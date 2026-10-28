//! A recording in progress (sub-project 5 spec §6.2–§6.5; plan 5c Task
//! 9): what `Session.record` started, what it collects while the
//! transport rolls, the disk and input checks `idle` runs, and finishing
//! it into a [`RecordPass`]. The session holds one in its state; nothing
//! here takes the session lock.

use crate::error::VizError;
use crate::guard::lock;
use crate::passes::{AudioTake, Captured, PassFate, RecordRange, decide, plan};
use core_engine::record::{
    Mark, MarkReader, MidiReader, RecordHandles, RecordStatus, RecordedMidi,
};
use core_media::record::{LivePeaks, RING_SECONDS, RecordWriter, WriterConfig, WriterResult};
use core_model::command::{RecordPass, RecordedMedia};
use core_model::document::{Document, MediaItem, MediaOrigin};
use core_model::format::AudioFormat;
use core_model::ids::{LaneId, SeqId};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
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

/// Holds [`Recording::finish`] before the writer finishes (test hook,
/// pre-flight I1).
static HOLD_FINISH: AtomicBool = AtomicBool::new(false);
/// A ring length in frames instead of [`RING_SECONDS`] of input (test
/// hook, pre-flight I4; 0: none).
static RING_FRAMES: AtomicUsize = AtomicUsize::new(0);

/// Holds (`true`) or releases the finishing of every pass (test hook,
/// pre-flight I1): Stop then waits with the session unlocked.
#[cfg(feature = "test-hooks")]
pub fn set_hold_finish(on: bool) {
    HOLD_FINISH.store(on, Ordering::Relaxed);
}

/// Makes input rings `frames` long (test hook, pre-flight I4: a ring
/// shorter than a block loses every block; `None`: the default).
#[cfg(feature = "test-hooks")]
pub fn set_ring_frames(frames: Option<usize>) {
    RING_FRAMES.store(frames.unwrap_or(0), Ordering::Relaxed);
}

/// The input ring's length in frames at `rate` ([`RING_SECONDS`]).
pub fn ring_frames(rate: f64) -> usize {
    match RING_FRAMES.load(Ordering::Relaxed) {
        0 => (RING_SECONDS * rate) as usize,
        n => n,
    }
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
    /// Bars of count-in, 0–4 (plan 5d-1 Ruling 7: clicks before the roll).
    pub count_in_bars: u8,
    /// Bars of pre-roll, 0–8.
    pub pre_roll_bars: u8,
    /// Kept loop passes become takes.
    pub passes_as_takes: bool,
}

/// One armed lane while recording.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct RecordLaneRow {
    /// The lane.
    pub lane: u64,
    /// It records audio (else MIDI).
    pub audio: bool,
    /// Its file (package-relative), for audio.
    pub path: Option<String>,
    /// Samples clipped so far (integer depths).
    pub clipped: u64,
    /// The largest absolute sample written so far (audio; plan 5d-1
    /// Ruling 21).
    #[uniffi(default)]
    pub peak: f32,
    /// How much earlier its take lands: a synth return lane's return
    /// latency, in frames (0 otherwise). A view draws this lane's growing
    /// take at file frame `pass_start_file_frame + return_frames` for the
    /// pass's start tick (Task 10 review I1).
    #[uniffi(default)]
    pub return_frames: i64,
}

/// What a recording is doing (`state.get record`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
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
    /// Stream underruns while recording (pre-flight M11).
    #[uniffi(default)]
    pub underruns: u64,
    /// Late stream starts while recording (pre-flight M11).
    #[uniffi(default)]
    pub late_starts: u64,
    /// Recordings found after a crash and not yet imported or moved to
    /// the Trash (package-relative; plan 5c Task 10).
    #[uniffi(default)]
    pub recovered: Vec<String>,
    /// The same files with what each holds (pre-flight M9).
    #[uniffi(default)]
    pub recovered_files: Vec<RecoveredFileRow>,
    /// Where the pass recording now started, as a tick of the sequence
    /// (plan 5d-1 Ruling 21; `None` before the roll): with
    /// `pass_start_file_frame`, where a view draws the growing take.
    #[uniffi(default)]
    pub pass_start_tick: Option<i64>,
    /// The file frame heard at `pass_start_tick`: where Stop will place
    /// it, run frame + graph latency + the input delay less the trim (one
    /// file holds every loop pass); a synth return lane adds its
    /// `return_frames`. Signed and unclamped: below zero the file starts
    /// that many frames after the pass's tick, as Stop places it, so a
    /// view maps file frame f to the tick at `f - anchor - return_frames`
    /// frames from `pass_start_tick`. Resyncs after a Locate are not followed (Task 10
    /// review M3): the anchor is the last wrap, else the roll.
    #[uniffi(default)]
    pub pass_start_file_frame: i64,
    /// Where the record range starts, a tick of the sequence: regions
    /// cover only the range, so a view draws nothing before it.
    #[uniffi(default)]
    pub range_start_tick: i64,
}

