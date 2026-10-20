//! Recording (sub-project 5 spec §6.2; plan 5c Rulings 3–5): the input
//! ring the real-time thread fills and the writer thread that drains it
//! into one CAF per armed audio lane.
//!
//! **The ring** holds records of whole blocks: a four-word header
//! (frames, the block's run frame as two words, channels) and the block's
//! samples interleaved by ring channel, as `f32` bits in `AtomicU32`s. The
//! producer ([`RingWriter`], real-time) writes a record only when all of
//! it fits and publishes it with one `Release` store of the head; the
//! consumer ([`RingReader`]) frees it with one `Release` store of the
//! tail. A block that does not fit is lost; the next record's run frame
//! shows the gap, and the writer fills it with silence.
//!
//! **The writer** ([`RecordWriter`]) runs on its own thread: it writes
//! each lane's channels (one, or a pair) to its own recording-mode CAF
//! (data size −1 until finished, so a crash leaves a readable file),
//! syncs the files every [`SYNC_EVERY`], and pads gaps and the end with
//! silence so file frame `k` is always run frame `k`.

use crate::MediaError;
use crate::caf::{CafWriter, WriteMode, Written};
use core_model::format::BitDepth;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Words in a record header.
pub const HEADER: usize = 4;
/// How long the ring holds (spec §6.2: 4 s).
pub const RING_SECONDS: f64 = 4.0;
/// How often the writer syncs its files (spec §6.2: 2 s).
pub const SYNC_EVERY: Duration = Duration::from_secs(2);
/// The most channels one ring carries.
pub const MAX_RING_CHANNELS: usize = 64;

#[derive(Debug)]
struct Ring {
    buf: Box<[AtomicU32]>,
    channels: usize,
    head: AtomicUsize,
    tail: AtomicUsize,
    closed: AtomicBool,
    end_run: AtomicU64,
}

impl Ring {
    fn word(&self, at: usize) -> Option<&AtomicU32> {
        self.buf.get(at % self.buf.len().max(1))
    }
}

/// The real-time side of an input ring. Dropping it closes the ring.
#[derive(Debug)]
pub struct RingWriter {
    ring: Arc<Ring>,
    head: usize,
}

/// The writer thread's side of an input ring.
#[derive(Debug)]
pub struct RingReader {
    ring: Arc<Ring>,
    tail: usize,
}

/// A ring for `channels` channels (1–[`MAX_RING_CHANNELS`]) holding
/// `frames` frames of samples plus the headers of blocks of 8 frames or
/// more. Allocates; call off the real-time thread.
pub fn input_ring(channels: usize, frames: usize) -> (RingWriter, RingReader) {
    let channels = channels.clamp(1, MAX_RING_CHANNELS);
    let words = frames
        .saturating_mul(channels)
        .saturating_add((frames / 8 + 16).saturating_mul(HEADER))
        .max(HEADER + channels);
    let ring = Arc::new(Ring {
        buf: (0..words).map(|_| AtomicU32::new(0)).collect(),
        channels,
        head: AtomicUsize::new(0),
        tail: AtomicUsize::new(0),
        closed: AtomicBool::new(false),
        end_run: AtomicU64::new(0),
    });
    (
        RingWriter {
            ring: Arc::clone(&ring),
            head: 0,
        },
        RingReader { ring, tail: 0 },
    )
}

impl RingWriter {
    /// The ring's channels.
    pub fn channels(&self) -> usize {
        self.ring.channels
    }

