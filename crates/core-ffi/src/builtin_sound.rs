//! The Built-in Synth's sounds (plan release-fixes Scope 1): five General
//! MIDI programs, kept in the destination's own default patch and channel
//! (Decision 3: a sound belongs to the destination, so every track that
//! plays it changes with it).

use core_model::document::{DestKind, Destination};

/// A Built-in Synth sound (plan release-fixes Scope 1): a GM program and channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BuiltinSound {
    /// Acoustic Grand Piano (program 0, channel 0).
    Piano,
    /// Electric Bass, finger (program 33, channel 0).
    Bass,
    /// Pad 2, warm (program 89, channel 0).
    Pad,
    /// Lead 1, square (program 80, channel 0).
    Lead,
    /// The GM drum kit (program 0, channel 9).
    Drums,
}

impl BuiltinSound {
    /// Every sound, in the window's order.
    pub const ALL: [BuiltinSound; 5] = [
        BuiltinSound::Piano,
        BuiltinSound::Bass,
        BuiltinSound::Pad,
        BuiltinSound::Lead,
        BuiltinSound::Drums,
    ];

    /// Piano (0, ch 0), Bass (33, ch 0), Pad (89, ch 0), Lead (80, ch 0), Drums (0, ch 9).
    pub fn program_and_channel(self) -> (u8, u8) {
        match self {
            BuiltinSound::Piano => (0, 0),
            BuiltinSound::Bass => (33, 0),
            BuiltinSound::Pad => (89, 0),
            BuiltinSound::Lead => (80, 0),
            BuiltinSound::Drums => (0, 9),
        }
    }

    /// The sound a destination's (default_patch program, channel) is, if any.
    /// A default_patch of None on channel 0 is Piano (no program change
    /// plays program 0).
    pub fn of(program: Option<u8>, channel: u8) -> Option<BuiltinSound> {
        let pc = (program.unwrap_or(0), channel);
        Self::ALL
            .into_iter()
            .find(|s| s.program_and_channel() == pc)
    }
}

/// `d`'s sound when it is a Built-in Synth (and its patch has no bank),
/// else None.
pub(crate) fn sound_of(d: &Destination) -> Option<BuiltinSound> {
    match &d.kind {
        DestKind::MidiEndpoint { channel, .. } if d.kind.is_builtin_synth() => {
            match d.default_patch {
                Some(p) if p.bank.is_some() => None,
                p => BuiltinSound::of(p.map(|p| p.program), *channel),
            }
        }
        _ => None,
    }
}
