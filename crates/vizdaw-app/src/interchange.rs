//! The import half of Swift `InterchangeModel` (slice 7c Task 1): File ›
//! Import MIDI File…'s sheet. The file is read off the UI thread
//! ([`crate::DocumentStore::begin_midi_import`]; only the newest read of a
//! document lands, a refusal is the alert "Import Failed"); the read
//! opens the sheet with Swift's defaults: each channel on the core's
//! suggested destination, Split by channel for a type 0 file, Treat as MPE
//! when the file has an MPE configuration, the tempo map off unless the
//! shown sequence has no events (then it is on and locked, Ruling T7-a),
//! and this project as where it goes. Import with this project imports at
//! the playhead into the shown sequence (the selected track, if one, as
//! the drop track) as an `ImportMidi` job ("MIDI Import Failed"); with a
//! new project the store makes a 48 kHz project for it
//! ([`crate::DocumentStore::import_midi_as_new_project`]), every channel
//! on a new built-in synth, the file's tempo map always taken.

use vizdaw_ffi::{
    ChannelMapRow, JobKindRow, MidiFileInfoRow, MidiFileKindRow, MidiImportRequestRow,
};

use crate::alerts::AlertItem;
use crate::document::DocumentModel;
use crate::jobs::import_target;
use crate::store::MidiRead;

/// The sheet's title.
pub const IMPORT_TITLE: &str = "Import MIDI File";
/// The "where" picker's label and choices.
pub const IMPORT_INTO: &str = "Import into";
pub const THIS_PROJECT: &str = "This project, at the playhead";
pub const A_NEW_PROJECT: &str = "A new project";
/// Shown instead of the channel list when the file plays no notes.
pub const NO_NOTES: &str = "The file plays no notes on any channel.";
/// The channel list's heading, into this project and into a new one.
pub const WHERE_EACH_CHANNEL_PLAYS: &str = "Where each channel plays";
pub const NEW_PROJECT_CHANNELS: &str = "In a new project each channel plays a new built-in synth.";
/// The destination picker's first item (`dest` none).
pub const NEW_BUILT_IN_SYNTH: &str = "New built-in synth";
/// The toggles, their accessibility labels and help.
pub const SPLIT_BY_CHANNEL: &str = "Split by channel";
pub const SPLIT_LABEL: &str = "Split tracks by channel";
pub const TREAT_AS_MPE: &str = "Treat as MPE";
pub const MPE_HELP: &str =
    "Member-channel notes, bend, CC 74 and pressure become per-note expression";
pub const USE_TEMPO: &str = "Use the file's tempo map";
pub const TEMPO_LOCKED_HELP: &str =
    "A sequence without events always takes the file's tempo and meter";
pub const TEMPO_HELP: &str =
    "Replace this sequence's tempo and meter from the playhead with the file's";
/// The buttons.
pub const CANCEL: &str = "Cancel";
pub const IMPORT: &str = "Import";
/// The title of a refused start into this project, and of a failed job.
pub const MIDI_IMPORT_FAILED: &str = "MIDI Import Failed";
/// Import with no sequence shown.
pub const NO_SEQUENCE: &str = "No sequence is shown.";

/// One row of the sheet: where a channel's notes go (`dest` none: a new
/// built-in synth).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelChoice {
    pub group: u8,
    pub channel: u8,
    pub dest: Option<u64>,
    /// Notes on it in the file.
    pub notes: u64,
}

/// "Channel 10", or "Group 2, channel 10" past the first group (1-based).
pub fn channel_name(c: &ChannelChoice) -> String {
    if c.group == 0 {
        format!("Channel {}", u16::from(c.channel) + 1)
    } else {
        format!(
            "Group {}, channel {}",
            u16::from(c.group) + 1,
            u16::from(c.channel) + 1
        )
    }
}

/// A row's destination picker's accessibility label: "Destination for
/// channel 10".
pub fn destination_label(c: &ChannelChoice) -> String {
    format!("Destination for {}", channel_name(c).to_lowercase())
}

/// "1 note" / "N notes".
pub fn notes_text(notes: u64) -> String {
    if notes == 1 {
        "1 note".to_owned()
    } else {
        format!("{notes} notes")
    }
}

/// The line under the title: "<file>: <kind>, N track(s)".
pub fn describe(info: &MidiFileInfoRow, path: &str) -> String {
    let kind = match info.kind {
        MidiFileKindRow::Smf0 => "Standard MIDI File, type 0",
        MidiFileKindRow::Smf1 => "Standard MIDI File, type 1",
        MidiFileKindRow::Clip => "MIDI Clip File",
    };
    let tracks = if info.tracks == 1 {
        "1 track".to_owned()
    } else {
        format!("{} tracks", info.tracks)
    };
    let name = std::path::Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
    format!("{name}: {kind}, {tracks}")
}

/// The import sheet's state: the file as read and the choices.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiImport {
    read: MidiRead,
    shown: bool,
    pub choices: Vec<ChannelChoice>,
    pub split_channels: bool,
    pub mpe: bool,
    /// Where: a new project instead of this one.
    pub into_new_project: bool,
    /// The tempo box as ticked (ignored while locked).
    wants_tempo: bool,
}

