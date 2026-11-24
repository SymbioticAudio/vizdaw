//! The Built-in Synth window apart from drawing (Swift
//! `BuiltinSynthWindow`, `SynthSheets` sounds; plan release-fixes Task 8):
//! a Built-in Synth destination's five sounds. A sound belongs to the
//! destination (Decision 3), so a change is heard on every track that
//! plays it, and the window says so. One window per (document,
//! destination); it opens from a double-click on the mixer's Built-in
//! Synth slot, never from the control channel.

use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::{BuiltinSound, VizError};

use crate::alerts::error_message;
use crate::document::DocumentModel;

/// The window's title.
pub const TITLE: &str = "Built-in Synth";
/// The sounds' radio group.
pub const SOUND: &str = "Sound";
/// The caption under the sounds.
pub const CAPTION: &str = "Changes every track that plays this Built-in Synth.";
/// A set's undo step (the core names it).
pub const SET_SOUND: &str = "Set Built-in Synth Sound";
/// The refusal for any other destination.
pub const NOT_BUILTIN_SYNTH: &str = "Only a Built-in Synth has these sounds.";
/// The window's content size in points.
pub const SIZE: (f64, f64) = (300.0, 200.0);

/// The sounds, in the window's order.
pub const SOUNDS: [BuiltinSound; 5] = BuiltinSound::ALL;

/// What the window and the status line call `s`.
pub fn sound_title(s: BuiltinSound) -> &'static str {
    match s {
        BuiltinSound::Piano => "Piano",
        BuiltinSound::Bass => "Bass",
        BuiltinSound::Pad => "Pad",
        BuiltinSound::Lead => "Lead",
        BuiltinSound::Drums => "Drums",
    }
}

/// Whether `d` is a Built-in Synth: no plug-in and not external (its sound
/// may be none of the five, from a file).
pub fn is_builtin_synth(d: &DestRow) -> bool {
    d.plugin.is_none() && !d.external
}

/// The destination the mixer's one Built-in Synth strip opens: `focus`
/// (the track the left pane shows plays it) when it is a Built-in Synth,
/// else the first Built-in Synth.
pub fn builtin_synth_destination(dests: &[DestRow], focus: Option<u64>) -> Option<u64> {
    let mut synths = dests.iter().filter(|d| is_builtin_synth(d));
    if let Some(f) = focus
        && synths.clone().any(|d| d.id == f)
    {
        return Some(f);
    }
    synths.next().map(|d| d.id)
}

/// What a sound change did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoundOutcome {
    Done,
    /// Not a Built-in Synth (or not found): nothing was asked.
    Unavailable(String),
    /// The core was busy (an open edit); the status line says why.
    Busy(String),
    /// The core refused; the status line says why.
    Refused(String),
}

/// One Built-in Synth window's state: its destination's sound, read once
/// per document version, so the window follows every step (an undo too).
#[derive(Debug, Clone)]
pub struct BuiltinSynthWindowModel {
    dest: u64,
    sound: Option<BuiltinSound>,
    /// The document version of the last good read.
    seen: Option<u64>,
}

impl BuiltinSynthWindowModel {
    pub fn new(doc: &DocumentModel, dest: u64) -> Self {
        let mut w = BuiltinSynthWindowModel {
            dest,
            sound: None,
            seen: None,
        };
        w.refresh(doc);
        w
    }

    pub fn dest(&self) -> u64 {
        self.dest
    }

    /// The sound the destination plays; `None` for any other destination
    /// (or one that plays none of the five).
    pub fn sound(&self) -> Option<BuiltinSound> {
        self.sound
    }

    /// The document moved since the last good read (or that read failed).
    pub fn needs_refresh(&self, doc: &DocumentModel) -> bool {
        self.seen != Some(doc.snapshot().version)
    }

    /// Reads the destination's sound again; a refused read keeps the last
    /// and returns false.
    pub fn refresh(&mut self, doc: &DocumentModel) -> bool {
        match doc.session().destinations() {
            Ok(ds) => {
                self.sound = ds
                    .iter()
                    .find(|d| d.id == self.dest)
                    .and_then(|d| d.builtin_sound);
                self.seen = Some(doc.snapshot().version);
                true
            }
            Err(_) => {
                self.seen = None;
                false
            }
        }
    }

    /// Sets the destination's sound: one undo step "Set Built-in Synth
    /// Sound", heard by every track that plays it. Any other destination
    /// asks nothing; a refusal is said on the status line.
    pub fn set_sound(&mut self, doc: &mut DocumentModel, s: BuiltinSound) -> SoundOutcome {
        let session = doc.session().clone();
        let synth = session
            .destinations()
            .ok()
            .and_then(|ds| ds.into_iter().find(|d| d.id == self.dest))
            .is_some_and(|d| is_builtin_synth(&d));
        if !synth {
            return SoundOutcome::Unavailable(NOT_BUILTIN_SYNTH.into());
        }
        let out = match session.set_builtin_sound(self.dest, s) {
            Ok(()) => {
                if let Ok(snap) = session.snapshot() {
                    doc.apply(snap);
                }
                SoundOutcome::Done
            }
            Err(VizError::Busy { message }) => {
                doc.set_status(Some(message.clone()));
                SoundOutcome::Busy(message)
            }
            Err(e) => {
                let message = error_message(&e);
                doc.set_status(Some(message.clone()));
                SoundOutcome::Refused(message)
            }
        };
        self.refresh(doc);
        out
    }
}
