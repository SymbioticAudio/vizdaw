//! The disk streamer (sub-project 5 spec §5.5; plan 5a Ruling 9).
//!
//! **Voices.** A voice is a ring of `cap` frames × `stride` channels of
//! f32 bits in `AtomicU32`s, with atomics for its key (`media` + 1, 0 =
//! unpublished; the need's `start` and `origin`, or `u64::MAX` for a
//! whole-file cache voice), `base` (the first file frame it streamed) and
//! `hi` (one past the last frame written, stored with `Release` after the
//! data). Frames `[max(base, hi − cap), hi)` are readable.
//!
//! **Real-time side.** [`StreamReader::read`] scans the slots for a voice
//! with the key whose readable range covers the request and copies with
//! `Relaxed` loads; it never writes a voice. [`StreamReader::begin_block`]
//! bumps the epoch and publishes the playhead.
//!
//! **Disk side.** It writes frame `f` of a voice only when `f < floor +
//! cap`, `floor` being the file frame under the published playhead for
//! that voice (else its `base`), so nothing at or after the playhead is
//! overwritten while it can be read. It retires a voice by unpublishing it
//! and changes or frees it only once the epoch advanced twice (Manual mode
//! is single-threaded and retires at once). Each pass plans the windows
//! `[P, P + ahead)` (wrapping at the loop end) and, while looping,
//! `[loop start, loop start + seam)`.
//!
//! **Media.** Small files are read whole into cache voices within a
//! budget (least recently needed evicted); a read error or a short read
//! marks the media failed, reported once by [`Streamer::take_errors`], and
//! reads of it answer [`Read::Offline`].

use crate::MediaError;
use crate::caf::CafReader;
use core_model::ids::MediaId;
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::{
    AtomicBool, AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering, fence,
};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Slots for stream voices.
pub const STREAM_VOICES: usize = 256;
/// Slots for whole-file cache voices.
pub const CACHE_VOICES: usize = 256;
/// Every slot.
pub const SLOTS: usize = STREAM_VOICES + CACHE_VOICES;
/// The key of a cache voice (any need of its media).
const ANY: u64 = u64::MAX;
/// How long a hint the published playhead never reached stays planned
/// (and holds the overwrite floor back).
const HINT_TTL: Duration = Duration::from_secs(1);
/// Failed media the real-time side can tell apart (beyond this they
/// count as misses).
const FAILED_SLOTS: usize = 64;

/// Streaming sizes (spec §5.5); tests shrink them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamConfig {
    /// Ring length of a stream voice (2 s).
    pub voice_seconds: f64,
    /// Read-ahead window (3 s).
    pub ahead_seconds: f64,
    /// Kept prefilled after the loop start (2 s).
    pub seam_seconds: f64,
    /// What `ready` checks after a position (0.25 s, the locate wait).
    pub ready_seconds: f64,
    /// Files of at most this much sample data are cached whole (4 MiB).
    pub cache_file_bytes: u64,
    /// Float bytes the cache may hold (512 MiB).
    pub cache_budget_bytes: u64,
    /// Smallest read (64 KiB), unless it reaches the window's end.
    pub min_read_bytes: u64,
    /// Stream voices (256).
    pub stream_voices: usize,
}

impl Default for StreamConfig {
    fn default() -> StreamConfig {
        StreamConfig {
            voice_seconds: 2.0,
            ahead_seconds: 3.0,
            seam_seconds: 2.0,
            ready_seconds: 0.25,
            cache_file_bytes: 4 << 20,
            cache_budget_bytes: 512 << 20,
            min_read_bytes: 64 << 10,
            stream_voices: STREAM_VOICES,
        }
    }
}

/// Who runs disk passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskMode {
    /// A disk thread, every few milliseconds and when woken.
    Thread,
    /// Nobody: the owner calls [`Streamer::service`] (tests; retirement
    /// is immediate, so no real-time thread may read meanwhile).
    Manual,
}

/// A media file the streamer may read (an online item).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFile {
    /// The item.
    pub id: MediaId,
    /// Its CAF file.
    pub path: PathBuf,
}

/// A stretch the schedule plays: schedule frames `[start, start +
/// frames)` read media frames from `file_frame` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Need {
    /// The media.
    pub media: MediaId,
    /// First schedule frame.
    pub start: u64,
    /// Frames.
    pub frames: u64,
    /// Media frame at `start`.
    pub file_frame: u64,
}

impl Need {
    fn end(&self) -> u64 {
        self.start.saturating_add(self.frames)
    }
}

/// What a real-time read did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Read {
    /// The frames were copied.
    Done,
    /// Not ready (count it with [`StreamReader::count_miss`]).
    Miss,
    /// The media failed; play silence and count nothing.
    Offline,
}

/// Counters for the UI and tests.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamCounters {
    /// Misses after the late-start window.
    pub underruns: u64,
    /// Misses within it.
    pub late: u64,
    /// Files in the RAM cache.
    pub cached_files: usize,
    /// Stream voices in use.
    pub stream_voices: usize,
}

struct Voice {
    media: AtomicU64,
    start: AtomicU64,
    origin: AtomicU64,
    base: AtomicU64,
    hi: AtomicU64,
    /// The overwrite guard: frames below it may have been overwritten
    /// (raised before each chunk's data is written; see [`Voice::guard`]).
    guard: AtomicU64,
    channels: AtomicU32,
    cap: u64,
    stride: usize,
    data: Box<[AtomicU32]>,
}

impl Voice {
    fn new(cap: u64, stride: usize) -> Voice {
        Voice {
            media: AtomicU64::new(0),
            start: AtomicU64::new(0),
            origin: AtomicU64::new(0),
            base: AtomicU64::new(0),
            hi: AtomicU64::new(0),
            guard: AtomicU64::new(0),
            channels: AtomicU32::new(1),
            cap: cap.max(1),
            stride,
            data: (0..cap.max(1) as usize * stride)
                .map(|_| AtomicU32::new(0))
                .collect(),
        }
    }

    /// The readable file frames.
    fn range(&self) -> (u64, u64) {
        let hi = self.hi.load(Ordering::Acquire);
        let lo = self
            .base
            .load(Ordering::Relaxed)
            .max(hi.saturating_sub(self.cap))
            .max(self.guard.load(Ordering::Relaxed));
        (lo, hi)
    }

    /// Disk side, before writing frames below `end`: their slots held
    /// frames below `end - cap`, which stop being readable now.
    ///
    /// The seqlock argument (fix round 1, C1). Writer: `guard` RMW, then
    /// `fence(Release)`, then the data stores. Reader ([`Voice::valid`]):
    /// the data loads, then `fence(Acquire)`, then a `guard` load. If any
    /// data load saw a store made after the writer's fence, the fences
    /// synchronise (C++ [atomics.fences]), so the guard raise happens
    /// before the reader's guard load, which therefore sees it (or a
    /// later value). Overwriting the slot of a frame `f` the reader wants
    /// means writing frame `f + cap` (or later), which raised the guard
    /// above `f >= start`, so the copy is rejected. Every slot is an
    /// `AtomicU32`, so a slot read during its write is no data race
    /// (relaxed 32-bit loads and stores are plain moves on aarch64 and
    /// x86_64); it is merely a stale or new value, which the check sorts.
    fn guard(&self, end: u64) {
        self.guard
            .fetch_max(end.saturating_sub(self.cap), Ordering::Relaxed);
        fence(Ordering::Release);
    }

