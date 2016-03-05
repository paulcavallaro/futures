use std::sync::atomic::{AtomicUsize, Ordering};

use microspinlock::{MicroSpinLock};

/// A helper struct for writing Finite State Machines
/// TODO(ptc) Make state an enum type param if we can
/// find a way to encode Enum's to usize and create a
/// trait bound for T s.t. that ensures correctness
pub struct FSM {
    lock : MicroSpinLock,
    state : AtomicUsize,
}

impl FSM {
    pub fn new(start : usize) -> FSM {
        FSM {
            lock : MicroSpinLock::new(),
            state : AtomicUsize::new(start),
        }
    }

    /// Atomically do a state transition with accompanying action.
    /// The action will see the old state.
    /// returns true on success, false and action unexecuted otherwise
    pub fn update_state<F>(&mut self, old_state : usize, new_state : usize,
                           action : F) -> bool
        where F : FnOnce() {
        if !self.lock.try_lock() {
            self.lock.lock();
        }
        if self.state.load(Ordering::Acquire) != old_state {
            self.lock.unlock();
            return false
        }
        action();
        self.state.store(new_state, Ordering::Release);
        self.lock.unlock();
        return true;
    }
}