//! Universal MIDI Packets: one packet type, builders and a decoder for the
//! messages a MIDI Clip File carries.
//!
//! Every layout here was checked against *M2-104-UM UMP Format and MIDI 2.0
//! Protocol v1.1.2* (2023-10-27): packet sizes (Table 4), Delta Clockstamps
//! (figures 33, 34), Start and End of Clip (24, 25), MIDI 2.0 channel voice
//! (46-66), Flex Data (69-77, Tables 9-11 and 16), SysEx 7 (84, Table 18)
//! and SysEx 8 (85, Table 19). Builders mask each field to its width.

use crate::IoError;

/// One packet of 1 to 4 words; the words past its length are zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Packet {
    words: [u32; 4],
    len: u8,
}

impl Packet {
    /// The packet of `words` if their count is the one its message type
    /// takes ([`packet_words`]).
    pub fn new(words: &[u32]) -> Option<Packet> {
        let first = *words.first()?;
        if words.len() != packet_words(mt_of(first)) {
            return None;
        }
        let mut w = [0u32; 4];
        for (slot, word) in w.iter_mut().zip(words) {
            *slot = *word;
        }
        Some(Packet {
            words: w,
            len: u8::try_from(words.len()).ok()?,
        })
    }

    /// Its words, in order.
    pub fn words(&self) -> &[u32] {
        self.words
            .get(..usize::from(self.len))
            .unwrap_or(&self.words)
    }

    /// Its message type (bits 31-28 of word 0).
    pub fn mt(&self) -> u8 {
        mt_of(self.word(0))
    }

    /// Its group (bits 27-24 of word 0). Utility (MT 0x0) and UMP Stream
    /// (MT 0xF) messages have no group; for them this is whatever the bits hold.
    pub fn group(&self) -> u8 {
        nibble(self.word(0), 24)
    }

    /// Word `i`, or 0 past its length.
    fn word(&self, i: usize) -> u32 {
        self.words().get(i).copied().unwrap_or(0)
    }

    /// The bytes of words 1.. (big-endian), then nothing past the packet.
    fn tail_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.words().iter().skip(1).flat_map(|w| w.to_be_bytes())
    }
}

fn mt_of(w: u32) -> u8 {
    nibble(w, 28)
}

fn nibble(w: u32, shift: u32) -> u8 {
    ((w >> shift) & 0xF) as u8
}

fn byte(w: u32, shift: u32) -> u8 {
    ((w >> shift) & 0xFF) as u8
}

/// The number of 32-bit words a packet of message type `mt` (low 4 bits)
/// takes (M2-104-UM Table 4).
pub fn packet_words(mt: u8) -> usize {
    match mt & 0xF {
        0x0 | 0x1 | 0x2 | 0x6 | 0x7 => 1,
        0x3 | 0x4 | 0x8 | 0x9 | 0xA => 2,
        0xB | 0xC => 3,
        _ => 4,
    }
}

/// "…ends in the middle of a message."
pub(crate) const CUT_OFF: &str =
    "This MIDI Clip File can't be read: it ends in the middle of a message.";

/// The packet at the start of `bytes` and the bytes after it; `None` when
/// `bytes` is empty, `Err` when the packet is cut off.
pub(crate) fn next_packet(bytes: &[u8]) -> Option<Result<(Packet, &[u8]), IoError>> {
    if bytes.is_empty() {
        return None;
    }
    let Some(head) = bytes.first_chunk::<4>() else {
        return Some(Err(IoError::Refused(CUT_OFF.into())));
    };
    let n = packet_words(mt_of(u32::from_be_bytes(*head)));
    let Some((body, rest)) = n.checked_mul(4).and_then(|len| bytes.split_at_checked(len)) else {
        return Some(Err(IoError::Refused(CUT_OFF.into())));
    };
    let mut words = [0u32; 4];
    for (slot, chunk) in words.iter_mut().zip(body.chunks_exact(4)) {
        if let Some(c) = chunk.first_chunk::<4>() {
            *slot = u32::from_be_bytes(*c);
        }
    }
    let len = u8::try_from(n).unwrap_or(4);
    Some(Ok((Packet { words, len }, rest)))
}

