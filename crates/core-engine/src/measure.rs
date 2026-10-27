//! Measuring a synth's return latency (sub-project 5 spec §6.7; plan 5c-2
//! Ruling 8): with the transport stopped, the renderer sends five short
//! notes to the synth's destination and watches the lane's return input
//! for each note's onset. It counts *runs* (output frames from the block
//! that installed it); the control side turns what it found into
//! milliseconds. // RT where marked

use crate::midi1::Midi1Msg;
use crate::schedule::TargetIdx;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

/// Notes a measurement sends (spec §6.7: 5).
pub const PINGS: usize = 5;
/// Their pitch (spec §6.7: 60).
pub const PING_PITCH: u8 = 60;
/// Their velocity (spec §6.7: 100).
pub const PING_VELOCITY: u8 = 100;
/// How far apart they start (spec §6.7: 500 ms); each note's return is
/// watched this long.
pub const PING_EVERY_SECONDS: f64 = 0.5;
/// How long each sounds ("short notes"; plan 5c-2 Ruling 8).
pub const PING_SECONDS: f64 = 0.1;
/// From the measurement's start to its first note.
pub const LEAD_SECONDS: f64 = 0.1;
/// An onset is the first sample above this (spec §6.7: −40 dBFS) …
pub const ONSET_LEVEL: f32 = 0.01;
/// … after at least this long at or below it (plan 5c-2 Ruling 8).
pub const QUIET_SECONDS: f64 = 0.005;

const NONE: i64 = i64::MIN;

fn read(a: &AtomicI64) -> Option<i64> {
    let v = a.load(Ordering::Acquire);
    (v != NONE).then_some(v)
}

/// What a measurement shares with the control side.
#[derive(Debug)]
pub struct MeasureStatus {
    sample_rate: f64,
    window: i64,
    sent: [AtomicI64; PINGS],
    onsets: [AtomicI64; PINGS],
    delay: AtomicI64,
    done: AtomicBool,
    closed: AtomicBool,
}

impl MeasureStatus {
    /// The engine's rate.
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Frames each note's return is watched (500 ms).
    pub fn window(&self) -> i64 {
        self.window
    }

    /// The run at which note `i` was heard (sent, plus the graph latency
    /// and the MIDI offset), once it was sent.
    pub fn sent(&self, i: usize) -> Option<i64> {
        self.sent.get(i).and_then(read)
    }

    /// The input run of note `i`'s onset, once one was found.
    pub fn onset(&self, i: usize) -> Option<i64> {
        self.onsets.get(i).and_then(read)
    }

    /// The device's input delay (spec §6.3: the round trip) at the first
    /// block.
    pub fn delay(&self) -> Option<i64> {
        read(&self.delay)
    }

    /// Every note was sent and its window has passed.
    pub fn done(&self) -> bool {
        self.done.load(Ordering::Acquire)
    }

    /// The measurement was removed, or dropped with its renderer.
    pub fn closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
}

/// What to measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeasureRequest {
    /// The synth's target.
    pub target: TargetIdx,
    /// The MIDI channel the notes go on (0–15).
    pub channel: u8,
    /// The return's device input channels (one, or a pair; more are
    /// ignored).
    pub inputs: Vec<u16>,
}

/// The real-time side of a measurement, owned by the renderer.
#[derive(Debug)]
pub struct MeasureTap {
    status: Arc<MeasureStatus>,
    target: TargetIdx,
    on: Midi1Msg,
    off: Midi1Msg,
    inputs: [u16; 2],
    count: usize,
    lead: u64,
    every: u64,
    length: u64,
    quiet_need: u64,
    run: u64,
    next: usize,
    /// The run the sounding note ends at.
    off_at: Option<u64>,
    sent: [Option<i64>; PINGS],
    found: [bool; PINGS],
    delay: Option<i64>,
    /// Samples at or below [`ONSET_LEVEL`] in a row.
    quiet: u64,
}

impl Drop for MeasureTap {
    fn drop(&mut self) {
        self.status.closed.store(true, Ordering::Release);
    }
}