/// A recording offered for recovery (plan 5c Task 10; pre-flight M9).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct RecoveredFileRow {
    /// Package-relative path, under `Media/Recorded/`.
    pub path: String,
    /// Whole frames it holds (0 when unreadable).
    pub frames: u64,
    /// Its size was written when it was finished: a take that was
    /// finished cleanly and that nothing names (undone, then saved; or
    /// kept after a failed pass). False: a crash left it unfinished.
    pub size_known: bool,
    /// It reads as a VizDAW CAF (an unreadable kept file is still
    /// offered: recovery never deletes a recording on its own).
    pub readable: bool,
}

/// One armed audio lane's file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioLane {
    /// The lane.
    pub lane: LaneId,
    /// Its name when recording started (for messages).
    pub name: String,
    /// Package-relative path.
    pub path: String,
    /// Channels in the file (1 or 2).
    pub channels: u16,
    /// How much earlier its take lands: the return latency of the synth
    /// it records, in frames (spec §6.7; plan 5c-2 Ruling 4), read at
    /// Record.
    pub return_frames: i64,
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
    /// The session recording.
    pub owner: u64,
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
    /// The sequence's tempo, in schedule frames.
    tempo: core_engine::schedule::TempoTable,
    marks: Vec<Mark>,
    midi: Vec<RecordedMidi>,
    fates: Vec<PassFate>,
    disk_checked: Instant,
    warned_low: bool,
    /// The streamer's `(underruns, late starts)` when it started.
    stream_base: (u64, u64),
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

    /// What landed in `a`'s file: all of it, or what the writer wrote
    /// before the file failed; `None` when it could not be closed.
    pub fn written_of(&self, a: &AudioLane) -> Option<&core_media::caf::Written> {
        let w = self.written.as_ref()?;
        if let Some(l) = w.lanes.iter().find(|l| l.path.ends_with(&a.path)) {
            return Some(&l.written);
        }
        w.failed
            .iter()
            .find(|l| l.path.ends_with(&a.path))
            .and_then(|l| l.written.as_ref())
    }

    /// Why `a`'s file could not be finished, if it could not.
    fn failure_of(&self, a: &AudioLane) -> Option<String> {
        let Some(w) = self.written.as_ref() else {
            return Some("the record writer did not run".into());
        };
        if w.lanes.iter().any(|l| l.path.ends_with(&a.path)) {
            return None;
        }
        Some(
            w.failed
                .iter()
                .find(|l| l.path.ends_with(&a.path))
                .map_or_else(|| "the record writer lost it".into(), |l| l.error.clone()),
        )
    }
}

/// Transport fault name: input was lost because the ring was full
/// (pre-flight M11; spec §12.2 `record_overflow`).
pub const RECORD_OVERFLOW: &str = "record-overflow";
/// Transport fault name: the input went away while recording
/// (pre-flight M11; spec §12.2 `input_lost`).
pub const INPUT_LOST: &str = "input-lost";

/// What a finished pass leaves besides what it placed (pre-flight I3).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Leftovers {
    /// Files no region uses that must stay for the recovery offer: their
    /// lane was removed, or the writer could not close them.
    pub keep: Vec<String>,
    /// What to tell the user, one line per lane.
    pub problems: Vec<String>,
}

/// The files of `f` that `pass` (what [`build`] made of it in `doc`)
/// leaves without a region, and the lines saying why (pre-flight I3). A
/// file whose lane is gone, or that the writer could not close, is kept
/// for the recovery offer; one that only recorded nothing in the record
/// range (a punch never reached, discarded passes) may go. A file the
/// writer failed on is always reported.
pub fn leftovers(f: &Finished, doc: &Document, pass: Option<&RecordPass>) -> Leftovers {
    let mut out = Leftovers::default();
    for a in &f.audio {
        let placed = pass.is_some_and(|p| p.media.iter().any(|m| m.path == a.path));
        let failure = f.failure_of(a);
        let written = f.written_of(a).map_or(0, |w| w.frames);
        if placed {
            if let Some(why) = failure {
                out.problems.push(format!(
                    "{}: the recording was cut short ({why}); what was written before is in the take.",
                    a.name
                ));
            }
            continue;
        }
        if doc.lane(f.seq, a.lane).is_err() {
            if written > 0 || failure.is_some() {
                out.keep.push(a.path.clone());
                out.problems.push(format!(
                    "{}: the recording could not be placed: its lane was removed. It is kept and offered at the next open.",
                    a.name
                ));
            }
            continue;
        }
        match (failure, f.written_of(a).is_some()) {
            // Nothing could be read back: kept as it is on disk.
            (Some(why), false) => {
                out.keep.push(a.path.clone());
                out.problems.push(format!(
                    "{}: the recording could not be finished ({why}). It is kept and offered at the next open.",
                    a.name
                ));
            }
            (Some(why), true) => out.problems.push(format!(
                "{}: the recording could not be finished ({why}); nothing of it fell in the record range.",
                a.name
            )),
            (None, _) => {}
        }
    }
    out
}