/// Reads big-endian packets until `bytes` ends; a packet cut off at the end
/// refuses. Memory grows with the bytes present.
pub fn read_packets(bytes: &[u8]) -> Result<Vec<Packet>, IoError> {
    let mut out = Vec::new();
    let mut rest = bytes;
    while let Some(next) = next_packet(rest) {
        let (p, after) = next?;
        out.push(p);
        rest = after;
    }
    Ok(out)
}

/// Appends `p`'s words to `out`, big-endian.
pub fn write_packets(p: &[Packet], out: &mut Vec<u8>) {
    for packet in p {
        for w in packet.words() {
            out.extend_from_slice(&w.to_be_bytes());
        }
    }
}

/// A note's attribute (M2-104-UM 7.4.14): type 1 to 255 and its data.
/// Type 0 means no attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteAttribute {
    /// The attribute type (1 manufacturer, 2 profile, 3 pitch 7.9, …).
    pub kind: u8,
    /// The attribute data.
    pub data: u16,
}

/// One packet, decoded. Fields are as in the packet, unjudged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decoded {
    /// NOOP (MT 0x0, status 0x0).
    Noop,
    /// Delta Clockstamp Ticks Per Quarter Note.
    Dctpq(u16),
    /// Delta Clockstamp: ticks since the last event (20 bits).
    Dcs(u32),
    /// A MIDI 1.0 channel voice message (MT 0x2).
    Midi1 {
        /// Group.
        group: u8,
        /// Status byte with channel (0x80 to 0xEF).
        status: u8,
        /// First data byte.
        data1: u8,
        /// Second data byte (0 for two-byte messages).
        data2: u8,
    },
    /// One SysEx 7 packet (MT 0x3) with a byte count of 0 to 6.
    SysEx7 {
        /// Group.
        group: u8,
        /// 0 complete, 1 start, 2 continue, 3 end.
        form: u8,
        /// Its data bytes.
        data: Vec<u8>,
    },
    /// One SysEx 8 packet (MT 0x5) with a byte count of 1 to 14.
    SysEx8 {
        /// Group.
        group: u8,
        /// 0 complete, 1 start, 2 continue, 3 end.
        form: u8,
        /// Stream ID.
        stream: u8,
        /// Its data bytes, after the stream ID.
        data: Vec<u8>,
    },
    /// A SysEx 8 end packet with byte count 0xF: the message is abandoned.
    SysEx8Abort {
        /// Group.
        group: u8,
        /// Stream ID.
        stream: u8,
    },
    /// MIDI 2.0 Note Off.
    NoteOff {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// Note number.
        note: u8,
        /// 16-bit velocity.
        velocity: u16,
        /// Attribute, if its type is not 0.
        attribute: Option<NoteAttribute>,
    },
    /// MIDI 2.0 Note On.
    NoteOn {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// Note number.
        note: u8,
        /// 16-bit velocity.
        velocity: u16,
        /// Attribute, if its type is not 0.
        attribute: Option<NoteAttribute>,
    },
    /// MIDI 2.0 Poly Pressure.
    PolyPressure {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// Note number.
        note: u8,
        /// 32-bit value.
        value: u32,
    },
    /// MIDI 2.0 Control Change.
    ControlChange {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// Controller index.
        index: u8,
        /// 32-bit value.
        value: u32,
    },
    /// MIDI 2.0 Program Change.
    ProgramChange {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// Program.
        program: u8,
        /// Bank MSB and LSB, if the bank-valid flag is set.
        bank: Option<(u8, u8)>,
    },
    /// MIDI 2.0 Channel Pressure.
    ChannelPressure {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// 32-bit value.
        value: u32,
    },
    /// MIDI 2.0 Pitch Bend (centre 0x8000_0000).
    PitchBend {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// 32-bit value.
        value: u32,
    },
    /// MIDI 2.0 Registered Per-Note Controller.
    PerNoteRegistered {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// Note number.
        note: u8,
        /// Controller index.
        index: u8,
        /// 32-bit value.
        value: u32,
    },
    /// MIDI 2.0 Assignable Per-Note Controller.
    PerNoteAssignable {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// Note number.
        note: u8,
        /// Controller index.
        index: u8,
        /// 32-bit value.
        value: u32,
    },
    /// MIDI 2.0 Per-Note Pitch Bend (centre 0x8000_0000).
    PerNotePitchBend {
        /// Group.
        group: u8,
        /// Channel 0-15.
        channel: u8,
        /// Note number.
        note: u8,
        /// 32-bit value.
        value: u32,
    },
    /// Flex Data Set Tempo: 10-ns units per quarter note.
    SetTempo(u32),
    /// Flex Data Set Time Signature.
    TimeSignature {
        /// Beats per bar.
        numerator: u8,
        /// The denominator as a power of 2 (2 is a quarter note; 0 is non-standard).
        denominator: u8,
        /// 1/32 notes in 24 MIDI clocks (0 unknown).
        thirty_seconds: u8,
    },
    /// Flex Data Set Key Signature.
    KeySignature {
        /// Sharps (positive) or flats (negative), -8 (unknown) to 7.
        sharps: i8,
        /// Tonic letter: 0 unknown, 1 A, 2 B, … 7 G.
        tonic: u8,
    },
    /// One Flex Data text packet (status bank 0x01 or 0x02).
    Text {
        /// Group.
        group: u8,
        /// 0 complete, 1 start, 2 continue, 3 end.
        form: u8,
        /// Status bank: 0x01 metadata, 0x02 performance.
        bank: u8,
        /// Status within the bank.
        status: u8,
        /// The 12 payload bytes, zero-padded.
        data: [u8; 12],
    },
    /// Start of Clip.
    StartOfClip,
    /// End of Clip.
    EndOfClip,
    /// Anything else.
    Unknown,
}

