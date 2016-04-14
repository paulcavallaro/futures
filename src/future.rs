use std::io;
use std::ptr;

use detail::core::{Core};
use executor::{InlineExecutor, Executor};
use try::{Try};


pub struct Future<T> {
    core_ptr : *mut Core<T>,
}

static INLINE_EXECUTOR : InlineExecutor = InlineExecutor::new();

impl<T> Drop for Future<T> {
    fn drop(&mut self) {
        unsafe {
            self.detach()
        }
    }
}

impl<T> Future<T> {
    pub fn new(try : Try<T>) -> Future<T> {
        Future {
            core_ptr : Box::into_raw(Box::new(Core::new(&INLINE_EXECUTOR))),
        }
    }

    fn detach(&mut self) {
        unsafe {
            (*self.core_ptr).detach_future();
            self.core_ptr = ptr::null_mut();
        }
    }

    pub fn get_executor(&self) -> &'static Executor {
        unsafe {
            (*self.core_ptr).get_executor()
        }
    }

    pub fn set_executor(&self, x : &'static Executor) {
        unsafe {
            (*self.core_ptr).set_executor(x, -1)
        }
    }

    fn panic_if_invalid(&self) {
        // TODO(ptc) Change this to just return an Error
        if self.core_ptr.is_null() {
            panic!("No State")
        }
    }

    fn set_callback<F>(&mut self, func : F)
        where F : FnOnce(Try<T>) + 'static {
        self.panic_if_invalid();
        unsafe {
            (*self.core_ptr).set_callback(func);
        }
    }

    pub fn then<F, U>(&self, func : F) -> Future<U>
        where F : FnOnce(Try<T>) -> Future<U> {
        self.panic_if_invalid();
        // TODO(ptc) implement the rest of then
        panic!("Not implemented")
    }
}


#[cfg(test)]
mod tests {

    use super::{Future};
    use try::{Try};

    #[test]
    fn test_then_try() {
        let future = Future::new(Try::new_value(0));
        future.get_executor();
    }
}
