use std::io::{Error, ErrorKind};
use std::ptr;

use detail::core::Core;
use executor::InlineExecutor;
use future::Future;
use try::Try;

pub struct Promise<T> {
    pub core_ptr: *mut Core<T>,
    pub retrieved: bool,
}

impl<T> Drop for Promise<T> {
    fn drop(&mut self) {
        unsafe { self.detach() }
    }
}

impl<T> Promise<T> {
    pub fn new() -> Promise<T> {
        Promise {
            retrieved: false,
            core_ptr: Box::into_raw(Box::new(Core::new())),
        }
    }

    fn detach(&mut self) {
        unsafe {
            if !self.core_ptr.is_null() {
                if !self.retrieved {
                    (*self.core_ptr).detach_future();
                }
                (*self.core_ptr).detach_promise();
                self.core_ptr = ptr::null_mut();
            }
        }
    }

    fn error_if_retrieved(&self) -> Result<(), Error> {
        // TODO(ptc) use UNLIKELY in future
        if self.retrieved {
            return Err(Error::new(ErrorKind::Other, "Promise already retrieved"));
        }
        return Ok(());
    }

    fn error_if_fulfilled(&self) -> Result<(), Error> {
        // TODO(ptc) Use UNLIKELY for both tests
        if self.core_ptr.is_null() {
            return Err(Error::new(ErrorKind::Other, "No state"));
        }
        if unsafe { (*self.core_ptr).ready() } {
            return Err(Error::new(ErrorKind::Other, "Promise already satisfied"));
        }
        return Ok(());
    }

    pub fn set_try(&self, try: Try<T>) -> Result<(), Error> {
        try!(self.error_if_fulfilled());
        unsafe {
            return (*self.core_ptr).set_result(try);
        }
    }

    pub fn set_error<U>(&self, try: Try<U>) -> Result<(), Error> {
        try!(self.error_if_fulfilled());
        unsafe {
            return (*self.core_ptr).set_result(Try::new_error(try.get_error()));
        }
    }

    pub fn get_future(&mut self) -> Result<Future<T>, Error> {
        // TODO(ptc) Implement get_future
        try!(self.error_if_retrieved());
        self.retrieved = true;
        return Ok(Future::new_core_ptr(self.core_ptr));
    }
}
