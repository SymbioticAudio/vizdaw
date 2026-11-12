//! MIDI Clip File bytes (*M2-116-U MIDI Clip File Specification v1.0*): a
//! reader that treats every file as hostile and a writer that puts a Delta
//! Clockstamp before every message.
//!
//! Layout: `SMF2CLIP`; the Clip Configuration Header (optional Set Profile
//! On messages with no clockstamp, `DCS 0` + DCTPQ, then timed messages);
//! the Clip Sequence Data (`DCS` + Start of Clip, timed messages, `DCS` +
//! End of Clip). All words are big-endian.
//!
//! The reader keeps what the file says; values are not judged here, except
//! SysEx and text length and termination. SysEx and Flex Data text are
//! assembled from their packets and their limits applied once (F9).

use crate::IoError;
use crate::limits::Limits;
use crate::report::{ItemKind, Report, group};
use crate::smf::{SYSEX_OVER, SYSEX_UNFINISHED, TEXT_CUT};
use crate::text::sanitize;
use crate::ump::{self, Decoded, MAX_DCS, Packet};
use std::sync::atomic::{AtomicBool, Ordering};

/// One message of a clip, with multi-packet messages assembled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipEvent {
    /// Any other packet, as it is in the file.
    Packet(Packet),
    /// A SysEx 7 message: its data without `F0` and `F7`.
    SysEx7 {
        /// Group.
        group: u8,
        /// Data bytes.
        data: Vec<u8>,
    },
    /// A SysEx 8 message: its data without the stream ID (written as
    /// stream 0).
    SysEx8 {
        /// Group.
        group: u8,
        /// Data bytes.
        data: Vec<u8>,
    },
    /// A Flex Data text message (status bank 0x01 or 0x02), cleaned by
    /// [`sanitize`]. Its address is written as the group.
    Text {
        /// Group.
        group: u8,
        /// Status bank: 0x01 metadata, 0x02 performance.
        bank: u8,
        /// Status within the bank.
        status: u8,
        /// The text.
        text: String,
    },
}

/// A MIDI Clip File.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipFile {
    /// Ticks per quarter note (DCTPQ), 1 to 65 535.
    pub tpq: u16,
    /// The timed configuration messages after DCTPQ: at most one Set Tempo
    /// and one Set Time Signature, tempo first (F8).
    pub header: Vec<ClipEvent>,
    /// The sequence data at ticks from Start of Clip (bar 1), in file
    /// order; Start and End of Clip are not here.
    pub events: Vec<(u64, ClipEvent)>,
    /// The End of Clip tick.
    pub end: u64,
}

const MAGIC: &[u8; 8] = b"SMF2CLIP";
/// How many packets are read between looks at the cancel flag.
const CANCEL_EVERY: usize = 65_536;

const NOT_CLIP: &str = "This isn't a MIDI Clip File.";
const NO_TPQ: &str = "This MIDI Clip File can't be read: it has no ticks per quarter note.";
const ZERO_TPQ: &str = "This MIDI Clip File can't be read: it has a ticks per quarter note of 0.";
const NO_START: &str = "This MIDI Clip File can't be read: it has no Start of Clip.";
const SECOND_START: &str = "This MIDI Clip File can't be read: it has a second Start of Clip.";
const TOO_LONG: &str = "This MIDI Clip File can't be read: it is too long.";
const AFTER_END: &str = "Data after the end of the clip was ignored";
const NO_END: &str = "The clip has no End of Clip; it ends at its last event";
const UNKNOWN: &str = "Unknown MIDI messages";
const PROFILES: &str = "Profile settings in the clip were not used";
const MOVED: &str = "Clip settings moved to the start of the clip";
const SYSEX_DAMAGED: &str = "Damaged SysEx messages";
const TEXT_UNFINISHED: &str = "Unfinished texts";

fn refused(s: &str) -> IoError {
    IoError::Refused(s.to_string())
}

fn at(tick: u64) -> Option<String> {
    Some(format!("tick {tick}"))
}

/// Where the reader is in the file.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Before DCTPQ: profile settings.
    Profiles,
    /// After DCTPQ, before Start of Clip.
    Header,
    /// After Start of Clip.
    Sequence,
}