    /// Writes one block of `frames` frames stamped `run`, each sample from
    /// `sample(ring channel, frame)`. False when it does not fit (the
    /// block is lost). // RT
    pub fn push(&mut self, run: u64, frames: u32, sample: impl Fn(usize, usize) -> f32) -> bool {
        let ch = self.ring.channels;
        let n = frames as usize;
        let need = HEADER + n.saturating_mul(ch);
        let tail = self.ring.tail.load(Ordering::Acquire);
        let used = self.head.wrapping_sub(tail);
        if need > self.ring.buf.len().saturating_sub(used) {
            return false;
        }
        let header = [frames, run as u32, (run >> 32) as u32, ch as u32];
        for (k, w) in header.iter().enumerate() {
            if let Some(cell) = self.ring.word(self.head.wrapping_add(k)) {
                cell.store(*w, Ordering::Relaxed);
            }
        }
        let mut at = self.head.wrapping_add(HEADER);
        for f in 0..n {
            for c in 0..ch {
                if let Some(cell) = self.ring.word(at) {
                    cell.store(sample(c, f).to_bits(), Ordering::Relaxed);
                }
                at = at.wrapping_add(1);
            }
        }
        self.head = self.head.wrapping_add(need);
        self.ring.head.store(self.head, Ordering::Release);
        true
    }

    /// Records that run frames `[0, run)` were captured or lost: the
    /// writer pads to it at the end. // RT
    pub fn set_end(&self, run: u64) {
        self.ring.end_run.store(run, Ordering::Release);
    }

    /// No more records will come. // RT
    pub fn close(&self) {
        self.ring.closed.store(true, Ordering::Release);
    }
}

impl Drop for RingWriter {
    fn drop(&mut self) {
        self.close();
    }
}

/// One record taken from the ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    /// The run frame of its first frame.
    pub run: u64,
    /// Its frames.
    pub frames: u32,
}

impl RingReader {
    /// The ring's channels.
    pub fn channels(&self) -> usize {
        self.ring.channels
    }

    /// The next record: its samples (interleaved by ring channel) replace
    /// `out`'s contents.
    pub fn pop(&mut self, out: &mut Vec<f32>) -> Option<Block> {
        let head = self.ring.head.load(Ordering::Acquire);
        if head.wrapping_sub(self.tail) < HEADER {
            return None;
        }
        let w = |k: usize| {
            self.ring
                .word(self.tail.wrapping_add(k))
                .map_or(0, |c| c.load(Ordering::Relaxed))
        };
        let frames = w(0);
        let run = u64::from(w(1)) | (u64::from(w(2)) << 32);
        let ch = (w(3) as usize).max(1);
        let n = (frames as usize).saturating_mul(ch);
        out.clear();
        out.extend((0..n).map(|k| f32::from_bits(w(HEADER + k))));
        self.tail = self.tail.wrapping_add(HEADER + n);
        self.ring.tail.store(self.tail, Ordering::Release);
        Some(Block { run, frames })
    }

    /// A record is waiting.
    pub fn has_records(&self) -> bool {
        self.ring
            .head
            .load(Ordering::Acquire)
            .wrapping_sub(self.tail)
            >= HEADER
    }

    /// The producer closed the ring (or was dropped).
    pub fn closed(&self) -> bool {
        self.ring.closed.load(Ordering::Acquire)
    }

    /// Run frames captured or lost so far ([`RingWriter::set_end`]).
    pub fn end_run(&self) -> u64 {
        self.ring.end_run.load(Ordering::Acquire)
    }
}

/// One armed audio lane's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterLane {
    /// The CAF to create (it must not exist).
    pub path: PathBuf,
    /// Its ring channels: one (mono) or two (left, right).
    pub channels: Vec<usize>,
}

/// What a [`RecordWriter`] writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterConfig {
    /// The project rate.
    pub rate: u32,
    /// The project depth.
    pub depth: BitDepth,
    /// Each lane's file.
    pub lanes: Vec<WriterLane>,
}

/// A finished lane file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneWritten {
    /// Its path.
    pub path: PathBuf,
    /// What was written.
    pub written: Written,
}

/// A lane file that could not be finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedLane {
    /// Its path.
    pub path: PathBuf,
    /// Why.
    pub error: String,
    /// What landed before the failure, when the file could still be
    /// closed: its whole frames and the hash of exactly those bytes.
    pub written: Option<Written>,
}

/// What a finished writer wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterResult {
    /// Each lane's file, in [`WriterConfig::lanes`] order (a lane whose
    /// file failed is missing; see `error`).
    pub lanes: Vec<LaneWritten>,
    /// Lanes whose file could not be finished: the file is still on disk
    /// (recording-mode or short) and must be kept and reported.
    pub failed: Vec<FailedLane>,
    /// Frames written as silence because their blocks were lost.
    pub gap_frames: u64,
    /// Why writing stopped early (a full disk), if it did.
    pub error: Option<String>,
}

