//! Standard MIDI File bytes (SMF 1.0, types 0 and 1): a reader that treats
//! every file as hostile and a writer that never uses running status.
//!
//! The reader keeps what the file says: values are not judged here (a
//! note-on of velocity 0 stays a note-on, a tempo of 0 stays a tempo), except
//! SysEx length and termination; the mapping to the model does the rest.
//! Damage to the structure inside a track chunk refuses the file (plan 6a
//! Ruling 3).

use crate::IoError;
use crate::limits::{Limits, MAX_VLQ_BYTES};
use crate::report::{ItemKind, Report, group};
use core_engine::midi1::Midi1Msg;
use std::sync::atomic::{AtomicBool, Ordering};

/// How ticks are counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Division {
    /// Ticks per quarter note (1 to 32 767).
    Ppq(u16),
    /// Ticks per SMPTE frame at 24, 25, 29 (drop-frame) or 30 frames per second.
    Smpte {
        /// Frames per second: 24, 25, 29 or 30.
        fps: u8,
        /// Ticks per frame (1 to 255).
        ticks_per_frame: u8,
    },
}

/// One event of a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmfEvent {
    /// A channel message, as it is in the file (running status expanded).
    Channel(Midi1Msg),
    /// A system-exclusive message: the payload without `F0` and the final
    /// `F7`, reassembled from its packets.
    SysEx(Vec<u8>),
    /// An escape (an `F7` packet outside a SysEx): bytes to send as they are.
    Escape(Vec<u8>),
    /// A meta event.
    Meta {
        /// The meta type (`0x2F` is End of Track).
        kind: u8,
        /// Its data.
        data: Vec<u8>,
    },
}

/// One `MTrk` chunk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SmfTrack {
    /// Events at absolute ticks in file units, in file order.
    pub events: Vec<(u64, SmfEvent)>,
}

/// A Standard MIDI File.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmfFile {
    /// 0 or 1.
    pub format: u16,
    /// How ticks are counted.
    pub division: Division,
    /// The `MTrk` chunks, in file order.
    pub tracks: Vec<SmfTrack>,
}

/// The largest delta (and data length) a 4-byte variable-length quantity holds.
const MAX_VLQ: u32 = 0x0FFF_FFFF;
/// How many events are read between looks at the cancel flag.
const CANCEL_EVERY: usize = 65_536;
/// The End of Track meta type.
const END_OF_TRACK: u8 = 0x2F;

const NOT_MIDI: &str = "This isn't a MIDI file.";
const BAD_TIMING: &str = "This MIDI file's timing can't be read.";
const UNKNOWN_CHUNKS: &str = "Unknown file sections skipped";
pub(crate) const SYSEX_OVER: &str = "SysEx messages over 1 MiB";
pub(crate) const SYSEX_UNFINISHED: &str = "Unfinished SysEx messages";
pub(crate) const TEXT_CUT: &str = "Texts over 64 KiB cut";
const NO_END: &str = "Tracks without an End of Track";
const AFTER_END: &str = "Data after the End of Track skipped";

fn refused(s: impl Into<String>) -> IoError {
    IoError::Refused(s.into())
}

/// "This MIDI file can't be read: track `n` `what`."
fn damaged(n: usize, what: &str) -> IoError {
    refused(format!("This MIDI file can't be read: track {n} {what}."))
}

fn truncated(n: usize) -> IoError {
    damaged(n, "ends in the middle of an event")
}

/// "1 track", "5 tracks".
fn tracks_word(n: usize) -> String {
    let word = if n == 1 { "track" } else { "tracks" };
    format!("{} {word}", group(n as u64))
}

/// A position in bytes that only moves forward through what is there.
struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

/// Why a variable-length quantity could not be read.
enum VlqError {
    End,
    TooLong,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    fn byte(&mut self) -> Option<u8> {
        let b = *self.bytes.get(self.pos)?;
        self.pos = self.pos.checked_add(1)?;
        Some(b)
    }

