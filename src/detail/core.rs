use std::boxed::{Box, FnBox};
use std::cell::UnsafeCell;
use std::io::ErrorKind;
use std::io;
use std::mem;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use executor::{InlineExecutor, Executor};
use microspinlock::MicroSpinLock;
use scopeguard::ScopeGuard;
use try::Try;
use future::Future;

/// Assume a cache line is 64 bytes
#[repr(simd)]
struct CacheLine(u64, u64, u64, u64, u64, u64, u64, u64);
/// Helper for aligning a possibly smaller piece of data
/// to different sizes.
struct AlignedAs<T, A>(T, [A; 0]);

impl<T, A> AlignedAs<T, A> {
    pub fn new(item: T) -> AlignedAs<T, A> {
        return AlignedAs(item, []);
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
    lock: MicroSpinLock,
    state: AtomicUsize,
}

impl FSM {
    pub fn new(start: State) -> FSM {
        FSM {
            lock: MicroSpinLock::new(),
            state: AtomicUsize::new(start as usize),
        }
    }

    /// Atomically do a state transition with accompanying action.
    /// The action will see the old state.
    /// returns true on success, false and action unexecuted otherwise
    pub fn update_state<F>(&self, old_state: State, new_state: State, action: F) -> bool
        where F: FnOnce()
    {
        if !self.lock.try_lock() {
            self.lock.lock();
        }
        if self.state.load(Ordering::Acquire) != (old_state as usize) {
            self.lock.unlock();
            return false;
        }
        action();
        self.state.store(new_state as usize, Ordering::Release);
        self.lock.unlock();
        return true;
    }

