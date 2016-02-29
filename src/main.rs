#![feature(unboxed_closures)]
#![feature(asm)]
#![feature(fnbox)]
#![feature(const_fn)]

extern crate libc;

mod microspinlock;

use microspinlock::{MicroSpinLock, spawn_unsafe};
use std::thread;
use std::time;

fn main() {
    let spinlock = MicroSpinLock::new();
    spinlock.lock();
    let child = unsafe {
        spawn_unsafe(|| {
            // Sleep 2 seconds then release lock
            assert!(!spinlock.try_lock());
            thread::sleep(time::Duration::new(1, 0));
            spinlock.unlock();
        })
    };
    spinlock.lock();
    assert!(!spinlock.try_lock());
    spinlock.unlock();
    let _res = child.join();
}