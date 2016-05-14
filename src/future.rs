use std::io;
use std::ptr;

use detail::core::Core;
use executor::{InlineExecutor, Executor};
use try::Try;


pub struct Future<T> {
    core_ptr: *mut Core<T>,
}

impl<T> Drop for Future<T> {
    fn drop(&mut self) {
        unsafe { self.detach() }
    }
}

impl<T> Future<T> {
    pub fn new(try: Try<T>) -> Future<T> {
        Future { core_ptr: Box::into_raw(Box::new(Core::new_try(try))) }
    }

    fn detach(&mut self) {
        unsafe {
            (*self.core_ptr).detach_future();
            self.core_ptr = ptr::null_mut();
        }
    }

    pub fn get_executor(&self) -> *const Executor {
        unsafe { (*self.core_ptr).get_executor() }
    }

    pub fn set_executor(&self, x: *const Executor) {
        unsafe { (*self.core_ptr).set_executor(x, -1) }
    }

    fn panic_if_invalid(&self) {
        // TODO(ptc) Change this to just return an Error
        if self.core_ptr.is_null() {
            panic!("No State")
        }
    }

    fn set_callback<F>(&mut self, func: F)
        where F: FnOnce(Try<T>) + 'static
    {
        self.panic_if_invalid();
        unsafe {
            (*self.core_ptr).set_callback(func);
        }
    }

    pub fn then<F, U>(&self, func: F) -> Future<U>
        where F: FnOnce(Try<T>) -> Future<U>
    {
        self.panic_if_invalid();
        // TODO(ptc) implement the rest of then by creating promise then setting
        // the callback to fulfill the promise and returning the future for that
        // promise
        panic!("Not implemented")
    }

    pub fn value(&self) -> Result<T, io::Error> {
        self.panic_if_invalid();
        unsafe {
            return (*self.core_ptr).get_try().value();
        }
    }
}


#[cfg(test)]
mod tests {

    use test::Bencher;

    use super::Future;
    use try::Try;


    #[test]
    fn test_future_value() {
        let future = Future::new(Try::new_value(0));
        assert_eq!(future.value().unwrap(), 0);
    }

    #[bench]
    fn bench_constant_future(b: &mut Bencher) {
        b.iter(|| {
            let future = Future::new(Try::new_value(0));
        })
    }
}