    /// The next `n` bytes, if all of them are there.
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let s = self.bytes.get(self.pos..end)?;
        self.pos = end;
        Some(s)
    }

    /// A chunk's type and declared length.
    fn chunk_head(&mut self) -> Option<([u8; 4], u32)> {
        let kind = *self.take(4)?.first_chunk::<4>()?;
        let len = *self.take(4)?.first_chunk::<4>()?;
        Some((kind, u32::from_be_bytes(len)))
    }

    fn vlq(&mut self) -> Result<u32, VlqError> {
        let mut v = 0u32;
        for _ in 0..MAX_VLQ_BYTES {
            let b = self.byte().ok_or(VlqError::End)?;
            v = (v << 7) | u32::from(b & 0x7F);
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(VlqError::TooLong)
    }

    /// A length-prefixed run of bytes of track `n`, wholly inside its chunk.
    fn data(&mut self, n: usize) -> Result<&'a [u8], IoError> {
        let len = self.vlq().map_err(|e| vlq_error(n, e))?;
        let len = usize::try_from(len).map_err(|_| truncated(n))?;
        self.take(len).ok_or_else(|| truncated(n))
    }
}

fn vlq_error(n: usize, e: VlqError) -> IoError {
    match e {
        VlqError::End => truncated(n),
        VlqError::TooLong => damaged(n, "has a number longer than 4 bytes"),
    }
}

fn cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

fn division(d: u16) -> Result<Division, IoError> {
    let [hi, lo] = d.to_be_bytes();
    if hi & 0x80 == 0 {
        return if d == 0 {
            Err(refused(BAD_TIMING))
        } else {
            Ok(Division::Ppq(d))
        };
    }
    // The high byte is minus the frame rate, in two's complement.
    let fps = 0u8.wrapping_sub(hi);
    if matches!(fps, 24 | 25 | 29 | 30) && lo != 0 {
        Ok(Division::Smpte {
            fps,
            ticks_per_frame: lo,
        })
    } else {
        Err(refused(BAD_TIMING))
    }
}

/// Reads a Standard MIDI File of type 0 or 1 from `bytes`, within `limits`.
///
/// Refuses, with one sentence that names the track where there is one, a
/// file that is not an SMF (a type above 2 is not one), is type 2, has unreadable timing, has more
/// tracks or events than the limits allow, or whose track structure is
/// damaged. SysEx over `limits.max_sysex_bytes` and SysEx never finished are
/// left out, texts over `limits.max_text_bytes` are cut and unknown chunks
/// are skipped, each in the report. Memory grows only with the bytes
/// present, never with a declared length. Returns `Err(Cancelled)` once
/// `cancel` is set (looked at before each chunk and every 65 536 events).
pub fn read(
    bytes: &[u8],
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<(SmfFile, Report), IoError> {
    if cancelled(cancel) {
        return Err(IoError::Cancelled);
    }
    let mut cur = Cursor::new(bytes);
    let header = match cur.chunk_head() {
        Some((kind, len)) if &kind == b"MThd" && len >= 6 => usize::try_from(len)
            .ok()
            .and_then(|len| cur.take(len))
            .and_then(|body| body.first_chunk::<6>()),
        _ => None,
    };
    let Some(&[f0, f1, n0, n1, d0, d1]) = header else {
        return Err(refused(NOT_MIDI));
    };
    let format = u16::from_be_bytes([f0, f1]);
    let declared = usize::from(u16::from_be_bytes([n0, n1]));
    match format {
        0 | 1 => {}
        2 => {
            return Err(refused(
                "This is a type 2 MIDI file; VizDAW reads types 0 and 1.",
            ));
        }
        _ => return Err(refused(NOT_MIDI)),
    }
    let division = division(u16::from_be_bytes([d0, d1]))?;

    let mut r = Reader {
        limits,
        cancel,
        events: 0,
        report: Report::default(),
    };
    let mut tracks = Vec::new();
    while !cur.at_end() {
        if cancelled(cancel) {
            return Err(IoError::Cancelled);
        }
        let start = cur.pos;
        let Some((kind, len)) = cur.chunk_head() else {
            r.report.add(
                ItemKind::Note,
                UNKNOWN_CHUNKS,
                Some(format!("loose bytes at byte {start}")),
            );
            break;
        };
        let body = usize::try_from(len).ok().and_then(|len| cur.take(len));
        if &kind == b"MTrk" {
            if tracks.len() >= limits.max_tracks {
                return Err(refused(format!(
                    "This MIDI file can't be read: it has more than {}.",
                    tracks_word(limits.max_tracks)
                )));
            }
            let n = tracks.len().saturating_add(1);
            tracks.push(r.track(body.ok_or_else(|| truncated(n))?, n)?);
        } else {
            let name: String = kind
                .iter()
                .map(|&b| {
                    if b.is_ascii_alphanumeric() {
                        char::from(b)
                    } else {
                        '?'
                    }
                })
                .collect();
            r.report.add(
                ItemKind::Note,
                UNKNOWN_CHUNKS,
                Some(format!("\"{name}\" at byte {start}")),
            );
            if body.is_none() {
                break;
            }
        }
    }

    let mut report = r.report;
    if format == 0 && tracks.len() != 1 {
        let what = format!("Type 0 file with {}", tracks_word(tracks.len()));
        report.add(ItemKind::Changed, &what, None);
    }
    if declared != tracks.len() {
        let what = format!(
            "The file declares {} and holds {}",
            tracks_word(declared),
            group(tracks.len() as u64)
        );
        report.add(ItemKind::Changed, &what, None);
    }
    Ok((
        SmfFile {
            format,
            division,
            tracks,
        },
        report,
    ))
}

/// What carries over from track to track while reading.
struct Reader<'l> {
    limits: &'l Limits,
    cancel: &'l AtomicBool,
    /// Events read so far, all tracks together.
    events: usize,
    report: Report,
}

