use std::boxed::{Box, FnBox};
use std::error::{Error};
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
    pub fn update_state<F>(&mut self, old_state : State, new_state : State,
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
}

pub enum State {
    Start,
    OnlyResult,
    OnlyCallback,
    Armed,
    Done,
}

/// Core is the shared struct between Future and Promise that
/// implements the core functionality
pub struct Core<'a, T, E> {
    /// TODO(ptc) See if we can do the actual trick of C++ styel placement
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
    executor : &'a Executor,
    context : Arc<RequestContext>,
    interrupt : Box<Error>,
    interrupt_handler : Box<FnBox(Error)>,
}

/// TODO(ptc) implement Try
pub struct Try<T, E> {
    result : Result<T, E>,
}

/// TODO(ptc) implement RequestContext
pub struct RequestContext;