/// Reads a MIDI Clip File from `bytes`, within `limits`.
///
/// Refuses, with one sentence, a file without the `SMF2CLIP` magic, without
/// DCTPQ or with DCTPQ 0, without Start of Clip or with a second one, with a
/// packet cut off at its end, or with more packets than `limits.max_events`
/// (Delta Clockstamps, NOOPs and the clip markers are not counted). Messages
/// before DCTPQ (profile settings) are noted and not used; header messages
/// past the first Set Tempo and Set Time Signature move to tick 0 of the
/// sequence (Changed); unknown messages are left out; SysEx over
/// `limits.max_sysex_bytes`, damaged or never finished is left out; texts
/// over `limits.max_text_bytes` are cut; data after End of Clip is ignored;
/// without End of Clip the clip ends at its last event. Memory grows only
/// with the bytes present. Returns `Err(Cancelled)` once `cancel` is set
/// (looked at first and every 65 536 packets).
pub fn read(
    bytes: &[u8],
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<(ClipFile, Report), IoError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(IoError::Cancelled);
    }
    let mut rest = bytes.strip_prefix(MAGIC).ok_or_else(|| refused(NOT_CLIP))?;
    let mut r = Reader::new(limits, cancel);
    let mut phase = Phase::Profiles;
    let mut tpq = 0u16;
    let mut profiles = false;
    let mut header_len = 0usize;
    let mut tick = 0u64;
    let mut ended = false;

    while let Some(next) = ump::next_packet(rest) {
        let (p, after) = next?;
        rest = after;
        r.look_at_cancel()?;
        let d = ump::decode(&p);
        match (phase, d) {
            (_, Decoded::Noop) => {}
            (Phase::Profiles, Decoded::Dcs(_)) => {}
            (Phase::Profiles, Decoded::Dctpq(0)) => return Err(refused(ZERO_TPQ)),
            (Phase::Profiles, Decoded::Dctpq(t)) => {
                tpq = t;
                phase = Phase::Header;
            }
            (Phase::Profiles, Decoded::StartOfClip | Decoded::EndOfClip) => {
                return Err(refused(NO_TPQ));
            }
            (Phase::Profiles, _) => {
                r.count()?;
                profiles = true;
            }
            // Header messages all sit at tick 0.
            (Phase::Header, Decoded::Dcs(_)) => {}
            (Phase::Sequence, Decoded::Dcs(n)) => {
                // Within the limits this cannot overflow (F4).
                tick = tick
                    .checked_add(u64::from(n))
                    .ok_or_else(|| refused(TOO_LONG))?;
            }
            (Phase::Header, Decoded::StartOfClip) => {
                phase = Phase::Sequence;
                header_len = r.events.len();
            }
            (Phase::Sequence, Decoded::StartOfClip) => return Err(refused(SECOND_START)),
            (Phase::Header, Decoded::EndOfClip) => return Err(refused(NO_START)),
            (Phase::Sequence, Decoded::EndOfClip) => {
                ended = true;
                break;
            }
            (_, d) => {
                r.count()?;
                let t = if phase == Phase::Sequence { tick } else { 0 };
                r.message(p, d, t);
            }
        }
    }
    match phase {
        Phase::Profiles => return Err(refused(NO_TPQ)),
        Phase::Header => return Err(refused(NO_START)),
        Phase::Sequence => {}
    }
    r.finish();
    let Reader {
        mut events,
        mut dead,
        mut report,
        ..
    } = r;

    if profiles {
        report.add(ItemKind::Note, PROFILES, None);
    }
    dead.sort_unstable();
    dead.dedup();
    let header_len =
        header_len.saturating_sub(dead.iter().take_while(|i| **i < header_len).count());
    if !dead.is_empty() {
        let mut dead = dead.into_iter().peekable();
        events = events
            .into_iter()
            .enumerate()
            .filter(|(i, _)| dead.next_if_eq(i).is_none())
            .map(|(_, e)| e)
            .collect();
    }
    let sequence = events.split_off(header_len.min(events.len()));
    let (header, moved) = split_header(events);
    for _ in &moved {
        report.add(ItemKind::Changed, MOVED, None);
    }
    let mut events = moved;
    events.extend(sequence);

    let end = if ended {
        if !rest.is_empty() {
            report.add(ItemKind::Changed, AFTER_END, None);
        }
        tick
    } else {
        report.add(ItemKind::Changed, NO_END, None);
        events.last().map_or(0, |(t, _)| *t)
    };
    Ok((
        ClipFile {
            tpq,
            header,
            events,
            end,
        },
        report,
    ))
}