// Utility (MT 0x0) statuses, bits 23-20 (figures 33, 34).
const UTIL_NOOP: u32 = 0x0;
const UTIL_DCTPQ: u32 = 0x3;
const UTIL_DCS: u32 = 0x4;
/// The largest Delta Clockstamp.
pub const MAX_DCS: u32 = 0x000F_FFFF;
// MIDI 2.0 channel voice opcodes (MT 0x4, bits 23-20).
const OP_PN_REGISTERED: u8 = 0x0;
const OP_PN_ASSIGNABLE: u8 = 0x1;
const OP_PN_PITCH_BEND: u8 = 0x6;
const OP_NOTE_OFF: u8 = 0x8;
const OP_NOTE_ON: u8 = 0x9;
const OP_POLY_PRESSURE: u8 = 0xA;
const OP_CONTROL_CHANGE: u8 = 0xB;
const OP_PROGRAM: u8 = 0xC;
const OP_CHANNEL_PRESSURE: u8 = 0xD;
const OP_PITCH_BEND: u8 = 0xE;
// Flex Data (MT 0xD): status bank and status (Tables 11, 16).
const FLEX_SETUP: u8 = 0x00;
const FLEX_TEMPO: u8 = 0x00;
const FLEX_TIME_SIG: u8 = 0x01;
const FLEX_KEY_SIG: u8 = 0x05;
/// Flex Data status bank for metadata text (0x00 unknown, 0x01 project name,
/// 0x02 composition name, 0x03 MIDI clip name, 0x04 copyright, …).
pub const BANK_METADATA: u8 = 0x01;
/// Flex Data status bank for performance text (0x01 lyrics, …).
pub const BANK_PERFORMANCE: u8 = 0x02;
/// Flex Data address "group" (bits 21-20 = 1).
const FLEX_ADDR_GROUP: u32 = 1;
// UMP Stream (MT 0xF) statuses, bits 25-16 (figures 24, 25).
const STREAM_START_OF_CLIP: u32 = 0x020;
const STREAM_END_OF_CLIP: u32 = 0x021;

fn w0(mt: u8, group: u8, b2: u8, b3: u8, b4: u8) -> u32 {
    (u32::from(mt & 0xF) << 28)
        | (u32::from(group & 0xF) << 24)
        | (u32::from(b2) << 16)
        | (u32::from(b3) << 8)
        | u32::from(b4)
}

