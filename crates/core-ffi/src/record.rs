//! A recording in progress (sub-project 5 spec §6.2–§6.5; plan 5c Task
//! 9): what `Session.record` started, what it collects while the
//! transport rolls, the disk and input checks `idle` runs, and finishing
//! it into a [`RecordPass`]. The session holds one in its state; nothing
//! here takes the session lock.

use crate::error::VizError;
use crate::passes::{AudioTake, Captured, PassFate, RecordRange, decide, plan};
use core_engine::record::{
    Mark, MarkReader, MidiReader, RecordHandles, RecordStatus, RecordedMidi,
};
use core_media::record::{RecordWriter, WriterConfig, WriterResult};
use core_model::command::{RecordPass, RecordedMedia};
use core_model::document::{Document, MediaItem, MediaOrigin};
use core_model::format::AudioFormat;
use core_model::ids::{LaneId, SeqId};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime};

/// Where recordings go in a package (spec §6.2).
pub const RECORDED_DIR: &str = "Media/Recorded";
/// Below this, recording warns (spec §6.2: 1 GiB).
pub const LOW_DISK_WARN: u64 = 1 << 30;
/// Below this, recording stops (spec §6.2: 100 MiB).
pub const LOW_DISK_STOP: u64 = 100 << 20;
/// How often free space is checked while recording (spec §6.2: 10 s).
pub const DISK_CHECK_EVERY: Duration = Duration::from_secs(10);
/// How long Stop waits for the last input to be written.
pub const FINISH_WAIT: Duration = Duration::from_secs(2);
/// The largest latency trim, in frames either way (1 s at 192 kHz).
pub const MAX_TRIM: i32 = 192_000;

static TRIM: AtomicI64 = AtomicI64::new(0);
/// A fake free-space reading (test hook; `u64::MAX`: none).
static FAKE_FREE: AtomicU64 = AtomicU64::new(u64::MAX);

/// Sets the recording latency trim (spec §6.3, Settings › Recording ›
/// Latency trim): frames added to where recorded audio lands, for devices
/// that misreport their latency; clamped to ±[`MAX_TRIM`]. Takes effect
/// from the next pass.
#[uniffi::export]
pub fn set_record_latency_trim(frames: i32) {
    TRIM.store(
        i64::from(frames.clamp(-MAX_TRIM, MAX_TRIM)),
        Ordering::Relaxed,
    );
}

/// The latency trim in frames.
pub fn latency_trim() -> i64 {
    TRIM.load(Ordering::Relaxed)
}

/// Makes free-space checks read `bytes` (test hook; `None`: the disk's).
#[cfg(feature = "test-hooks")]
pub fn set_free_bytes(bytes: Option<u64>) {
    FAKE_FREE.store(bytes.unwrap_or(u64::MAX), Ordering::Relaxed);
}

/// Free bytes on the volume of `root` (or of its nearest existing
/// parent).
pub fn free_bytes(root: &Path) -> Option<u64> {
    let fake = FAKE_FREE.load(Ordering::Relaxed);
    if fake != u64::MAX {
        return Some(fake);
    }
    let mut p = Some(root);
    while let Some(d) = p {
        if d.exists() {
            return core_media::record::free_bytes(d);
        }
        p = d.parent();
    }
    None
}

/// What `Session.record` is asked for (spec §6.4 `RecordSpec`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecordSpecRow {
    /// Replace (else overdub).
    pub replace: bool,
    /// The punch range in ticks, when punch is on.
    pub punch: Option<crate::selection::TickRange>,
    /// Loop recording over the transport loop (bar 1 to the end).
    pub looped: bool,
    /// Bars of count-in, 0–4 (refused above 0 until plan 5d plays them).
    pub count_in_bars: u8,
    /// Bars of pre-roll, 0–8.
    pub pre_roll_bars: u8,
    /// Kept loop passes become takes.
    pub passes_as_takes: bool,
}

/// One armed lane while recording.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecordLaneRow {
    /// The lane.
    pub lane: u64,
    /// It records audio (else MIDI).
    pub audio: bool,
    /// Its file (package-relative), for audio.
    pub path: Option<String>,
    /// Samples clipped so far (integer depths).
    pub clipped: u64,
}