/// The header's first Set Tempo and first Set Time Signature (tempo first),
/// and the rest, at tick 0.
fn split_header(events: Vec<(u64, ClipEvent)>) -> (Vec<ClipEvent>, Vec<(u64, ClipEvent)>) {
    let mut tempo = None;
    let mut meter = None;
    let mut moved = Vec::new();
    for (_, e) in events {
        let kind = match &e {
            ClipEvent::Packet(p) => ump::decode(p),
            _ => Decoded::Unknown,
        };
        match kind {
            Decoded::SetTempo(_) if tempo.is_none() => tempo = Some(e),
            Decoded::TimeSignature { .. } if meter.is_none() => meter = Some(e),
            _ => moved.push((0, e)),
        }
    }
    (tempo.into_iter().chain(meter).collect(), moved)
}

/// A SysEx message whose last packet has not been read yet.
struct OpenSysEx {
    /// Where its event sits; `None` once it is faulty.
    index: Option<usize>,
    /// Data bytes so far.
    len: usize,
    /// Its start.
    tick: u64,
    /// Why it will be left out, once that is known.
    fault: Option<&'static str>,
}

/// A text whose last packet has not been read yet.
struct OpenText {
    /// Where its event sits.
    index: usize,
    /// Its bytes so far, without padding zeros, at most `cap` of them.
    raw: Vec<u8>,
    /// True once bytes past `cap` were dropped.
    cut: bool,
    /// Its start.
    tick: u64,
}

/// Which SysEx: 7-bit (keyed by group) or 8-bit (by group and stream).
#[derive(Clone, Copy)]
enum SysExKind {
    Seven,
    Eight,
}

struct Reader<'l> {
    limits: &'l Limits,
    cancel: &'l AtomicBool,
    /// Packets read, for the cancel flag.
    packets: usize,
    /// Packets counted against `limits.max_events`.
    counted: usize,
    events: Vec<(u64, ClipEvent)>,
    /// Indices of events found faulty, removed at the end.
    dead: Vec<usize>,
    report: Report,
    /// Open SysEx 7 by group (16).
    sysex7: Vec<Option<OpenSysEx>>,
    /// Open SysEx 8 by group × 256 + stream (4 096).
    sysex8: Vec<Option<OpenSysEx>>,
    /// Open texts by group (16).
    texts: Vec<Option<OpenText>>,
}

impl<'l> Reader<'l> {
    fn new(limits: &'l Limits, cancel: &'l AtomicBool) -> Self {
        Self {
            limits,
            cancel,
            packets: 0,
            counted: 0,
            events: Vec::new(),
            dead: Vec::new(),
            report: Report::default(),
            sysex7: (0..16).map(|_| None).collect(),
            sysex8: (0..4096).map(|_| None).collect(),
            texts: (0..16).map(|_| None).collect(),
        }
    }

    fn look_at_cancel(&mut self) -> Result<(), IoError> {
        self.packets = self.packets.wrapping_add(1);
        if self.packets % CANCEL_EVERY == 0 && self.cancel.load(Ordering::Relaxed) {
            return Err(IoError::Cancelled);
        }
        Ok(())
    }

    /// Counts one packet against the limit.
    fn count(&mut self) -> Result<(), IoError> {
        self.counted = self.counted.saturating_add(1);
        if self.counted > self.limits.max_events {
            return Err(IoError::Refused(format!(
                "This MIDI Clip File can't be read: it has more than {} events.",
                group(self.limits.max_events as u64)
            )));
        }
        Ok(())
    }

    /// One message packet at `tick`.
    fn message(&mut self, p: Packet, d: Decoded, tick: u64) {
        let g = p.group();
        let form = ((p.words().first().copied().unwrap_or(0) >> 20) & 0xF) as u8;
        match (p.mt(), d) {
            (0x3, Decoded::SysEx7 { form, data, .. }) => {
                self.sysex(SysExKind::Seven, g, 0, form, Some(&data), tick);
            }
            (0x3, _) if form <= 3 => self.sysex(SysExKind::Seven, g, 0, form, None, tick),
            (
                0x5,
                Decoded::SysEx8 {
                    form, stream, data, ..
                },
            ) => self.sysex(SysExKind::Eight, g, stream, form, Some(&data), tick),
            (0x5, Decoded::SysEx8Abort { stream, .. }) => {
                self.abort(SysExKind::Eight, g, stream, tick);
            }
            (0x5, _) if form <= 3 => {
                let stream = ((p.words().first().copied().unwrap_or(0) >> 8) & 0xFF) as u8;
                self.sysex(SysExKind::Eight, g, stream, form, None, tick);
            }
            (
                0xD,
                Decoded::Text {
                    form,
                    bank,
                    status,
                    data,
                    ..
                },
            ) => self.text(g, form, bank, status, &data, tick),
            (0x1 | 0x2 | 0x4 | 0xD, _) => self.events.push((tick, ClipEvent::Packet(p))),
            _ => self.report.add(ItemKind::LeftOut, UNKNOWN, at(tick)),
        }
    }

