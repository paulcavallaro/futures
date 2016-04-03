#![feature(asm)]
#![feature(const_fn)]
#![feature(extended_compare_and_swap)]
#![feature(fnbox)]
#![feature(repr_simd)]
#![feature(test)]
#![feature(unboxed_closures)]

extern crate libc;
extern crate test;

pub mod executor;
pub mod microspinlock;
pub mod scopeguard;
pub mod future;
mod detail;