/// What a running writer has done so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriterStatus {
    /// Frames written per file (gaps included).
    pub frames: u64,
    /// Frames written as silence for lost blocks.
    pub gap_frames: u64,
    /// Samples clipped per lane (integer depths).
    pub clipped: Vec<u64>,
    /// Why writing stopped, if it did.
    pub error: Option<String>,
}

#[derive(Debug)]
struct Shared {
    /// Stop once the ring is drained, or at this instant at the latest.
    stop_by: Mutex<Option<Instant>>,
    frames: AtomicU64,
    gaps: AtomicU64,
    clipped: Box<[AtomicU64]>,
    error: Mutex<Option<String>>,
}

/// The writer thread (spec §6.2).
#[derive(Debug)]
pub struct RecordWriter {
    thread: Option<JoinHandle<WriterResult>>,
    shared: Arc<Shared>,
    paths: Vec<PathBuf>,
}

fn made_paths(paths: &[PathBuf]) {
    for p in paths {
        let _ = std::fs::remove_file(p);
    }
}

/// The lock of a mutex even if a panic poisoned it.
fn guard<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Lane {
    writer: CafWriter,
    path: PathBuf,
    channels: Vec<usize>,
}

impl RecordWriter {
    /// Creates every lane's file (parent folders too; never overwriting)
    /// and starts the thread. When a file cannot be created, the files
    /// made so far are removed and nothing runs.
    pub fn start(reader: RingReader, cfg: WriterConfig) -> Result<RecordWriter, MediaError> {
        let mut lanes: Vec<Lane> = Vec::new();
        let made = |lanes: &[Lane]| {
            made_paths(&lanes.iter().map(|l| l.path.clone()).collect::<Vec<_>>());
        };
        for l in &cfg.lanes {
            let n = l.channels.len();
            if !(1..=2).contains(&n) || l.channels.iter().any(|c| *c >= reader.channels()) {
                made(&lanes);
                return Err(MediaError::Unsupported(format!(
                    "{}: ring channels {:?}",
                    l.path.display(),
                    l.channels
                )));
            }
            if let Some(dir) = l.path.parent() {
                if let Err(e) = std::fs::create_dir_all(dir) {
                    made(&lanes);
                    return Err(e.into());
                }
            }
            match CafWriter::create(&l.path, cfg.rate, n as u16, cfg.depth, WriteMode::Recording) {
                Ok(writer) => {
                    if let Err(e) = writer.sync() {
                        let _ = std::fs::remove_file(&l.path);
                        made(&lanes);
                        return Err(e);
                    }
                    lanes.push(Lane {
                        writer,
                        path: l.path.clone(),
                        channels: l.channels.clone(),
                    });
                }
                Err(e) => {
                    made(&lanes);
                    return Err(e);
                }
            }
        }
        let shared = Arc::new(Shared {
            stop_by: Mutex::new(None),
            frames: AtomicU64::new(0),
            gaps: AtomicU64::new(0),
            clipped: (0..lanes.len()).map(|_| AtomicU64::new(0)).collect(),
            error: Mutex::new(None),
        });
        let paths: Vec<PathBuf> = lanes.iter().map(|l| l.path.clone()).collect();
        let s = Arc::clone(&shared);
        let thread = std::thread::Builder::new()
            .name("vizdaw-record".into())
            .spawn(move || run(reader, lanes, &s));
        match thread {
            Ok(t) => Ok(RecordWriter {
                thread: Some(t),
                shared,
                paths,
            }),
            Err(e) => {
                made_paths(&paths);
                Err(MediaError::Io(format!("record writer thread: {e}")))
            }
        }
    }

    /// What it has done so far.
    pub fn status(&self) -> WriterStatus {
        WriterStatus {
            frames: self.shared.frames.load(Ordering::Relaxed),
            gap_frames: self.shared.gaps.load(Ordering::Relaxed),
            clipped: self
                .shared
                .clipped
                .iter()
                .map(|c| c.load(Ordering::Relaxed))
                .collect(),
            error: guard(&self.shared.error).clone(),
        }
    }