    fn slot(&mut self, kind: SysExKind, g: u8, stream: u8) -> Option<&mut Option<OpenSysEx>> {
        match kind {
            SysExKind::Seven => self.sysex7.get_mut(usize::from(g)),
            SysExKind::Eight => self
                .sysex8
                .get_mut(usize::from(g) * 256 + usize::from(stream)),
        }
    }

    /// One SysEx packet of `form`; `data` is `None` when its byte count is
    /// damaged.
    fn sysex(
        &mut self,
        kind: SysExKind,
        g: u8,
        stream: u8,
        form: u8,
        data: Option<&[u8]>,
        tick: u64,
    ) {
        let open = self.slot(kind, g, stream).and_then(Option::take);
        match form {
            0 | 1 => {
                if let Some(o) = open {
                    self.close(o, Some(SYSEX_UNFINISHED));
                }
                let index = self.events.len();
                let event = match kind {
                    SysExKind::Seven => ClipEvent::SysEx7 {
                        group: g,
                        data: Vec::new(),
                    },
                    SysExKind::Eight => ClipEvent::SysEx8 {
                        group: g,
                        data: Vec::new(),
                    },
                };
                self.events.push((tick, event));
                let mut o = OpenSysEx {
                    index: Some(index),
                    len: 0,
                    tick,
                    fault: None,
                };
                self.feed(&mut o, data);
                if form == 0 {
                    self.close(o, None);
                } else if let Some(slot) = self.slot(kind, g, stream) {
                    *slot = Some(o);
                }
            }
            _ => match open {
                None => {
                    self.report
                        .add(ItemKind::LeftOut, SYSEX_UNFINISHED, at(tick));
                }
                Some(mut o) => {
                    self.feed(&mut o, data);
                    if form == 3 {
                        self.close(o, None);
                    } else if let Some(slot) = self.slot(kind, g, stream) {
                        *slot = Some(o);
                    }
                }
            },
        }
    }

    /// A SysEx 8 end packet that abandons its message.
    fn abort(&mut self, kind: SysExKind, g: u8, stream: u8, tick: u64) {
        match self.slot(kind, g, stream).and_then(Option::take) {
            Some(o) => self.close(o, Some(SYSEX_UNFINISHED)),
            None => self
                .report
                .add(ItemKind::LeftOut, SYSEX_UNFINISHED, at(tick)),
        }
    }

    /// Adds `data` to `o`, or marks it damaged or over the limit.
    fn feed(&mut self, o: &mut OpenSysEx, data: Option<&[u8]>) {
        let Some(data) = data else {
            self.fault(o, SYSEX_DAMAGED);
            return;
        };
        if o.fault.is_some() {
            return;
        }
        o.len = o.len.saturating_add(data.len());
        if o.len > self.limits.max_sysex_bytes {
            self.fault(o, SYSEX_OVER);
            return;
        }
        let event = o.index.and_then(|i| self.events.get_mut(i));
        if let Some((_, ClipEvent::SysEx7 { data: v, .. } | ClipEvent::SysEx8 { data: v, .. })) =
            event
        {
            v.extend_from_slice(data);
        }
    }

    /// Marks `o` as left out for `why` (the first reason stays) and frees
    /// its bytes.
    fn fault(&mut self, o: &mut OpenSysEx, why: &'static str) {
        if o.fault.is_some() {
            return;
        }
        o.fault = Some(why);
        if let Some(i) = o.index.take() {
            if let Some((_, ClipEvent::SysEx7 { data, .. } | ClipEvent::SysEx8 { data, .. })) =
                self.events.get_mut(i)
            {
                *data = Vec::new();
            }
            self.dead.push(i);
        }
    }

    /// Ends `o`: kept unless it is faulty or `why` says it is.
    fn close(&mut self, mut o: OpenSysEx, why: Option<&'static str>) {
        if let Some(w) = why {
            self.fault(&mut o, w);
        }
        if let Some(w) = o.fault {
            self.report.add(ItemKind::LeftOut, w, at(o.tick));
        }
    }

