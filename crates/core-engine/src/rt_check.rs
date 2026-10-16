//! A counting global allocator for real-time checks. Install it with
//! `#[global_allocator] static A: CountingAlloc = CountingAlloc;` in a test
//! or binary; allocations are counted only on threads that are armed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static COUNT: AtomicU64 = AtomicU64::new(0);
static ARM_WORKERS: AtomicBool = AtomicBool::new(false);

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
}

/// Forwards to the system allocator and counts `alloc`, `dealloc` and
/// `realloc` made while the calling thread is armed.
#[derive(Debug, Default, Clone, Copy)]
pub struct CountingAlloc;

fn note() {
    if ARMED.try_with(Cell::get).unwrap_or(false) {
        COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: every method forwards to `System` with the caller's arguments.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: forwarded contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note();
        // SAFETY: forwarded contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note();
        // SAFETY: forwarded contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note();
        // SAFETY: forwarded contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Starts counting on this thread.
pub fn arm() {
    let _ = ARMED.try_with(|a| a.set(true));
}

/// Stops counting on this thread.
pub fn disarm() {
    let _ = ARMED.try_with(|a| a.set(false));
}

/// Allocations counted so far (all threads).
pub fn count() -> u64 {
    COUNT.load(Ordering::Relaxed)
}

/// Makes render workers arm themselves around every job (tests set this
/// when they install the counting allocator).
pub fn set_arm_workers(on: bool) {
    ARM_WORKERS.store(on, Ordering::Relaxed);
}

/// Whether render workers arm themselves (always with `rt-check`).
pub fn arm_workers() -> bool {
    ARM_WORKERS.load(Ordering::Relaxed) || cfg!(feature = "rt-check")
}