    /// Finishes: drains the ring until it is closed (or for `wait` at
    /// most), pads to the end, patches every file's size and syncs it.
    pub fn finish(mut self, wait: Duration) -> WriterResult {
        *guard(&self.shared.stop_by) = Some(Instant::now() + wait);
        let r = self.thread.take().map(JoinHandle::join);
        match r {
            Some(Ok(r)) => r,
            _ => WriterResult {
                lanes: Vec::new(),
                failed: self
                    .paths
                    .iter()
                    .map(|p| FailedLane {
                        path: p.clone(),
                        error: "the record writer stopped unexpectedly".into(),
                        written: None,
                    })
                    .collect(),
                gap_frames: self.shared.gaps.load(Ordering::Relaxed),
                error: Some("the record writer stopped unexpectedly".into()),
            },
        }
    }
}

impl Drop for RecordWriter {
    fn drop(&mut self) {
        if let Some(t) = self.thread.take() {
            *guard(&self.shared.stop_by) = Some(Instant::now());
            let _ = t.join();
        }
    }
}

/// Writes `frames` frames of silence to every lane.
fn pad(lanes: &mut [Lane], frames: u64, scratch: &mut Vec<f32>) -> Result<(), MediaError> {
    let mut left = frames;
    while left > 0 {
        let n = left.min(4096) as usize;
        for l in lanes.iter_mut() {
            scratch.clear();
            scratch.resize(n * l.channels.len(), 0.0);
            l.writer.write(scratch)?;
        }
        left -= n as u64;
    }
    Ok(())
}

/// Writes one block (interleaved by ring channel) to every lane.
fn write_block(
    lanes: &mut [Lane],
    block: &[f32],
    ch: usize,
    skip: usize,
    scratch: &mut Vec<f32>,
) -> Result<(), MediaError> {
    let frames = block.len() / ch.max(1);
    for l in lanes.iter_mut() {
        scratch.clear();
        for f in skip..frames {
            for c in &l.channels {
                scratch.push(block.get(f * ch + c).copied().unwrap_or(0.0));
            }
        }
        l.writer.write(scratch)?;
    }
    Ok(())
}

