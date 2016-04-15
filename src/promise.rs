use std::ptr;

use detail::core::{Core};
use executor::{InlineExecutor};

struct Promise<T> {
    core_ptr : *mut Core<T>,
    retrieved : bool,
}

impl<T> Drop for Promise<T> {
    fn drop(&mut self) {
        unsafe {
            self.detach()
        }
    }
}


// TODO(ptc) Remove this executor horse shitttt
static INLINE_EXECUTOR : InlineExecutor = InlineExecutor::new();


impl<T> Promise<T> {
    pub fn new() -> Promise<T> {
        Promise {
            retrieved : false,
            core_ptr : Box::into_raw(Box::new(Core::new(&INLINE_EXECUTOR))),
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