    pub fn update_state2<F1, F2>(&self,
                                 old_state: State,
                                 new_state: State,
                                 protected_action: F1,
                                 unprotected_action: F2)
                                 -> bool
        where F1: FnOnce(),
              F2: FnOnce()
    {
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
pub struct Core<T> {
    /// TODO(ptc) See if we can do the actual trick of C++ style placement
    /// new of the Box<FnBox()> into callback or if that's just faulty
    /// translation/thinking
    callback: UnsafeCell<Box<FnBox(Try<T>) + 'static>>,
    result: UnsafeCell<Option<Try<T>>>,
    state: FSM,
    /// TODO(ptc) Shouldn't need an entire u64 to store the number of attached
    attached: AtomicUsize,
    active: AtomicBool,
    interrupt_handler_set: AtomicBool,
    interrupt_lock: MicroSpinLock,
    executor_lock: MicroSpinLock,
    priority: i8,
    executor: *const Executor,
    context: Arc<RequestContext>,
    interrupt: UnsafeCell<Option<io::Error>>,
    interrupt_handler: UnsafeCell<Option<Arc<Fn(&io::Error)>>>,
}

struct NullExecutor(usize, usize);

unsafe fn null_executor() -> *const Executor {
    return mem::transmute([0 as usize; 2]);
}

impl<T> Core<T> {
    pub fn new() -> Core<T> {
        Core {
            callback: UnsafeCell::new(Box::new(|_| {})),
            result: UnsafeCell::new(None),
            state: FSM::new(State::Start),
            attached: AtomicUsize::new(2),
            active: AtomicBool::new(true),
            interrupt_handler_set: AtomicBool::new(false),
            interrupt_lock: MicroSpinLock::new(),
            executor_lock: MicroSpinLock::new(),
            priority: -1,
            // TODO(ptc) fix this when ptr::null doesn't need to be sized
            executor: unsafe { null_executor() },
            context: Arc::new(RequestContext::new()),
            interrupt: UnsafeCell::new(None),
            interrupt_handler: UnsafeCell::new(None),
        }
    }

    pub fn new_try(try: Try<T>) -> Core<T> {
        Core {
            callback: UnsafeCell::new(Box::new(|_| {})),
            result: UnsafeCell::new(Some(try)),
            state: FSM::new(State::OnlyResult),
            attached: AtomicUsize::new(1),
            active: AtomicBool::new(true),
            interrupt_handler_set: AtomicBool::new(false),
            interrupt_lock: MicroSpinLock::new(),
            executor_lock: MicroSpinLock::new(),
            priority: -1,
            // TODO(ptc) fix this when ptr::null doesn't need to be sized
            executor: unsafe { null_executor() },
            context: Arc::new(RequestContext::new()),
            interrupt: UnsafeCell::new(None),
            interrupt_handler: UnsafeCell::new(None),
        }
    }

    fn detach_one(&self) -> () {
        let attached = self.attached.fetch_sub(1, Ordering::SeqCst) - 1;
        assert!(attached >= 0);
        assert!(attached <= 2);
        if attached == 0 {
            // TODO(ptc) make sure this actually runs the destructor
            mem::drop(self)
        }
    }

    /// Called by a destructing Future from the Future thread
    pub fn detach_future(&self) {
        self.activate();
        self.detach_one();
    }

    /// Called by a destructing Promise from the Promise thread
    pub fn detach_promise(&self) {
        // detach_promise() and set_result() should never be called in parallel
        // so we don't need to protect this.
        unsafe {
            // TODO(ptc) use UNLIKELY here
            if (*self.result.get()).is_none() {
                self.set_result(Try::new_error(io::Error::new(ErrorKind::Other, "Broken Promise")));
            }
        }
        self.detach_one();
    }

    /// Call only from Future thread
    pub fn set_callback<F>(&self, func: F)
        where F: FnOnce(Try<T>) + 'static
    {
        let mut transition_to_armed = false;
        let callback: UnsafeCell<Box<FnBox(Try<T>) + 'static>> = UnsafeCell::new(Box::new(func));
        let mut set_callback_ = || unsafe {
            let context = RequestContext::save_context();

            // TODO(ptc) if we do change to having a space to put the lambda
            // inline with the Core object, here is where we would check the
            // size of the callback and place there if it fits

            ptr::swap(self.callback.get(), callback.get());
        };
        let mut done = false;
        while !done {
            let state = self.state.get_state();
            match state {
                State::Start => {
                    done = self.state.update_state(state, State::OnlyCallback, &mut set_callback_);
                }
                State::OnlyResult => {
                    done = self.state.update_state(state, State::Armed, &mut set_callback_);
                    transition_to_armed = true;
                }
                State::OnlyCallback => {
                    panic!("logic error: set_callback called twice");
                }
                State::Armed => {
                    panic!("logic error: set_callback called twice");
                }
                State::Done => {
                    panic!("logic error: set_callback called twice");
                }
            }
        }

        if transition_to_armed {
            self.maybe_callback();
        }
    }

    /// Call only from Promise thread
    fn set_result(&self, res: Try<T>) {
        let mut transition_to_armed = false;
        let res = UnsafeCell::new(Some(res));
        let mut set_result_ = || unsafe {
            ptr::swap(self.result.get(), res.get());
        };
        // TODO(ptc) investigate porting over the FSM_START/FSM_UPDATE/FSM_CASE
        // macros
        let mut done = false;
        while !done {
            let state = self.state.get_state();
            match state {
                State::Start => {
                    done = self.state.update_state(state, State::OnlyResult, &mut set_result_);
                }
                State::OnlyCallback => {
                    done = self.state.update_state(state, State::Armed, &mut set_result_);
                    transition_to_armed = true;
                }
                State::OnlyResult => {
                    panic!("logic error: set_result called twice");
                }
                State::Armed => {
                    panic!("logic error: set_result called twice");
                }
                State::Done => {
                    panic!("logic error: set_result called twice");
                }
            }
        }
        if transition_to_armed {
            self.maybe_callback();
        }
    }

    pub fn set_executor(&mut self, exec: *const Executor, priority: i8) {
        if !self.executor_lock.try_lock() {
            self.executor_lock.lock();
        }
        self.executor = exec;
        self.priority = priority;
        self.executor_lock.unlock();
    }

    fn set_executor_nolock(&mut self, exec: *const Executor, priority: i8) {
        self.executor = exec;
        self.priority = priority;
    }

    pub fn get_executor(&self) -> *const Executor {
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
            State::OnlyCallback => {
                return false;
            }
            State::Start => {
                return false;
            }
            State::OnlyResult => {
                unsafe {
                    assert!((*self.result.get()).is_some());
                }
                return true;
            }
            State::Armed => {
                unsafe {
                    assert!((*self.result.get()).is_some());
                }
                return true;
            }
            State::Done => {
                unsafe {
                    assert!((*self.result.get()).is_some());
                }
                return true;
            }
        }
    }

    fn raise(&self, err: io::Error) {
        if !self.interrupt_lock.try_lock() {
            self.interrupt_lock.lock();
        }
        unsafe {
            if (*self.interrupt.get()).is_none() && !self.has_result() {
                *self.interrupt.get() = Some(err);
                if (*self.interrupt_handler.get()).is_some() {
                    let func = (*self.interrupt_handler.get()).clone().unwrap();
                    let err = (*self.interrupt.get()).as_ref().unwrap();
                    func(err);
                }
            }
        }
        self.interrupt_lock.unlock();
    }

    /// Should only be called from Promise thread
    /// Sets the interrupt handler on the Core object, if it already has
    /// an exception/interrupt than just cann the handler on the interrupt
    fn set_interrupt_handler(&self, handler: Arc<Fn(&io::Error)>) {
        if !self.interrupt_lock.try_lock() {
            self.interrupt_lock.lock();
        }
        unsafe {
            if !self.has_result() {
                if (*self.interrupt.get()).is_some() {
                    let err = (*self.interrupt.get()).as_ref().unwrap();
                    handler(err);
                } else {
                    self.set_interrupt_handler_nolock(handler);
                }
            }
        }
        self.interrupt_lock.unlock();
    }

    fn set_interrupt_handler_nolock(&self, handler: Arc<Fn(&io::Error)>) {
        self.interrupt_handler_set.store(true, Ordering::Relaxed);
        unsafe {
            *self.interrupt_handler.get() = Some(handler);
        }
    }

    fn get_interrupt_handler(&self) -> Option<Arc<Fn(&io::Error)>> {
        if !self.interrupt_handler_set.load(Ordering::Acquire) {
            return None;
        }
        if !self.interrupt_lock.try_lock() {
            self.interrupt_lock.lock();
        }
        unsafe {
            let handler = (*self.interrupt_handler.get()).clone();
            self.interrupt_lock.unlock();
            return handler;
        }
    }

    /// Can call from any thread
    pub fn ready(&self) -> bool {
        return self.has_result();
    }

    pub fn get_try(&self) -> Try<T> {
        if self.ready() {
            unsafe {
                return (*self.result.get()).take().unwrap();
            }
        } else {
            panic!("Future not ready")
        }
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
                }
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
        if unsafe { executor != null_executor() } {
            if unsafe { (*executor).get_num_priorities() == 1 } {
                scope_exit!(self.detach_one());
                RequestContext::set_context(self.context.clone());
                unsafe {
                    let result = self.result.get();
                    let callback = mem::replace(&mut (*self.callback.get()), Box::new(|_try| {}));
                    if let Some(try) = (*result).take() {
                        callback(try);
                    }
                }
            } else {
                // TODO(ptc) implement add_with_priority to executors
            }
        } else {
            scope_exit!(self.detach_one());
            RequestContext::set_context(self.context.clone());
            unsafe {
                let result = self.result.get();
                let callback = mem::replace(&mut (*self.callback.get()), Box::new(|_try| {}));
                if let Some(try) = (*result).take() {
                    callback(try);
                }
            }
        }
        // NOTE(ptc) Folly::Future allows executor to be null and then calls
        // the callback inline. Currently we do not allow that, but maybe
        // there is a good reason to do so, although unsure why this just
        // couldn't be done with InlineExecutor.
    }
}

/// TODO(ptc) implement RequestContext
pub struct RequestContext;

impl RequestContext {
    pub fn new() -> RequestContext {
        RequestContext
    }