    /// Real-time side, after copying frames from `start`: false when a
    /// slot of the copy may have been overwritten meanwhile. // RT
    fn valid(&self, start: u64) -> bool {
        fence(Ordering::Acquire);
        start >= self.guard.load(Ordering::Relaxed)
    }

    fn put(&self, f: u64, ch: usize, x: f32) {
        let i = (f % self.cap) as usize * self.stride + ch;
        if let Some(c) = self.data.get(i) {
            c.store(x.to_bits(), Ordering::Relaxed);
        }
    }

    fn get(&self, f: u64, ch: usize) -> f32 {
        let i = (f % self.cap) as usize * self.stride + ch;
        self.data
            .get(i)
            .map_or(0.0, |c| f32::from_bits(c.load(Ordering::Relaxed)))
    }
}

/// What the control side tells the disk side.
#[derive(Default)]
struct Control {
    media: BTreeMap<MediaId, PathBuf>,
    needs: Arc<[Need]>,
    loop_range: Option<(u64, u64)>,
    hint: Option<Hint>,
}

/// A locate the real-time side may not have applied yet.
#[derive(Debug, Clone, Copy)]
struct Hint {
    /// Where playback goes.
    frame: u64,
    /// The published playhead when it was given.
    from: u64,
    /// When it was given.
    at: Instant,
}

impl Hint {
    /// True once the published playhead `pos` has reached the hint: it
    /// is in the hint's window, and (for a hint behind where playback
    /// was) it got there by jumping back, not by playing on.
    /// `pos` is wrapped at the loop `lp`, and so is the hint here.
    fn reached(&self, pos: u64, ahead: u64, lp: Option<(u64, u64)>) -> bool {
        let (frame, from) = (wrapped(self.frame, lp), wrapped(self.from, lp));
        let in_window = pos >= frame && pos < frame.saturating_add(ahead.max(1));
        in_window && (frame >= from || pos < from)
    }
}

struct Shared {
    rate: f64,
    cfg: StreamConfig,
    manual: bool,
    slots: Box<[AtomicPtr<Voice>]>,
    epoch: AtomicU64,
    position: AtomicU64,
    /// The loop the real-time side wraps at (`loop_end` 0: none); set
    /// with `loop_end` cleared first and stored last.
    loop_start: AtomicU64,
    loop_end: AtomicU64,
    underruns: AtomicU64,
    late: AtomicU64,
    /// `Disk::cache.len()`, for lock-free counters.
    cached_files: AtomicUsize,
    /// Stream voices in use, for lock-free counters.
    stream_voices: AtomicUsize,
    failed: Box<[AtomicU64]>,
    control: Mutex<Control>,
    wake: Condvar,
    stop: AtomicBool,
    errors: Mutex<Vec<(MediaId, String)>>,
    retired: Mutex<Vec<(*mut Voice, u64)>>,
}

// SAFETY: the raw pointers in `slots` and `retired` come from
// `Box::into_raw` and are only freed by `Drop` or by the disk side after
// the epoch rule; every other access is through atomics.
unsafe impl Send for Shared {}
// SAFETY: as for `Send`.
unsafe impl Sync for Shared {}

impl Drop for Shared {
    fn drop(&mut self) {
        // The last owner: nothing reads any voice any more.
        for s in self.slots.iter() {
            let p = s.swap(std::ptr::null_mut(), Ordering::AcqRel);
            if !p.is_null() {
                // SAFETY: `p` came from `Box::into_raw` and is in no other
                // slot or list.
                drop(unsafe { Box::from_raw(p) });
            }
        }
        if let Ok(r) = self.retired.get_mut() {
            for (p, _) in r.drain(..) {
                // SAFETY: as above; retired voices left their slots.
                drop(unsafe { Box::from_raw(p) });
            }
        }
    }
}

impl Shared {
    fn slot(&self, i: usize) -> Option<&Voice> {
        let p = self.slots.get(i)?.load(Ordering::Acquire);
        // SAFETY: a published pointer stays valid until the epoch rule
        // (or `Drop`, which needs every reference gone) frees it.
        (!p.is_null()).then(|| unsafe { &*p })
    }

    fn is_failed(&self, media: MediaId) -> bool {
        let key = media.0.wrapping_add(1);
        self.failed.iter().any(|f| f.load(Ordering::Relaxed) == key)
    }
}

/// The real-time handle.
#[derive(Clone)]
pub struct StreamReader {
    shared: Arc<Shared>,
}

impl std::fmt::Debug for StreamReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamReader")
    }
}

impl StreamReader {
    /// A block starts at schedule frame `position` (the playhead the disk
    /// side plans around; pass the current position also while stopped).
    /// While looping, a block starting exactly at the loop end plays from
    /// the loop start (the transport wraps first), so that is published;
    /// past the loop end the transport plays on unwrapped. // RT
    pub fn begin_block(&self, position: u64) {
        let s = &self.shared;
        let e1 = s.loop_end.load(Ordering::Acquire);
        let ls = s.loop_start.load(Ordering::Acquire);
        let e2 = s.loop_end.load(Ordering::Acquire);
        let p = if e1 != 0 && e1 == e2 && ls < e1 && position == e1 {
            ls
        } else {
            position
        };
        s.position.store(p, Ordering::Relaxed);
        s.epoch.fetch_add(1, Ordering::AcqRel);
    }

    /// Copies `left.len()` frames of `media` from `file_frame` for the
    /// segment keyed `(start, origin)` (its need's `start` and
    /// `file_frame`) into `left`/`right` (mono files copy to both).
    /// On a miss the outputs are untouched, or silence when the copy
    /// raced an overwrite (see [`Voice::guard`]). // RT
    pub fn read(
        &self,
        media: MediaId,
        start: u64,
        origin: u64,
        file_frame: u64,
        left: &mut [f32],
        right: &mut [f32],
    ) -> Read {
        let n = left.len().min(right.len()) as u64;
        let key = media.0.wrapping_add(1);
        let s = &self.shared;
        for i in 0..s.slots.len() {
            let Some(v) = s.slot(i) else {
                continue;
            };
            if v.media.load(Ordering::Acquire) != key {
                continue;
            }
            let (vs, vo) = (
                v.start.load(Ordering::Relaxed),
                v.origin.load(Ordering::Relaxed),
            );
            if !(vs == ANY || (vs == start && vo == origin)) {
                continue;
            }
            let (lo, hi) = v.range();
            if file_frame < lo || file_frame.saturating_add(n) > hi {
                continue;
            }
            let stereo = v.channels.load(Ordering::Relaxed) >= 2;
            for (k, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
                let f = file_frame + k as u64;
                *l = v.get(f, 0);
                *r = if stereo { v.get(f, 1) } else { *l };
            }
            if v.valid(file_frame) {
                return Read::Done;
            }
            // The disk overwrote part of it during the copy: silence.
            left.fill(0.0);
            right.fill(0.0);
        }
        if s.is_failed(media) {
            Read::Offline
        } else {
            Read::Miss
        }
    }