fn run(mut reader: RingReader, mut lanes: Vec<Lane>, s: &Shared) -> WriterResult {
    let ch = reader.channels();
    let mut block = Vec::new();
    let mut scratch = Vec::new();
    let mut next = 0u64;
    let mut gaps = 0u64;
    let mut synced = Instant::now();
    let mut error: Option<String> = None;
    let fail = |e: MediaError, error: &mut Option<String>| {
        let why = e.to_string();
        *guard(&s.error) = Some(why.clone());
        *error = Some(why);
    };
    loop {
        if error.is_some() {
            break;
        }
        match reader.pop(&mut block) {
            Some(b) => {
                let end = b.run.saturating_add(u64::from(b.frames));
                if end <= next {
                    continue;
                }
                if b.run > next {
                    let gap = b.run - next;
                    if let Err(e) = pad(&mut lanes, gap, &mut scratch) {
                        fail(e, &mut error);
                        continue;
                    }
                    gaps += gap;
                    s.gaps.store(gaps, Ordering::Relaxed);
                    next = b.run;
                }
                let skip = (next - b.run) as usize;
                if let Err(e) = write_block(&mut lanes, &block, ch, skip, &mut scratch) {
                    fail(e, &mut error);
                    continue;
                }
                next = end;
                s.frames.store(next, Ordering::Relaxed);
                for (l, c) in lanes.iter().zip(s.clipped.iter()) {
                    c.store(l.writer.clipped(), Ordering::Relaxed);
                }
            }
            None => {
                let stop_by = *guard(&s.stop_by);
                // `closed` is read before the head is read again, so a
                // record pushed before the close is never missed.
                let closed = reader.closed();
                let late = stop_by.is_some_and(|t| Instant::now() >= t);
                if (stop_by.is_some() && closed) || late {
                    if !late && reader.has_records() {
                        continue;
                    }
                    let end = reader.end_run();
                    if end > next {
                        match pad(&mut lanes, end - next, &mut scratch) {
                            Ok(()) => {
                                gaps += end - next;
                                next = end;
                            }
                            Err(e) => fail(e, &mut error),
                        }
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        if synced.elapsed() >= SYNC_EVERY {
            for l in &lanes {
                if let Err(e) = l.writer.sync() {
                    fail(e, &mut error);
                    break;
                }
            }
            synced = Instant::now();
        }
    }
    s.gaps.store(gaps, Ordering::Relaxed);
    s.frames.store(next, Ordering::Relaxed);
    let mut out = Vec::with_capacity(lanes.len());
    let mut failed = Vec::new();
    for l in lanes {
        let path = l.path.clone();
        match l.writer.finish_partial() {
            Ok((written, false)) => out.push(LaneWritten { path, written }),
            Ok((written, true)) => {
                let why = "a write failed (the disk is full?)".to_string();
                if error.is_none() {
                    error = Some(format!("{}: {why}", path.display()));
                }
                failed.push(FailedLane {
                    path,
                    error: why,
                    written: Some(written),
                });
            }
            Err(e) => {
                if error.is_none() {
                    error = Some(format!("{}: {e}", path.display()));
                }
                failed.push(FailedLane {
                    path,
                    error: e.to_string(),
                    written: None,
                });
            }
        }
    }
    WriterResult {
        lanes: out,
        failed,
        gap_frames: gaps,
        error,
    }
}

/// Free bytes on the volume holding `path`, or `None` when the system
/// cannot say.
pub fn free_bytes(path: &Path) -> Option<u64> {
    free_bytes_impl(path)
}

#[cfg(unix)]
fn free_bytes_impl(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `statvfs` only writes the zeroed struct we pass and reads the
    // NUL-terminated path.
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    let r = unsafe { libc::statvfs(c.as_ptr(), &mut st) };
    if r != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)]
    Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64))
}

#[cfg(not(unix))]
fn free_bytes_impl(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lane_whose_file_fails_is_reported_with_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let (mut w, r) = input_ring(2, 4800);
        let mut lanes = Vec::new();
        for n in ["a.caf", "b.caf"] {
            let path = dir.path().join(n);
            let mut writer =
                CafWriter::create(&path, 48_000, 1, BitDepth::Int16, WriteMode::Recording).unwrap();
            if n == "b.caf" {
                writer.write(&[0.5; 40]).unwrap();
                writer.fail_writes_for_test();
            }
            lanes.push(Lane {
                writer,
                path,
                channels: vec![usize::from(n == "b.caf")],
            });
        }
        assert!(w.push(0, 100, |_, _| 0.25));
        w.set_end(100);
        w.close();
        let shared = Shared {
            stop_by: Mutex::new(Some(Instant::now())),
            frames: AtomicU64::new(0),
            gaps: AtomicU64::new(0),
            clipped: (0..2).map(|_| AtomicU64::new(0)).collect(),
            error: Mutex::new(None),
        };
        let res = run(r, lanes, &shared);
        assert!(res.error.is_some());
        assert_eq!(res.lanes.len(), 1);
        assert_eq!(res.lanes[0].path, dir.path().join("a.caf"));
        assert_eq!(res.failed.len(), 1);
        assert_eq!(res.failed[0].path, dir.path().join("b.caf"));
        // The 40 frames that landed before the failure, hashed exactly.
        let w = res.failed[0].written.as_ref().unwrap();
        assert_eq!(w.frames, 40);
        let p = dir.path().join("b.caf");
        let info = crate::caf::CafReader::open(&p).unwrap().info().clone();
        assert_eq!((info.frames, info.size_known), (40, true));
        let raw = std::fs::read(&p).unwrap();
        let data = &raw[info.data_offset as usize..];
        assert_eq!(data.len(), 80);
        assert_eq!(w.content_hash, blake3::hash(data).to_hex().to_string());
    }
}