/// The line telling the user input was lost (pre-flight I4), when it
/// was: the seconds written as silence, or the MIDI or positions lost.
pub fn gaps_problem(f: &Finished) -> Option<String> {
    let gap = f.written.as_ref().map_or(0, |w| w.gap_frames);
    if gap > 0 || f.status.lost_blocks() > 0 {
        let secs = gap as f64 / f64::from(f.format.sample_rate.max(1));
        return Some(format!(
            "Recording: {secs:.1} s of input could not be written in time and is silence."
        ));
    }
    if f.status.lost_midi() > 0 {
        return Some(format!(
            "Recording: {} MIDI messages could not be recorded in time.",
            f.status.lost_midi()
        ));
    }
    if f.status.lost_marks() > 0 {
        return Some(
            "Recording: the transport moved faster than it could be followed; the take may be placed inexactly."
                .into(),
        );
    }
    None
}

/// The start of the fault the host reports when macOS refused audio
/// input (Task 11's `DENIED_MESSAGE`).
pub const DENIED_PREFIX: &str = "VizDAW may not use audio input";

/// The start of the fault the host reports when the audio input failed
/// to open (the device was busy, or refused the rate).
pub const INPUT_FAILED_PREFIX: &str = "audio input could not open";

/// Why lane `name` cannot record input `channels` from an input with
/// `inputs` channels, if it cannot (pre-flight M6: with no input because
/// access was denied or the input failed to open, the host's fault says
/// why).
pub fn channel_refusal(
    name: &str,
    channels: core_model::lane::InputChannels,
    inputs: u32,
    faults: &[String],
) -> Option<String> {
    if channels.last().is_some_and(|c| u32::from(c) < inputs) {
        return None;
    }
    if inputs == 0 {
        // Why there is no input: a denial first (it is why an open
        // fails), then a failed open (final review M4).
        for prefix in [DENIED_PREFIX, INPUT_FAILED_PREFIX] {
            if let Some(f) = faults.iter().find(|f| f.starts_with(prefix)) {
                return Some(f.clone());
            }
        }
    }
    Some(format!(
        "lane {name} records input {}, but the audio input has {inputs} channel(s)",
        u32::from(channels.first()) + 1
    ))
}

