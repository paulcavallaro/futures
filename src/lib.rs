#![feature(asm)]
#![feature(const_fn)]
#![feature(fnbox)]
#![feature(repr_simd)]
#![feature(test)]
#![feature(unboxed_closures)]

extern crate libc;
extern crate test;

pub mod executor;
pub mod microspinlock;
#[macro_use]
pub mod scopeguard;
pub mod future;
pub mod promise;
mod detail;
mod try;
