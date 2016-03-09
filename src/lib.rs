#![feature(asm)]
#![feature(const_fn)]
#![feature(fnbox)]
#![feature(repr_simd)]
#![feature(unboxed_closures)]

extern crate libc;

pub mod executor;
pub mod microspinlock;
pub mod future;
mod detail;