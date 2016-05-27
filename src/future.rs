use std::io::{Error, ErrorKind};
use std::ptr;

use detail::core::Core;
use executor::{InlineExecutor, Executor};
use promise::Promise;
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
    pub fn new_core_ptr(core_ptr: *mut Core<T>) -> Future<T> {
        Future { core_ptr: core_ptr }
    }

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

    fn error_if_invalid(&self) -> Result<(), Error> {
        if self.core_ptr.is_null() {
            return Err(Error::new(ErrorKind::Other, "No state"));
        }
        return Ok(());
    }

    fn set_callback<F>(&mut self, func: F) -> Result<(), Error>
        where F: FnOnce(Try<T>) + 'static
    {
        try!(self.error_if_invalid());
        unsafe {
            return (*self.core_ptr).set_callback(func);
        }
    }

    pub fn then<F, U>(&mut self, func: F) -> Result<Future<U>, Error>
        where F: FnOnce(Try<T>) -> Future<U> + 'static,
              U: 'static
    {
        try!(self.error_if_invalid());
        let mut p: Promise<U> = Promise::new();
        unsafe {
            if let Some(handler) = (*self.core_ptr).get_interrupt_handler() {
                (*p.core_ptr).set_interrupt_handler_nolock(handler);
            }
        }
        let f = try!(p.get_future());
        f.set_executor(self.get_executor());

        self.set_callback(move |try| {
            if try.has_error() {
                p.set_error(try);
            } else {
                let mut f2 = func(try);
                f2.set_callback(move |try2| {
                    p.set_try(try2);
                });
            }
        });
        return Ok(f);
    }

    pub fn then_val<F, U>(&mut self, func: F) -> Result<Future<U>, Error>
        where F: FnOnce(Try<T>) -> U + 'static,
              U: 'static
    {
        try!(self.error_if_invalid());
        let mut p: Promise<U> = Promise::new();
        unsafe {
            if let Some(handler) = (*self.core_ptr).get_interrupt_handler() {
                (*p.core_ptr).set_interrupt_handler_nolock(handler);
            }
        }

        let f = try!(p.get_future());
        f.set_executor(self.get_executor());
        self.set_callback(move |try| {
            if try.has_error() {
                p.set_error(try);
            } else {
                // TODO(ptc) see if this is right to just call this in-line
                p.set_try(Try::new_value(func(try)));
            }
        });
        return Ok(f);
    }

    pub fn value(&self) -> Result<T, Error> {
        try!(self.error_if_invalid());
        unsafe {
            return try!((*self.core_ptr).get_try()).value();
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

    #[test]
    fn test_future_then() {
        let mut future = Future::new(Try::new_value(0));
        let res = future.then(|try| {
                let v = try.value().unwrap();
                return Future::new(Try::new_value(v + 1));
            })
            .unwrap()
            .value()
            .unwrap();
        assert_eq!(res, 1);
    }

    #[test]
    fn test_future_then_val() {
        let mut future = Future::new(Try::new_value(0));
        let res = future.then_val(|try| {
                let v = try.value().unwrap();
                return v + 1;
            })
            .unwrap()
            .value()
            .unwrap();
        assert_eq!(res, 1);
    }
}