/// A measurement and its control side. Allocates; call off the real-time
/// thread.
pub fn tap(req: &MeasureRequest, sample_rate: f64) -> (MeasureTap, Arc<MeasureStatus>) {
    let frames = |s: f64| (s * sample_rate).round().max(1.0) as u64;
    let every = frames(PING_EVERY_SECONDS);
    let status = Arc::new(MeasureStatus {
        sample_rate,
        window: i64::try_from(every).unwrap_or(i64::MAX),
        sent: std::array::from_fn(|_| AtomicI64::new(NONE)),
        onsets: std::array::from_fn(|_| AtomicI64::new(NONE)),
        delay: AtomicI64::new(NONE),
        done: AtomicBool::new(false),
        closed: AtomicBool::new(false),
    });
    let mut inputs = [0u16; 2];
    for (d, s) in inputs.iter_mut().zip(&req.inputs) {
        *d = *s;
    }
    let ch = req.channel & 0x0F;
    (
        MeasureTap {
            status: Arc::clone(&status),
            target: req.target,
            on: Midi1Msg::three(0x90 | ch, PING_PITCH, PING_VELOCITY),
            off: Midi1Msg::three(0x80 | ch, PING_PITCH, 64),
            inputs,
            count: req.inputs.len().min(2),
            lead: frames(LEAD_SECONDS),
            every,
            length: frames(PING_SECONDS),
            quiet_need: frames(QUIET_SECONDS),
            run: 0,
            next: 0,
            off_at: None,
            sent: [None; PINGS],
            found: [false; PINGS],
            delay: None,
            quiet: 0,
        },
        status,
    )
}

impl MeasureTap {
    /// The synth's target.
    pub fn target(&self) -> TargetIdx {
        self.target
    }

    /// At a block's start: sends what is due (the note-off of the note
    /// sounding, then the next note), each at block offset 0; `late` is
    /// how long after the block's first frame its MIDI is heard (the graph
    /// latency plus the MIDI offset), in frames. // RT
    pub fn sends(&mut self, late: u64, mut send: impl FnMut(Midi1Msg)) {
        if let Some(at) = self.off_at
            && self.run >= at
        {
            send(self.off);
            self.off_at = None;
        }
        if self.off_at.is_some() || self.next >= PINGS {
            return;
        }
        let due = self
            .lead
            .saturating_add(self.every.saturating_mul(self.next as u64));
        if self.run < due {
            return;
        }
        send(self.on);
        let heard = i64::try_from(self.run.saturating_add(late)).unwrap_or(i64::MAX);
        if let (Some(s), Some(st)) = (
            self.sent.get_mut(self.next),
            self.status.sent.get(self.next),
        ) {
            *s = Some(heard);
            st.store(heard, Ordering::Release);
        }
        self.off_at = Some(self.run.saturating_add(self.length));
        self.next += 1;
    }

    /// At a block's end: watches its input (device channel `c` is
    /// `input[c]`) for onsets and advances the run; `delay` is the device's
    /// input delay for the block. // RT
    pub fn capture(&mut self, input: &[&[f32]], frames: u32, delay: i64) {
        let d = match self.delay {
            Some(d) => d,
            None => {
                self.delay = Some(delay);
                self.status.delay.store(delay, Ordering::Release);
                delay
            }
        };
        let window = self.status.window;
        let run = i64::try_from(self.run).unwrap_or(i64::MAX);
        for j in 0..frames as usize {
            let mut level = 0.0f32;
            for c in self.inputs.iter().take(self.count) {
                let x = input
                    .get(usize::from(*c))
                    .and_then(|ch| ch.get(j))
                    .copied()
                    .unwrap_or(0.0);
                level = level.max(x.abs());
            }
            if level <= ONSET_LEVEL {
                self.quiet = self.quiet.saturating_add(1);
                continue;
            }
            if self.quiet >= self.quiet_need {
                let k = run.saturating_add(j as i64);
                for ((s, f), st) in self
                    .sent
                    .iter()
                    .zip(self.found.iter_mut())
                    .zip(self.status.onsets.iter())
                {
                    let Some(u) = *s else {
                        continue;
                    };
                    let from = u.saturating_add(d);
                    if !*f && k >= from && k < from.saturating_add(window) {
                        *f = true;
                        st.store(k, Ordering::Release);
                    }
                }
            }
            self.quiet = 0;
        }
        self.run = self.run.saturating_add(u64::from(frames));
        if self.next >= PINGS
            && let Some(Some(u)) = self.sent.last()
        {
            let end = i64::try_from(self.run).unwrap_or(i64::MAX);
            if end >= u.saturating_add(d).saturating_add(window) {
                self.status.done.store(true, Ordering::Release);
            }
        }
    }

    /// The measurement is removed: the sounding note ends. // RT
    pub fn release(&mut self, mut send: impl FnMut(Midi1Msg)) {
        if self.off_at.take().is_some() {
            send(self.off);
        }
    }

    /// The control side reads it closed now, before the retire thread
    /// frees it. // RT
    pub fn close(&self) {
        self.status.closed.store(true, Ordering::Release);
    }
}