    /// Counts a miss: a late start (`late`) or an underrun. // RT
    pub fn count_miss(&self, late: bool) {
        let c = if late {
            &self.shared.late
        } else {
            &self.shared.underruns
        };
        c.fetch_add(1, Ordering::Relaxed);
    }
}

/// Disk-side state, owned by the disk thread (or `service`'s caller).
#[derive(Default)]
struct Disk {
    readers: BTreeMap<MediaId, CafReader>,
    failed: BTreeSet<MediaId>,
    /// Stream slot → its need key and when it was retired (None: in use
    /// or never published).
    stream: BTreeMap<usize, Option<u64>>,
    /// Cached media → (slot, float bytes, last pass it was needed).
    cache: BTreeMap<MediaId, (usize, u64, u64)>,
    cache_bytes: u64,
    pass: u64,
    scratch: Vec<u8>,
    buf: Vec<f32>,
}

/// The control handle (see the module docs).
pub struct Streamer {
    shared: Arc<Shared>,
    disk: Arc<Mutex<Disk>>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for Streamer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Streamer")
            .field("rate", &self.shared.rate)
            .finish_non_exhaustive()
    }
}

impl Drop for Streamer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.wake.notify_all();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Streamer {
    /// A streamer at `rate` (the project rate) and its real-time reader;
    /// with `DiskMode::Thread`, starts the disk thread.
    pub fn new(
        rate: f64,
        cfg: StreamConfig,
        mode: DiskMode,
    ) -> Result<(Streamer, StreamReader), MediaError> {
        let slots = cfg.stream_voices.min(STREAM_VOICES) + CACHE_VOICES;
        let shared = Arc::new(Shared {
            rate: rate.max(1.0),
            cfg,
            manual: mode == DiskMode::Manual,
            slots: (0..slots)
                .map(|_| AtomicPtr::new(std::ptr::null_mut()))
                .collect(),
            epoch: AtomicU64::new(0),
            position: AtomicU64::new(0),
            loop_start: AtomicU64::new(0),
            loop_end: AtomicU64::new(0),
            underruns: AtomicU64::new(0),
            late: AtomicU64::new(0),
            cached_files: AtomicUsize::new(0),
            stream_voices: AtomicUsize::new(0),
            failed: (0..FAILED_SLOTS).map(|_| AtomicU64::new(0)).collect(),
            control: Mutex::new(Control::default()),
            wake: Condvar::new(),
            stop: AtomicBool::new(false),
            errors: Mutex::new(Vec::new()),
            retired: Mutex::new(Vec::new()),
        });
        let disk = Arc::new(Mutex::new(Disk::default()));
        let thread = match mode {
            DiskMode::Manual => None,
            DiskMode::Thread => {
                let (s, d) = (Arc::clone(&shared), Arc::clone(&disk));
                Some(
                    std::thread::Builder::new()
                        .name("vizdaw-disk".into())
                        .spawn(move || {
                            while !s.stop.load(Ordering::Relaxed) {
                                pass(&s, &mut lock(&d));
                                let c = lock(&s.control);
                                let _ = s.wake.wait_timeout(c, Duration::from_millis(5));
                            }
                        })
                        .map_err(|e| MediaError::Io(format!("disk thread: {e}")))?,
                )
            }
        };
        let reader = StreamReader {
            shared: Arc::clone(&shared),
        };
        Ok((
            Streamer {
                shared,
                disk,
                thread,
            },
            reader,
        ))
    }

    /// The media that may be read (online items); others are never read.
    pub fn set_media(&self, files: Vec<MediaFile>) {
        let mut c = lock(&self.shared.control);
        c.media = files.into_iter().map(|f| (f.id, f.path)).collect();
        drop(c);
        self.shared.wake.notify_all();
    }

    /// What the loaded schedule plays.
    pub fn set_plan(&self, needs: Arc<[Need]>) {
        lock(&self.shared.control).needs = needs;
        self.shared.wake.notify_all();
    }

    /// The loop range, while looping.
    pub fn set_loop(&self, range: Option<(u64, u64)>) {
        let sh = &self.shared;
        let range = range.filter(|(s, e)| e > s);
        let mut c = lock(&sh.control);
        c.loop_range = range;
        sh.loop_end.store(0, Ordering::Release);
        if let Some((ls, le)) = range {
            sh.loop_start.store(ls, Ordering::Release);
            sh.loop_end.store(le, Ordering::Release);
        }
        drop(c);
        sh.wake.notify_all();
    }

    /// The loop the real-time side wraps at, as last set (`None`: not
    /// looping).
    pub fn loop_range(&self) -> Option<(u64, u64)> {
        let _c = lock(&self.shared.control);
        let le = self.shared.loop_end.load(Ordering::Acquire);
        let ls = self.shared.loop_start.load(Ordering::Acquire);
        (le != 0).then_some((ls, le))
    }

    /// Playback is about to start at (or move to) `frame`: plan around it
    /// now.
    /// The hint is planned alongside the published playhead (never
    /// instead of it) until the playhead reaches it.
    pub fn hint(&self, frame: u64) {
        let from = self.shared.position.load(Ordering::Relaxed);
        lock(&self.shared.control).hint = Some(Hint {
            frame,
            from,
            at: Instant::now(),
        });
        self.shared.wake.notify_all();
    }

    /// True when every need playing in `[frame, frame + ready_seconds)`
    /// has its frames (failed and unknown media count as ready).
    pub fn ready(&self, frame: u64) -> bool {
        let s = &self.shared;
        let c = lock(&s.control);
        let end = frame.saturating_add((s.cfg.ready_seconds * s.rate) as u64);
        c.needs
            .iter()
            .filter(|n| n.start < end && n.end() > frame)
            .filter(|n| c.media.contains_key(&n.media) && !s.is_failed(n.media))
            .all(|n| {
                let from = n.file_frame + (frame.max(n.start) - n.start);
                let to = n.file_frame + (end.min(n.end()) - n.start);
                (0..s.slots.len()).any(|i| {
                    s.slot(i).is_some_and(|v| {
                        let key_ok = v.media.load(Ordering::Acquire) == n.media.0.wrapping_add(1)
                            && (v.start.load(Ordering::Relaxed) == ANY
                                || (v.start.load(Ordering::Relaxed) == n.start
                                    && v.origin.load(Ordering::Relaxed) == n.file_frame));
                        let (lo, hi) = v.range();
                        key_ok && lo <= from && to <= hi
                    })
                })
            })
    }

