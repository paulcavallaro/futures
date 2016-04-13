use std::marker::{PhantomData};

#[must_use]
pub struct ScopeGuard<'a, F> where F : 'a + FnOnce() {
    pub cleanup : Option<F>,
    phantom : PhantomData<&'a F>,
}

impl<'a, F> ScopeGuard<'a, F> where F : 'a + FnOnce() {
    pub fn new(func : F) -> ScopeGuard<'a, F> {
        return ScopeGuard {
            cleanup : Some(func),
            phantom : PhantomData,
        }
    }
}

impl<'a, F> Drop for ScopeGuard<'a, F> where F : 'a + FnOnce() {
    fn drop(&mut self) {
        if let Some(f) = self.cleanup.take() {
            f();
        }
    }
}

#[macro_export]
macro_rules! scope_exit {
    ($e:expr) => {
        let gensym = ScopeGuard::new(|| { $e })
    };
    ($b:block) => {
        let gensym = ScopeGuard::new(|| { $b })
    };
}

#[test]
fn test_scope_guard() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let bool = AtomicBool::new(false);
    {
        let guard = ScopeGuard::new(|| {bool.store(true, Ordering::Release)});
    }
    assert_eq!(bool.load(Ordering::Acquire), true);
}

#[test]
fn test_scope_exit_macro() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let bool = AtomicBool::new(false);
    {
        scope_exit!(bool.store(true, Ordering::Release));
    }
    assert_eq!(bool.load(Ordering::Acquire), true);
}