    pub fn set_context(ctxt: Arc<RequestContext>) {
        // TODO(ptc) implement
    }

    pub fn save_context() -> Arc<RequestContext> {
        // TODO(ptc) implement
        return Arc::new(RequestContext::new());
    }
}


#[cfg(test)]
mod tests {

    use std::io::{Error, ErrorKind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use test::Bencher;

    use executor::InlineExecutor;
    use super::Core;
    use try::Try;

    #[test]
    fn raise_set_handler_after() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let core: Core<usize> = Core::new();
        let err = Error::new(ErrorKind::Other, "bollocks!");
        core.raise(err);
        assert_eq!(COUNTER.load(Ordering::SeqCst), 0);
        core.set_interrupt_handler(Arc::new(|_| {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        }));
        // Should call interrupt handler immediately and not bind it
        assert!(core.get_interrupt_handler().is_none());
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
        // Setting the interrupt handler again will call the handler
        // but still not bind it
        core.set_interrupt_handler(Arc::new(|_| {
            COUNTER.fetch_add(4, Ordering::SeqCst);
        }));
        assert_eq!(COUNTER.load(Ordering::SeqCst), 5);
        assert!(core.get_interrupt_handler().is_none());
    }

    #[test]
    fn raise_set_handler_before() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let core: Core<usize> = Core::new();
        core.set_interrupt_handler(Arc::new(|_| {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        }));
        assert!(core.get_interrupt_handler().is_some());
        assert_eq!(COUNTER.load(Ordering::SeqCst), 0);
        core.raise(Error::new(ErrorKind::Other, "bollocks!"));
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
        // Can't raise twice, won't reset current interrupt, nor call
        // handler twice
        core.raise(Error::new(ErrorKind::Other, "bollocks!"));
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
        // Should be able to get handler and call it though
        let handler = core.get_interrupt_handler().unwrap();
        handler(&Error::new(ErrorKind::Other, "bollocks!"));
        assert_eq!(COUNTER.load(Ordering::SeqCst), 2);
    }