/// What a recording records (everything [`Recording::start`] needs
/// besides the tap).
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    /// The session recording.
    pub owner: u64,
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
    /// The sequence's tempo, in schedule frames (for the rows' ticks).
    pub tempo: core_engine::schedule::TempoTable,
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
            stream_base: crate::host::stream_counters(plan.owner).unwrap_or((0, 0)),
            owner: plan.owner,
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
            tempo: plan.tempo,
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
    /// away, or another window played), the writer failed (a full disk),
    /// free space fell below [`LOW_DISK_STOP`], or the transport stopped
    /// by itself. Pushes a warning once below [`LOW_DISK_WARN`], and the
    /// transport fault names (pre-flight M11) of what happened to
    /// `faults`: `record-overflow` when input was lost, `input-lost`
    /// when the input went away.
    pub fn check(
        &mut self,
        transport_stopped: bool,
        problems: &mut Vec<String>,
        faults: &mut Vec<String>,
    ) -> Option<Ending> {
        self.drain();
        let gaps = self
            .writer
            .as_ref()
            .is_some_and(|w| w.status().gap_frames > 0);
        if self.status.lost_blocks() > 0 || gaps {
            faults.push(RECORD_OVERFLOW.into());
        }
        if self.status.closed() {
            // Pre-flight M5: another session's Play removed the tap.
            if crate::host::holder().is_some_and(|o| o != self.owner) {
                return Some(Ending::Because(
                    "another window started playing; what was recorded is kept".into(),
                ));
            }
            faults.push(INPUT_LOST.into());
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
        // Since it started (pre-flight M11); 0 once the engine is gone.
        let stream = crate::host::stream_counters(self.owner).map_or((0, 0), |(u, l)| {
            (
                u.saturating_sub(self.stream_base.0),
                l.saturating_sub(self.stream_base.1),
            )
        });
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
                peak: self.peaks(k).map_or(0.0, |p| lock(&p).peak()),
                return_frames: a.return_frames,
            })
            .collect();
        lanes.extend(self.midi_lanes.iter().map(|l| RecordLaneRow {
            lane: l.0,
            audio: false,
            path: None,
            clipped: 0,
            peak: 0.0,
            return_frames: 0,
        }));
        // The pass recording now: from the last wrap, else the roll.
        let pass = self
            .marks
            .iter()
            .rev()
            .find(|m| m.wrap)
            .or_else(|| self.marks.first());
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
            underruns: stream.0,
            late_starts: stream.1,
            recovered: Vec::new(),
            recovered_files: Vec::new(),
            pass_start_tick: pass.map(|m| self.tempo.frame_to_tick(m.frame).0),
            // File frame k is placed at S(k - D - L) at Stop (passes.rs), so
            // the file frame at the pass's start tick is run + L + D (a
            // lane adds its `return_frames`), negative when the file
            // starts later than the pass (a trim past the delay).
            pass_start_file_frame: pass.map_or(0i64, |m| {
                let d = self.status.delay().unwrap_or(0) - latency_trim();
                i64::try_from(m.run)
                    .unwrap_or(i64::MAX)
                    .saturating_add(i64::try_from(m.latency).unwrap_or(0))
                    .saturating_add(d)
            }),
            range_start_tick: self.tempo.frame_to_tick(self.range.start).0,
        }
    }

    /// Audio lane `k`'s live peaks (plan 5d-1 Ruling 21).
    fn peaks(&self, k: usize) -> Option<Arc<std::sync::Mutex<LivePeaks>>> {
        self.writer.as_ref().and_then(|w| w.peaks(k))
    }

    /// `columns` columns of lane `lane`'s take so far over file frames
    /// `[from, to)` (plan 5d-1 Ruling 21): what was written, `Ready`;
    /// `None` when the lane records no audio here.
    pub fn waveform(
        &self,
        lane: LaneId,
        from: u64,
        to: u64,
        columns: u32,
    ) -> Option<crate::overviews::OverviewRows> {
        use crate::overviews::{OverviewRows, OverviewStateRow};
        let k = self.audio.iter().position(|a| a.lane == lane)?;
        let peaks = self.peaks(k)?;
        // Build the columns under the lock, from `range`: no clone of the
        // take so far, and the writer is held only for this loop.
        let p = lock(&*peaks);
        let channels = u16::try_from(p.channels()).unwrap_or(1);
        let mut out = OverviewRows::silent(OverviewStateRow::Ready, channels, columns);
        let n = out.columns as usize;
        if n == 0 {
            return Some(out);
        }
        // The columns divide the requested span; those past what was
        // written stay silent (saturating, for hostile frames).
        let written = p.frames();
        let w = to.saturating_sub(from) as f64 / n as f64;
        for c in 0..n {
            let a = from.saturating_add((c as f64 * w).floor() as u64);
            if a >= written {
                break;
            }
            let b = from
                .saturating_add(((c + 1) as f64 * w).ceil() as u64)
                .min(to);
            for ch in 0..p.channels() {
                if let Some((lo, hi)) = p.range(ch, a, b) {
                    let i = ch * n + c;
                    if let Some(v) = out.min.get_mut(i) {
                        *v = lo;
                    }
                    if let Some(v) = out.max.get_mut(i) {
                        *v = hi;
                    }
                }
            }
        }
        Some(out)
    }

    /// Ends it (the engine was told to stop and drop the tap): waits up
    /// to `wait` for the tap to close, takes the last marks and MIDI and
    /// lets the writer finish its files. Blocks: call without the session
    /// lock.
    pub fn finish(mut self, wait: Duration) -> Finished {
        while HOLD_FINISH.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(2));
        }
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
    for a in f.audio.iter().filter(|a| lanes_alive(&a.lane)) {
        // A file the writer could not finish still holds what landed
        // before it failed (pre-flight I3): that part is placed.
        let Some(written) = f.written_of(a) else {
            continue;
        };
        if written.frames == 0 {
            continue;
        }
        let (_, stem) = MediaItem::file_parts(&a.path);
        audio.push(AudioTake {
            lane: a.lane,
            return_frames: a.return_frames,
            media: RecordedMedia {
                path: a.path.clone(),
                sample_rate: f.format.sample_rate,
                channels: a.channels,
                length_frames: written.frames,
                content_hash: written.content_hash.clone(),
                name: stem,
                pass: f.pass_number,
            },
        });
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
    // As 5b's free-name loop (`core_media::import`): reserved paths are
    // compared in lower case, as macOS volumes resolve names.
    let reserved: BTreeSet<String> = reserved.iter().map(|r| r.to_lowercase()).collect();
    let taken = |rel: &str| root.join(rel).exists() || reserved.contains(&rel.to_lowercase());
    let mut rel = format!("{RECORDED_DIR}/{base}.caf");
    let mut k = 2u32;
    // The writer creates without replacing, so a name taken after this
    // check fails the start rather than overwriting.
    while taken(&rel) && k <= 10_000 {
        rel = format!("{RECORDED_DIR}/{base} {k}.caf");
        k += 1;
    }
    rel
}