/// A packet whose words are known to fit its type (built here).
fn packet(words: &[u32]) -> Packet {
    // Every caller passes as many words as its message type takes.
    Packet::new(words).unwrap_or(Packet {
        words: [0; 4],
        len: 1,
    })
}

fn utility(status: u32, data: u32) -> Packet {
    packet(&[(status << 20) | (data & MAX_DCS)])
}

/// NOOP.
pub fn noop() -> Packet {
    utility(UTIL_NOOP, 0)
}

/// Delta Clockstamp Ticks Per Quarter Note.
pub fn dctpq(tpq: u16) -> Packet {
    utility(UTIL_DCTPQ, u32::from(tpq))
}

/// Delta Clockstamp of `ticks` (masked to 20 bits; at most [`MAX_DCS`]).
pub fn dcs(ticks: u32) -> Packet {
    utility(UTIL_DCS, ticks)
}

/// A MIDI 1.0 channel voice message on `group` (MT 0x2).
pub fn midi1(group: u8, status: u8, data1: u8, data2: u8) -> Packet {
    packet(&[w0(0x2, group, status, data1 & 0x7F, data2 & 0x7F)])
}

/// A MIDI 2.0 channel voice message: `opcode`, `channel`, bytes 3 and 4 of
/// word 0 and word 1.
fn cv2(group: u8, opcode: u8, channel: u8, b3: u8, b4: u8, word1: u32) -> Packet {
    let status = ((opcode & 0xF) << 4) | (channel & 0xF);
    packet(&[w0(0x4, group, status, b3, b4), word1])
}

fn note(op: u8, group: u8, ch: u8, pitch: u8, vel16: u16, attr: Option<NoteAttribute>) -> Packet {
    let (kind, data) = attr.map_or((0, 0), |a| (a.kind, a.data));
    let word1 = (u32::from(vel16) << 16) | u32::from(data);
    cv2(group, op, ch, pitch & 0x7F, kind, word1)
}

/// MIDI 2.0 Note On.
pub fn note_on(group: u8, ch: u8, pitch: u8, vel16: u16, attr: Option<NoteAttribute>) -> Packet {
    note(OP_NOTE_ON, group, ch, pitch, vel16, attr)
}

/// MIDI 2.0 Note Off.
pub fn note_off(group: u8, ch: u8, pitch: u8, vel16: u16, attr: Option<NoteAttribute>) -> Packet {
    note(OP_NOTE_OFF, group, ch, pitch, vel16, attr)
}

/// MIDI 2.0 Poly Pressure.
pub fn poly_pressure(group: u8, ch: u8, pitch: u8, value: u32) -> Packet {
    cv2(group, OP_POLY_PRESSURE, ch, pitch & 0x7F, 0, value)
}

/// MIDI 2.0 Control Change.
pub fn control_change(group: u8, ch: u8, index: u8, value: u32) -> Packet {
    cv2(group, OP_CONTROL_CHANGE, ch, index & 0x7F, 0, value)
}

/// MIDI 2.0 Program Change, with a bank (MSB, LSB) if given.
pub fn program_change(group: u8, ch: u8, program: u8, bank: Option<(u8, u8)>) -> Packet {
    let (flags, msb, lsb) = bank.map_or((0, 0, 0), |(m, l)| (1, m & 0x7F, l & 0x7F));
    let word1 = (u32::from(program & 0x7F) << 24) | (u32::from(msb) << 8) | u32::from(lsb);
    cv2(group, OP_PROGRAM, ch, 0, flags, word1)
}

/// MIDI 2.0 Channel Pressure.
pub fn channel_pressure(group: u8, ch: u8, value: u32) -> Packet {
    cv2(group, OP_CHANNEL_PRESSURE, ch, 0, 0, value)
}

/// MIDI 2.0 Pitch Bend.
pub fn pitch_bend(group: u8, ch: u8, value: u32) -> Packet {
    cv2(group, OP_PITCH_BEND, ch, 0, 0, value)
}

/// MIDI 2.0 Registered Per-Note Controller.
pub fn per_note_registered(group: u8, ch: u8, pitch: u8, index: u8, value: u32) -> Packet {
    cv2(group, OP_PN_REGISTERED, ch, pitch & 0x7F, index, value)
}