    /// Hints `frame` and waits up to `timeout` for [`Streamer::ready`]
    /// (Manual mode runs passes itself). True when ready.
    pub fn wait_ready(&self, frame: u64, timeout: Duration) -> bool {
        self.hint(frame);
        let until = Instant::now() + timeout;
        loop {
            if self.shared.manual {
                self.service();
            }
            if self.ready(frame) {
                return true;
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// One disk pass (what the disk thread runs).
    pub fn service(&self) {
        pass(&self.shared, &mut lock(&self.disk));
    }

    /// Counters since the streamer started.
    /// Lock-free: never waits for a disk pass.
    pub fn counters(&self) -> StreamCounters {
        let s = &self.shared;
        StreamCounters {
            underruns: s.underruns.load(Ordering::Relaxed),
            late: s.late.load(Ordering::Relaxed),
            cached_files: s.cached_files.load(Ordering::Relaxed),
            stream_voices: s.stream_voices.load(Ordering::Relaxed),
        }
    }

    /// Media that failed since the last call, with why (each once).
    pub fn take_errors(&self) -> Vec<(MediaId, String)> {
        std::mem::take(&mut *lock(&self.shared.errors))
    }
}

/// The playhead `p` after a loop wrap that is due: at the loop end the
/// transport plays from the loop start.
fn wrapped(p: u64, lp: Option<(u64, u64)>) -> u64 {
    match lp {
        Some((ls, le)) if p == le => ls,
        _ => p,
    }
}

/// The windows a pass fills, around playhead `p`. At the loop end that is
/// the loop start; past it (a locate there) playback runs on unwrapped,
/// and the seam stays planned.
fn windows(p: u64, lp: Option<(u64, u64)>, ahead: u64, seam: u64) -> Vec<(u64, u64)> {
    let p = wrapped(p, lp);
    match lp {
        Some((ls, le)) if p >= le => vec![
            (p, p.saturating_add(ahead)),
            (ls, ls.saturating_add(seam).min(le)),
        ],
        Some((ls, le)) => {
            let mut w = vec![(p, p.saturating_add(ahead).min(le))];
            let over = p.saturating_add(ahead).saturating_sub(le);
            if over > 0 {
                w.push((ls, ls.saturating_add(over).min(le)));
            }
            w.push((ls, ls.saturating_add(seam).min(le)));
            w
        }
        _ => vec![(p, p.saturating_add(ahead))],
    }
}

/// Marks `media` failed: reported once, then read as offline.
fn fail(s: &Shared, d: &mut Disk, media: MediaId, why: String) {
    if !d.failed.insert(media) {
        return;
    }
    d.readers.remove(&media);
    let key = media.0.wrapping_add(1);
    if let Some(f) = s.failed.iter().find(|f| f.load(Ordering::Relaxed) == 0) {
        f.store(key, Ordering::Relaxed);
    }
    lock(&s.errors).push((media, why));
}

/// Opens (once) the reader of `media`.
fn reader<'a>(
    s: &Shared,
    d: &'a mut Disk,
    media: MediaId,
    path: &std::path::Path,
) -> Option<&'a CafReader> {
    let err = match d.readers.entry(media) {
        Entry::Occupied(_) => None,
        Entry::Vacant(v) => match CafReader::open(path) {
            Ok(r) => {
                v.insert(r);
                None
            }
            Err(e) => Some(e.to_string()),
        },
    };
    if let Some(why) = err {
        fail(s, d, media, why);
        return None;
    }
    d.readers.get(&media)
}

/// Unpublishes slot `i`'s voice, noting the epoch (Manual: free to reuse
/// at once).
fn retire_stream(s: &Shared, d: &mut Disk, i: usize) {
    if let Some(v) = s.slot(i) {
        v.media.store(0, Ordering::Release);
    }
    let at = if s.manual {
        0
    } else {
        s.epoch.load(Ordering::Acquire)
    };
    d.stream.insert(i, Some(at));
    sync_counts(s, d);
}

/// Publishes the disk-side counts for [`Streamer::counters`].
fn sync_counts(s: &Shared, d: &Disk) {
    s.cached_files.store(d.cache.len(), Ordering::Relaxed);
    let used = d.stream.values().filter(|r| r.is_none()).count();
    s.stream_voices.store(used, Ordering::Relaxed);
}

/// A free stream slot (a never-used one gets a voice), or `None`.
fn free_stream_slot(s: &Shared, d: &mut Disk) -> Option<usize> {
    let now = s.epoch.load(Ordering::Acquire);
    let reusable = d
        .stream
        .iter()
        .find(|(_, r)| matches!(r, Some(e) if s.manual || now >= e + 2))
        .map(|(i, _)| *i);
    if reusable.is_some() {
        return reusable;
    }
    let max = s.cfg.stream_voices.min(STREAM_VOICES);
    let i = (0..max).find(|i| !d.stream.contains_key(i))?;
    let cap = (s.cfg.voice_seconds * s.rate).ceil().max(1.0) as u64;
    let p = Box::into_raw(Box::new(Voice::new(cap, 2)));
    s.slots.get(i)?.store(p, Ordering::Release);
    d.stream.insert(i, Some(0));
    Some(i)
}

/// Frees retired cache voices whose epoch has passed (all in Manual).
fn collect(s: &Shared) {
    let now = s.epoch.load(Ordering::Acquire);
    let mut r = lock(&s.retired);
    r.retain(|(p, e)| {
        if s.manual || now >= e + 2 {
            // SAFETY: `p` left its slot at epoch `e`; two block starts
            // since mean no reader still holds it (Ruling 9).
            drop(unsafe { Box::from_raw(*p) });
            false
        } else {
            true
        }
    });
}

/// Reads `n` frames of `media` from `from` into voice `v` at the same
/// frames; false (and the media failed) on an error or a short read.
fn fill(
    s: &Shared,
    d: &mut Disk,
    v: &Voice,
    media: MediaId,
    path: &std::path::Path,
    from: u64,
    n: u64,
) -> bool {
    let mut scratch = std::mem::take(&mut d.scratch);
    let mut buf = std::mem::take(&mut d.buf);
    let ok = (|| {
        let r = reader(s, d, media, path)?;
        let ch = usize::from(r.info().channels);
        v.channels.store(ch as u32, Ordering::Relaxed);
        let chunk = 16_384u64;
        let mut at = from;
        while at < from + n {
            let k = chunk.min(from + n - at);
            buf.resize(k as usize * ch, 0.0);
            let got = match r.read(at, &mut buf, &mut scratch) {
                Ok(g) => g as u64,
                Err(_) => return None,
            };
            // Raise the overwrite guard before any slot changes.
            v.guard(at + got);
            for f in 0..got {
                for c in 0..ch.min(v.stride) {
                    let x = buf.get(f as usize * ch + c).copied().unwrap_or(0.0);
                    v.put(at + f, c, x);
                }
            }
            if got < k {
                return None;
            }
            at += k;
        }
        Some(())
    })()
    .is_some();
    d.scratch = scratch;
    d.buf = buf;
    if !ok && !d.failed.contains(&media) {
        fail(
            s,
            d,
            media,
            format!(
                "{}: read failed or file shorter than expected",
                path.display()
            ),
        );
    }
    ok
}

/// Loads `media` whole into a cache voice if it is small; true when it is
/// (or already was) cached.
fn ensure_cached(s: &Shared, d: &mut Disk, media: MediaId, path: &std::path::Path) -> bool {
    if let Some(e) = d.cache.get_mut(&media) {
        e.2 = d.pass;
        return true;
    }
    let Some(r) = reader(s, d, media, path) else {
        return false;
    };
    let (frames, ch, fb) = (
        r.info().frames,
        usize::from(r.info().channels),
        r.info().frame_bytes() as u64,
    );
    if frames == 0 || frames.saturating_mul(fb) > s.cfg.cache_file_bytes {
        return false;
    }
    let bytes = frames * ch as u64 * 4;
    if bytes > s.cfg.cache_budget_bytes {
        return false;
    }
    // Evict the least recently needed until it fits (never ones needed in
    // this pass).
    while d.cache_bytes + bytes > s.cfg.cache_budget_bytes {
        let Some((&m, &(slot, b, _))) = d
            .cache
            .iter()
            .filter(|(_, e)| e.2 < d.pass)
            .min_by_key(|(_, e)| e.2)
        else {
            return false;
        };
        evict(s, d, m, slot, b);
    }
    let first = s.cfg.stream_voices.min(STREAM_VOICES);
    let Some(slot) = (first..s.slots.len()).find(|i| s.slot(*i).is_none()) else {
        return false;
    };
    let v = Box::new(Voice::new(frames, ch));
    if !fill(s, d, &v, media, path, 0, frames) {
        return false;
    }
    v.base.store(0, Ordering::Relaxed);
    v.start.store(ANY, Ordering::Relaxed);
    v.origin.store(ANY, Ordering::Relaxed);
    v.hi.store(frames, Ordering::Release);
    v.media.store(media.0.wrapping_add(1), Ordering::Release);
    if let Some(cell) = s.slots.get(slot) {
        cell.store(Box::into_raw(v), Ordering::Release);
    }
    d.cache.insert(media, (slot, bytes, d.pass));
    d.cache_bytes += bytes;
    sync_counts(s, d);
    true
}

fn evict(s: &Shared, d: &mut Disk, media: MediaId, slot: usize, bytes: u64) {
    if let Some(cell) = s.slots.get(slot) {
        let p = cell.swap(std::ptr::null_mut(), Ordering::AcqRel);
        if !p.is_null() {
            lock(&s.retired).push((p, s.epoch.load(Ordering::Acquire)));
        }
    }
    d.cache.remove(&media);
    d.cache_bytes = d.cache_bytes.saturating_sub(bytes);
    sync_counts(s, d);
}

/// One disk pass (see the module docs).
fn pass(s: &Shared, d: &mut Disk) {
    d.pass += 1;
    collect(s);
    let rate = s.rate;
    let ahead = (s.cfg.ahead_seconds * rate) as u64;
    let seam = (s.cfg.seam_seconds * rate) as u64;
    let (media, needs, lp, pos, hint) = {
        let mut c = lock(&s.control);
        let lp = c.loop_range;
        // The published playhead: what the real-time side reads now.
        let pos = wrapped(s.position.load(Ordering::Relaxed), lp);
        // A hint stays planned until the playhead reaches it (or it
        // goes stale), so the voices still playing are kept meanwhile.
        if c.hint
            .is_some_and(|h| h.reached(pos, ahead, lp) || h.at.elapsed() >= HINT_TTL)
        {
            c.hint = None;
        }
        let hint = c.hint.map(|h| wrapped(h.frame, lp));
        (c.media.clone(), Arc::clone(&c.needs), lp, pos, hint)
    };
    // Media gone from the table leave the cache and the readers.
    let gone: Vec<(MediaId, usize, u64)> = d
        .cache
        .iter()
        .filter(|(m, _)| !media.contains_key(m))
        .map(|(m, e)| (*m, e.0, e.1))
        .collect();
    for (m, slot, b) in gone {
        evict(s, d, m, slot, b);
    }
    d.readers.retain(|m, _| media.contains_key(m));
    let mut win = windows(pos, lp, ahead, seam);
    if let Some(h) = hint {
        win.extend(windows(h, lp, ahead, seam));
    }
    // Stream slots used this pass, with the file frame to fill up to.
    let mut used: BTreeMap<usize, (Need, u64)> = BTreeMap::new();
    for n in needs.iter() {
        let Some(path) = media.get(&n.media) else {
            continue;
        };
        if d.failed.contains(&n.media) {
            continue;
        }
        let mut wanted = Vec::new();
        for &(a, b) in &win {
            let (from, to) = (a.max(n.start), b.min(n.end()));
            if from < to {
                wanted.push((
                    n.file_frame + (from - n.start),
                    n.file_frame + (to - n.start),
                ));
            }
        }
        if wanted.is_empty() {
            continue;
        }
        if ensure_cached(s, d, n.media, path) {
            continue;
        }
        if d.failed.contains(&n.media) {
            continue;
        }
        for (want, until) in wanted {
            let key = n.media.0.wrapping_add(1);
            let found = d.stream.iter().find_map(|(&i, r)| {
                let v = s.slot(i)?;
                let live = r.is_none()
                    && v.media.load(Ordering::Acquire) == key
                    && v.start.load(Ordering::Relaxed) == n.start
                    && v.origin.load(Ordering::Relaxed) == n.file_frame;
                let (lo, hi) = v.range();
                (live && lo <= want && want <= hi).then_some(i)
            });
            let slot = match found {
                Some(i) => i,
                None => {
                    let Some(i) = free_stream_slot(s, d) else {
                        continue;
                    };
                    if let Some(v) = s.slot(i) {
                        v.start.store(n.start, Ordering::Relaxed);
                        v.origin.store(n.file_frame, Ordering::Relaxed);
                        v.base.store(want, Ordering::Relaxed);
                        // Retired two epochs ago: no reader, so the guard
                        // may go back down.
                        v.guard.store(want, Ordering::Relaxed);
                        v.hi.store(want, Ordering::Release);
                        v.media.store(key, Ordering::Release);
                    }
                    d.stream.insert(i, None);
                    sync_counts(s, d);
                    i
                }
            };
            let e = used.entry(slot).or_insert((*n, until));
            e.1 = e.1.max(until);
        }
    }
    // Retire stream voices no window used.
    let idle: Vec<usize> = d
        .stream
        .iter()
        .filter(|(i, r)| r.is_none() && !used.contains_key(i))
        .map(|(i, _)| *i)
        .collect();
    for i in idle {
        retire_stream(s, d, i);
    }
    // Fill the used ones, nearest deadline first.
    let mut order: Vec<(usize, Need, u64)> =
        used.into_iter().map(|(i, (n, u))| (i, n, u)).collect();
    order.sort_by_key(|(i, n, _)| {
        let hi = s.slot(*i).map_or(0, |v| v.range().1);
        n.start + hi.saturating_sub(n.file_frame)
    });
    for (i, n, until) in order {
        let Some(v) = s.slot(i) else {
            continue;
        };
        let Some(path) = media.get(&n.media).cloned() else {
            continue;
        };
        let (base, hi) = (v.base.load(Ordering::Relaxed), v.hi.load(Ordering::Acquire));
        // The floor is the lowest frame of this voice the real-time side
        // may read: under the published playhead, or under the hint
        // should the locate land first (never past the playhead).
        let floor = [Some(pos), hint]
            .into_iter()
            .flatten()
            .filter(|&p| p >= n.start && p < n.end())
            .map(|p| n.file_frame + (p - n.start))
            .filter(|&f| f >= base && f <= hi)
            .min()
            .unwrap_or(base);
        let space = (floor + v.cap).saturating_sub(hi);
        let count = space.min(until.saturating_sub(hi));
        let fb = 4 * v.stride as u64;
        let min = (s.cfg.min_read_bytes / fb).max(1);
        if count == 0 || (count < min && hi + count < until) {
            continue;
        }
        if fill(s, d, v, n.media, &path, hi, count) {
            v.hi.store(hi + count, Ordering::Release);
        } else {
            retire_stream(s, d, i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::caf::{CafWriter, WriteMode};
    use core_model::format::BitDepth;
    use proptest::prelude::*;

    /// Frame f, channel c of the test files.
    fn sample(f: u64, c: usize) -> f32 {
        ((f % 100_000) as f32) * 1e-5 + c as f32 * 0.5 - 0.5
    }

    fn file(dir: &std::path::Path, name: &str, frames: u64, ch: u16) -> PathBuf {
        let p = dir.join(name);
        let mut w =
            CafWriter::create(&p, 48_000, ch, BitDepth::Float32, WriteMode::Normal).unwrap();
        let data: Vec<f32> = (0..frames)
            .flat_map(|f| (0..usize::from(ch)).map(move |c| sample(f, c)))
            .collect();
        w.write(&data).unwrap();
        w.finish().unwrap();
        p
    }

    /// Small sizes so tests stream small files: 0.1 s voices, 0.2 s ahead.
    fn small() -> StreamConfig {
        StreamConfig {
            voice_seconds: 0.1,
            ahead_seconds: 0.2,
            seam_seconds: 0.05,
            ready_seconds: 0.01,
            cache_file_bytes: 0,
            cache_budget_bytes: 1 << 30,
            min_read_bytes: 4096,
            stream_voices: 8,
        }
    }

    fn need(media: u64, start: u64, frames: u64, file_frame: u64) -> Need {
        Need {
            media: MediaId(media),
            start,
            frames,
            file_frame,
        }
    }

    /// Reads `n` frames at `file_frame` of segment `n0`; checks them when
    /// done.
    fn read(r: &StreamReader, n0: &Need, file_frame: u64, n: usize, stereo: bool) -> Read {
        let (mut l, mut rr) = (vec![9.0; n], vec![9.0; n]);
        let got = r.read(
            n0.media,
            n0.start,
            n0.file_frame,
            file_frame,
            &mut l,
            &mut rr,
        );
        if got == Read::Done {
            for k in 0..n {
                let f = file_frame + k as u64;
                assert_eq!(l[k], sample(f, 0), "left at {f}");
                assert_eq!(rr[k], sample(f, if stereo { 1 } else { 0 }), "right at {f}");
            }
        } else {
            assert!(l.iter().all(|x| *x == 9.0), "a miss wrote");
        }
        got
    }

    /// The hint the disk side still plans around.
    fn pending_hint(s: &Streamer) -> Option<u64> {
        lock(&s.shared.control).hint.map(|h| h.frame)
    }

    fn manual(cfg: StreamConfig) -> (Streamer, StreamReader) {
        Streamer::new(48_000.0, cfg, DiskMode::Manual).unwrap()
    }

    #[test]
    fn a_planned_need_is_streamed_and_read_back_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 96_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile {
            id: MediaId(0),
            path: p,
        }]);
        let n = need(0, 1000, 90_000, 500);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(1000);
        assert_eq!(read(&r, &n, 500, 256, true), Read::Miss);
        s.service();
        assert_eq!(read(&r, &n, 500, 256, true), Read::Done);
        // The key matters: the same media for another segment misses.
        let other = need(0, 2000, 10, 500);
        assert_eq!(read(&r, &other, 500, 4, true), Read::Miss);
        // Beyond the ring (0.1 s = 4800 frames).
        assert_eq!(read(&r, &n, 500 + 6000, 4, true), Read::Miss);
        assert_eq!(s.counters().stream_voices, 1);
    }

    #[test]
    fn playback_streams_every_frame_without_tearing() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 200_000, 1);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile {
            id: MediaId(3),
            path: p,
        }]);
        let n = need(3, 0, 200_000, 0);
        s.set_plan(Arc::from(vec![n]));
        let mut pos = 0u64;
        while pos + 512 <= 200_000 {
            r.begin_block(pos);
            s.service();
            assert_eq!(read(&r, &n, pos, 512, false), Read::Done, "at {pos}");
            pos += 512;
        }
    }

    #[test]
    fn small_files_are_cached_whole() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "s.caf", 20_000, 2);
        let mut cfg = small();
        cfg.cache_file_bytes = 4 << 20;
        let (s, r) = manual(cfg);
        s.set_media(vec![MediaFile {
            id: MediaId(1),
            path: p,
        }]);
        let n = need(1, 0, 20_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(0);
        s.service();
        // Far from the playhead, any segment of the media.
        assert_eq!(read(&r, &n, 19_000, 1000, true), Read::Done);
        assert_eq!(read(&r, &need(1, 7, 7, 7), 3, 10, true), Read::Done);
        assert_eq!(s.counters().cached_files, 1);
        // Media leaving the table leave the cache.
        s.set_media(vec![]);
        s.service();
        assert_eq!(s.counters().cached_files, 0);
        assert_eq!(read(&r, &n, 0, 10, true), Read::Miss);
    }

    #[test]
    fn the_cache_budget_evicts_the_least_recently_needed() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = small();
        cfg.cache_file_bytes = 4 << 20;
        // Room for two 10 000-frame stereo files (80 000 float bytes each).
        cfg.cache_budget_bytes = 170_000;
        let (s, r) = manual(cfg);
        let files: Vec<MediaFile> = (0..3)
            .map(|i| MediaFile {
                id: MediaId(i),
                path: file(dir.path(), &format!("{i}.caf"), 10_000, 2),
            })
            .collect();
        s.set_media(files);
        for i in 0..3u64 {
            s.set_plan(Arc::from(vec![need(i, 0, 10_000, 0)]));
            r.begin_block(0);
            s.service();
        }
        assert_eq!(s.counters().cached_files, 2);
        assert_eq!(read(&r, &need(0, 0, 10_000, 0), 0, 10, true), Read::Miss);
        assert_eq!(read(&r, &need(2, 0, 10_000, 0), 0, 10, true), Read::Done);
    }

    #[test]
    fn locate_fills_from_the_new_position() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile {
            id: MediaId(0),
            path: p,
        }]);
        let n = need(0, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(0);
        s.service();
        assert_eq!(read(&r, &n, 250_000, 64, true), Read::Miss);
        s.hint(250_000);
        s.service();
        r.begin_block(250_000);
        assert_eq!(read(&r, &n, 250_000, 64, true), Read::Done);
        assert!(s.wait_ready(250_000, Duration::from_millis(50)));
    }

    #[test]
    fn the_loop_seam_is_kept_prefilled() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 1);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile {
            id: MediaId(0),
            path: p,
        }]);
        let n = need(0, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        s.set_loop(Some((10_000, 200_000)));
        // Near the loop end: the seam at the loop start is ready too.
        r.begin_block(199_000);
        s.service();
        assert_eq!(read(&r, &n, 199_000, 512, false), Read::Done);
        assert_eq!(read(&r, &n, 10_000, 512, false), Read::Done);
        // After the wrap, the seam voice plays on.
        let mut pos = 10_000;
        for _ in 0..40 {
            r.begin_block(pos);
            s.service();
            assert_eq!(read(&r, &n, pos, 512, false), Read::Done, "at {pos}");
            pos += 512;
        }
    }

    #[test]
    fn a_file_removed_mid_play_goes_offline_and_plays_silence() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile {
            id: MediaId(5),
            path: p.clone(),
        }]);
        let n = need(5, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(0);
        s.service();
        assert_eq!(read(&r, &n, 0, 64, true), Read::Done);
        // Truncate it to 1000 frames under the open reader.
        let off = CafReader::open(&p).unwrap().info().data_offset;
        std::fs::OpenOptions::new()
            .write(true)
            .open(&p)
            .unwrap()
            .set_len(off + 1000 * 8)
            .unwrap();
        let mut pos = 0;
        for _ in 0..100 {
            r.begin_block(pos);
            s.service();
            pos += 512;
        }
        let errors = s.take_errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(errors[0].0, MediaId(5));
        assert!(s.take_errors().is_empty(), "reported once");
        assert_eq!(read(&r, &n, 40_000, 64, true), Read::Offline);
        assert_eq!(s.counters().underruns, 0);
    }

    #[test]
    fn a_starved_disk_misses_and_counts_only_what_it_is_told() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 100_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile {
            id: MediaId(0),
            path: p,
        }]);
        let n = need(0, 0, 100_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(0);
        assert_eq!(read(&r, &n, 0, 64, true), Read::Miss);
        r.count_miss(false);
        r.count_miss(true);
        let c = s.counters();
        assert_eq!((c.underruns, c.late), (1, 1));
    }

    #[test]
    fn the_disk_thread_gets_ready() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 2);
        let (s, r) = Streamer::new(48_000.0, small(), DiskMode::Thread).unwrap();
        s.set_media(vec![MediaFile {
            id: MediaId(0),
            path: p,
        }]);
        let n = need(0, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        r.begin_block(100_000);
        assert!(s.wait_ready(100_000, Duration::from_secs(5)));
        assert_eq!(read(&r, &n, 100_000, 256, true), Read::Done);
        drop(s);
        // The reader outlives the streamer safely.
        assert_ne!(read(&r, &n, 100_000, 8, true), Read::Offline);
    }

    /// F1: the windows of a playhead at or past the loop end.
    #[test]
    fn windows_wrap_at_the_loop_end_and_keep_the_seam_past_it() {
        let lp = Some((1_000, 5_000));
        // Before the loop end: cut there and continued at the loop start.
        assert_eq!(
            windows(4_900, lp, 800, 300),
            vec![(4_900, 5_000), (1_000, 1_700), (1_000, 1_300)]
        );
        // At the loop end the transport wraps first: plan as at the start.
        assert_eq!(windows(5_000, lp, 800, 300), windows(1_000, lp, 800, 300));
        assert_eq!(
            windows(5_000, lp, 800, 300),
            vec![(1_000, 1_800), (1_000, 1_300)]
        );
        // Past the loop end the transport plays on unwrapped; the seam
        // stays planned.
        assert_eq!(
            windows(6_000, lp, 800, 300),
            vec![(6_000, 6_800), (1_000, 1_300)]
        );
        assert_eq!(windows(6_000, None, 800, 300), vec![(6_000, 6_800)]);
    }

    /// F1: a block that starts exactly at the loop end (the renderer cuts
    /// runs there) plays from the loop start, and the seam survives it.
    #[test]
    fn a_block_starting_at_the_loop_end_reads_the_seam_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 1);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile {
            id: MediaId(0),
            path: p,
        }]);
        let n = need(0, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        s.set_loop(Some((10_000, 200_000)));
        let mut pos = 196_000;
        while pos < 200_000 {
            r.begin_block(pos);
            s.service();
            let k = 500.min(200_000 - pos);
            assert_eq!(read(&r, &n, pos, k as usize, false), Read::Done, "at {pos}");
            pos += k;
        }
        r.begin_block(200_000);
        assert_eq!(s.shared.position.load(Ordering::Relaxed), 10_000);
        // The wrap never waits on the disk, and a pass during the block
        // overwrites nothing it reads.
        assert_eq!(read(&r, &n, 10_000, 512, false), Read::Done);
        s.service();
        assert_eq!(read(&r, &n, 10_000, 512, false), Read::Done);
        let mut at = 10_512;
        for _ in 0..30 {
            r.begin_block(at);
            assert_eq!(read(&r, &n, at, 512, false), Read::Done, "at {at}");
            s.service();
            assert_eq!(read(&r, &n, at, 512, false), Read::Done, "at {at}");
            at += 512;
        }
        // Past the loop end (a locate there) the transport plays on
        // unwrapped, so that is what is published.
        r.begin_block(200_005);
        assert_eq!(s.shared.position.load(Ordering::Relaxed), 200_005);
        s.set_loop(None);
        r.begin_block(200_000);
        assert_eq!(s.shared.position.load(Ordering::Relaxed), 200_000);
    }

    /// F2: a locate hint before the real-time side applies it keeps the
    /// voices it is still playing.
    #[test]
    fn a_far_hint_keeps_the_voices_still_playing() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile {
            id: MediaId(0),
            path: p,
        }]);
        let n = need(0, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        let a = 20_000;
        r.begin_block(a);
        s.service();
        assert_eq!(read(&r, &n, a, 512, true), Read::Done);
        s.hint(150_000);
        s.service();
        // Same block, the locate not applied yet.
        assert_eq!(read(&r, &n, a, 512, true), Read::Done);
        let mut at = a + 512;
        for _ in 0..4 {
            r.begin_block(at);
            s.service();
            assert_eq!(read(&r, &n, at, 512, true), Read::Done, "at {at}");
            at += 512;
        }
        assert_eq!(pending_hint(&s), Some(150_000));
        // The locate lands: prefilled, and the hint is done.
        r.begin_block(150_000);
        assert_eq!(read(&r, &n, 150_000, 512, true), Read::Done);
        s.service();
        assert_eq!(pending_hint(&s), None);
        assert_eq!(read(&r, &n, 150_000, 512, true), Read::Done);
        assert_eq!(s.counters().underruns, 0);
    }

    /// F2: a forward hint inside the ring being played never moves the
    /// overwrite floor past the published playhead.
    #[test]
    fn a_hint_inside_the_playing_ring_never_overwrites_the_playhead() {
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", 300_000, 2);
        let (s, r) = manual(small());
        s.set_media(vec![MediaFile {
            id: MediaId(0),
            path: p,
        }]);
        let n = need(0, 0, 300_000, 0);
        s.set_plan(Arc::from(vec![n]));
        let a = 20_000;
        r.begin_block(a);
        s.service();
        assert_eq!(read(&r, &n, a, 512, true), Read::Done);
        s.hint(a + 1_000);
        s.service();
        assert_eq!(read(&r, &n, a, 1_512, true), Read::Done);
        r.begin_block(a + 1_000);
        assert_eq!(read(&r, &n, a + 1_000, 512, true), Read::Done);
        s.service();
        assert_eq!(read(&r, &n, a + 1_000, 512, true), Read::Done);
        assert_eq!(pending_hint(&s), None);
    }

    /// F3: the counters never wait for a disk pass (which holds the disk
    /// lock across file reads).
    #[test]
    fn counters_never_wait_for_the_disk_pass() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = small();
        cfg.cache_file_bytes = 100_000;
        let (s, r) = manual(cfg);
        s.set_media(vec![
            MediaFile {
                id: MediaId(0),
                path: file(dir.path(), "a.caf", 300_000, 2),
            },
            MediaFile {
                id: MediaId(1),
                path: file(dir.path(), "b.caf", 1_000, 2),
            },
        ]);
        s.set_plan(Arc::from(vec![
            need(0, 0, 300_000, 0),
            need(1, 0, 1_000, 0),
        ]));
        r.begin_block(0);
        s.service();
        r.count_miss(false);
        r.count_miss(true);
        let want = StreamCounters {
            underruns: 1,
            late: 1,
            cached_files: 1,
            stream_voices: 1,
        };
        assert_eq!(s.counters(), want);
        let guard = lock(&s.disk);
        let s = &s;
        std::thread::scope(|sc| {
            let (tx, rx) = std::sync::mpsc::channel();
            sc.spawn(move || {
                let _ = tx.send(s.counters());
            });
            let got = rx.recv_timeout(Duration::from_secs(2));
            drop(guard);
            assert_eq!(got, Ok(want));
        });
    }

    /// Plays in Thread mode for `secs`: forward blocks, and every third
    /// block a small backward or forward locate (`hint` then
    /// `begin_block`, as `Engine::locate_frame` does) while the disk
    /// thread fills. Returns (wrong samples, done reads, misses).
    fn stress(secs: f64) -> (u64, u64, u64) {
        const FRAMES: u64 = 600_000;
        let dir = tempfile::tempdir().unwrap();
        let p = file(dir.path(), "a.caf", FRAMES, 2);
        let cfg = StreamConfig {
            voice_seconds: 0.25,
            ahead_seconds: 0.4,
            seam_seconds: 0.05,
            ready_seconds: 0.01,
            cache_file_bytes: 0,
            cache_budget_bytes: 1 << 30,
            min_read_bytes: 1,
            stream_voices: 16,
        };
        let cap = (cfg.voice_seconds * 48_000.0) as u64;
        let (s, r) = Streamer::new(48_000.0, cfg, DiskMode::Thread).unwrap();
        s.set_media(vec![MediaFile {
            id: MediaId(0),
            path: p,
        }]);
        let n = need(0, 0, FRAMES, 0);
        s.set_plan(Arc::from(vec![n]));
        let block = 256u64;
        let mut rng = 0x9E37_79B9_7F4A_7C15u64;
        let mut next = move || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        let (mut l, mut rr) = (vec![0.0f32; block as usize], vec![0.0f32; block as usize]);
        let (mut wrong, mut done, mut miss) = (0u64, 0u64, 0u64);
        let mut pos = 10_000u64;
        let until = Instant::now() + Duration::from_secs_f64(secs);
        let mut k = 0u64;
        while Instant::now() < until {
            k += 1;
            if k % 3 == 0 {
                let d = next() % cap;
                pos = if next() % 4 == 0 {
                    pos + d / 4
                } else {
                    pos.saturating_sub(d)
                };
                s.hint(pos);
            }
            if pos + block > FRAMES - 1_000 {
                pos = 10_000;
                s.hint(pos);
            }
            r.begin_block(pos);
            match r.read(n.media, n.start, n.file_frame, pos, &mut l, &mut rr) {
                Read::Done => {
                    done += 1;
                    for i in 0..block as usize {
                        let f = pos + i as u64;
                        if l[i] != sample(f, 0) || rr[i] != sample(f, 1) {
                            wrong += 1;
                        }
                    }
                }
                _ => miss += 1,
            }
            pos += block;
        }
        (wrong, done, miss)
    }

    /// I1/C1: no wrong sample while locates race the disk thread.
    #[test]
    fn thread_mode_locates_never_read_overwritten_frames() {
        let (wrong, done, miss) = stress(2.0);
        assert_eq!(
            wrong, 0,
            "{wrong} wrong samples ({done} done, {miss} missed)"
        );
        assert!(done > 0, "never read ({miss} missed)");
    }

    /// The 20 s variant: `cargo test --release -p core-media -- --ignored`.
    #[test]
    #[ignore]
    fn thread_mode_locates_never_read_overwritten_frames_long() {
        let (wrong, done, miss) = stress(20.0);
        assert_eq!(
            wrong, 0,
            "{wrong} wrong samples ({done} done, {miss} missed)"
        );
        assert!(done > 0, "never read ({miss} missed)");
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 24, .. ProptestConfig::default() })]

        /// Any layout, any playhead path (forward steps, jumps), any
        /// service pattern: a read is exact or misses, never garbage.
        #[test]
        fn reads_are_exact_or_miss(
            segs in prop::collection::vec((0u64..50_000, 1u64..40_000, 0u64..60_000), 1..4),
            steps in prop::collection::vec((0u8..10, 1u64..2048), 1..60),
            looped in any::<bool>(),
        ) {
            let dir = tempfile::tempdir().unwrap();
            let p = file(dir.path(), "a.caf", 100_000, 2);
            let (s, r) = manual(small());
            s.set_media(vec![MediaFile { id: MediaId(0), path: p }]);
            let needs: Vec<Need> = segs
                .iter()
                .map(|&(start, frames, ff)| need(0, start, frames.min(100_000 - ff.min(99_999)), ff.min(99_999)))
                .collect();
            s.set_plan(Arc::from(needs.clone()));
            if looped {
                s.set_loop(Some((5_000, 45_000)));
            }
            let mut pos = 0u64;
            for (what, n) in steps {
                match what {
                    0 => pos = n * 37 % 90_000,
                    1 => {}
                    2 => s.hint(n * 53 % 90_000),
                    _ => s.service(),
                }
                r.begin_block(pos);
                for nd in &needs {
                    if pos >= nd.start && pos < nd.start + nd.frames {
                        let f = nd.file_frame + (pos - nd.start);
                        let k = (n as usize).min((nd.start + nd.frames - pos) as usize);
                        let got = read(&r, nd, f, k, true);
                        prop_assert!(got != Read::Offline);
                    }
                }
                pos += n;
            }
        }
    }
}
