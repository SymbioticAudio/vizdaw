//! MIDI 2.0 → MIDI 1.0 translation (M2-104 default rules), done at compile
//! time. Pure functions; no allocation.

use core_model::event::EventBody;

/// One MIDI 1.0 channel message of 1–3 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Midi1Msg {
    /// Number of valid bytes (1–3).
    pub len: u8,
    /// The bytes; unused ones are zero.
    pub bytes: [u8; 3],
}

impl Midi1Msg {
    /// A two-byte message (program change, channel pressure).
    pub const fn two(status: u8, d1: u8) -> Midi1Msg {
        Midi1Msg {
            len: 2,
            bytes: [status, d1, 0],
        }
    }

    /// A three-byte message.
    pub const fn three(status: u8, d1: u8, d2: u8) -> Midi1Msg {
        Midi1Msg {
            len: 3,
            bytes: [status, d1, d2],
        }
    }

    /// The valid bytes.
    pub fn as_slice(&self) -> &[u8] {
        let n = usize::from(self.len.min(3));
        self.bytes.get(..n).unwrap_or(&[])
    }
}

/// Sort class for events on the same frame: note-offs first, then patch
/// changes, then controllers, then note-ons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Class {
    /// Note off.
    NoteOff = 0,
    /// Bank select and program change.
    Patch = 1,
    /// CC, pitch bend, pressure, SysEx.
    Control = 2,
    /// Note on.
    NoteOn = 3,
}

impl Class {
    /// The class of a MIDI 1.0 message by its status byte.
    pub fn of(status: u8) -> Class {
        match status & 0xF0 {
            0x80 => Class::NoteOff,
            0x90 => Class::NoteOn,
            0xC0 => Class::Patch,
            _ => Class::Control,
        }
    }
}

/// When a translated message plays relative to its event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    /// At the event's tick.
    Start,
    /// At `tick + duration` (note-offs).
    End,
}

/// MIDI 2.0-only data that a translation could not carry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Drops {
    /// Per-note controllers and per-note pitch bend on the note.
    pub per_note: usize,
    /// A note attribute was present.
    pub attribute: bool,
    /// 8-bit SysEx was skipped.
    pub sysex8: bool,
}

/// One translated message with its timing and sort class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Out {
    /// Start or end of the event.
    pub when: When,
    /// Sort class (bank-select CCs, raw or from a program change, belong to
    /// [`Class::Patch`]).
    pub class: Class,
    /// The message.
    pub msg: Midi1Msg,
}

/// The result of translating one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Translated<'a> {
    msgs: [Out; 3],
    len: usize,
    /// A 7-bit SysEx payload (without `F0`/`F7`).
    pub sysex: Option<&'a [u8]>,
    /// What was dropped.
    pub drops: Drops,
}

impl<'a> Translated<'a> {
    fn empty() -> Translated<'a> {
        Translated {
            msgs: [Out {
                when: When::Start,
                class: Class::Control,
                msg: Midi1Msg::two(0, 0),
            }; 3],
            len: 0,
            sysex: None,
            drops: Drops::default(),
        }
    }

    fn push(&mut self, when: When, msg: Midi1Msg) {
        let class = Class::of(msg.bytes[0]);
        self.push_class(when, class, msg);
    }

    fn push_class(&mut self, when: When, class: Class, msg: Midi1Msg) {
        if let Some(slot) = self.msgs.get_mut(self.len) {
            *slot = Out { when, class, msg };
            self.len += 1;
        }
    }

    /// The channel messages, in output order.
    pub fn msgs(&self) -> &[Out] {
        self.msgs.get(..self.len).unwrap_or(&[])
    }
}

/// 16-bit note-on velocity to 7 bits; never 0 (0 would mean note-off).
pub fn note_on_velocity(v: u16) -> u8 {
    ((v >> 9) as u8).max(1)
}

/// 16-bit release velocity to 7 bits.
pub fn release_velocity(v: u16) -> u8 {
    (v >> 9) as u8
}

/// 32-bit controller or pressure value to 7 bits.
pub fn value7(v: u32) -> u8 {
    (v >> 25) as u8
}

/// 32-bit pitch bend to 14 bits as `(lsb, msb)`.
pub fn bend14(v: u32) -> (u8, u8) {
    let b = v >> 18;
    ((b & 0x7F) as u8, ((b >> 7) & 0x7F) as u8)
}

/// Translates one event body on `channel` (0–15).
pub fn translate(body: &EventBody, channel: u8) -> Translated<'_> {
    let ch = channel & 0x0F;
    let mut t = Translated::empty();
    match body {
        EventBody::Note {
            pitch,
            velocity,
            release_velocity: rel,
            attribute,
            per_note,
            ..
        } => {
            let p = pitch & 0x7F;
            t.push(
                When::Start,
                Midi1Msg::three(0x90 | ch, p, note_on_velocity(*velocity)),
            );
            t.push(
                When::End,
                Midi1Msg::three(0x80 | ch, p, release_velocity(*rel)),
            );
            t.drops.per_note = per_note.len();
            t.drops.attribute = attribute.is_some();
        }
        EventBody::Cc { index, value } => {
            let index = index & 0x7F;
            // Bank select (CC0/CC32) sorts with program changes so
            // bank-then-program holds on a shared frame (ruling O19).
            let class = if matches!(index, 0 | 32) {
                Class::Patch
            } else {
                Class::Control
            };
            t.push_class(
                When::Start,
                class,
                Midi1Msg::three(0xB0 | ch, index, value7(*value)),
            );
        }
        EventBody::PitchBend { value } => {
            let (lsb, msb) = bend14(*value);
            t.push(When::Start, Midi1Msg::three(0xE0 | ch, lsb, msb));
        }
        EventBody::ChannelPressure { value } => {
            t.push(When::Start, Midi1Msg::two(0xD0 | ch, value7(*value)));
        }
        EventBody::PolyPressure { pitch, value } => {
            t.push(
                When::Start,
                Midi1Msg::three(0xA0 | ch, pitch & 0x7F, value7(*value)),
            );
        }
        EventBody::Program { program, bank } => {
            if let Some((msb, lsb)) = bank {
                let (msb, lsb) = (msb & 0x7F, lsb & 0x7F);
                t.push_class(
                    When::Start,
                    Class::Patch,
                    Midi1Msg::three(0xB0 | ch, 0, msb),
                );
                t.push_class(
                    When::Start,
                    Class::Patch,
                    Midi1Msg::three(0xB0 | ch, 32, lsb),
                );
            }
            t.push(When::Start, Midi1Msg::two(0xC0 | ch, program & 0x7F));
        }
        EventBody::SysEx { data, eight_bit } => {
            if *eight_bit {
                t.drops.sysex8 = true;
            } else {
                t.sysex = Some(data);
            }
        }
        EventBody::Text { .. } | EventBody::Reference { .. } | EventBody::AudioRegion { .. } => {}
    }
    t
}
