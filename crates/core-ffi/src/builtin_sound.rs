//! The Built-in Synth's sounds (plan release-fixes Scope 1): five General
//! MIDI programs, kept in the destination's own default patch and channel
//! (Decision 3: a sound belongs to the destination, so every track that
//! plays it changes with it).

use crate::error::VizError;
use core_model::command::{Command, RoutingCmd};
use core_model::document::{DestKind, Destination, Document, Patch};
use core_model::ids::{AnyId, DestId};

/// [`sound_command`]'s refusal of another destination.
pub(crate) const NOT_BUILTIN: &str = "Only a Built-in Synth has these sounds.";

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

    /// The sound `destination.set_sound` names `name` ("piano" … "drums").
    pub fn named(name: &str) -> Option<BuiltinSound> {
        match name {
            "piano" => Some(BuiltinSound::Piano),
            "bass" => Some(BuiltinSound::Bass),
            "pad" => Some(BuiltinSound::Pad),
            "lead" => Some(BuiltinSound::Lead),
            "drums" => Some(BuiltinSound::Drums),
            _ => None,
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

/// The one command that gives Built-in Synth `dest` of `doc` `sound`
/// (Decision 3): a `RoutingCmd::EditDestination` with the sound's channel
/// and `default_patch = Some(Patch { program, bank: None })`. Another
/// destination: `Model` [`NOT_BUILTIN`]; an unknown one: `Model` (unknown
/// ID). Set Built-in Synth Sound and Co-Producer's preview (plan 8e Task
/// 6b) both build it here.
pub(crate) fn sound_command(
    doc: &Document,
    dest: DestId,
    sound: BuiltinSound,
) -> Result<Command, VizError> {
    let d = doc
        .instruments()
        .get(&dest)
        .ok_or(core_model::ModelError::UnknownId(AnyId::Dest(dest)))?;
    let DestKind::MidiEndpoint {
        endpoint_ref,
        group,
        ..
    } = &d.kind
    else {
        return Err(crate::events::bad(NOT_BUILTIN));
    };
    if !d.kind.is_builtin_synth() {
        return Err(crate::events::bad(NOT_BUILTIN));
    }
    let (program, channel) = sound.program_and_channel();
    Ok(Command::Routing(RoutingCmd::EditDestination {
        dest,
        kind: DestKind::MidiEndpoint {
            endpoint_ref: endpoint_ref.clone(),
            group: *group,
            channel,
        },
        default_patch: Some(Patch {
            program,
            bank: None,
        }),
    }))
}