/// The recordings a crash left in `package` (spec §6.5 "Crash
/// recovery"): regular `.caf` files directly in `Media/Recorded/` (not
/// hidden) that carry the `vizdaw-recording` mark and that no media item
/// of `doc` names, package-relative and sorted. A file that is not a
/// readable VizDAW CAF is never offered.
pub fn recovered(package: &Path, doc: &Document) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(package.join(RECORDED_DIR)) else {
        return Vec::new();
    };
    let named = named_lower(doc);
    let mut out: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            if name.starts_with('.') || !name.to_lowercase().ends_with(".caf") {
                return None;
            }
            let rel = format!("{RECORDED_DIR}/{name}");
            if named.contains(&rel.to_lowercase()) || !e.file_type().ok()?.is_file() {
                return None;
            }
            let r = core_media::caf::CafReader::open(&e.path()).ok()?;
            (r.info().recording && r.info().frames > 0).then_some(rel)
        })
        .collect();
    out.sort();
    out
}

/// The media paths `doc` names, in lower case (as macOS volumes resolve
/// names).
fn named_lower(doc: &Document) -> BTreeSet<String> {
    doc.media()
        .values()
        .map(|m| m.path.to_lowercase())
        .collect()
}

/// What the file at `rel` in `package` holds.
pub fn recovered_row(package: &Path, rel: &str) -> RecoveredFileRow {
    match core_media::caf::CafReader::open(&package.join(rel)) {
        Ok(r) => RecoveredFileRow {
            path: rel.to_string(),
            frames: r.info().frames,
            size_known: r.info().size_known,
            readable: true,
        },
        Err(_) => RecoveredFileRow {
            path: rel.to_string(),
            frames: 0,
            size_known: false,
            readable: false,
        },
    }
}

/// What recovery offers at open: [`recovered`] and the kept takes
/// (`kept`, from [`read_kept`]) that are still files no media item of
/// `doc` names, sorted by path.
pub fn offered(package: &Path, doc: &Document, kept: &BTreeSet<String>) -> Vec<RecoveredFileRow> {
    let named = named_lower(doc);
    let mut paths: BTreeSet<String> = recovered(package, doc).into_iter().collect();
    for k in kept {
        if !named.contains(&k.to_lowercase()) && package.join(k).is_file() {
            paths.insert(k.clone());
        }
    }
    paths.iter().map(|p| recovered_row(package, p)).collect()
}

/// The package file listing the takes kept for the recovery offer (a
/// take no region uses, a pass that could not be applied): they go with
/// the project, so a save in a later session never moves them to the
/// Trash (plan 5c Task 10).
pub const KEEP_FILE: &str = "recordings.json";

#[derive(serde::Serialize, serde::Deserialize)]
struct KeepFile {
    keep: Vec<String>,
}

/// A path a keep file may name: a non-hidden file directly in
/// `Media/Recorded/` (the file is edited by hand at worst; nothing it
/// names outside that folder is ever touched).
fn keepable(rel: &str) -> bool {
    rel.strip_prefix(RECORDED_DIR)
        .and_then(|r| r.strip_prefix('/'))
        .is_some_and(|n| !n.is_empty() && !n.starts_with('.') && !n.contains(['/', '\\']))
}

/// The kept takes `package`'s keep file names (none when it has none);
/// `None` when it exists but cannot be read or parsed.
pub fn read_kept(package: &Path) -> Option<BTreeSet<String>> {
    let text = match std::fs::read_to_string(package.join(KEEP_FILE)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(BTreeSet::new()),
        Err(_) => return None,
        Ok(t) => t,
    };
    serde_json::from_str::<KeepFile>(&text)
        .ok()
        .map(|k| k.keep.into_iter().filter(|p| keepable(p)).collect())
}

/// The problem told when the keep file cannot be read (Task 10 review M3).
pub const KEEP_UNREADABLE: &str =
    "The list of kept recordings could not be read; all recordings were kept.";