    #[test]
    #[should_panic(expected = "logic error: set_result called twice")]
    fn set_result_twice() {
        let core: Core<usize> = Core::new();
        let mut try = Try::new_value(1);
        core.set_result(try);
        try = Try::new_value(2);
        core.set_result(try);
    }

    #[test]
    fn set_result_once() {
        let core: Core<usize> = Core::new();
        let try = Try::new_value(1);
        core.set_result(try);
    }

    #[test]
    #[should_panic(expected = "logic error: set_callback called twice")]
    fn set_callback_twice() {
        let core: Core<usize> = Core::new();
        core.set_callback(|_| {});
        core.set_callback(|_| {});
    }

    #[test]
    fn set_result_then_set_callback() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let core: Core<usize> = Core::new();
        let try = Try::new_value(1);
        core.set_result(try);
        core.set_callback(|_| {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn set_callback_then_set_result() {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let core: Core<usize> = Core::new();
        core.set_callback(|_| {
            COUNTER.fetch_add(1, Ordering::SeqCst);
        });
        assert_eq!(COUNTER.load(Ordering::SeqCst), 0);
        let try = Try::new_value(1);
        core.set_result(try);
        assert_eq!(COUNTER.load(Ordering::SeqCst), 1);
    }

    #[bench]
    fn set_callback_then_set_result_bench(b: &mut Bencher) {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        b.iter(|| {
            let core: Core<usize> = Core::new();
            core.set_callback(|_| {
                COUNTER.fetch_add(1, Ordering::SeqCst);
            });
            core.set_result(Try::new_value(1));
        });
    }
}