/// MIDI 2.0 Assignable Per-Note Controller.
pub fn per_note_assignable(group: u8, ch: u8, pitch: u8, index: u8, value: u32) -> Packet {
    cv2(group, OP_PN_ASSIGNABLE, ch, pitch & 0x7F, index, value)
}

/// MIDI 2.0 Per-Note Pitch Bend.
pub fn per_note_pitch_bend(group: u8, ch: u8, pitch: u8, value: u32) -> Packet {
    cv2(group, OP_PN_PITCH_BEND, ch, pitch & 0x7F, 0, value)
}

/// Word 0 of a Flex Data packet addressed to `group`.
fn flex_w0(group: u8, form: u8, bank: u8, status: u8) -> u32 {
    (0xD << 28)
        | (u32::from(group & 0xF) << 24)
        | (u32::from(form & 3) << 22)
        | (FLEX_ADDR_GROUP << 20)
        | (u32::from(bank) << 8)
        | u32::from(status)
}

/// Flex Data Set Tempo on group 0: `ten_ns_per_quarter` 10-ns units per
/// quarter note (50 000 000 is 120 BPM).
pub fn set_tempo(ten_ns_per_quarter: u32) -> Packet {
    packet(&[
        flex_w0(0, 0, FLEX_SETUP, FLEX_TEMPO),
        ten_ns_per_quarter,
        0,
        0,
    ])
}

/// Flex Data Set Time Signature on group 0: `numerator`, the denominator
/// as a power of 2 (2 for quarters) and 1/32 notes per 24 MIDI clocks.
pub fn time_sig(numerator: u8, denominator_pow2: u8, thirty_seconds: u8) -> Packet {
    let word1 = (u32::from(numerator) << 24)
        | (u32::from(denominator_pow2) << 16)
        | (u32::from(thirty_seconds) << 8);
    packet(&[flex_w0(0, 0, FLEX_SETUP, FLEX_TIME_SIG), word1, 0, 0])
}

/// Tonic letters along the circle of fifths: F C G D A E B (A = 1 … G = 7).
const FIFTHS: [u8; 7] = [6, 3, 7, 4, 1, 5, 2];

/// The tonic letter of the key with `sharps` (negative for flats; -7 to 7),
/// major or its relative minor (C major 3, A minor 1). 0 (unknown) outside
/// -7 to 7.
pub fn tonic(sharps: i8, minor: bool) -> u8 {
    if !(-7..=7).contains(&sharps) {
        return 0;
    }
    let step = if minor { 4 } else { 1 };
    let i = (i32::from(sharps) + step).rem_euclid(7);
    usize::try_from(i)
        .ok()
        .and_then(|i| FIFTHS.get(i).copied())
        .unwrap_or(0)
}

/// True when `tonic` is that of the relative minor of the key with `sharps`.
pub fn is_minor(sharps: i8, tonic_letter: u8) -> bool {
    tonic_letter != 0 && tonic(sharps, true) == tonic_letter
}

/// Flex Data Set Key Signature on group 0: `sharps` (negative for flats)
/// and the tonic of the major key or, when `minor`, of its relative minor.
pub fn key_sig(sharps: i8, minor: bool) -> Packet {
    let sf = u32::from((sharps as u8) & 0xF);
    let word1 = (sf << 28) | (u32::from(tonic(sharps, minor)) << 24);
    packet(&[flex_w0(0, 0, FLEX_SETUP, FLEX_KEY_SIG), word1, 0, 0])
}

/// The form (0 complete, 1 start, 2 continue, 3 end) of part `i` of `n`.
fn form(i: usize, n: usize) -> u8 {
    match (i, n) {
        (_, 0 | 1) => 0,
        (0, _) => 1,
        _ if i.saturating_add(1) == n => 3,
        _ => 2,
    }
}

/// The packet whose bytes (big-endian, zero-padded) begin with `bytes`.
fn from_bytes(bytes: &[u8]) -> Packet {
    let mut w = [0u32; 4];
    for (i, b) in bytes.iter().enumerate().take(16) {
        if let Some(slot) = w.get_mut(i / 4) {
            *slot |= u32::from(*b) << (24 - 8 * (i % 4));
        }
    }
    let [first, ..] = w;
    let n = packet_words(mt_of(first));
    packet(w.get(..n).unwrap_or(&w))
}

