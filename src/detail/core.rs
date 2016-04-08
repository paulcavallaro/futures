use std::boxed::{Box, FnBox};
use std::error::{Error};
use std::cell::{UnsafeCell};
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc};

use executor::{Executor};
use microspinlock::{MicroSpinLock};
use scopeguard::{ScopeGuard};

/// Assume a cache line is 64 bytes
#[repr(simd)]
struct CacheLine(
    u64, u64, u64, u64,
    u64, u64, u64, u64);
/// Helper for aligning a possibly smaller piece of data
/// to different sizes.
struct AlignedAs<T, A>(T, [A;0]);

impl<T,A> AlignedAs<T, A> {
    pub fn new(item : T) -> AlignedAs<T, A> {
        return AlignedAs(item, [])
    }

    pub fn get(self) -> T {
        return self.0;
    }
}

#[test]
fn is_cache_line_64_bytes() {
    use std::mem;
    assert_eq!(mem::size_of::<CacheLine>(), 64);
}

/// A helper struct for writing Finite State Machines
/// TODO(ptc) Make state an enum type param if we can
/// find a way to encode Enum's to usize and create a
/// trait bound for T s.t. that ensures correctness
/// TODO(ptc) would be nice to have AtomicU8 as well
/// so that we don't have to do `as usize` everywhere
/// which is probably having to zero extend State everywhere
pub struct FSM {
    lock : MicroSpinLock,
    state : AtomicUsize,
}

impl FSM {
    pub fn new(start : State) -> FSM {
        FSM {
            lock : MicroSpinLock::new(),
            state : AtomicUsize::new(start as usize),
        }
    }

    /// Atomically do a state transition with accompanying action.
    /// The action will see the old state.
    /// returns true on success, false and action unexecuted otherwise
    pub fn update_state<F>(&self, old_state : State, new_state : State,
                           action : F) -> bool
        where F : FnOnce() {
        if !self.lock.try_lock() {
            self.lock.lock();
        }
        if self.state.load(Ordering::Acquire) != (old_state as usize) {
            self.lock.unlock();
            return false
        }
        action();
        self.state.store(new_state as usize, Ordering::Release);
        self.lock.unlock();
        return true;
    }

    pub fn update_state2<F1, F2>(&self, old_state : State, new_state : State,
                                 protected_action : F1, unprotected_action : F2) -> bool
        where F1 : FnOnce(), F2 : FnOnce() {
        let result = self.update_state(old_state, new_state, protected_action);
        if result {
            unprotected_action();
        }
        result
    }

    pub fn get_state(&self) -> State {
        unsafe {
            return mem::transmute(self.state.load(Ordering::Acquire) as u8);
        }
    }
}

#[derive(PartialEq, Eq, Debug)]
pub enum State {
    Start,
    OnlyResult,
    OnlyCallback,
    Armed,
    Done,
}

#[test]
fn back_and_forth_state() {
    assert_eq!(FSM::new(State::Start).get_state(), State::Start);
    assert_eq!(FSM::new(State::OnlyResult).get_state(), State::OnlyResult);
    assert_eq!(FSM::new(State::OnlyCallback).get_state(), State::OnlyCallback);
    assert_eq!(FSM::new(State::Armed).get_state(), State::Armed);
    assert_eq!(FSM::new(State::Done).get_state(), State::Done);
}

/// Core is the shared struct between Future and Promise that
/// implements the core functionality
pub struct Core<T, E> where E : Error {
    /// TODO(ptc) See if we can do the actual trick of C++ style placement
    /// new of the Box<FnBox()> into callback or if that's just faulty
    /// translation/thinking
    callback : UnsafeCell<Box<FnBox(Try<T, E>)>>,
    result : UnsafeCell<Option<Try<T, E>>>,
    state : FSM,
    /// TODO(ptc) Shouldn't need an entire u64 to store the number of attached
    attached : AtomicUsize,
    active : AtomicBool,
    interrupt_handler_set : AtomicBool,
    interrupt_lock : MicroSpinLock,
    executor_lock : MicroSpinLock,
    priority : i8,
    // TODO(ptc) Fix this static borrowed executor, just doesn't seem right
    // and will almost certainly be a pain later in development
    executor : &'static Executor,
    context : Arc<RequestContext>,
    interrupt : UnsafeCell<Option<E>>,
    interrupt_handler : UnsafeCell<Option<Box<FnBox(E)>>>,
}

impl<T, E> Core<T, E> where E : Error {

