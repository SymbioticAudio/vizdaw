//! MIDI note input (plan 4c, Plan Ruling 11): notes played on any MIDI
//! source, for the list editor's "play a note to enter it". Recording
//! proper is sub-project 5.

use crate::error::VizError;
use crate::guard::{ffi_guard, lock};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// A note-on played on a MIDI source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, uniffi::Record)]
pub struct NoteInRow {
    /// Note number 0–127.
    pub pitch: u8,
    /// 16-bit velocity (MIDI 2.0 upscale of the 7-bit one).
    pub velocity: u16,
    /// MIDI channel 0–15.
    pub channel: u8,
}

/// How many note-ons are kept until taken (the newest win).
pub const CAPACITY: usize = 256;

type Queue = Arc<Mutex<VecDeque<NoteInRow>>>;

/// A MIDI 1.0 velocity as 16 bits by the MIDI 2.0 min-center-max rule.
pub fn upscale_velocity(v: u8) -> u16 {
    let v = u16::from(v & 0x7F);
    let shifted = v << 9;
    if v <= 64 {
        return shifted;
    }
    let rep = v & 0x3F;
    shifted | (rep << 3) | (rep >> 3)
}

/// The note-on in a MIDI 1.0 message (velocity 0 is a note-off).
pub fn note_on(bytes: &[u8]) -> Option<NoteInRow> {
    match bytes {
        [s, p, v] if s & 0xF0 == 0x90 && *v > 0 => Some(NoteInRow {
            pitch: p & 0x7F,
            velocity: upscale_velocity(*v),
            channel: s & 0x0F,
        }),
        _ => None,
    }
}

fn push(q: &Queue, bytes: &[u8]) {
    if let Some(n) = note_on(bytes) {
        let mut q = lock(q);
        if q.len() >= CAPACITY {
            q.pop_front();
        }
        q.push_back(n);
    }
}

/// Notes played on MIDI sources (the app makes one).
#[derive(uniffi::Object)]
pub struct NoteInput {
    queue: Queue,
    #[cfg(target_os = "macos")]
    listener: Mutex<Option<platform_macos::midi::NoteListener>>,
}

#[cfg(target_os = "macos")]
fn listen(input: &NoteInput) -> Result<u32, VizError> {
    // Drop the old connection first: one client at a time.
    *lock(&input.listener) = None;
    let q = Arc::clone(&input.queue);
    let l = platform_macos::midi::NoteListener::connect_all(
        "VizDAW Note Input",
        Box::new(move |b: &[u8]| push(&q, b)),
    )?;
    let n = l.sources();
    *lock(&input.listener) = Some(l);
    Ok(n)
}

#[cfg(not(target_os = "macos"))]
fn listen(_: &NoteInput) -> Result<u32, VizError> {
    Ok(0)
}

#[cfg(target_os = "macos")]
fn unlisten(input: &NoteInput) {
    *lock(&input.listener) = None;
}

#[cfg(not(target_os = "macos"))]
fn unlisten(_: &NoteInput) {}

#[uniffi::export]
impl NoteInput {
    /// Not listening yet.
    #[uniffi::constructor]
    pub fn new() -> Arc<NoteInput> {
        Arc::new(NoteInput {
            queue: Arc::default(),
            #[cfg(target_os = "macos")]
            listener: Mutex::new(None),
        })
    }

    /// Listens to every MIDI source present now (again: picks up new
    /// ones); returns how many (0 off macOS).
    pub fn start(&self) -> Result<u32, VizError> {
        ffi_guard(|| listen(self))
    }

    /// Stops listening; queued notes stay until taken.
    pub fn stop(&self) -> Result<(), VizError> {
        ffi_guard(|| {
            unlisten(self);
            Ok(())
        })
    }

    /// The note-ons since the last call, oldest first.
    pub fn take(&self) -> Result<Vec<NoteInRow>, VizError> {
        ffi_guard(|| Ok(lock(&self.queue).drain(..).collect()))
    }
}

/// Test hooks.
#[cfg(feature = "test-hooks")]
#[uniffi::export]
impl NoteInput {
    /// Handles `bytes` as if a source sent them.
    pub fn inject(&self, bytes: Vec<u8>) -> Result<(), VizError> {
        ffi_guard(|| {
            push(&self.queue, &bytes);
            Ok(())
        })
    }
}