/// A SysEx whose final `F7` has not been read yet.
struct OpenSysEx {
    /// Where its event sits in the track; `None` once it is over the limit
    /// (its bytes are no longer kept).
    index: Option<usize>,
    /// Payload bytes so far.
    len: usize,
    /// Its start.
    tick: u64,
}

impl Reader<'_> {
    /// Reads track `n` (1-based) from its chunk body.
    fn track(&mut self, body: &[u8], n: usize) -> Result<SmfTrack, IoError> {
        let mut cur = Cursor::new(body);
        let mut events: Vec<(u64, SmfEvent)> = Vec::new();
        // Indices of SysEx events found unfinished or too long, ascending;
        // removed once the track is read.
        let mut dead: Vec<usize> = Vec::new();
        let mut open: Option<OpenSysEx> = None;
        let mut running: Option<u8> = None;
        let mut tick = 0u64;
        let mut ended = false;

        while !cur.at_end() {
            self.count()?;
            let delta = cur.vlq().map_err(|e| vlq_error(n, e))?;
            tick = tick
                .checked_add(u64::from(delta))
                .ok_or_else(|| damaged(n, "is too long"))?;
            let status = cur.byte().ok_or_else(|| truncated(n))?;
            match status {
                0xFF => {
                    running = None;
                    let kind = cur.byte().ok_or_else(|| truncated(n))?;
                    let mut data = cur.data(n)?;
                    let max = self.limits.max_text_bytes;
                    if (0x01..=0x0F).contains(&kind) && data.len() > max {
                        data = data.get(..max).unwrap_or_default();
                        let at = format!("track {n}, tick {tick}");
                        self.report.add(ItemKind::Changed, TEXT_CUT, Some(at));
                    }
                    let data = data.to_vec();
                    events.push((tick, SmfEvent::Meta { kind, data }));
                    if kind == END_OF_TRACK {
                        ended = true;
                        break;
                    }
                }
                0xF0 | 0xF7 => {
                    running = None;
                    let data = cur.data(n)?;
                    let continuing = status == 0xF7 && open.is_some();
                    if status == 0xF7 && !continuing {
                        events.push((tick, SmfEvent::Escape(data.to_vec())));
                        continue;
                    }
                    if !continuing && let Some(o) = open.take() {
                        self.unfinished(o, &mut dead, n);
                    }
                    let (payload, done) = match data.split_last() {
                        Some((&0xF7, rest)) => (rest, true),
                        _ => (data, false),
                    };
                    let mut o = open.take().unwrap_or(OpenSysEx {
                        index: None,
                        len: 0,
                        tick,
                    });
                    if !continuing && payload.len() <= self.limits.max_sysex_bytes {
                        o.index = Some(events.len());
                        events.push((tick, SmfEvent::SysEx(Vec::new())));
                    }
                    o.len = o.len.saturating_add(payload.len());
                    if let Some(i) = o.index
                        && let Some((_, SmfEvent::SysEx(v))) = events.get_mut(i)
                    {
                        if o.len > self.limits.max_sysex_bytes {
                            *v = Vec::new();
                            dead.push(i);
                            o.index = None;
                        } else {
                            v.extend_from_slice(payload);
                        }
                    }
                    if !done {
                        open = Some(o);
                    } else if o.index.is_none() {
                        let at = format!("track {n}, tick {}", o.tick);
                        self.report.add(ItemKind::LeftOut, SYSEX_OVER, Some(at));
                    }
                }
                0x80..=0xEF => {
                    running = Some(status);
                    events.push((tick, channel(&mut cur, status, None, n)?));
                }
                0xF1..=0xFE => return Err(damaged(n, "has a damaged event")),
                _ => {
                    let s = running.ok_or_else(|| {
                        damaged(n, "has a data byte with no status byte before it")
                    })?;
                    events.push((tick, channel(&mut cur, s, Some(status), n)?));
                }
            }
        }

        if let Some(o) = open {
            self.unfinished(o, &mut dead, n);
        }
        if !ended {
            let at = format!("track {n}");
            self.report.add(ItemKind::Note, NO_END, Some(at));
        } else if !cur.at_end() {
            let at = format!("track {n}");
            self.report.add(ItemKind::Note, AFTER_END, Some(at));
        }
        if !dead.is_empty() {
            let mut dead = dead.into_iter().peekable();
            events = events
                .into_iter()
                .enumerate()
                .filter(|(i, _)| dead.next_if_eq(i).is_none())
                .map(|(_, e)| e)
                .collect();
        }
        Ok(SmfTrack { events })
    }

    /// Counts one event against the limit, looking at the cancel flag every
    /// [`CANCEL_EVERY`] events.
    fn count(&mut self) -> Result<(), IoError> {
        self.events = self.events.saturating_add(1);
        if self.events > self.limits.max_events {
            return Err(refused(format!(
                "This MIDI file can't be read: it has more than {} events.",
                group(self.limits.max_events as u64)
            )));
        }
        if self.events % CANCEL_EVERY == 0 && cancelled(self.cancel) {
            return Err(IoError::Cancelled);
        }
        Ok(())
    }

    /// Leaves out a SysEx that never got its final `F7`.
    fn unfinished(&mut self, o: OpenSysEx, dead: &mut Vec<usize>, n: usize) {
        if let Some(i) = o.index {
            dead.push(i);
        }
        let at = format!("track {n}, tick {}", o.tick);
        self.report
            .add(ItemKind::LeftOut, SYSEX_UNFINISHED, Some(at));
    }
}

