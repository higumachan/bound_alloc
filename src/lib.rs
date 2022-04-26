extern crate core;

use once_cell::sync::OnceCell;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::thread::ThreadId;
use std::time::Duration;
use std::{alloc::GlobalAlloc, sync::atomic::AtomicUsize};

type OverflowHandler = dyn Fn(usize) + Send + Sync;

struct BoundAlloc<Alloc> {
    alloc: Alloc,
    bound_size: usize,
    overflow_handler: Option<Box<OverflowHandler>>,
    current: AtomicUsize,
    peak: AtomicUsize,
    bound_overflowed: AtomicBool,
    overflow_handler_thread: OnceCell<ThreadId>,
}

impl<Alloc> BoundAlloc<Alloc> {
    pub fn new(
        alloc: Alloc,
        bound_size: usize,
        overflow_handler: Option<Box<OverflowHandler>>,
    ) -> Self {
        Self {
            alloc,
            bound_size,
            overflow_handler,
            current: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            bound_overflowed: AtomicBool::new(false),
            overflow_handler_thread: OnceCell::new(),
        }
    }
}

struct OverflowState {
    is_overflowed: bool,
    handler_thread: ThreadId,
}

fn default_overflow_handler(_usage: usize) {
    panic!("overflow(default overflow handler)");
}

fn sleep_eternally() {
    thread::sleep(Duration::from_secs(1024 * 1024));
}

unsafe impl<Alloc: GlobalAlloc> GlobalAlloc for BoundAlloc<Alloc> {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        let ret = self.alloc.alloc(layout);

        if self.bound_overflowed.load(Ordering::SeqCst)
            && self
                .overflow_handler_thread
                .get()
                .map(|id| thread::current().id() == *id)
                .unwrap_or(false)
        {
            sleep_eternally();
        }

        if !ret.is_null() {
            self.current.fetch_add(layout.size(), Ordering::SeqCst);
            self.peak
                .fetch_max(self.current.load(Ordering::SeqCst), Ordering::SeqCst);
        }

        if self.bound_size < self.peak.load(Ordering::SeqCst) {
            self.bound_overflowed.store(true, Ordering::SeqCst);
            match self.overflow_handler_thread.set(thread::current().id()) {
                Ok(_) => {
                    if let Some(handler) = &self.overflow_handler {
                        handler(self.peak.load(Ordering::SeqCst));
                        panic!("overflow handler must be panic");
                    } else {
                        default_overflow_handler(self.peak.load(Ordering::SeqCst));
                    }
                }
                Err(_) => {
                    sleep_eternally();
                }
            }
        }

        ret
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        self.dealloc(ptr, layout);
        self.current.fetch_add(layout.size(), Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use crate::BoundAlloc;
    use std::alloc::{GlobalAlloc, Layout, System};

    #[test]
    #[should_panic(expected = "overflow(default overflow handler)")]
    fn it_works() {
        let alloc = BoundAlloc::new(System, 1024, None);

        unsafe {
            alloc.alloc(Layout::from_size_align(512, 8).unwrap());
            alloc.alloc(Layout::from_size_align(512, 8).unwrap());
            alloc.alloc(Layout::from_size_align(512, 8).unwrap());
        }
    }

    #[test]
    fn it_not_works() {
        let alloc = BoundAlloc::new(System, 2048, None);

        unsafe {
            alloc.alloc(Layout::from_size_align(512, 8).unwrap());
            alloc.alloc(Layout::from_size_align(512, 8).unwrap());
            alloc.alloc(Layout::from_size_align(512, 8).unwrap());
        }
    }
}