/// `data` in parts of `size` bytes; nothing is one empty part.
fn parts(data: &[u8], size: usize) -> Vec<&[u8]> {
    if data.is_empty() {
        vec![&[]]
    } else {
        data.chunks(size).collect()
    }
}

/// Flex Data text on `group`, in as many packets as it needs (12 bytes
/// each, the last zero-padded): status `bank` 0x01 (metadata) or 0x02
/// (performance), then `status`. Empty text is one packet of zeros.
///
/// The packets form one message whatever its length; M2-104-UM 7.5.1 caps
/// a Flex Data message at 32 packets (384 bytes), which callers keep to.
pub fn text(bank: u8, status: u8, group: u8, s: &str) -> Vec<Packet> {
    let p = parts(s.as_bytes(), 12);
    let n = p.len();
    p.iter()
        .enumerate()
        .map(|(i, c)| {
            let mut b = flex_w0(group, form(i, n), bank, status)
                .to_be_bytes()
                .to_vec();
            b.extend_from_slice(c);
            from_bytes(&b)
        })
        .collect()
}

/// SysEx 7 on `group`: `data` (no `F0` or `F7`) in packets of 6 bytes.
pub fn sysex7(group: u8, data: &[u8]) -> Vec<Packet> {
    let p = parts(data, 6);
    let n = p.len();
    p.iter()
        .enumerate()
        .map(|(i, c)| {
            let mut b = vec![0x30 | (group & 0xF), (form(i, n) << 4) | c.len() as u8];
            b.extend_from_slice(c);
            from_bytes(&b)
        })
        .collect()
}

/// SysEx 8 on `group` and `stream`: `data` in packets of 13 bytes after
/// the stream ID (the byte count includes the stream ID).
pub fn sysex8(group: u8, stream: u8, data: &[u8]) -> Vec<Packet> {
    let p = parts(data, 13);
    let n = p.len();
    p.iter()
        .enumerate()
        .map(|(i, c)| {
            let count = c.len() as u8 + 1;
            let mut b = vec![0x50 | (group & 0xF), (form(i, n) << 4) | count, stream];
            b.extend_from_slice(c);
            from_bytes(&b)
        })
        .collect()
}

/// Start of Clip.
pub fn start_of_clip() -> Packet {
    packet(&[(0xF << 28) | (STREAM_START_OF_CLIP << 16), 0, 0, 0])
}

/// End of Clip.
pub fn end_of_clip() -> Packet {
    packet(&[(0xF << 28) | (STREAM_END_OF_CLIP << 16), 0, 0, 0])
}

/// What `p` is (see [`Decoded`]).
pub fn decode(p: &Packet) -> Decoded {
    let w = p.word(0);
    let group = p.group();
    match p.mt() {
        0x0 => {
            let data = w & MAX_DCS;
            match (w >> 20) & 0xF {
                UTIL_NOOP if w & 0x0FFF_FFFF == 0 => Decoded::Noop,
                UTIL_DCTPQ => Decoded::Dctpq((data & 0xFFFF) as u16),
                UTIL_DCS => Decoded::Dcs(data),
                _ => Decoded::Unknown,
            }
        }
        0x2 => match byte(w, 16) {
            status @ 0x80..=0xEF => Decoded::Midi1 {
                group,
                status,
                data1: byte(w, 8),
                data2: byte(w, 0),
            },
            _ => Decoded::Unknown,
        },
        0x3 => {
            let (form, count) = (nibble(w, 20), usize::from(nibble(w, 16)));
            if form > 3 || count > 6 {
                return Decoded::Unknown;
            }
            let data = [byte(w, 8), byte(w, 0)]
                .into_iter()
                .chain(p.word(1).to_be_bytes())
                .take(count)
                .collect();
            Decoded::SysEx7 { group, form, data }
        }
        0x4 => decode_cv2(p),
        0x5 => {
            let (form, count) = (nibble(w, 20), usize::from(nibble(w, 16)));
            let stream = byte(w, 8);
            if form == 3 && count == 0xF {
                return Decoded::SysEx8Abort { group, stream };
            }
            if form > 3 || !(1..=14).contains(&count) {
                return Decoded::Unknown;
            }
            let data = std::iter::once(byte(w, 0))
                .chain(p.tail_bytes())
                .take(count - 1)
                .collect();
            Decoded::SysEx8 {
                group,
                form,
                stream,
                data,
            }
        }
        0xD => decode_flex(p),
        0xF => match (w >> 16) & 0x3FF {
            STREAM_START_OF_CLIP if (w >> 26) & 3 == 0 => Decoded::StartOfClip,
            STREAM_END_OF_CLIP if (w >> 26) & 3 == 0 => Decoded::EndOfClip,
            _ => Decoded::Unknown,
        },
        _ => Decoded::Unknown,
    }
}