/// How many data bytes a channel message with `status` has.
fn data_len(status: u8) -> usize {
    match status & 0xF0 {
        0xC0 | 0xD0 => 1,
        _ => 2,
    }
}

/// A channel message of track `n` with `status`; `first` is its first data
/// byte when running status already read it.
fn channel(
    cur: &mut Cursor<'_>,
    status: u8,
    first: Option<u8>,
    n: usize,
) -> Result<SmfEvent, IoError> {
    let mut next = || -> Result<u8, IoError> {
        let b = cur.byte().ok_or_else(|| truncated(n))?;
        if b & 0x80 == 0 {
            Ok(b)
        } else {
            Err(damaged(n, "has a damaged event"))
        }
    };
    let d1 = match first {
        Some(b) => b,
        None => next()?,
    };
    let msg = if data_len(status) == 1 {
        Midi1Msg::two(status, d1)
    } else {
        Midi1Msg::three(status, d1, next()?)
    };
    Ok(SmfEvent::Channel(msg))
}

/// "This MIDI file can't be written: track `n` `what`."
fn unwritable(n: usize, what: &str) -> IoError {
    refused(format!(
        "This MIDI file can't be written: track {n} {what}."
    ))
}

/// Writes `f` as a Standard MIDI File: every event with its own status byte
/// (no running status), each SysEx as one `F0` packet, deltas of at most 4
/// bytes. Refuses a format other than 0 or 1, an unwritable division, more
/// than 65 535 tracks, a track over `u32::MAX` bytes, events out of tick
/// order or more than `0x0FFF_FFFF` ticks apart, a damaged channel message,
/// data over `0x0FFF_FFFF` bytes and events after an End of Track.
pub fn write(f: &SmfFile) -> Result<Vec<u8>, IoError> {
    if f.format > 1 {
        return Err(refused(format!(
            "This MIDI file can't be written: VizDAW writes types 0 and 1, not type {}.",
            f.format
        )));
    }
    let ntrks = u16::try_from(f.tracks.len())
        .map_err(|_| refused("This MIDI file can't be written: it has more than 65 535 tracks."))?;
    let division = match f.division {
        Division::Ppq(t) if t > 0 && t < 0x8000 => t,
        Division::Smpte {
            fps,
            ticks_per_frame,
        } if matches!(fps, 24 | 25 | 29 | 30) && ticks_per_frame > 0 => {
            u16::from_be_bytes([0u8.wrapping_sub(fps), ticks_per_frame])
        }
        _ => return Err(refused("This MIDI file's timing can't be written.")),
    };
    let mut out = b"MThd\0\0\0\x06".to_vec();
    for x in [f.format, ntrks, division] {
        out.extend_from_slice(&x.to_be_bytes());
    }
    for (i, t) in f.tracks.iter().enumerate() {
        let n = i.saturating_add(1);
        let body = track_bytes(t, n)?;
        let len = u32::try_from(body.len()).map_err(|_| unwritable(n, "is longer than 4 GiB"))?;
        out.extend_from_slice(b"MTrk");
        out.extend_from_slice(&len.to_be_bytes());
        out.extend(body);
    }
    Ok(out)
}