impl MidiImport {
    /// The sheet for `read`, with Swift's defaults (`landed`).
    fn new(read: MidiRead) -> Self {
        let choices = read
            .info
            .channels
            .iter()
            .map(|c| ChannelChoice {
                group: c.group,
                channel: c.channel,
                dest: c.suggested,
                notes: c.notes,
            })
            .collect();
        MidiImport {
            split_channels: read.info.kind == MidiFileKindRow::Smf0,
            mpe: read.info.mpe_found,
            choices,
            into_new_project: false,
            wants_tempo: false,
            shown: true,
            read,
        }
    }

    pub fn path(&self) -> &str {
        &self.read.path
    }

    pub fn info(&self) -> &MidiFileInfoRow {
        &self.read.info
    }

    pub fn read(&self) -> &MidiRead {
        &self.read
    }

    /// Whether the sheet is up (Swift `importShown`).
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// The request for a new project whose shown sequence is `seq`: every
    /// channel on a new destination (it has none yet), the tempo map
    /// taken.
    pub fn new_project_request(&self, seq: u64) -> MidiImportRequestRow {
        MidiImportRequestRow {
            path: self.read.path.clone(),
            new_project: true,
            seq,
            lane: None,
            tick: 0,
            channel_map: self
                .choices
                .iter()
                .map(|c| ChannelMapRow {
                    group: c.group,
                    channel: c.channel,
                    dest: None,
                })
                .collect(),
            split_channels: Some(self.split_channels),
            mpe: self.mpe,
            use_tempo: true,
        }
    }
}

/// The tempo box as the sheet shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TempoBox {
    pub on: bool,
    pub enabled: bool,
    pub help: &'static str,
}

impl DocumentModel {
    /// The import sheet's state, once a read landed (shown or not).
    pub fn midi_import(&self) -> Option<&MidiImport> {
        self.midi_import.as_ref()
    }

    /// The MIDI file the import sheet read, once read.
    pub fn midi_read(&self) -> Option<&MidiRead> {
        self.midi_import.as_ref().map(MidiImport::read)
    }

    /// A read landed: the sheet shows it with the defaults.
    pub(crate) fn midi_read_landed(&mut self, read: MidiRead) {
        self.midi_import = Some(MidiImport::new(read));
    }

    /// Edits the sheet's choices.
    pub fn edit_midi_import(&mut self, f: impl FnOnce(&mut MidiImport)) {
        if let Some(m) = self.midi_import.as_mut() {
            f(m);
        }
    }

    /// The sheet goes (Cancel, or Import).
    pub fn hide_midi_import(&mut self) {
        if let Some(m) = self.midi_import.as_mut() {
            m.shown = false;
        }
    }

    /// The core takes the file's tempo into a sequence without events
    /// (Ruling T7-a): the box is on and cannot be turned off.
    pub fn midi_import_tempo_locked(&self) -> bool {
        !self
            .lanes()
            .iter()
            .any(|l| l.midi_event_count > 0 || l.region_count > 0 || l.reference_count > 0)
    }

    /// Whether an import into this project takes the file's tempo map.
    pub fn midi_import_use_tempo(&self) -> bool {
        self.midi_import_tempo_locked() || self.midi_import.as_ref().is_some_and(|m| m.wants_tempo)
    }

    /// Ticks or clears the tempo box (ignored while it is locked).
    pub fn set_midi_import_use_tempo(&mut self, on: bool) {
        if let Some(m) = self.midi_import.as_mut() {
            m.wants_tempo = on;
        }
    }

    /// The tempo box: on and locked for an empty sequence or a new
    /// project, with the help that says why.
    pub fn midi_import_tempo_box(&self) -> TempoBox {
        let fresh = self
            .midi_import
            .as_ref()
            .is_some_and(|m| m.into_new_project);
        let locked = self.midi_import_tempo_locked() || fresh;
        TempoBox {
            on: self.midi_import_use_tempo() || fresh,
            enabled: !locked,
            help: if locked {
                TEMPO_LOCKED_HELP
            } else {
                TEMPO_HELP
            },
        }
    }

    /// The sheet's Import into this project: the shown sequence at the
    /// playhead, the selected track (if one) as the drop track. No
    /// sequence shown is the alert "Import Failed".
    pub fn start_midi_import(&mut self) {
        let Some(m) = self.midi_import.as_ref() else {
            return;
        };
        let target = import_target(
            self.selected_seq(),
            &self.selection().lanes,
            1,
            self.last_transport().tick,
        );
        let Some(target) = target else {
            self.alert = Some(AlertItem::new(
                crate::interchange_commands::IMPORT_FAILED,
                NO_SEQUENCE,
            ));
            return;
        };
        let req = MidiImportRequestRow {
            path: m.read.path.clone(),
            new_project: false,
            seq: target.seq,
            lane: target.lane,
            tick: target.tick,
            channel_map: m
                .choices
                .iter()
                .map(|c| ChannelMapRow {
                    group: c.group,
                    channel: c.channel,
                    dest: c.dest,
                })
                .collect(),
            split_channels: Some(m.split_channels),
            mpe: m.mpe,
            use_tempo: self.midi_import_use_tempo(),
        };
        self.hide_midi_import();
        self.start_job(JobKindRow::ImportMidi, MIDI_IMPORT_FAILED, |s| {
            s.import_midi(req)
        });
    }

    /// Whether the report of an import or export this document's UI ran
    /// waits to be shown (Swift `InterchangeModel.reportShown`; the
    /// report sheet, slice 7c Task 3, shows it and hides it again).
    pub fn report_shown(&self) -> bool {
        self.report_shown
    }

    /// The report sheet went.
    pub fn hide_report(&mut self) {
        self.report_shown = false;
    }
}
