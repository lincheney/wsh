use std::alloc::{GlobalAlloc, Layout, System};

pub struct Zalloc;

unsafe impl GlobalAlloc for Zalloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // if i don't do this i could end up with deadlock
        // if a signal handler triggers in the middle
        super::queue_signals();
        let ptr = unsafe{ System.alloc(layout) };
        let _ = super::unqueue_signals();
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            System.dealloc(ptr, layout)
        }
    }
}

#[global_allocator]
static GLOBAL: Zalloc = Zalloc;