/// The body of track `n`.
fn track_bytes(t: &SmfTrack, n: usize) -> Result<Vec<u8>, IoError> {
    let mut out = Vec::new();
    let mut prev = 0u64;
    let mut ended = false;
    for (tick, e) in &t.events {
        if ended {
            return Err(unwritable(n, "has events after its End of Track"));
        }
        let delta = tick
            .checked_sub(prev)
            .ok_or_else(|| unwritable(n, "has events out of order"))?;
        let delta = u32::try_from(delta)
            .ok()
            .filter(|d| *d <= MAX_VLQ)
            .ok_or_else(|| unwritable(n, "has events too far apart"))?;
        put_vlq(&mut out, delta);
        match e {
            SmfEvent::Channel(m) => {
                let s = m.as_slice();
                let ok = s.split_first().is_some_and(|(&st, data)| {
                    (0x80..=0xEF).contains(&st)
                        && data.len() == data_len(st)
                        && data.iter().all(|b| b & 0x80 == 0)
                });
                if !ok {
                    return Err(unwritable(n, "has a damaged channel message"));
                }
                out.extend_from_slice(s);
            }
            SmfEvent::SysEx(p) => {
                out.push(0xF0);
                put_len(&mut out, p.len().saturating_add(1), n)?;
                out.extend_from_slice(p);
                out.push(0xF7);
            }
            SmfEvent::Escape(d) => {
                out.push(0xF7);
                put_len(&mut out, d.len(), n)?;
                out.extend_from_slice(d);
            }
            SmfEvent::Meta { kind, data } => {
                out.extend_from_slice(&[0xFF, *kind]);
                put_len(&mut out, data.len(), n)?;
                out.extend_from_slice(data);
                ended = *kind == END_OF_TRACK;
            }
        }
        prev = *tick;
    }
    Ok(out)
}

/// Writes a data length of track `n` (at most `0x0FFF_FFFF`).
fn put_len(out: &mut Vec<u8>, len: usize, n: usize) -> Result<(), IoError> {
    let len = u32::try_from(len)
        .ok()
        .filter(|l| *l <= MAX_VLQ)
        .ok_or_else(|| unwritable(n, "has an event too long for a MIDI file"))?;
    put_vlq(out, len);
    Ok(())
}

/// Writes `v` (at most `0x0FFF_FFFF`) as a variable-length quantity.
fn put_vlq(out: &mut Vec<u8>, v: u32) {
    let groups = [21u32, 14, 7, 0].map(|s| ((v >> s) & 0x7F) as u8);
    let skip = groups.iter().take(3).take_while(|g| **g == 0).count();
    for (i, g) in groups.iter().enumerate().skip(skip) {
        out.push(if i < 3 { g | 0x80 } else { *g });
    }
}
