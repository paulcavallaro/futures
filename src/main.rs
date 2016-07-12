#![feature(asm)]
#![feature(const_fn)]
#![feature(fnbox)]
#![feature(repr_simd)]
#![feature(test)]
#![feature(unboxed_closures)]


extern crate libc;
extern crate test;

mod microspinlock;

use microspinlock::{MicroSpinLock, spawn_unsafe};
use std::thread;
use std::time;

fn main() {
    let spinlock = MicroSpinLock::new();
    spinlock.lock();
    let child = unsafe {
        spawn_unsafe(|| {
            // Sleep then release lock
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