fn decode_cv2(p: &Packet) -> Decoded {
    let w = p.word(0);
    let v = p.word(1);
    let (group, channel) = (p.group(), nibble(w, 16));
    let (b3, b4) = (byte(w, 8), byte(w, 0));
    let note = b3 & 0x7F;
    match nibble(w, 20) {
        op @ (OP_NOTE_OFF | OP_NOTE_ON) => {
            let velocity = (v >> 16) as u16;
            let attribute = (b4 != 0).then_some(NoteAttribute {
                kind: b4,
                data: (v & 0xFFFF) as u16,
            });
            if op == OP_NOTE_ON {
                Decoded::NoteOn {
                    group,
                    channel,
                    note,
                    velocity,
                    attribute,
                }
            } else {
                Decoded::NoteOff {
                    group,
                    channel,
                    note,
                    velocity,
                    attribute,
                }
            }
        }
        OP_POLY_PRESSURE => Decoded::PolyPressure {
            group,
            channel,
            note,
            value: v,
        },
        OP_CONTROL_CHANGE => Decoded::ControlChange {
            group,
            channel,
            index: note,
            value: v,
        },
        OP_PROGRAM => Decoded::ProgramChange {
            group,
            channel,
            program: byte(v, 24) & 0x7F,
            bank: (b4 & 1 == 1).then_some((byte(v, 8) & 0x7F, byte(v, 0) & 0x7F)),
        },
        OP_CHANNEL_PRESSURE => Decoded::ChannelPressure {
            group,
            channel,
            value: v,
        },
        OP_PITCH_BEND => Decoded::PitchBend {
            group,
            channel,
            value: v,
        },
        OP_PN_REGISTERED => Decoded::PerNoteRegistered {
            group,
            channel,
            note,
            index: b4,
            value: v,
        },
        OP_PN_ASSIGNABLE => Decoded::PerNoteAssignable {
            group,
            channel,
            note,
            index: b4,
            value: v,
        },
        OP_PN_PITCH_BEND => Decoded::PerNotePitchBend {
            group,
            channel,
            note,
            value: v,
        },
        _ => Decoded::Unknown,
    }
}

fn decode_flex(p: &Packet) -> Decoded {
    let w = p.word(0);
    let form = ((w >> 22) & 3) as u8;
    let (bank, status) = (byte(w, 8), byte(w, 0));
    let v = p.word(1);
    match (bank, status) {
        (FLEX_SETUP, FLEX_TEMPO) if form == 0 => Decoded::SetTempo(v),
        (FLEX_SETUP, FLEX_TIME_SIG) if form == 0 => Decoded::TimeSignature {
            numerator: byte(v, 24),
            denominator: byte(v, 16),
            thirty_seconds: byte(v, 8),
        },
        (FLEX_SETUP, FLEX_KEY_SIG) if form == 0 => Decoded::KeySignature {
            // A 4-bit two's complement value, sign-extended.
            sharps: ((byte(v, 24) as i8) >> 4),
            tonic: nibble(v, 24),
        },
        (BANK_METADATA | BANK_PERFORMANCE, _) => {
            let mut data = [0u8; 12];
            for (slot, b) in data.iter_mut().zip(p.tail_bytes()) {
                *slot = b;
            }
            Decoded::Text {
                group: p.group(),
                form,
                bank,
                status,
                data,
            }
        }
        _ => Decoded::Unknown,
    }
}