/// What a recording is doing (`state.get record`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecordStateRow {
    /// A pass is recording.
    pub recording: bool,
    /// Its sequence.
    pub seq: Option<u64>,
    /// What it was asked for.
    pub spec: Option<RecordSpecRow>,
    /// The pass playing, from 1 (0 before the transport rolls).
    pub pass: u32,
    /// Completed passes kept so far.
    pub kept: u32,
    /// Completed passes discarded so far.
    pub discarded: u32,
    /// The armed lanes.
    pub lanes: Vec<RecordLaneRow>,
    /// Frames recorded so far.
    pub frames: u64,
    /// Input blocks lost because the ring was full (`record_overflows`).
    pub overflows: u64,
    /// Frames written as silence for lost input.
    pub gap_frames: u64,
    /// Free space on the project's volume.
    pub free_bytes: Option<u64>,
}

/// One armed audio lane's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioLane {
    /// The lane.
    pub lane: LaneId,
    /// Package-relative path.
    pub path: String,
    /// Channels in the file (1 or 2).
    pub channels: u16,
}

/// Why a recording must end now (spec §6.2, §12.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The transport stopped by itself (the end without a loop).
    Ended,
    /// Something went wrong; the message says what.
    Because(String),
}

/// A recording in progress.
pub struct Recording {
    /// The sequence.
    pub seq: SeqId,
    /// What was asked.
    pub spec: RecordSpecRow,
    /// The record range in schedule frames.
    pub range: RecordRange,
    /// The armed audio lanes.
    pub audio: Vec<AudioLane>,
    /// The armed MIDI lanes.
    pub midi_lanes: Vec<LaneId>,
    /// The writer (none for MIDI only).
    pub writer: Option<RecordWriter>,
    /// The tap's marks.
    marks_rx: MarkReader,
    /// The tap's recorded MIDI.
    midi_rx: MidiReader,
    /// The tap's counters.
    status: Arc<RecordStatus>,
    /// The package root files are written under.
    pub root: PathBuf,
    /// The project format.
    pub format: AudioFormat,
    /// The pass number media get.
    pub pass_number: u32,
    marks: Vec<Mark>,
    midi: Vec<RecordedMidi>,
    fates: Vec<PassFate>,
    disk_checked: Instant,
    warned_low: bool,
}

impl std::fmt::Debug for Recording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recording")
            .field("seq", &self.seq)
            .field("spec", &self.spec)
            .finish_non_exhaustive()
    }
}

/// What a finished recording captured.
#[derive(Debug)]
pub struct Finished {
    /// The sequence.
    pub seq: SeqId,
    /// What was asked.
    pub spec: RecordSpecRow,
    /// The record range.
    pub range: RecordRange,
    /// The armed audio lanes.
    pub audio: Vec<AudioLane>,
    /// The armed MIDI lanes.
    pub midi_lanes: Vec<LaneId>,
    /// What the writer wrote.
    pub written: Option<WriterResult>,
    /// The marks.
    pub marks: Vec<Mark>,
    /// The recorded MIDI.
    pub midi: Vec<RecordedMidi>,
    /// The passes' fates.
    pub fates: Vec<PassFate>,
    /// The tap's counters.
    pub status: Arc<RecordStatus>,
    /// The project format.
    pub format: AudioFormat,
    /// The pass number.
    pub pass_number: u32,
}

impl Finished {
    /// Every file it wrote (package-relative).
    pub fn files(&self) -> Vec<String> {
        self.audio.iter().map(|a| a.path.clone()).collect()
    }
}

/// What a recording records (everything [`Recording::start`] needs
/// besides the tap).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The sequence.
    pub seq: SeqId,
    /// What was asked.
    pub spec: RecordSpecRow,
    /// The record range in schedule frames.
    pub range: RecordRange,
    /// The armed audio lanes and their files.
    pub audio: Vec<AudioLane>,
    /// The armed MIDI lanes.
    pub midi_lanes: Vec<LaneId>,
    /// The package root.
    pub root: PathBuf,
    /// The project format.
    pub format: AudioFormat,
    /// The pass number media get.
    pub pass_number: u32,
}

impl Recording {
    /// Starts recording what `handles` taps: with `writer` lanes, a writer
    /// thread takes the tap's ring and makes their files (when they cannot
    /// be made nothing starts).
    pub fn start(
        plan: Plan,
        handles: RecordHandles,
        writer: Option<WriterConfig>,
    ) -> Result<Recording, core_media::MediaError> {
        let RecordHandles {
            ring,
            marks,
            midi,
            status,
        } = handles;
        let writer = match writer {
            Some(cfg) => Some(RecordWriter::start(ring, cfg)?),
            // MIDI only: the ring carries nothing.
            None => None,
        };
        Ok(Recording {
            seq: plan.seq,
            spec: plan.spec,
            range: plan.range,
            audio: plan.audio,
            midi_lanes: plan.midi_lanes,
            writer,
            marks_rx: marks,
            midi_rx: midi,
            status,
            root: plan.root,
            format: plan.format,
            pass_number: plan.pass_number,
            marks: Vec::new(),
            midi: Vec::new(),
            fates: Vec::new(),
            disk_checked: Instant::now(),
            warned_low: false,
        })
    }

