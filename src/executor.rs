use std::boxed::{Box, FnBox};
use std::cell::UnsafeCell;
use std::collections::vec_deque::VecDeque;
use std::mem;

/// An Executor accepts units of work with add(), which must be
/// threadsafe.
pub trait Executor {
    /// Enqueue a function to executed by this executor. This and all
    /// variants must be threadsafe.
    /// We ensure that the work to be done will outlive the Executor
    /// ensuring that the work will be alive by the time the Executor
    /// can execute it and consume its lifetime.
    fn add<'a, 'b>(&'a self, work: Box<FnBox() + Send + 'b>) -> () where 'b: 'a;

    fn get_num_priorities(&self) -> u8 {
        return 1;
    }
}

pub struct InlineExecutor;

/// When work is "queued", execute it immediately inline.
/// Usually when you think you want this, you actually want a
/// QueuedImmediateExecutor.
impl InlineExecutor {
    pub const fn new() -> InlineExecutor {
        InlineExecutor
    }
}

impl Executor for InlineExecutor {
    fn add<'a, 'b>(&'a self, work: Box<FnBox() + Send + 'b>) -> ()
        where 'b: 'a
    {
        work.call_box(());
    }
}

#[test]
fn test_inline_executor() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::mem;

    let inline = InlineExecutor::new();
    let cntr = AtomicUsize::new(0);
    inline.add(Box::new(|| {
        cntr.fetch_add(1, Ordering::AcqRel);
    }));
    let val = cntr.load(Ordering::Acquire);
    assert_eq!(val, 1);
}

thread_local!(static QUEUE: UnsafeCell<VecDeque<Box<FnBox() + Send>>>
              = UnsafeCell::new(VecDeque::new()));

/// Runs inline like InlineExecutor, but with a queue so that any tasks added
/// to this executor by one of its own callbacks will be queued instead of
/// executed inline (nested). This is usually better behavior than Inline.
pub struct QueuedImmediateExecutor;

impl QueuedImmediateExecutor {
    pub fn new() -> QueuedImmediateExecutor {
        return QueuedImmediateExecutor;
    }
}

impl Executor for QueuedImmediateExecutor {
    fn add<'a, 'b>(&'a self, work: Box<FnBox() + Send + 'b>) -> ()
        where 'b: 'a
    {
        QUEUE.with(|queue| {
            unsafe {
                let queue: *mut VecDeque<Box<FnBox() + Send + 'static>> = queue.get();
                if (*queue).is_empty() {
                    // We have to transmute the work to pretend it has
                    // 'static lifetime so we can stuff it into the thread local
                    // queue. However this is reasonably safe since the work is
                    // guaranteed to live longer than the Executor and in most
                    // cases the Executor should be fully consuming the work.
                    // Only edge case that comes to mind is if the work/closure
                    // had some RAII items it was using to signal behavior, but
                    // that seems okay to break.
                    // TODO(ptc) see if there's a better way around this transmute
                    (*queue).push_back(mem::transmute(work));
                    while !(*queue).is_empty() {
                        // TODO(ptc) Since we have to own the Box<FnBox> in order
                        // to call it we have to pop it off the queue, but that
                        // changes the queue's size so that future calls to add will
                        // execute immediately, so we use a place holder we remove
                        // later.
                        // Figure out a better way to do this so we
                        // don't need a placeholder
                        let fnbox = (*queue).pop_front().unwrap();
                        (*queue).push_front(Box::new(|| {
                            // placeholder
                        }));
                        fnbox.call_box(());
                        let _discarded_placeholder = (*queue).pop_front();
                    }
                } else {
                    (*queue).push_back(mem::transmute(work));
                }
            }
        });
    }
}

#[test]
fn test_queued_executor() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::mem;

    let queued = QueuedImmediateExecutor::new();
    let cntr = AtomicUsize::new(0);
    queued.add(Box::new(|| {
        queued.add(Box::new(|| {
            // Should happen last
            let val = cntr.load(Ordering::Acquire);
            assert_eq!(val, 1);
            cntr.fetch_add(1, Ordering::AcqRel);
        }));
        let val = cntr.load(Ordering::Acquire);
        assert_eq!(val, 0);
        cntr.fetch_add(1, Ordering::AcqRel);
    }));
    let val = cntr.load(Ordering::Acquire);
    assert_eq!(val, 2);
}