    fn new(executor : &'static Executor) -> Core<T,E> {
        Core {
            callback : UnsafeCell::new(Box::new(|_| {})),
            result : UnsafeCell::new(None),
            state : FSM::new(State::Start),
            attached : AtomicUsize::new(2),
            active : AtomicBool::new(true),
            interrupt_handler_set : AtomicBool::new(false),
            interrupt_lock : MicroSpinLock::new(),
            executor_lock : MicroSpinLock::new(),
            priority : -1,
            executor : executor,
            context : Arc::new(RequestContext::new()),
            interrupt : UnsafeCell::new(None),
            interrupt_handler : UnsafeCell::new(None),
        }
    }

    fn detach_one(&self) -> () {
        let attached = self.attached.fetch_sub(1, Ordering::SeqCst) - 1;
        assert!(attached >= 0);
        assert!(attached <= 2);
        if attached == 0 {
            mem::drop(self)
        }
    }

    fn set_executor(&mut self, exec : &'static Executor, priority : i8) {
        if !self.executor_lock.try_lock() {
            self.executor_lock.lock();
        }
        self.executor = exec;
        self.priority = priority;
        self.executor_lock.unlock();
    }

    fn set_executor_nolock(&mut self, exec : &'static Executor, priority : i8) {
        self.executor = exec;
        self.priority = priority;
    }

    fn get_executor(&self) -> &'static Executor {
        return self.executor;
    }

    /// May call from any thread
    fn is_active(&self) -> bool {
        return self.active.load(Ordering::Acquire);
    }

    /// May call from any thread
    fn deactivate(&self) {
        self.active.store(false, Ordering::Release);
    }

    /// May call from any thread
    fn activate(&self) {
        self.active.store(true, Ordering::Release);
        self.maybe_callback();
    }

    fn has_result(&self) -> bool {
        match self.state.get_state() {
            State::OnlyCallback => { return false; },
            State::Start => { return false; },
            State::OnlyResult => {
                unsafe { assert!((*self.result.get()).is_some()); }
                return true;
            },
            State::Armed => {
                unsafe { assert!((*self.result.get()).is_some()); }
                return true;
            },
            State::Done => {
                unsafe { assert!((*self.result.get()).is_some()); }
                return true;
            },
        }
    }

    fn raise(&self, err : E) {
        if !self.interrupt_lock.try_lock() {
            self.interrupt_lock.lock();
        }
        unsafe {
            if (*self.interrupt.get()).is_none() && !self.has_result() {
                *self.interrupt.get() = Some(err);
                if (*self.interrupt_handler.get()).is_some() {
                    let func = (*self.interrupt_handler.get()).take().unwrap();
                    let err = (*self.interrupt.get()).take().unwrap();
                    func.call_box((err,));
                }
            }
        }
        self.interrupt_lock.unlock();
    }

    fn maybe_callback(&self) {
        let mut done = false;
        while !done {
            let state = self.state.get_state();
            match state {
                State::Armed => {
                    if self.active.load(Ordering::Acquire) {
                        self.state.update_state2(state, State::Done, || {}, || {
                            self.do_callback();
                        });
                    }
                    done = true;
                },
                _ => {
                    done = true;
                }
            };
        }
    }

    fn do_callback(&self) -> () {
        // Grab the current executor
        if !self.executor_lock.try_lock() {
            self.executor_lock.lock();
        }
        let executor = self.executor;
        let priority = self.priority;
        self.executor_lock.unlock();

        // Keep Core alive until callback is run
        self.attached.fetch_add(1, Ordering::SeqCst);

        // See if rust has llvm.expect intrinsic exposed
        if executor.get_num_priorities() == 1 {
            scope_exit!(self.detach_one());
            RequestContext::set_context(self.context.clone());
            unsafe {
                let result = self.result.get();
                let callback = mem::replace(& mut (*self.callback.get()), Box::new(|_try| {}));
                if let Some(try) = (*result).take() {
                    callback(try);
                }
            }
        } else {
            // TODO(ptc) implement add_with_priority to executors
        }
        // NOTE(ptc) Folly::Future allows executor to be null and then calls
        // the callback inline. Currently we do not allow that, but maybe
        // there is a good reason to do so, although unsure why this just
        // couldn't be done with InlineExecutor.
    }
}

/// TODO(ptc) implement Try
pub struct Try<T, E> {
    result : Result<T, E>,
}

/// TODO(ptc) implement RequestContext
pub struct RequestContext;

impl RequestContext {
    pub fn new() -> RequestContext {
        RequestContext
    }

    pub fn set_context(ctxt : Arc<RequestContext>) {
        // TODO(ptc) implement
    }
}


#[cfg(test)]
mod tests {

    use std::io::{Error, ErrorKind};
    use executor::{InlineExecutor};
    use super::{Core};

    #[test]
    fn core_raise() {
        static executor : InlineExecutor = InlineExecutor::new();
        let core : Core<usize, Error> = Core::new(&executor);
        let err = Error::new(ErrorKind::Other, "bollocks!");
        core.raise(err);
        // TODO(ptc) implement and test setting the interrupt handler
    }
}