    /// Takes what the tap published since the last call.
    pub fn drain(&mut self) {
        while let Ok(m) = self.marks_rx.pop() {
            self.marks.push(m);
        }
        while let Ok(m) = self.midi_rx.pop() {
            self.midi.push(m);
        }
    }

    /// Passes started so far (the roll and every wrap).
    pub fn passes(&self) -> usize {
        if self.marks.is_empty() {
            return 0;
        }
        1 + self.marks.iter().skip(1).filter(|m| m.wrap).count()
    }

    /// `record.keep_pass` (`keep`) or `record.discard_pass`: acts on the
    /// passes completed so far (plan 5c Ruling 13). `Model` unless loop
    /// recording.
    pub fn press(&mut self, keep: bool) -> Result<(), VizError> {
        if !self.spec.looped {
            return Err(crate::events::bad(
                "keep and discard act on loop passes; this recording does not loop",
            ));
        }
        self.drain();
        let completed = self.passes().saturating_sub(1);
        decide(&mut self.fates, completed, keep);
        Ok(())
    }

    /// The files it writes (package-relative).
    pub fn files(&self) -> Vec<String> {
        self.audio.iter().map(|a| a.path.clone()).collect()
    }

    /// Whether it must end now: the tap closed under it (the device went
    /// away), the writer failed (a full disk), free space fell below
    /// [`LOW_DISK_STOP`], or the transport stopped by itself. Pushes a
    /// warning once below [`LOW_DISK_WARN`].
    pub fn check(&mut self, transport_stopped: bool, problems: &mut Vec<String>) -> Option<Ending> {
        self.drain();
        if self.status.closed() {
            return Some(Ending::Because(
                "the audio device went away; what was recorded is kept".into(),
            ));
        }
        if let Some(e) = self.writer.as_ref().and_then(|w| w.status().error) {
            return Some(Ending::Because(format!(
                "the recording could not be written ({e})"
            )));
        }
        // A faked reading (tests) is looked at every time.
        let faked = FAKE_FREE.load(Ordering::Relaxed) != u64::MAX;
        if faked || self.disk_checked.elapsed() >= DISK_CHECK_EVERY {
            self.disk_checked = Instant::now();
            if let Some(free) = free_bytes(&self.root) {
                if free < LOW_DISK_STOP {
                    return Some(Ending::Because("the disk is almost full".into()));
                }
                if free < LOW_DISK_WARN && !self.warned_low {
                    self.warned_low = true;
                    problems.push("Recording: less than 1 GB of disk space is left.".into());
                }
            }
        }
        if transport_stopped && self.status.run() > 0 {
            return Some(Ending::Ended);
        }
        None
    }

    /// `state.get record` while it runs.
    pub fn row(&self) -> RecordStateRow {
        let w = self.writer.as_ref().map(RecordWriter::status);
        let completed = self.passes().saturating_sub(1);
        let count = |f: PassFate| {
            self.fates
                .iter()
                .take(completed)
                .filter(|x| **x == f)
                .count() as u32
        };
        let mut lanes: Vec<RecordLaneRow> = self
            .audio
            .iter()
            .enumerate()
            .map(|(k, a)| RecordLaneRow {
                lane: a.lane.0,
                audio: true,
                path: Some(a.path.clone()),
                clipped: w
                    .as_ref()
                    .and_then(|w| w.clipped.get(k).copied())
                    .unwrap_or(0),
            })
            .collect();
        lanes.extend(self.midi_lanes.iter().map(|l| RecordLaneRow {
            lane: l.0,
            audio: false,
            path: None,
            clipped: 0,
        }));
        RecordStateRow {
            recording: true,
            seq: Some(self.seq.0),
            spec: Some(self.spec.clone()),
            pass: self.passes() as u32,
            kept: count(PassFate::Kept),
            discarded: count(PassFate::Discarded),
            lanes,
            frames: self.status.run(),
            overflows: self.status.lost_blocks(),
            gap_frames: w.map_or(0, |w| w.gap_frames),
            free_bytes: free_bytes(&self.root),
        }
    }

