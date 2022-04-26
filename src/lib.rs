extern crate core;

use std::{alloc::GlobalAlloc, sync::atomic::AtomicUsize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::atomic::Ordering::SeqCst;
use std::thread::ThreadId;
use std::thread;
use std::time::Duration;
use once_cell::sync::OnceCell;


static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static BOUND_OVERFLOWED: AtomicBool = AtomicBool::new(false);
static OVERFLOW_HANDLER_THREAD: OnceCell<ThreadId> = OnceCell::new();

type OverflowHandler = dyn Fn(usize) + Send + Sync;

struct BoundAlloc<Alloc> {
    alloc: Alloc,
    bound_size: usize,
    overflow_handler: Option<Box<OverflowHandler>>,
}

impl<Alloc> BoundAlloc<Alloc> {
    pub fn new(alloc: Alloc, bound_size: usize, overflow_handler: Option<Box<OverflowHandler>>) -> Self {
        Self { alloc, bound_size, overflow_handler }
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

        if BOUND_OVERFLOWED.load(Ordering::SeqCst) && OVERFLOW_HANDLER_THREAD.get().map(|id| thread::current().id() == *id).unwrap_or(false) {
            sleep_eternally();
        }

        if !ret.is_null() {
            CURRENT.fetch_add(layout.size(), Ordering::SeqCst);
            PEAK.fetch_max(CURRENT.load(Ordering::SeqCst), Ordering::SeqCst);
        }

        if self.bound_size < PEAK.load(Ordering::SeqCst) {
            BOUND_OVERFLOWED.store(true, Ordering::SeqCst);
            match OVERFLOW_HANDLER_THREAD.set(thread::current().id()) {
                Ok(_) => {
                    if let Some(handler) = &self.overflow_handler {
                        handler(PEAK.load(Ordering::SeqCst));
                        panic!("overflow handler must be panic");
                    } else {
                        default_overflow_handler(PEAK.load(Ordering::SeqCst));
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
        CURRENT.fetch_add(layout.size(), Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use std::alloc::{GlobalAlloc, Layout, System};
    use crate::BoundAlloc;

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
}