/// What to keep at open in `package` (Task 10 review M3): the keep
/// file's takes, or, when it cannot be read, every file in
/// `Media/Recorded/` that `doc` does not name (rewritten into the list at
/// the next save, so no later save trashes them either), with the problem
/// to tell.
pub fn kept_at_open(package: &Path, doc: &Document) -> (BTreeSet<String>, Option<String>) {
    if let Some(k) = read_kept(package) {
        return (k, None);
    }
    let named = named_lower(doc);
    let all = std::fs::read_dir(package.join(RECORDED_DIR))
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                .filter_map(|e| Some(format!("{RECORDED_DIR}/{}", e.file_name().to_str()?)))
                .filter(|rel| keepable(rel) && !named.contains(&rel.to_lowercase()))
                .collect()
        })
        .unwrap_or_default();
    (all, Some(KEEP_UNREADABLE.to_string()))
}

/// Writes `package`'s keep file: the kept takes that are files in it, or
/// no file when there are none. Returns what it wrote.
pub fn write_kept(package: &Path, kept: &BTreeSet<String>) -> Result<BTreeSet<String>, String> {
    let keep: BTreeSet<String> = kept
        .iter()
        .filter(|p| keepable(p) && package.join(p).is_file())
        .cloned()
        .collect();
    let path = package.join(KEEP_FILE);
    if keep.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(keep),
        };
    }
    let text = serde_json::to_string_pretty(&KeepFile {
        keep: keep.iter().cloned().collect(),
    })
    .map_err(|e| e.to_string())?;
    core_model::file::write_atomic(&path, text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(keep)
}

/// Pre-flight I5: moves the recordings an untitled project lost to the
/// Trash (`trash`, else the platform's). They are the recording-mode CAFs
/// holding frames in `<parent>/VizDAW-untitled/<pid>-<n>.vizdaw/Media/Recorded/`
/// whose `pid` is not this process and no longer exists. A running
/// VizDAW's packages are never touched, and nothing is ever unlinked.
/// Returns the one line to tell the user, `None` when there were none.
pub fn trash_orphaned_untitled(parent: &Path, trash: Option<&Path>) -> Option<String> {
    let mut packages: Vec<PathBuf> = std::fs::read_dir(parent.join(crate::media::UNTITLED_DIR))
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter(|e| {
            let name = e.file_name();
            let pid = name
                .to_str()
                .and_then(|n| n.strip_suffix(".vizdaw"))
                .and_then(|n| n.split_once('-'))
                .and_then(|(pid, _)| pid.parse::<u32>().ok());
            pid.is_some_and(|p| p != std::process::id() && !core_media::record::process_exists(p))
        })
        .map(|e| e.path())
        .collect();
    packages.sort();
    let (mut moved, mut left) = (Vec::new(), Vec::new());
    for pkg in packages {
        let Ok(rd) = std::fs::read_dir(pkg.join(RECORDED_DIR)) else {
            continue;
        };
        let mut files: Vec<(String, PathBuf)> = rd
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| Some((e.file_name().to_str()?.to_string(), e.path())))
            .filter(|(n, p)| {
                !n.starts_with('.')
                    && core_media::caf::CafReader::open(p)
                        .is_ok_and(|r| r.info().recording && r.info().frames > 0)
            })
            .collect();
        files.sort();
        for (name, path) in files {
            match crate::retention::trash(trash, &path) {
                Ok(()) => moved.push(name),
                Err(e) => left.push(format!("{} ({e})", path.display())),
            }
        }
    }
    let count = |n: usize, was: &str, were: &str| {
        if n == 1 {
            format!("1 recording from an untitled project VizDAW lost {was}")
        } else {
            format!("{n} recordings from an untitled project VizDAW lost {were}")
        }
    };
    let mut parts = Vec::new();
    if !moved.is_empty() {
        parts.push(format!(
            "{}: {}.",
            count(
                moved.len(),
                "was moved to the Trash",
                "were moved to the Trash"
            ),
            moved.join(", ")
        ));
    }
    if !left.is_empty() {
        parts.push(format!(
            "{}: {}.",
            count(
                left.len(),
                "could not be moved to the Trash and is still in place",
                "could not be moved to the Trash and are still in place"
            ),
            left.join(", ")
        ));
    }
    if parts.is_empty() {
        return None;
    }
    let mut line = parts.join(" ");
    line.pop();
    Some(line)
}

