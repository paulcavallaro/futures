use std::ptr;

use detail::core::Core;
use executor::InlineExecutor;

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
}