    /// Ends it (the engine was told to stop and drop the tap): waits up
    /// to `wait` for the tap to close, takes the last marks and MIDI and
    /// lets the writer finish its files. Blocks: call without the session
    /// lock.
    pub fn finish(mut self, wait: Duration) -> Finished {
        let until = Instant::now() + wait;
        while !self.status.closed() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(2));
        }
        self.drain();
        let left = until.saturating_duration_since(Instant::now());
        let written = self
            .writer
            .take()
            .map(|w| w.finish(left.max(Duration::from_millis(100))));
        Finished {
            seq: self.seq,
            spec: self.spec,
            range: self.range,
            audio: self.audio,
            midi_lanes: self.midi_lanes,
            written,
            marks: self.marks,
            midi: self.midi,
            fates: self.fates,
            status: Arc::clone(&self.status),
            format: self.format,
            pass_number: self.pass_number,
        }
    }
}

/// The pass a finished recording makes in `doc`, or `None` when nothing
/// landed in the record range. Lanes deleted meanwhile are left out.
pub fn build(f: &Finished, doc: &Document) -> Result<Option<RecordPass>, String> {
    let lanes_alive = |l: &LaneId| doc.lane(f.seq, *l).is_ok();
    let mut audio = Vec::new();
    if let Some(w) = &f.written {
        for a in f.audio.iter().filter(|a| lanes_alive(&a.lane)) {
            let Some(lw) = w.lanes.iter().find(|l| l.path.ends_with(&a.path)) else {
                continue;
            };
            if lw.written.frames == 0 {
                continue;
            }
            let (_, stem) = MediaItem::file_parts(&a.path);
            audio.push(AudioTake {
                lane: a.lane,
                media: RecordedMedia {
                    path: a.path.clone(),
                    sample_rate: f.format.sample_rate,
                    channels: a.channels,
                    length_frames: lw.written.frames,
                    content_hash: lw.written.content_hash.clone(),
                    name: stem,
                    pass: f.pass_number,
                },
            });
        }
    }
    let midi_lanes: Vec<LaneId> = f.midi_lanes.iter().copied().filter(lanes_alive).collect();
    let gaps = f.written.as_ref().is_some_and(|w| w.gap_frames > 0)
        || f.status.lost_blocks() > 0
        || f.status.lost_midi() > 0
        || f.status.lost_marks() > 0;
    let delay = f.status.delay().unwrap_or(0) - latency_trim();
    plan(&Captured {
        doc,
        seq: f.seq,
        marks: &f.marks,
        end_run: f.status.run(),
        latency: f.status.latency,
        input_delay: delay,
        range: f.range,
        looped: f.spec.looped,
        replace: f.spec.replace,
        passes_as_takes: f.spec.passes_as_takes,
        fates: &f.fates,
        audio: &audio,
        midi_lanes: &midi_lanes,
        midi: &f.midi,
        with_gaps: gaps,
    })
}

/// The pass number a new recording's media get: one more than any
/// recorded item of `doc` has.
pub fn next_pass(doc: &Document) -> u32 {
    doc.media()
        .values()
        .filter_map(|m| match m.origin {
            MediaOrigin::Recorded { pass } => Some(pass),
            _ => None,
        })
        .max()
        .map_or(1, |p| p.saturating_add(1))
}

/// `name` as a file-name part: no path or control characters, no leading
/// dot, at most 100 characters; "Lane" when nothing is left.
pub fn file_safe(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .take(100)
        .collect();
    let s = s.trim().trim_start_matches('.').trim().to_string();
    if s.is_empty() { "Lane".into() } else { s }
}

/// `yyyy-mm-dd HHmmss` of `t` in UTC (plan 5c Ruling 14).
pub fn stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let rest = secs % 86_400;
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}{:02}{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// A free package-relative path for lane `name`'s recording under
/// `Media/Recorded/` in `root`: `<name> <stamp>.caf`, then `… 2.caf`,
/// `… 3.caf`, never a file that exists or a path in `reserved` (compared
/// without case, as macOS volumes resolve names).
pub fn recorded_path(
    root: &Path,
    name: &str,
    at: SystemTime,
    reserved: &BTreeSet<String>,
) -> String {
    let base = format!("{} {}", file_safe(name), stamp(at));
    let taken =
        |rel: &str| root.join(rel).exists() || reserved.iter().any(|r| r.eq_ignore_ascii_case(rel));
    let mut rel = format!("{RECORDED_DIR}/{base}.caf");
    let mut k = 2;
    while taken(&rel) {
        rel = format!("{RECORDED_DIR}/{base} {k}.caf");
        k += 1;
    }
    rel
}