/// Pre-flight I5, for the app to call once at launch: moves the
/// recordings untitled projects lost in a crash to the Trash
/// ([`trash_orphaned_untitled`] on [`crate::media::scratch_parent`]) and
/// returns the line to show, if any.
#[uniffi::export]
pub fn recover_untitled_recordings() -> Result<Option<String>, VizError> {
    crate::guard::ffi_guard(|| {
        Ok(trash_orphaned_untitled(
            &crate::media::scratch_parent(),
            crate::retention::trash_dir(None).as_deref(),
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_engine::fixtures::Builder;
    use core_engine::record::{RecordRequest, tap};
    use core_media::caf::Written;
    use core_media::record::{FailedLane, LaneWritten};
    use core_model::lane::InputChannels;

    fn written(frames: u64) -> Written {
        Written {
            frames,
            content_hash: format!("hash{frames}"),
            clipped: 0,
        }
    }

    /// A pass of 2048 frames from schedule frame 0 on lanes "A" and "B"
    /// (`b_alive`: "B" is still in the document), written as `w`.
    fn finished(
        w: impl FnOnce(&Path) -> WriterResult,
        b_alive: bool,
        range: RecordRange,
    ) -> (Finished, Document) {
        let mut b = Builder::new();
        let (seq, a) = b.sequence("S", "A", None).unwrap();
        let lane_b = if b_alive {
            b.lane(seq, "B", None).unwrap()
        } else {
            LaneId(9_999)
        };
        let doc = b.finish();
        let req = RecordRequest {
            channels: vec![0],
            range: None,
            ring_frames: 8192,
        };
        let (mut t, h) = tap(&req, 0);
        let input = vec![0.25f32; 512];
        for k in 0..4u64 {
            t.begin_block(k * 512, 0);
            t.end_block(&[&input], 512, true);
        }
        let root = PathBuf::from("/nowhere/P.vizdaw");
        let lane = |lane: LaneId, name: &str| AudioLane {
            lane,
            name: name.into(),
            path: format!("{RECORDED_DIR}/{name}.caf"),
            channels: 1,
            return_frames: 0,
        };
        let f = Finished {
            seq,
            spec: RecordSpecRow {
                replace: true,
                punch: None,
                looped: false,
                count_in_bars: 0,
                pre_roll_bars: 0,
                passes_as_takes: true,
            },
            range,
            audio: vec![lane(a, "A"), lane(lane_b, "B")],
            midi_lanes: Vec::new(),
            written: Some(w(&root)),
            marks: vec![Mark {
                run: 0,
                frame: 0,
                wrap: false,
                latency: 0,
            }],
            midi: Vec::new(),
            fates: Vec::new(),
            status: Arc::clone(&h.status),
            format: doc.settings().format,
            pass_number: 1,
        };
        drop(t);
        (f, doc)
    }

    const ALL: RecordRange = RecordRange {
        start: 0,
        end: None,
    };

    fn result(root: &Path, b: Result<Written, Option<Written>>) -> WriterResult {
        let path = |n: &str| root.join(format!("{RECORDED_DIR}/{n}.caf"));
        let mut lanes = vec![LaneWritten {
            path: path("A"),
            written: written(2048),
        }];
        let mut failed = Vec::new();
        match b {
            Ok(w) => lanes.push(LaneWritten {
                path: path("B"),
                written: w,
            }),
            Err(w) => failed.push(FailedLane {
                path: path("B"),
                error: "a write failed".into(),
                written: w,
            }),
        }
        WriterResult {
            lanes,
            failed,
            gap_frames: 0,
            error: None,
        }
    }

    fn media_paths(p: &RecordPass) -> Vec<(String, u64)> {
        p.media
            .iter()
            .map(|m| (m.path.clone(), m.length_frames))
            .collect()
    }

    #[test]
    fn a_file_the_writer_cut_short_is_placed_as_far_as_it_got() {
        let (f, doc) = finished(|r| result(r, Err(Some(written(1024)))), true, ALL);
        let pass = build(&f, &doc).unwrap().unwrap();
        assert_eq!(
            media_paths(&pass),
            vec![
                (format!("{RECORDED_DIR}/A.caf"), 2048),
                (format!("{RECORDED_DIR}/B.caf"), 1024)
            ]
        );
        let left = leftovers(&f, &doc, Some(&pass));
        assert!(left.keep.is_empty());
        assert_eq!(left.problems.len(), 1, "{:?}", left.problems);
        assert!(left.problems[0].starts_with("B: the recording was cut short (a write failed)"));
    }

    #[test]
    fn a_file_the_writer_could_not_close_is_kept_and_reported() {
        let (f, doc) = finished(|r| result(r, Err(None)), true, ALL);
        let pass = build(&f, &doc).unwrap().unwrap();
        assert_eq!(pass.media.len(), 1, "only A");
        let left = leftovers(&f, &doc, Some(&pass));
        assert_eq!(left.keep, vec![format!("{RECORDED_DIR}/B.caf")]);
        assert!(left.problems[0].starts_with("B: the recording could not be finished"));
        assert!(left.problems[0].contains("kept and offered at the next open"));
    }

    #[test]
    fn a_take_whose_lane_went_is_kept() {
        let (f, doc) = finished(|r| result(r, Ok(written(2048))), false, ALL);
        let pass = build(&f, &doc).unwrap().unwrap();
        assert_eq!(pass.media.len(), 1, "only A");
        let left = leftovers(&f, &doc, Some(&pass));
        assert_eq!(left.keep, vec![format!("{RECORDED_DIR}/B.caf")]);
        assert!(left.problems[0].contains("its lane was removed"));
    }

    #[test]
    fn a_take_with_nothing_in_the_record_range_may_go() {
        let range = RecordRange {
            start: 1_000_000,
            end: None,
        };
        let (f, doc) = finished(|r| result(r, Ok(written(2048))), true, range);
        assert_eq!(build(&f, &doc).unwrap(), None);
        assert_eq!(leftovers(&f, &doc, None), Leftovers::default());
    }

    #[test]
    fn lost_input_is_told_in_seconds() {
        let (mut f, _) = finished(|r| result(r, Ok(written(2048))), true, ALL);
        assert_eq!(gaps_problem(&f), None);
        if let Some(w) = f.written.as_mut() {
            w.gap_frames = 72_000;
        }
        assert_eq!(
            gaps_problem(&f).as_deref(),
            Some("Recording: 1.5 s of input could not be written in time and is silence.")
        );
    }

    #[test]
    fn a_denied_input_refuses_with_the_reason() {
        let denied = format!("{DENIED_PREFIX}. Allow it in System Settings.");
        let faults = vec!["xrun".to_string(), denied.clone()];
        assert_eq!(
            channel_refusal("Mic", InputChannels::Mono(0), 0, &faults),
            Some(denied)
        );
        assert_eq!(
            channel_refusal("Mic", InputChannels::Stereo(1), 3, &faults),
            None
        );
        assert_eq!(
            channel_refusal("Mic", InputChannels::Stereo(2), 3, &faults).as_deref(),
            Some("lane Mic records input 3, but the audio input has 3 channel(s)")
        );
        assert_eq!(
            channel_refusal("Mic", InputChannels::Mono(0), 0, &[]).as_deref(),
            Some("lane Mic records input 1, but the audio input has 0 channel(s)")
        );
    }

    /// Final review M4: an input that failed to open says so, rather
    /// than "0 channel(s)".
    #[test]
    fn an_input_that_failed_to_open_refuses_with_the_reason() {
        let failed = format!("{INPUT_FAILED_PREFIX}: device busy");
        let faults = vec!["xrun".to_string(), failed.clone()];
        assert_eq!(
            channel_refusal("Mic", InputChannels::Mono(0), 0, &faults),
            Some(failed.clone())
        );
        // With channels open, too few is still a channel count.
        assert_eq!(
            channel_refusal("Mic", InputChannels::Stereo(2), 3, &faults).as_deref(),
            Some("lane Mic records input 3, but the audio input has 3 channel(s)")
        );
        // A denial is the reason first: it is why the open failed.
        let denied = format!("{DENIED_PREFIX}. Allow it in System Settings.");
        assert_eq!(
            channel_refusal("Mic", InputChannels::Mono(0), 0, &[failed, denied.clone()]),
            Some(denied)
        );
    }

    #[test]
    fn recorded_names_skip_reserved_paths_in_any_case() {
        let dir = tempfile::tempdir().unwrap();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(86_400 * 365);
        let first = recorded_path(dir.path(), "Mic", at, &BTreeSet::new());
        assert_eq!(first, "Media/Recorded/Mic 1971-01-01 000000.caf");
        let reserved: BTreeSet<String> = [first.to_uppercase()].into();
        assert_eq!(
            recorded_path(dir.path(), "Mic", at, &reserved),
            "Media/Recorded/Mic 1971-01-01 000000 2.caf"
        );
        std::fs::create_dir_all(dir.path().join(RECORDED_DIR)).unwrap();
        std::fs::write(dir.path().join(&first), b"x").unwrap();
        assert_eq!(
            recorded_path(dir.path(), "Mic", at, &BTreeSet::new()),
            "Media/Recorded/Mic 1971-01-01 000000 2.caf"
        );
        assert_eq!(file_safe("../a/b:c"), "-a-b-c");
        assert_eq!(file_safe(" .. "), "Lane");
    }
}
