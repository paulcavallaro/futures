#![feature(unboxed_closures)]
#![feature(fnbox)]
#![feature(asm)]
#![feature(const_fn)]

extern crate libc;

pub mod executor;
pub mod microspinlock;
pub mod future;
mod detail;