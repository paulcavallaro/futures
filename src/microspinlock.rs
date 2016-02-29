use std::sync::atomic::{AtomicBool, ATOMIC_BOOL_INIT, Ordering};
use libc::{nanosleep, timespec};

/// Called while spinning (name borrowed from Linux). Can be implemented to call
/// a platform-specific method of lightening CPU load in spinlocks.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
#[inline(always)]
fn cpu_relax() {
    // This instruction is meant for usage in spinlock loops
    // (see Intel x86 manual, III, 4.2)
    unsafe { asm!("pause" :::: "volatile"); }
}

#[cfg(not(any(target_arch = "x86", target_arch = "x86_64")))]
#[inline(always)]
fn cpu_relax() {
}

/// A helper object for the contended case. Starts off with eager
/// spinning, and falls back to sleeping for small quantums.
struct Sleeper {
    spin_count : u32,
}

const MAX_ACTIVE_SPIN : u32 = 4000;

impl Sleeper {
    pub fn new() -> Sleeper {
        Sleeper {
            spin_count : 0,
        }
    }

    pub fn wait(&mut self) {
        if self.spin_count < MAX_ACTIVE_SPIN {
            self.spin_count += 1;
            cpu_relax();
        } else {
            /*
            * Always sleep 0.5ms, assuming this will make the kernel put
            * us down for whatever its minimum timer resolution is (in
            * linux this varies by kernel version from 1ms to 10ms).
            */
            let sleep_time = timespec {
                tv_sec : 0,
                tv_nsec : 500000
            };
            unsafe {
                nanosleep(&sleep_time, 0 as *mut timespec);
            }
        }
    }
}

pub struct MicroSpinLock {
    lock : AtomicBool,
}

const FREE : bool = false;
const LOCKED : bool = true;

impl MicroSpinLock {

    pub const fn new() -> MicroSpinLock {
        MicroSpinLock {
            lock : ATOMIC_BOOL_INIT
        }
    }

    /// Tries to acquire the spinlock.
    /// Returns true if it acquires it, false otherwise
    pub fn try_lock(&self) -> bool {
        return self.cas(FREE, LOCKED) == FREE
    }

    pub fn lock(&self) {
        // Manual do-while
        let mut sleeper = Sleeper::new();
        while self.lock.load(Ordering::SeqCst) != FREE {
            sleeper.wait()
        }
        while !self.try_lock() {
            while self.lock.load(Ordering::SeqCst) != FREE {
                sleeper.wait()
            }
        }
    }

    pub fn unlock(&self) {
        assert!(self.lock.load(Ordering::SeqCst) == LOCKED);
        self.lock.store(FREE, Ordering::Release);
    }

    #[inline(always)]
    fn cas(&self, compare : bool, new_val : bool) -> bool {
        self.lock.compare_and_swap(compare, new_val, Ordering::AcqRel)
    }
}

unsafe impl Sync for MicroSpinLock {}


#[test]
fn test_microspinlock_sleep() {
    use std::thread;
    use std::time;

    static TEST_SPINLOCK : MicroSpinLock = MicroSpinLock::new();
    let spinlock = &TEST_SPINLOCK;
    spinlock.lock();
    let child = thread::spawn(move || {
        // Sleep 2 seconds then release lock
        assert!(!spinlock.try_lock());
        thread::sleep(time::Duration::new(1, 0));
        spinlock.unlock();
    });
    spinlock.lock();
    assert!(!spinlock.try_lock());
    spinlock.unlock();
    let _res = child.join();
}

#[test]
fn test_microspinlock_spin() {
    use std::thread;
    use std::time;

    static TEST_SPINLOCK : MicroSpinLock = MicroSpinLock::new();
    let spinlock = &TEST_SPINLOCK;
    spinlock.lock();
    let child = thread::spawn(move || {
        // Sleep 100 microseconds then release lock
        assert!(!spinlock.try_lock());
        thread::sleep(time::Duration::new(0, 100000));
        spinlock.unlock();
    });
    spinlock.lock();
    assert!(!spinlock.try_lock());
    spinlock.unlock();
    let _res = child.join();
}