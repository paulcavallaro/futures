use std::boxed::{Box, FnBox};
use std::error::{Error};
use std::mem;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc};

use executor::{Executor};
use microspinlock::{MicroSpinLock};

/// Assume a cache line is 64 bytes
#[repr(simd)]
struct CacheLine(
    u64, u64, u64, u64,
    u64, u64, u64, u64);
/// Helper for aligning a possibly smaller piece of data
/// to different sizes.
struct AlignedAs<T, A>(T, [A;0]);


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
pub struct Core<T, E> {
    /// TODO(ptc) See if we can do the actual trick of C++ style placement
    /// new of the Box<FnBox()> into callback or if that's just faulty
    /// translation/thinking
    callback : AlignedAs<Box<FnBox()>, CacheLine>,
    result : Option<Try<T, E>>,
    state : FSM,
    /// TODO(ptc) Shouldn't need an entire u64 to store the number of attached
    attached : AtomicUsize,
    active : AtomicBool,
    interrupt_handler_set : AtomicBool,
    interrupt_lock : MicroSpinLock,
    executor_lock : MicroSpinLock,
    priority : u8,
    executor : &'static Executor,
    context : Arc<RequestContext>,
    interrupt : Box<Error>,
    interrupt_handler : Box<FnBox(Error)>,
}

impl<T, E> Core<T, E> {
    fn detach_one(&self) -> () {
        let attached = self.attached.fetch_sub(1, Ordering::SeqCst) - 1;
        assert!(attached >= 0);
        assert!(attached <= 2);
        if attached == 0 {
            mem::drop(self)
        }
    }

    fn set_executor(&mut self, exec : &'static Executor, priority : u8) {
        if !self.executor_lock.try_lock() {
            self.executor_lock.lock();
        }
        self.executor = exec;
        self.priority = priority;
        self.executor_lock.unlock();
    }

    fn set_executor_nolock(&mut self, exec : &'static Executor, priority : u8) {
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
            // TODO(ptc) finish implementation
        }
    }
}

/// TODO(ptc) implement Try
pub struct Try<T, E> {
    result : Result<T, E>,
}

/// TODO(ptc) implement RequestContext
pub struct RequestContext;