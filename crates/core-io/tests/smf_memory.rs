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

#[test]
fn hostile_file_is_refused_before_it_allocates() {
    // One track: a note-on, then 3-byte running-status events (delta, note,
    // velocity) up to the file-size limit: more than 20 million events.
    let total = usize::try_from(MAX_MIDI_FILE_BYTES).unwrap();
    let body_len = total - 14 - 8;
    let mut f = b"MThd\0\0\0\x06\0\0\0\x01\x03\xC0".to_vec();
    f.extend_from_slice(b"MTrk");
    f.extend_from_slice(&u32::try_from(body_len).unwrap().to_be_bytes());
    f.extend_from_slice(&[0, 0x90, 0x3C, 0x40]);
    while f.len() < total {
        f.extend_from_slice(&[0, 0x3C, 0x40]);
    }
    f.truncate(total);
    assert!(f.len() as u64 <= MAX_MIDI_FILE_BYTES);

    let cancel = AtomicBool::new(false);
    let base = CURRENT.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let r = read(&f, &Limits::default(), &cancel);
    let peak = PEAK.load(Ordering::Relaxed) - base;

    match r {
        Err(IoError::Refused(s)) => assert!(
            s.contains(&format!("more than {} events", "10 000 000")),
            "{s}"
        ),
        Err(e) => panic!("wrong error: {e:?}"),
        Ok(_) => panic!("a hostile file was read"),
    }
    const { assert!(MAX_EVENTS == 10_000_000) };
    assert!(peak < 256 * 1024 * 1024, "peak {peak} bytes");
}
