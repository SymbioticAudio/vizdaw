//! A hostile 64 MiB file of running-status events is refused before the
//! reader allocates for its events (backlog: cap memory while importing).

use core_io::IoError;
use core_io::limits::{Limits, MAX_EVENTS, MAX_MIDI_FILE_BYTES};
use core_io::smf::read;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

struct Counting;

static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(n: usize) {
    let now = CURRENT.fetch_add(n, Ordering::Relaxed) + n;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

// SAFETY: every call is forwarded unchanged to the system allocator; the
// counters are only bookkeeping.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        grow(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        grow(l.size());
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        CURRENT.fetch_sub(l.size(), Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        if new >= l.size() {
            grow(new - l.size());
        } else {
            CURRENT.fetch_sub(l.size() - new, Ordering::Relaxed);
        }
        unsafe { System.realloc(p, l, new) }
    }
}

#[global_allocator]
static A: Counting = Counting;

/// A 64 MiB file of running-status events, then `tail` bytes.
fn hostile_file(tail: &[u8]) -> Vec<u8> {
    let total = usize::try_from(MAX_MIDI_FILE_BYTES).unwrap();
    let body_len = total - 14 - 8;
    let mut f = b"MThd\0\0\0\x06\0\0\0\x01\x03\xC0".to_vec();
    f.extend_from_slice(b"MTrk");
    f.extend_from_slice(&u32::try_from(body_len).unwrap().to_be_bytes());
    f.extend_from_slice(&[0, 0x90, 0x3C, 0x40]);
    while f.len() < total - tail.len() {
        f.extend_from_slice(&[0, 0x3C, 0x40]);
    }
    f.truncate(total - tail.len());
    f.extend_from_slice(tail);
    assert!(f.len() as u64 <= MAX_MIDI_FILE_BYTES);
    f
}

/// The counters are global: one measuring test at a time.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn refused_cheaply(tail: &[u8]) {
    let _g = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let f = hostile_file(tail);
    let cancel = AtomicBool::new(false);
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let r = read(&f, &Limits::default(), &cancel);
    let peak = PEAK.load(Ordering::Relaxed) - base;
    match r {
        Err(IoError::Refused(s)) => assert!(s.contains("more than 10 000 000 events"), "{s}"),
        Err(e) => panic!("wrong error: {e:?}"),
        Ok(_) => panic!("a hostile file was read"),
    }
    const { assert!(MAX_EVENTS == 10_000_000) };
    assert!(peak < 256 * 1024 * 1024, "peak {peak} bytes");
}

#[test]
fn hostile_file_is_refused_before_it_allocates() {
    refused_cheaply(&[]);
}

#[test]
fn hostile_file_with_trailing_damage_is_refused_before_it_allocates() {
    // A damaged event (0xF1) at the very end must not skip the count.
    refused_cheaply(&[0, 0xF1]);
}
