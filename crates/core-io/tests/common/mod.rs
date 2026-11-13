//! Helpers shared by the package tests: a hand-built ZIP writer that can lie
//! about any field, a per-thread counting allocator and the fuzz case count.

#![allow(dead_code)]

pub mod merge;
pub mod package;
pub mod rawzip_builder;
pub mod xmllint;

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

/// Cases per property: `VIZDAW_FUZZ_CASES` (a long local run), else `n`.
pub fn cases(n: u32) -> u32 {
    std::env::var("VIZDAW_FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(n)
}

thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static LIVE: Cell<usize> = const { Cell::new(0) };
    static PEAK: Cell<usize> = const { Cell::new(0) };
    static TOTAL: Cell<usize> = const { Cell::new(0) };
}

/// Forwards to the system allocator and, on an armed thread, counts the
/// bytes allocated (in total and at the peak above the arming point). Other
/// threads' allocations are never counted (S8).
pub struct CountingAlloc;

fn grew(n: usize) {
    if ARMED.try_with(Cell::get).unwrap_or(false) {
        let _ = TOTAL.try_with(|t| t.set(t.get().saturating_add(n)));
        let _ = LIVE.try_with(|l| {
            let now = l.get().saturating_add(n);
            l.set(now);
            let _ = PEAK.try_with(|p| p.set(p.get().max(now)));
        });
    }
}

fn shrank(n: usize) {
    if ARMED.try_with(Cell::get).unwrap_or(false) {
        let _ = LIVE.try_with(|l| l.set(l.get().saturating_sub(n)));
    }
}

// SAFETY: every method forwards to `System` with the caller's arguments.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        grew(layout.size());
        // SAFETY: forwarded contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        shrank(layout.size());
        // SAFETY: forwarded contract.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        grew(layout.size());
        // SAFETY: forwarded contract.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // Counted as an allocation then a free: while it moves, both exist.
        grew(new_size);
        shrank(layout.size());
        // SAFETY: forwarded contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// What `measure` saw on its thread.
#[derive(Debug, Clone, Copy)]
pub struct Allocated {
    /// Every byte allocated, freed or not.
    pub total: usize,
    /// The most bytes held at once (counting only what `f` allocated).
    pub peak: usize,
}

/// Runs `f` with this thread's counting armed.
pub fn measure<T>(f: impl FnOnce() -> T) -> (T, Allocated) {
    LIVE.with(|l| l.set(0));
    PEAK.with(|p| p.set(0));
    TOTAL.with(|t| t.set(0));
    ARMED.with(|a| a.set(true));
    let out = f();
    ARMED.with(|a| a.set(false));
    let seen = Allocated {
        total: TOTAL.with(Cell::get),
        peak: PEAK.with(Cell::get),
    };
    (out, seen)
}