    /// One Flex Data text packet.
    fn text(&mut self, g: u8, form: u8, bank: u8, status: u8, data: &[u8; 12], tick: u64) {
        let open = self.texts.get_mut(usize::from(g)).and_then(Option::take);
        let mut o = match (form, open) {
            (0 | 1, open) => {
                if let Some(o) = open {
                    self.unfinished_text(o);
                }
                let index = self.events.len();
                let text = String::new();
                self.events.push((
                    tick,
                    ClipEvent::Text {
                        group: g,
                        bank,
                        status,
                        text,
                    },
                ));
                OpenText {
                    index,
                    raw: Vec::new(),
                    cut: false,
                    tick,
                }
            }
            (_, Some(o)) => o,
            (_, None) => {
                self.report
                    .add(ItemKind::LeftOut, TEXT_UNFINISHED, at(tick));
                return;
            }
        };
        // Enough raw bytes for any text of the limit, after cleaning.
        let cap = self
            .limits
            .max_text_bytes
            .saturating_mul(4)
            .saturating_add(4);
        for &b in data.iter().filter(|b| **b != 0) {
            if o.raw.len() >= cap {
                o.cut = true;
                break;
            }
            o.raw.push(b);
        }
        if form == 0 || form == 3 {
            let (s, cut) = sanitize(&o.raw, self.limits);
            if cut || o.cut {
                self.report.add(ItemKind::Changed, TEXT_CUT, at(o.tick));
            }
            if let Some((_, ClipEvent::Text { text, .. })) = self.events.get_mut(o.index) {
                *text = s;
            }
        } else if let Some(slot) = self.texts.get_mut(usize::from(g)) {
            *slot = Some(o);
        }
    }

    fn unfinished_text(&mut self, o: OpenText) {
        self.dead.push(o.index);
        self.report
            .add(ItemKind::LeftOut, TEXT_UNFINISHED, at(o.tick));
    }

    /// Leaves out every message still open.
    fn finish(&mut self) {
        let open: Vec<OpenSysEx> = self
            .sysex7
            .iter_mut()
            .chain(self.sysex8.iter_mut())
            .filter_map(Option::take)
            .collect();
        for o in open {
            self.close(o, Some(SYSEX_UNFINISHED));
        }
        let texts: Vec<OpenText> = self.texts.iter_mut().filter_map(Option::take).collect();
        for o in texts {
            self.unfinished_text(o);
        }
    }
}

/// The packets of `e`.
fn packets_of(e: &ClipEvent) -> Vec<Packet> {
    match e {
        ClipEvent::Packet(p) => vec![*p],
        ClipEvent::SysEx7 { group, data } => ump::sysex7(*group, data),
        ClipEvent::SysEx8 { group, data } => ump::sysex8(*group, 0, data),
        ClipEvent::Text {
            group,
            bank,
            status,
            text,
        } => ump::text(*bank, *status, *group, text),
    }
}

/// Writes a gap of `ticks`: `DCS 0xFFFFF` + NOOP while it is longer, then
/// the rest.
fn put_gap(out: &mut Vec<u8>, mut ticks: u64) {
    let max = u64::from(MAX_DCS);
    while ticks > max {
        ump::write_packets(&[ump::dcs(MAX_DCS), ump::noop()], out);
        ticks -= max;
    }
    let rest = u32::try_from(ticks).unwrap_or(MAX_DCS);
    ump::write_packets(&[ump::dcs(rest)], out);
}

/// Writes `c` as a MIDI Clip File: `DCS 0` + DCTPQ, each header message
/// with `DCS 0`, `DCS 0` + Start of Clip, each event with its own DCS (gaps
/// over 1 048 575 ticks restarted with NOOP), then End of Clip at
/// `c.end`. SysEx 8 is written as stream 0 and texts are addressed to their
/// group. Ticks out of order are written as gaps of 0, and an `end` before
/// the last event as the last event's tick. A `Packet` holding SysEx or
/// text is written as it is (`read` gives it back assembled).
pub fn write(c: &ClipFile) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    ump::write_packets(&[ump::dcs(0), ump::dctpq(c.tpq)], &mut out);
    for e in &c.header {
        ump::write_packets(&[ump::dcs(0)], &mut out);
        ump::write_packets(&packets_of(e), &mut out);
    }
    ump::write_packets(&[ump::dcs(0), ump::start_of_clip()], &mut out);
    let mut prev = 0u64;
    for (tick, e) in &c.events {
        put_gap(&mut out, tick.saturating_sub(prev));
        prev = prev.max(*tick);
        ump::write_packets(&packets_of(e), &mut out);
    }
    put_gap(&mut out, c.end.saturating_sub(prev));
    ump::write_packets(&[ump::end_of_clip()], &mut out);
    out
}
