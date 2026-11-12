//! MIDI import (plan 6a Task 5): a Standard MIDI File or a MIDI Clip File
//! becomes [`ImportTrack`]s of [`EventSpec`]s ([`parse`]), then commands on
//! the document inside the caller's transaction ([`apply`]).
//!
//! Text and meta events map by one table (F21), which Task 6 writes back:
//!
//! | Model | SMF meta | Clip File |
//! |---|---|---|
//! | sequence markers | `06` (any track) | Metadata 0x00 `marker: …` |
//! | `Text { kind: Text }` | `01` | Metadata 0x00 (no prefix) |
//! | `Text { kind: Lyric }` | `05` | Performance 0x01 |
//! | `Text { kind: Cue }` | `07` | Metadata 0x00 `cue: …` |
//! | `Text { kind: Copyright }` | `02` on a track | Metadata 0x04 on a track |
//! | `SongInfo.copyright` | `02` on the conductor | Metadata 0x04 at tick 0 before any note |
//! | sequence name (title) | `03` on the conductor | Metadata 0x02 composition name |
//! | track name | `03` on the track | Metadata 0x03 clip name |
//!
//! The conductor is track 1 of a type 1 file, or the one track of a type 0
//! file (its copyright only at tick 0).

use crate::IoError;
use crate::clip::{self, ClipEvent, ClipFile};
use crate::gm;
use crate::limits::{Limits, MAX_MIDI_FILE_BYTES};
use crate::read::read_regular;
use crate::report::{ItemKind, Report};
use crate::smf::{self, Division, SmfEvent, TEXT_CUT};
use crate::text::sanitize;
use crate::ump::{self, BANK_METADATA, BANK_PERFORMANCE, Decoded};
use core_engine::input_fx::{CENTER, Expr, LiveMsg, up7, up32};
use core_engine::midi1::Midi1Msg;
use core_engine::mpe::MpeDecoder;
use core_model::command::{
    Command, DrumCmd, EventCmd, LaneCmd, Report as CmdReport, RoutingCmd, SequenceCmd, SettingsCmd,
};
use core_model::document::{DestKind, Document, MAX_INFO_BYTES};
use core_model::error::ModelError;
use core_model::event::{EventBody, EventSpec, NoteAttribute, PerNoteCtl, PerNoteKind, TextKind};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, DrumRowId, LaneId, SeqId};
use core_model::lane::valid_name;
use core_model::meter::{KeySig, Meter, MeterEntry, MeterMap};
use core_model::mixer::BUILTIN_SYNTH_REF;
use core_model::mpe::{DEFAULT_BEND_RANGE, MAX_MEMBERS, MpeConfig, MpeZone};
use core_model::rules::MAX_TICKS;
use core_model::selection::ViewFilter;
use core_model::sequence::Marker;
use core_model::tempo::{Bpm, TempoMap, TempoPoint};
use core_model::time::{PPQ, Tick};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// What kind of file was read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MidiFileKind {
    /// Standard MIDI File, type 0.
    Smf0,
    /// Standard MIDI File, type 1.
    Smf1,
    /// MIDI Clip File.
    Clip,
}

/// One track of an import.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportTrack {
    /// Its name (a valid track name).
    pub name: String,
    /// The `(group, channel)` with the most notes (ties go to the lowest);
    /// `None` for a track without channel messages.
    pub main: Option<(u8, u8)>,
    /// Its events in file order, at 15 360 ticks per quarter from file
    /// position 0. MIDI events carry `group_channel: Some((g, c))`; texts
    /// carry none.
    pub events: Vec<EventSpec>,
    /// Its main channel is 10: a drum track.
    pub drum: bool,
    /// Decoded as MPE through this zone.
    pub mpe: Option<MpeConfig>,
}

/// A parsed MIDI file, ready to [`apply`].
#[derive(Debug, Clone, PartialEq)]
pub struct MidiImport {
    /// What kind of file it was.
    pub kind: MidiFileKind,
    /// The title (the sequence name the file gives), at most
    /// [`MAX_INFO_BYTES`].
    pub title: Option<String>,
    /// The copyright, at most [`MAX_INFO_BYTES`], fit for
    /// `core_model::rules::check_info`.
    pub copyright: Option<String>,
    /// The tempo map, from file position 0.
    pub tempo: TempoMap,
    /// The meter map, from file position 0 (a pickup is its first bar).
    pub meter: MeterMap,
    /// Whether the file gave a tempo.
    pub has_tempo: bool,
    /// The pickup: the first bar when it is shorter than the second
    /// (Ruling 8); file position 0 is its start.
    pub start_offset: Tick,
    /// Markers, from file position 0, in tick order.
    pub markers: Vec<Marker>,
    /// The tracks; a track with no events (only metas) makes none (F11).
    pub tracks: Vec<ImportTrack>,
    /// What reading and mapping changed or left out.
    pub report: Report,
}

/// The import sheet's choices that affect parsing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportOptions {
    /// Split tracks by channel; `None` splits type 0 files only (Ruling 11).
    pub split_channels: Option<bool>,
    /// Decode every SMF track as MPE (the lower zone of 15 members unless
    /// the track begins with an MPE Configuration Message).
    pub mpe: bool,
}

/// Where a `(group, channel)` plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestChoice {
    /// This destination.
    Existing(DestId),
    /// A new built-in-synth destination on that group and channel.
    NewBuiltin,
}

/// Where and how an import lands.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// The sequence.
    pub seq: SeqId,
    /// The drop track: a one-track import goes into it; more tracks go
    /// after it as new tracks. `None`: new tracks at the end.
    pub lane: Option<LaneId>,
    /// Where file position 0 goes (a negative tick is 0).
    pub tick: Tick,
    /// Destinations by `(group, channel)`; a missing pair takes
    /// [`suggested_dest`] (F24).
    pub channel_map: BTreeMap<(u8, u8), DestChoice>,
    /// Use the file's tempo and meter maps.
    pub use_tempo: bool,
    /// The document is a new project made for this file (Ruling 6).
    pub new_project: bool,
}

const NOT_MIDI: &str = "This isn't a MIDI file.";
const TOO_LARGE: &str = "This MIDI file is too large to open.";
const ROUNDED: &str = "Positions rounded to VizDAW's resolution";
const TIMECODE: &str = "Timecode-based file placed at 120 BPM";
const TIMECODE_TEMPO: &str = "Tempo changes in a timecode-based file";
const NO_END: &str = "Notes with no end were ended at the end of their track";
const NO_NOTE: &str = "Note-offs with no note";
const NO_NOTE_PER_NOTE: &str = "Per-note messages with no note";
const PAST_END: &str = "Events past the end of the timeline";
const BAD_TEMPO: &str = "Unreadable tempo changes";
const TEMPO_CLAMPED: &str = "Tempos outside 5 to 999 BPM were clamped";
const BAD_METER: &str = "Unreadable time signatures";
const BAD_KEY: &str = "Unreadable key signatures";
const METER_MOVED: &str = "Meter changes moved to the next barline";
const SEQ_SPECIFIC: &str = "Sequencer-specific data";
const SMPTE_OFFSET: &str = "SMPTE offsets";
const ESCAPES: &str = "Escaped MIDI data";
const OTHER_TEXT: &str = "Text events of other kinds";
const OTHER_META: &str = "Unknown meta events";
const BAD_PORT: &str = "Port changes past group 16";
const BAD_SYSEX: &str = "Damaged SysEx messages";
const UNKNOWN: &str = "Unknown MIDI messages";
const INFO_CUT: &str = "Song info cut to 4 096 bytes";
const NOT_EMPTY: &str = "A new-project import needs an empty project";
const TEMPO_NOT_USED: &str = "The file's tempo map was not used";
const TEMPO_REPLACED: &str = "The project's tempo and meter were replaced by the file's";
const TITLE_NOT_USED: &str = "The file's title was not used";
const COPYRIGHT_NOT_USED: &str = "The file's copyright was not used";

/// How many events are mapped between looks at the cancel flag.
const CANCEL_EVERY: usize = 65_536;
/// Channel 10, 0-based.
const DRUM_CHANNEL: u8 = 9;

/// Parses `bytes`: `MThd` is a Standard MIDI File, `SMF2CLIP` a MIDI Clip
/// File; anything else, or more than [`MAX_MIDI_FILE_BYTES`], is refused.
///
/// Positions scale to 15 360 per quarter with round-half-up (Changed when
/// one moved); SMPTE divisions through seconds at 120 BPM. Notes pair
/// first-on first-off per group, channel and pitch; a note-on of velocity 0
/// ends a note in SMF and MIDI 1.0 packets only (Ruling 13). 7-bit values
/// scale by the MIDI 2.0 rules. CC 0 and CC 32 at the tick of a program
/// change, just before it, merge into its bank. Anything past
/// `core_model::rules::MAX_TICKS` is left out. MPE tracks decode through
/// 5g-2's [`MpeDecoder`]. Returns `Err(Cancelled)` once `cancel` is set.
pub fn parse(
    bytes: &[u8],
    opts: &ImportOptions,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<MidiImport, IoError> {
    if !u64::try_from(bytes.len()).is_ok_and(|n| n <= MAX_MIDI_FILE_BYTES) {
        return Err(IoError::Refused(TOO_LARGE.into()));
    }
    if bytes.starts_with(b"MThd") {
        parse_smf(bytes, opts, limits, cancel)
    } else if bytes.starts_with(b"SMF2CLIP") {
        parse_clip(bytes, opts, limits, cancel)
    } else {
        Err(IoError::Refused(NOT_MIDI.into()))
    }
}

/// Reads the file at `path` with [`read_regular`] (a regular file of at most
/// [`MAX_MIDI_FILE_BYTES`]), then [`parse`]s it.
pub fn parse_file(
    path: &Path,
    opts: &ImportOptions,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<MidiImport, IoError> {
    let bytes = read_regular(path, MAX_MIDI_FILE_BYTES)?;
    parse(&bytes, opts, limits, cancel)
}

/// Notes per `(group, channel)` the import's channel messages use, pairs
/// with none included at 0. SysEx and texts do not count.
pub fn channels(imp: &MidiImport) -> BTreeMap<(u8, u8), u64> {
    let mut out: BTreeMap<(u8, u8), u64> = BTreeMap::new();
    for e in imp.tracks.iter().flat_map(|t| &t.events) {
        if let Some(p) = e.group_channel
            && is_channel_voice(&e.body)
        {
            let n = out.entry(p).or_insert(0);
            if matches!(e.body, EventBody::Note { .. }) {
                *n = n.saturating_add(1);
            }
        }
    }
    out
}

/// Ruling 5: the first destination (by ID) that is a MIDI endpoint
/// (built-in synth or external) on `group` and `channel`, else a new
/// built-in-synth destination.
pub fn suggested_dest(doc: &Document, group: u8, channel: u8) -> DestChoice {
    doc.instruments()
        .iter()
        .find(|(_, d)| {
            matches!(&d.kind, DestKind::MidiEndpoint { group: g, channel: c, .. }
                if *g == group && *c == channel)
        })
        .map_or(DestChoice::NewBuiltin, |(id, _)| DestChoice::Existing(*id))
}

/// Applies `imp` to sequence `place.seq` with commands run through `h`,
/// inside the caller's open transaction (the caller commits, or aborts on
/// `Err`). Adds to `report` (pass the import's own report to keep both)
/// and fills its summary. Returns the track that received each import
/// track, in order.
///
/// In order: a destination per [`DestChoice::NewBuiltin`] pair, named
/// "Built-in Synth (ch N)"; with `use_tempo` or `new_project` the file's
/// tempo and meter maps (and, as a new project or into a sequence without
/// events, its pickup as the start offset; Ruling 8); per track a new track
/// named by it on its main pair's destination (or the drop track for a
/// one-track import), its events in one insert (an event whose pair plays
/// elsewhere than the track carries a `dest` override), General MIDI row
/// names for a drum track, its MPE zone; the markers merged in; and as a new
/// project the title as the sequence name and the copyright as song info.
/// Into a project the title and copyright are noted as not used (Ruling 7).
/// `new_project` on a document with any track is refused.
pub fn apply(
    h: &mut History,
    imp: &MidiImport,
    place: &Placement,
    report: &mut Report,
) -> Result<Vec<LaneId>, ModelError> {
    let seq = place.seq;
    let doc = h.doc();
    let s = doc.sequence(seq)?;
    if place.new_project && doc.sequences().values().any(|s| !s.lanes.is_empty()) {
        return Err(ModelError::invalid(NOT_EMPTY));
    }
    if let Some(l) = place.lane {
        doc.lane(seq, l)?;
    }
    let no_events = s
        .lanes
        .iter()
        .all(|l| l.takes.iter().all(|t| t.events.is_empty()));
    let use_maps = place.new_project || place.use_tempo;
    let pickup = if use_maps && (place.new_project || no_events) {
        imp.start_offset.0.max(0)
    } else {
        0
    };
    let base = place
        .tick
        .0
        .max(0)
        .checked_sub(pickup)
        .ok_or_else(|| ModelError::range("tick", place.tick.0))?;

    let mut dests: BTreeMap<(u8, u8), DestId> = BTreeMap::new();
    for (g, c) in channels(imp).into_keys() {
        let choice = place
            .channel_map
            .get(&(g, c))
            .copied()
            .unwrap_or_else(|| suggested_dest(h.doc(), g, c));
        let id = match choice {
            DestChoice::Existing(id) => id,
            DestChoice::NewBuiltin => {
                let r = run(
                    h,
                    Command::Routing(RoutingCmd::AddDestination {
                        name: builtin_name(g, c),
                        kind: DestKind::MidiEndpoint {
                            endpoint_ref: BUILTIN_SYNTH_REF.into(),
                            group: g,
                            channel: c,
                        },
                        default_patch: None,
                    }),
                )?;
                created(&r, |id| match id {
                    AnyId::Dest(d) => Some(*d),
                    _ => None,
                })
                .ok_or_else(|| ModelError::invalid("no destination was created"))?
            }
        };
        dests.insert((g, c), id);
    }

    if use_maps {
        let (tempo, meter) = if pickup > 0 {
            (
                shift_tempo(&imp.tempo, pickup),
                shift_meter(&imp.meter, pickup),
            )
        } else {
            (imp.tempo.clone(), imp.meter.clone())
        };
        run(h, Command::Sequence(SequenceCmd::SetTempo { seq, tempo }))?;
        run(h, Command::Sequence(SequenceCmd::SetMeter { seq, meter }))?;
        if pickup > 0 || place.new_project {
            run(
                h,
                Command::Sequence(SequenceCmd::SetStartOffset {
                    seq,
                    offset: Tick(pickup),
                }),
            )?;
        }
        report.summary.tempo_map = true;
        if !place.new_project {
            report.add(ItemKind::Changed, TEMPO_REPLACED, None);
        }
    } else if imp.has_tempo || imp.meter != MeterMap::default() {
        report.add(ItemKind::Note, TEMPO_NOT_USED, None);
    }

    let drop = place.lane.filter(|_| imp.tracks.len() == 1);
    let mut index = place
        .lane
        .and_then(|l| h.doc().sequence(seq).ok()?.lane_index(l))
        .and_then(|i| i.checked_add(1));
    let mut lanes = Vec::with_capacity(imp.tracks.len());
    let mut notes = 0u64;
    for t in &imp.tracks {
        let (lane, lane_dest, new) = match drop {
            Some(l) => (l, h.doc().lane(seq, l)?.midi_dest, false),
            None => {
                let r = run(
                    h,
                    Command::Lane(LaneCmd::Add {
                        seq,
                        name: t.name.clone(),
                        index,
                    }),
                )?;
                index = index.and_then(|i| i.checked_add(1));
                let lane = created(&r, |id| match id {
                    AnyId::Lane(l) => Some(*l),
                    _ => None,
                })
                .ok_or_else(|| ModelError::invalid("no track was created"))?;
                let dest = t.main.and_then(|m| dests.get(&m).copied());
                if dest.is_some() {
                    run(h, Command::Lane(LaneCmd::SetDest { seq, lane, dest }))?;
                }
                (lane, dest, true)
            }
        };
        let mut events = Vec::with_capacity(t.events.len());
        for e in &t.events {
            let Some(tick) = placed(e.tick, base) else {
                report.add(ItemKind::LeftOut, PAST_END, None);
                continue;
            };
            let mut e = e.clone();
            e.tick = tick;
            if is_channel_voice(&e.body)
                && let Some(d) = e.group_channel.and_then(|p| dests.get(&p)).copied()
                && Some(d) != lane_dest
            {
                e.dest = Some(d);
            }
            if matches!(e.body, EventBody::Note { .. }) {
                notes = notes.saturating_add(1);
            }
            events.push(e);
        }
        if !events.is_empty() {
            run(h, Command::Event(EventCmd::Insert { seq, lane, events }))?;
        }
        if new && t.drum {
            let r = run(
                h,
                Command::Drum(DrumCmd::MarkDrumLane {
                    seq,
                    lane,
                    drum: true,
                }),
            )?;
            name_drum_rows(h, seq, lane, &r)?;
        }
        if new && t.mpe.is_some() {
            run(
                h,
                Command::Lane(LaneCmd::SetMpe {
                    seq,
                    lane,
                    mpe: t.mpe,
                }),
            )?;
        }
        lanes.push(lane);
    }
    let s = &mut report.summary;
    s.tracks = s
        .tracks
        .saturating_add(u64::try_from(lanes.len()).unwrap_or(u64::MAX));
    s.notes = s.notes.saturating_add(notes);

    if !imp.markers.is_empty() {
        let mut markers: Vec<Marker> = h.doc().sequence(seq)?.markers.iter().cloned().collect();
        let mut added = 0u64;
        for m in &imp.markers {
            match placed(m.tick, base) {
                Some(tick) => {
                    markers.push(Marker {
                        tick,
                        name: m.name.clone(),
                    });
                    added = added.saturating_add(1);
                }
                None => report.add(ItemKind::LeftOut, PAST_END, None),
            }
        }
        markers.sort_by_key(|m| m.tick);
        run(
            h,
            Command::Sequence(SequenceCmd::SetMarkers {
                seq,
                markers: markers.into_iter().collect(),
            }),
        )?;
        report.summary.markers = report.summary.markers.saturating_add(added);
    }

    if place.new_project {
        if let Some(name) = imp.title.as_deref().and_then(valid_name) {
            run(
                h,
                Command::Sequence(SequenceCmd::Rename {
                    seq,
                    name: name.to_string(),
                }),
            )?;
        }
        if let Some(c) = &imp.copyright {
            let mut info = h.doc().settings().info.clone();
            info.copyright = Some(c.clone());
            run(h, Command::Settings(SettingsCmd::SetInfo { info }))?;
        }
    } else {
        if imp.title.is_some() {
            report.add(ItemKind::Note, TITLE_NOT_USED, None);
        }
        if imp.copyright.is_some() {
            report.add(ItemKind::Note, COPYRIGHT_NOT_USED, None);
        }
    }
    Ok(lanes)
}

// ---- apply helpers ----

fn run(h: &mut History, cmd: Command) -> Result<CmdReport, ModelError> {
    h.execute(&cmd, &ViewFilter::default())
}

fn created<T>(r: &CmdReport, f: impl Fn(&AnyId) -> Option<T>) -> Option<T> {
    r.created.iter().find_map(f)
}

/// "Built-in Synth (ch 1)" for group 0, channel 0 (Ruling 5).
fn builtin_name(group: u8, channel: u8) -> String {
    let ch = u16::from(channel).saturating_add(1);
    if group == 0 {
        format!("Built-in Synth (ch {ch})")
    } else {
        let g = u16::from(group).saturating_add(1);
        format!("Built-in Synth (group {g}, ch {ch})")
    }
}

/// `t` moved by `base`, if it stays within ±`MAX_TICKS`.
fn placed(t: Tick, base: i64) -> Option<Tick> {
    t.0.checked_add(base)
        .filter(|t| (-MAX_TICKS..=MAX_TICKS).contains(t))
        .map(Tick)
}

/// Gives the rows `MarkDrumLane` just made their General MIDI names.
fn name_drum_rows(
    h: &mut History,
    seq: SeqId,
    lane: LaneId,
    r: &CmdReport,
) -> Result<(), ModelError> {
    let made: BTreeSet<DrumRowId> = r
        .created
        .iter()
        .filter_map(|id| match id {
            AnyId::DrumRow(d) => Some(*d),
            _ => None,
        })
        .collect();
    let renames: Vec<(DrumRowId, &'static str)> = h
        .doc()
        .lane(seq, lane)?
        .drum
        .iter()
        .flat_map(|l| l.rows.iter())
        .filter(|row| made.contains(&row.id))
        .filter_map(|row| gm::drum_name(row.note).map(|n| (row.id, n)))
        .collect();
    for (row, name) in renames {
        run(
            h,
            Command::Drum(DrumCmd::RenameRow {
                seq,
                lane,
                row,
                name: name.into(),
            }),
        )?;
    }
    Ok(())
}

/// `map` with its pickup of `off` ticks removed: points move back by `off`,
/// the last at or before the pickup's end becoming the point at 0.
fn shift_tempo(map: &TempoMap, off: i64) -> TempoMap {
    let mut points: Vec<TempoPoint> = Vec::new();
    for p in map.points() {
        let tick = p.tick.0.saturating_sub(off);
        if tick <= 0 {
            points.clear();
            points.push(TempoPoint {
                tick: Tick::ZERO,
                ..*p
            });
        } else {
            points.push(TempoPoint {
                tick: Tick(tick),
                ..*p
            });
        }
    }
    TempoMap::new(points).unwrap_or_else(|_| map.clone())
}

/// `map` with its pickup bar removed, as [`shift_tempo`] does.
fn shift_meter(map: &MeterMap, off: i64) -> MeterMap {
    let mut entries: Vec<MeterEntry> = Vec::new();
    for e in map.entries() {
        let tick = e.tick.0.saturating_sub(off);
        if tick <= 0 {
            entries.clear();
            entries.push(MeterEntry {
                tick: Tick::ZERO,
                ..*e
            });
        } else {
            entries.push(MeterEntry {
                tick: Tick(tick),
                ..*e
            });
        }
    }
    MeterMap::new(entries).unwrap_or_else(|_| map.clone())
}

/// Notes, controllers, pressure, bend and programs: what a destination plays.
fn is_channel_voice(b: &EventBody) -> bool {
    matches!(
        b,
        EventBody::Note { .. }
            | EventBody::Cc { .. }
            | EventBody::PitchBend { .. }
            | EventBody::ChannelPressure { .. }
            | EventBody::PolyPressure { .. }
            | EventBody::Program { .. }
    )
}

// ---- positions ----

/// File ticks to model ticks: `t × num ÷ den`, rounded half up.
#[derive(Debug, Clone, Copy)]
struct Scale {
    num: u128,
    den: u128,
}

impl Scale {
    fn of(division: Division) -> Scale {
        // 120 BPM: one second is two quarters.
        let second = 2 * PPQ.unsigned_abs();
        match division {
            Division::Ppq(p) => Scale {
                num: u128::from(PPQ.unsigned_abs()),
                den: u128::from(p),
            },
            // 29.97 frames per second.
            Division::Smpte {
                fps: 29,
                ticks_per_frame,
            } => Scale {
                num: u128::from(second) * 100,
                den: 2997 * u128::from(ticks_per_frame),
            },
            Division::Smpte {
                fps,
                ticks_per_frame,
            } => Scale {
                num: u128::from(second),
                den: u128::from(fps) * u128::from(ticks_per_frame),
            },
        }
    }

    fn per_quarter(tpq: u16) -> Scale {
        Scale::of(Division::Ppq(tpq))
    }

    /// The model tick of file tick `t` and whether it was rounded; `None`
    /// past `MAX_TICKS` (or for a zero divisor).
    fn tick(self, t: u64) -> Option<(i64, bool)> {
        let n = u128::from(t).checked_mul(self.num)?;
        let q = n.checked_add(self.den / 2)?.checked_div(self.den)?;
        let rounded = n.checked_rem(self.den)? != 0;
        let q = i64::try_from(q).ok().filter(|q| *q <= MAX_TICKS)?;
        Some((q, rounded))
    }

    /// The end of a track at `t`: `MAX_TICKS` when past it.
    fn end(self, t: u64) -> i64 {
        self.tick(t).map_or(MAX_TICKS, |(q, _)| q)
    }
}

// ---- parsing ----

/// One parsed source track, before splitting by channel.
struct Source {
    /// Its name, if the file gives one.
    name: Option<String>,
    /// The name it has without one.
    fallback: String,
    events: Vec<EventSpec>,
    mpe: Option<MpeConfig>,
}

/// A time or key signature change.
#[derive(Debug, Clone, Copy)]
enum MeterChange {
    Time(Meter),
    Key(KeySig),
}

/// What carries over from track to track while mapping.
struct Ctx<'a> {
    limits: &'a Limits,
    cancel: &'a AtomicBool,
    report: Report,
    scale: Scale,
    seen: usize,
    tempo: Vec<(i64, Bpm)>,
    meter: Vec<(i64, MeterChange)>,
    markers: Vec<Marker>,
    title: Option<String>,
    copyright: Option<String>,
    has_tempo: bool,
}

impl<'a> Ctx<'a> {
    fn new(limits: &'a Limits, cancel: &'a AtomicBool, report: Report, scale: Scale) -> Self {
        Ctx {
            limits,
            cancel,
            report,
            scale,
            seen: 0,
            tempo: Vec::new(),
            meter: Vec::new(),
            markers: Vec::new(),
            title: None,
            copyright: None,
            has_tempo: false,
        }
    }

    /// Counts one event, looking at the cancel flag every [`CANCEL_EVERY`].
    fn step(&mut self) -> Result<(), IoError> {
        self.seen = self.seen.wrapping_add(1);
        if self.seen % CANCEL_EVERY == 0 && self.cancel.load(Ordering::Relaxed) {
            return Err(IoError::Cancelled);
        }
        Ok(())
    }

    fn add(&mut self, kind: ItemKind, what: &str) {
        self.report.add(kind, what, None);
    }

    /// The model tick of file tick `t`, reporting rounding; `None` (left
    /// out) past the end of the timeline.
    fn at(&mut self, t: u64) -> Option<i64> {
        match self.scale.tick(t) {
            Some((q, rounded)) => {
                if rounded {
                    self.add(ItemKind::Changed, ROUNDED);
                }
                Some(q)
            }
            None => {
                self.add(ItemKind::LeftOut, PAST_END);
                None
            }
        }
    }

    /// Cleaned text. The reader already reported a cut of the raw bytes;
    /// a cut here (replacement characters made it longer) is reported
    /// only when the raw bytes were under the limit, so one text counts
    /// once.
    fn text(&mut self, raw: &[u8]) -> String {
        let (s, cut) = sanitize(raw, self.limits);
        if cut && raw.len() < self.limits.max_text_bytes {
            self.add(ItemKind::Changed, TEXT_CUT);
        }
        s
    }

    /// `s` cut to [`MAX_INFO_BYTES`] for song info (`field` names it).
    fn info(&mut self, mut s: String, field: &str) -> String {
        if s.len() > MAX_INFO_BYTES {
            let end = s
                .char_indices()
                .map(|(i, c)| i.saturating_add(c.len_utf8()))
                .take_while(|e| *e <= MAX_INFO_BYTES)
                .last()
                .unwrap_or(0);
            s.truncate(end);
            self.report
                .add(ItemKind::Changed, INFO_CUT, Some(field.to_string()));
        }
        s
    }

    /// A tempo of `milli` milli-BPM at `tick`, clamped to 5–999 BPM.
    fn tempo(&mut self, tick: i64, milli: u64) {
        let lo = u64::from(Bpm::MIN.0);
        let hi = u64::from(Bpm::MAX.0);
        let clamped = milli.clamp(lo, hi);
        if clamped != milli {
            self.add(ItemKind::Changed, TEMPO_CLAMPED);
        }
        let bpm = Bpm(u32::try_from(clamped).unwrap_or(Bpm::MAX.0));
        self.tempo.push((tick, bpm));
        self.has_tempo = true;
    }

    /// A time signature of `num` over 2^`pow`.
    fn time_sig(&mut self, tick: i64, num: u8, pow: u8) {
        let meter = (pow <= 6).then(|| Meter {
            num,
            den: 1u8 << pow,
        });
        match meter.filter(|m| m.is_valid()) {
            Some(m) => self.meter.push((tick, MeterChange::Time(m))),
            None => self.add(ItemKind::LeftOut, BAD_METER),
        }
    }

    fn key_sig(&mut self, tick: i64, sharps: i8, minor: bool) {
        if (-7..=7).contains(&sharps) {
            self.meter
                .push((tick, MeterChange::Key(KeySig { sharps, minor })));
        } else {
            self.add(ItemKind::LeftOut, BAD_KEY);
        }
    }

    /// Maps SMF track `n` (1-based); `conductor` is track 1 of a type 1
    /// file or the track of a type 0 file.
    fn smf_track(
        &mut self,
        n: usize,
        events: &[(u64, SmfEvent)],
        format: u16,
        timecode: bool,
        opts: &ImportOptions,
    ) -> Result<Source, IoError> {
        let conductor = format == 0 || n == 1;
        let mpe = mcm(events).or(opts.mpe.then(MpeConfig::default));
        let mut mpe_state = mpe.map(|cfg| (MpeDecoder::new(cfg), cfg));
        let mut b = Builder::default();
        let mut group = 0u8;
        let mut name: Option<String> = None;
        let mut end = 0i64;
        for (t, e) in events {
            self.step()?;
            if let SmfEvent::Meta { kind: 0x2F, .. } = e {
                end = end.max(self.scale.end(*t));
                continue;
            }
            let Some(tick) = self.at(*t) else { continue };
            end = end.max(tick);
            match e {
                SmfEvent::Channel(m) => match mpe_state.as_mut() {
                    Some((dec, cfg)) => b.mpe(tick, group, *m, dec, cfg, &mut self.report),
                    None => b.midi1(tick, group, *m, &mut self.report),
                },
                SmfEvent::SysEx(data) => b.sysex(tick, group, data, false, &mut self.report),
                SmfEvent::Escape(_) => self.add(ItemKind::LeftOut, ESCAPES),
                SmfEvent::Meta { kind, data } => match *kind {
                    0x01 => {
                        let text = self.text(data);
                        b.text(tick, TextKind::Text, text);
                    }
                    0x02 => {
                        let text = self.text(data);
                        if conductor && (format == 1 || *t == 0) && self.copyright.is_none() {
                            self.copyright = Some(self.info(text, "copyright"));
                        } else {
                            b.text(tick, TextKind::Copyright, text);
                        }
                    }
                    0x03 => {
                        let text = self.text(data);
                        if conductor && self.title.is_none() {
                            self.title = Some(self.info(text.clone(), "title"));
                        }
                        if format == 1 && name.is_none() {
                            name = Some(text);
                        }
                    }
                    0x05 => {
                        let text = self.text(data);
                        b.text(tick, TextKind::Lyric, text);
                    }
                    0x06 => {
                        let name = self.text(data);
                        self.markers.push(Marker {
                            tick: Tick(tick),
                            name,
                        });
                    }
                    0x07 => {
                        let text = self.text(data);
                        b.text(tick, TextKind::Cue, text);
                    }
                    0x04 | 0x08..=0x0F => self.add(ItemKind::LeftOut, OTHER_TEXT),
                    // Channel prefix: its channel messages carry their own.
                    0x20 => {}
                    0x21 => match data.as_slice() {
                        [p] if *p <= 15 => group = *p,
                        _ => self.add(ItemKind::LeftOut, BAD_PORT),
                    },
                    0x51 if timecode => self.add(ItemKind::LeftOut, TIMECODE_TEMPO),
                    0x51 => match data.as_slice() {
                        &[a, b2, c] if [a, b2, c] != [0, 0, 0] => {
                            let us = u64::from(u32::from_be_bytes([0, a, b2, c]));
                            let milli = 60_000_000_000u64
                                .checked_add(us / 2)
                                .and_then(|x| x.checked_div(us))
                                .unwrap_or(0);
                            self.tempo(tick, milli);
                        }
                        _ => self.add(ItemKind::LeftOut, BAD_TEMPO),
                    },
                    0x54 => self.add(ItemKind::LeftOut, SMPTE_OFFSET),
                    0x58 => match data.as_slice() {
                        &[num, pow, _, _] => self.time_sig(tick, num, pow),
                        _ => self.add(ItemKind::LeftOut, BAD_METER),
                    },
                    0x59 => match data.as_slice() {
                        &[sf, mi] if mi <= 1 => {
                            self.key_sig(tick, i8::from_be_bytes([sf]), mi == 1);
                        }
                        _ => self.add(ItemKind::LeftOut, BAD_KEY),
                    },
                    0x7F => self.add(ItemKind::LeftOut, SEQ_SPECIFIC),
                    _ => self.add(ItemKind::LeftOut, OTHER_META),
                },
            }
        }
        let events = b.finish(end, &mut self.report);
        let mpe = mpe_state.map(|(_, cfg)| cfg);
        Ok(Source {
            name,
            fallback: format!("Track {n}"),
            events,
            mpe,
        })
    }

    /// Maps the one track of a clip.
    fn clip_track(&mut self, c: &ClipFile) -> Result<Source, IoError> {
        for e in &c.header {
            if let ClipEvent::Packet(p) = e {
                self.clip_meta(0, &ump::decode(p));
            }
        }
        let mut b = Builder::default();
        let mut name: Option<String> = None;
        let mut end = 0i64;
        for (t, e) in &c.events {
            self.step()?;
            let Some(tick) = self.at(*t) else { continue };
            end = end.max(tick);
            match e {
                ClipEvent::Packet(p) => {
                    let d = ump::decode(p);
                    if !b.ump(tick, &d, &mut self.report) && !self.clip_meta(tick, &d) {
                        self.add(ItemKind::LeftOut, UNKNOWN);
                    }
                }
                ClipEvent::SysEx7 { group, data } => {
                    b.sysex(tick, *group, data, false, &mut self.report);
                }
                ClipEvent::SysEx8 { group, data } => {
                    b.sysex(tick, *group, data, true, &mut self.report);
                }
                ClipEvent::Text {
                    bank, status, text, ..
                } => match (*bank, *status) {
                    (BANK_METADATA, 0x00) => {
                        if let Some(m) = text.strip_prefix("marker: ") {
                            self.markers.push(Marker {
                                tick: Tick(tick),
                                name: m.to_string(),
                            });
                        } else if let Some(cue) = text.strip_prefix("cue: ") {
                            b.text(tick, TextKind::Cue, cue.to_string());
                        } else {
                            b.text(tick, TextKind::Text, text.clone());
                        }
                    }
                    (BANK_METADATA, 0x02) if self.title.is_none() => {
                        self.title = Some(self.info(text.clone(), "title"));
                    }
                    (BANK_METADATA, 0x03) if name.is_none() => name = Some(text.clone()),
                    (BANK_METADATA, 0x04) => {
                        if *t == 0 && !b.has_notes && self.copyright.is_none() {
                            self.copyright = Some(self.info(text.clone(), "copyright"));
                        } else {
                            b.text(tick, TextKind::Copyright, text.clone());
                        }
                    }
                    (BANK_PERFORMANCE, 0x01) => b.text(tick, TextKind::Lyric, text.clone()),
                    _ => self.add(ItemKind::LeftOut, OTHER_TEXT),
                },
            }
        }
        end = end.max(self.scale.end(c.end));
        let events = b.finish(end, &mut self.report);
        Ok(Source {
            name,
            fallback: "Track 1".to_string(),
            events,
            mpe: None,
        })
    }

    /// A clip's tempo, time or key signature at `tick`; false for any other
    /// message.
    fn clip_meta(&mut self, tick: i64, d: &Decoded) -> bool {
        match *d {
            Decoded::SetTempo(0) => self.add(ItemKind::LeftOut, BAD_TEMPO),
            Decoded::SetTempo(v) => {
                let v = u64::from(v);
                let milli = 6_000_000_000_000u64
                    .checked_add(v / 2)
                    .and_then(|x| x.checked_div(v))
                    .unwrap_or(0);
                self.tempo(tick, milli);
            }
            Decoded::TimeSignature {
                numerator,
                denominator,
                ..
            } => self.time_sig(tick, numerator, denominator),
            Decoded::KeySignature { sharps, tonic } => {
                self.key_sig(tick, sharps, ump::is_minor(sharps, tonic));
            }
            _ => return false,
        }
        true
    }

    /// The tempo map: the changes in tick order (the last at a tick wins),
    /// 120 BPM before the first.
    fn tempo_map(&mut self) -> TempoMap {
        let mut changes = std::mem::take(&mut self.tempo);
        changes.sort_by_key(|(t, _)| *t);
        let mut points: Vec<TempoPoint> = Vec::new();
        for (t, bpm) in changes {
            match points.last_mut() {
                Some(p) if p.tick.0 == t => p.bpm = bpm,
                _ => points.push(TempoPoint {
                    tick: Tick(t),
                    bpm,
                    ramp: false,
                }),
            }
        }
        if points.first().is_none_or(|p| p.tick != Tick::ZERO) {
            points.insert(
                0,
                TempoPoint {
                    tick: Tick::ZERO,
                    bpm: Bpm::whole(120),
                    ramp: false,
                },
            );
        }
        TempoMap::new(points).unwrap_or_default()
    }

    /// The meter map: 4/4 in C until the first change; each change takes
    /// effect at the first barline at or after it (Changed when it moves).
    fn meter_map(&mut self) -> MeterMap {
        let mut changes = std::mem::take(&mut self.meter);
        changes.sort_by_key(|(t, _)| *t);
        let mut entries = vec![MeterEntry {
            tick: Tick::ZERO,
            meter: Meter { num: 4, den: 4 },
            key: KeySig::default(),
        }];
        for (t, change) in changes {
            let Some(last) = entries.last().copied() else {
                continue;
            };
            let bar = last.meter.bar_ticks().max(1);
            let rel = t.saturating_sub(last.tick.0);
            let at = if rel <= 0 {
                Some(last.tick.0)
            } else {
                rel.checked_add(bar - 1)
                    .and_then(|r| r.checked_div(bar))
                    .and_then(|k| k.checked_mul(bar))
                    .and_then(|d| d.checked_add(last.tick.0))
            };
            let Some(at) = at.filter(|a| *a <= MAX_TICKS) else {
                self.add(ItemKind::LeftOut, PAST_END);
                continue;
            };
            if at != t {
                self.add(ItemKind::Changed, METER_MOVED);
            }
            let mut e = if at == last.tick.0 {
                entries.pop().unwrap_or(last)
            } else {
                MeterEntry {
                    tick: Tick(at),
                    ..last
                }
            };
            match change {
                MeterChange::Time(m) => e.meter = m,
                MeterChange::Key(k) => e.key = k,
            }
            entries.push(e);
        }
        entries.dedup_by(|e, prev| e.meter == prev.meter && e.key == prev.key);
        MeterMap::new(entries).unwrap_or_default()
    }

    /// The import of `sources`.
    fn finish(
        mut self,
        kind: MidiFileKind,
        sources: Vec<Source>,
        split: bool,
    ) -> Result<MidiImport, IoError> {
        let tempo = self.tempo_map();
        let meter = self.meter_map();
        let start_offset = match (meter.entries().front(), meter.entries().get(1)) {
            (Some(a), Some(b))
                if b.tick.0 == a.meter.bar_ticks() && a.meter.bar_ticks() < b.meter.bar_ticks() =>
            {
                Tick(a.meter.bar_ticks())
            }
            _ => Tick::ZERO,
        };
        let mut markers = std::mem::take(&mut self.markers);
        markers.sort_by_key(|m| m.tick);
        let mut tracks = Vec::new();
        for s in sources {
            tracks.extend(split_source(s, split));
        }
        if self.cancel.load(Ordering::Relaxed) {
            return Err(IoError::Cancelled);
        }
        Ok(MidiImport {
            kind,
            title: self.title,
            copyright: self.copyright,
            tempo,
            meter,
            has_tempo: self.has_tempo,
            start_offset,
            markers,
            tracks,
            report: self.report,
        })
    }
}

fn parse_smf(
    bytes: &[u8],
    opts: &ImportOptions,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<MidiImport, IoError> {
    let (f, report) = smf::read(bytes, limits, cancel)?;
    let timecode = matches!(f.division, Division::Smpte { .. });
    let mut ctx = Ctx::new(limits, cancel, report, Scale::of(f.division));
    if timecode {
        ctx.add(ItemKind::Changed, TIMECODE);
    }
    let mut sources = Vec::with_capacity(f.tracks.len());
    for (i, t) in f.tracks.iter().enumerate() {
        let n = i.saturating_add(1);
        sources.push(ctx.smf_track(n, &t.events, f.format, timecode, opts)?);
    }
    let kind = if f.format == 0 {
        MidiFileKind::Smf0
    } else {
        MidiFileKind::Smf1
    };
    let split = opts.split_channels.unwrap_or(f.format == 0);
    ctx.finish(kind, sources, split)
}

fn parse_clip(
    bytes: &[u8],
    opts: &ImportOptions,
    limits: &Limits,
    cancel: &AtomicBool,
) -> Result<MidiImport, IoError> {
    let (c, report) = clip::read(bytes, limits, cancel)?;
    let mut ctx = Ctx::new(limits, cancel, report, Scale::per_quarter(c.tpq));
    let source = ctx.clip_track(&c)?;
    ctx.finish(
        MidiFileKind::Clip,
        vec![source],
        opts.split_channels.unwrap_or(false),
    )
}

/// The zone of an MPE Configuration Message (RPN 6 with a data entry of 1
/// or more on channel 1 or 16) before the track's first note, if any.
fn mcm(events: &[(u64, SmfEvent)]) -> Option<MpeConfig> {
    // Selected RPN on channels 1 and 16.
    let mut rpn = [(127u8, 127u8); 2];
    for (_, e) in events {
        let SmfEvent::Channel(m) = e else { continue };
        let [s, d1, d2] = m.bytes;
        let (kind, ch, d1, d2) = (s & 0xF0, s & 0x0F, d1 & 0x7F, d2 & 0x7F);
        if kind == 0x90 && d2 > 0 {
            return None;
        }
        let (slot, zone) = match ch {
            0 => (0, MpeZone::Lower),
            15 => (1, MpeZone::Upper),
            _ => continue,
        };
        let Some(r) = rpn.get_mut(slot) else { continue };
        if kind != 0xB0 {
            continue;
        }
        match d1 {
            101 => r.0 = d2,
            100 => r.1 = d2,
            99 | 98 => *r = (127, 127),
            6 if *r == (0, 6) && d2 > 0 => {
                return Some(MpeConfig {
                    zone,
                    members: d2.min(MAX_MEMBERS),
                    bend_range: DEFAULT_BEND_RANGE,
                });
            }
            _ => {}
        }
    }
    None
}

/// The pair with the most notes among `events`' channel messages, ties to
/// the lowest; without notes the lowest pair.
fn main_of(events: &[EventSpec]) -> Option<(u8, u8)> {
    let mut count: BTreeMap<(u8, u8), u64> = BTreeMap::new();
    for e in events {
        if let Some(p) = e.group_channel
            && is_channel_voice(&e.body)
        {
            let n = count.entry(p).or_insert(0);
            if matches!(e.body, EventBody::Note { .. }) {
                *n = n.saturating_add(1);
            }
        }
    }
    count
        .into_iter()
        .max_by_key(|(p, n)| (*n, std::cmp::Reverse(*p)))
        .map(|(p, _)| p)
}

/// SysEx plays on its group and the track's main channel.
fn fix_sysex(events: &mut [EventSpec], main: Option<(u8, u8)>) {
    let ch = main.map_or(0, |(_, c)| c);
    for e in events {
        if let (EventBody::SysEx { .. }, Some((g, _))) = (&e.body, e.group_channel) {
            e.group_channel = Some((g, ch));
        }
    }
}

/// One track for `s`, or (with `split`, unless it is MPE) one per pair of
/// its channel messages, the first also taking its texts and SysEx. None
/// for a source with no events (F11).
fn split_source(s: Source, split: bool) -> Vec<ImportTrack> {
    if s.events.is_empty() {
        return Vec::new();
    }
    let named = s.name.as_deref().and_then(valid_name).map(str::to_string);
    let pairs: BTreeSet<(u8, u8)> = s
        .events
        .iter()
        .filter(|e| is_channel_voice(&e.body))
        .filter_map(|e| e.group_channel)
        .collect();
    if !split || s.mpe.is_some() || pairs.len() <= 1 {
        let mut events = s.events;
        let main = main_of(&events);
        fix_sysex(&mut events, main);
        return vec![ImportTrack {
            name: named.unwrap_or(s.fallback),
            main,
            events,
            drum: main.is_some_and(|(_, c)| c == DRUM_CHANNEL),
            mpe: s.mpe,
        }];
    }
    let first = pairs.first().copied();
    let mut by_pair: BTreeMap<(u8, u8), Vec<EventSpec>> = BTreeMap::new();
    for e in s.events {
        let key = match e.group_channel {
            Some(p) if is_channel_voice(&e.body) => Some(p),
            _ => first,
        };
        if let Some(k) = key {
            by_pair.entry(k).or_default().push(e);
        }
    }
    by_pair
        .into_iter()
        .map(|((g, c), mut events)| {
            fix_sysex(&mut events, Some((g, c)));
            let n = u16::from(c).saturating_add(1);
            let name = match (&named, g, c) {
                (Some(base), _, _) => format!("{base} ch {n}"),
                (None, 0, DRUM_CHANNEL) => "Drums".to_string(),
                (None, 0, _) => format!("Channel {n}"),
                (None, g, _) => format!("Group {} Channel {n}", u16::from(g).saturating_add(1)),
            };
            ImportTrack {
                name,
                main: Some((g, c)),
                events,
                drum: c == DRUM_CHANNEL,
                mpe: None,
            }
        })
        .collect()
}

// ---- one track's events ----

/// A sounding note: its event and start.
#[derive(Debug, Clone, Copy)]
struct Open {
    slot: usize,
    start: i64,
}

/// Bank selects (CC 0 and CC 32) waiting for a program change on one pair.
#[derive(Debug, Clone, Copy, Default)]
struct Bank {
    tick: i64,
    msb: Option<(usize, u8)>,
    lsb: Option<(usize, u8)>,
}

/// One track's events as they are mapped. A slot emptied (a bank select
/// merged into its program) is dropped at the end.
#[derive(Default)]
struct Builder {
    slots: Vec<Option<EventSpec>>,
    /// Sounding notes by group, channel and pitch, oldest first.
    open: BTreeMap<(u8, u8, u8), VecDeque<Open>>,
    bank: BTreeMap<(u8, u8), Bank>,
    has_notes: bool,
}

impl Builder {
    fn push(&mut self, tick: i64, gc: Option<(u8, u8)>, body: EventBody) -> usize {
        let mut e = EventSpec::new(Tick(tick), body);
        e.group_channel = gc;
        self.slots.push(Some(e));
        self.slots.len().saturating_sub(1)
    }

    /// A channel message other than a bank select: it ends the wait for a
    /// program change on its pair.
    fn channel(&mut self, tick: i64, g: u8, c: u8, body: EventBody) -> usize {
        self.bank.remove(&(g, c));
        self.push(tick, Some((g, c)), body)
    }

    fn text(&mut self, tick: i64, kind: TextKind, text: String) {
        self.push(tick, None, EventBody::Text { kind, text });
    }

    fn sysex(&mut self, tick: i64, g: u8, data: &[u8], eight_bit: bool, report: &mut Report) {
        if !eight_bit && data.iter().any(|b| *b >= 0x80) {
            report.add(ItemKind::LeftOut, BAD_SYSEX, None);
            return;
        }
        let data: Arc<[u8]> = Arc::from(data);
        self.push(tick, Some((g, 0)), EventBody::SysEx { data, eight_bit });
    }

    fn note_on(
        &mut self,
        tick: i64,
        (g, c): (u8, u8),
        pitch: u8,
        velocity: u16,
        attribute: Option<NoteAttribute>,
    ) -> usize {
        let mut body = EventBody::note(pitch, velocity, Tick::ZERO);
        if let EventBody::Note { attribute: a, .. } = &mut body {
            *a = attribute;
        }
        let slot = self.channel(tick, g, c, body);
        self.open
            .entry((g, c, pitch))
            .or_default()
            .push_back(Open { slot, start: tick });
        self.has_notes = true;
        slot
    }

    /// Ends the oldest sounding note of the pitch.
    fn note_off(&mut self, tick: i64, (g, c): (u8, u8), pitch: u8, release: u16, r: &mut Report) {
        self.bank.remove(&(g, c));
        match self
            .open
            .get_mut(&(g, c, pitch))
            .and_then(VecDeque::pop_front)
        {
            Some(o) => self.close(o, tick, release),
            None => r.add(ItemKind::LeftOut, NO_NOTE, None),
        }
    }

    fn close(&mut self, o: Open, end: i64, release: u16) {
        if let Some(Some(EventSpec {
            body:
                EventBody::Note {
                    duration,
                    release_velocity,
                    ..
                },
            ..
        })) = self.slots.get_mut(o.slot)
        {
            *duration = Tick(end.saturating_sub(o.start).max(0));
            *release_velocity = release;
        }
    }

    /// A per-note value for the newest sounding note of the pitch, at its
    /// offset from that note's start.
    fn per_note(
        &mut self,
        tick: i64,
        key: (u8, u8, u8),
        kind: PerNoteKind,
        value: u32,
        r: &mut Report,
    ) {
        let note = self.open.get(&key).and_then(|q| q.back()).copied();
        let body = note.and_then(|o| {
            self.slots
                .get_mut(o.slot)
                .and_then(Option::as_mut)
                .map(|e| (&mut e.body, o.start))
        });
        match body {
            Some((EventBody::Note { per_note, .. }, start)) => {
                per_note.push_back(PerNoteCtl {
                    kind,
                    offset: Tick(tick.saturating_sub(start).max(0)),
                    value,
                });
            }
            _ => r.add(ItemKind::LeftOut, NO_NOTE_PER_NOTE, None),
        }
    }

    /// A MIDI 1.0 channel message on group `g`, translated up.
    fn midi1(&mut self, tick: i64, g: u8, m: Midi1Msg, r: &mut Report) {
        let [s, d1, d2] = m.bytes;
        let (c, d1, d2) = (s & 0x0F, d1 & 0x7F, d2 & 0x7F);
        match s & 0xF0 {
            0x90 if d2 > 0 => {
                self.note_on(tick, (g, c), d1, up7(d2), None);
            }
            0x80 => self.note_off(tick, (g, c), d1, up7(d2), r),
            0x90 => self.note_off(tick, (g, c), d1, 0, r),
            0xA0 => {
                self.channel(
                    tick,
                    g,
                    c,
                    EventBody::PolyPressure {
                        pitch: d1,
                        value: up32(u32::from(d2), 7),
                    },
                );
            }
            0xB0 if d1 == 0 || d1 == 32 => {
                let value = up32(u32::from(d2), 7);
                let slot = self.push(tick, Some((g, c)), EventBody::Cc { index: d1, value });
                let b = self.bank.entry((g, c)).or_default();
                if b.tick != tick {
                    *b = Bank {
                        tick,
                        ..Bank::default()
                    };
                }
                if d1 == 0 {
                    b.msb = Some((slot, d2));
                } else {
                    b.lsb = Some((slot, d2));
                }
            }
            0xB0 => {
                let value = up32(u32::from(d2), 7);
                self.channel(tick, g, c, EventBody::Cc { index: d1, value });
            }
            0xC0 => {
                let bank = match self.bank.remove(&(g, c)) {
                    Some(Bank {
                        tick: t,
                        msb: Some((ms, msb)),
                        lsb: Some((ls, lsb)),
                    }) if t == tick => {
                        for i in [ms, ls] {
                            if let Some(slot) = self.slots.get_mut(i) {
                                *slot = None;
                            }
                        }
                        Some((msb, lsb))
                    }
                    _ => None,
                };
                self.push(tick, Some((g, c)), EventBody::Program { program: d1, bank });
            }
            0xD0 => {
                let value = up32(u32::from(d1), 7);
                self.channel(tick, g, c, EventBody::ChannelPressure { value });
            }
            0xE0 => {
                let value = up32(u32::from(d1) | (u32::from(d2) << 7), 14);
                self.channel(tick, g, c, EventBody::PitchBend { value });
            }
            _ => {}
        }
    }

    /// A MIDI 1.0 message of an MPE track through `dec`. Notes of the zone
    /// play on its manager channel with their expression per note;
    /// messages outside the zone, or with the zone turned off, map as plain
    /// MIDI 1.0. `cfg` follows the zone's setting.
    fn mpe(
        &mut self,
        tick: i64,
        g: u8,
        m: Midi1Msg,
        dec: &mut MpeDecoder,
        cfg: &mut MpeConfig,
        r: &mut Report,
    ) {
        let [status, ..] = m.bytes;
        let ch = status & 0x0F;
        let live = dec.config().is_some() && dec.accepts(ch);
        let mut out: Vec<LiveMsg> = Vec::new();
        dec.decode(m, &mut |l| out.push(l));
        if let Some(Some(c)) = dec.take_changed() {
            *cfg = c;
        }
        if !live {
            self.midi1(tick, g, m, r);
            return;
        }
        let mgr = cfg.manager();
        for l in out {
            match l {
                LiveMsg::NoteOn {
                    pitch,
                    velocity,
                    expr,
                } => {
                    self.note_on(tick, (g, mgr), pitch, velocity, None);
                    for (kind, value) in [
                        (PerNoteKind::PitchBend, expr.bend),
                        (PerNoteKind::Registered(74), expr.timbre),
                    ] {
                        if value != CENTER {
                            self.per_note(tick, (g, mgr, pitch), kind, value, r);
                        }
                    }
                    if expr.pressure != 0 {
                        let value = expr.pressure;
                        self.channel(tick, g, mgr, EventBody::PolyPressure { pitch, value });
                    }
                }
                LiveMsg::NoteOff { pitch, velocity } => {
                    self.note_off(tick, (g, mgr), pitch, velocity, r);
                }
                LiveMsg::PerNote {
                    pitch,
                    expr: Expr::Pressure,
                    value,
                } => {
                    self.channel(tick, g, mgr, EventBody::PolyPressure { pitch, value });
                }
                LiveMsg::PerNote { pitch, expr, value } => {
                    let kind = match expr {
                        Expr::Bend => PerNoteKind::PitchBend,
                        _ => PerNoteKind::Registered(74),
                    };
                    self.per_note(tick, (g, mgr, pitch), kind, value, r);
                }
                LiveMsg::Channel(m) => self.midi1(tick, g, m, r),
            }
        }
    }

    /// A clip's channel voice message; false for anything else.
    fn ump(&mut self, tick: i64, d: &Decoded, r: &mut Report) -> bool {
        match *d {
            Decoded::Midi1 {
                group,
                status,
                data1,
                data2,
            } => {
                let m = if matches!(status & 0xF0, 0xC0 | 0xD0) {
                    Midi1Msg::two(status, data1)
                } else {
                    Midi1Msg::three(status, data1, data2)
                };
                self.midi1(tick, group, m, r);
            }
            Decoded::NoteOn {
                group,
                channel,
                note,
                velocity,
                attribute,
            } => {
                let attribute = attribute.map(|a| NoteAttribute {
                    kind: a.kind,
                    data: a.data,
                });
                self.note_on(tick, (group, channel), note & 0x7F, velocity, attribute);
            }
            Decoded::NoteOff {
                group,
                channel,
                note,
                velocity,
                ..
            } => self.note_off(tick, (group, channel), note & 0x7F, velocity, r),
            Decoded::PolyPressure {
                group,
                channel,
                note,
                value,
            } => {
                let pitch = note & 0x7F;
                self.channel(
                    tick,
                    group,
                    channel,
                    EventBody::PolyPressure { pitch, value },
                );
            }
            Decoded::ControlChange {
                group,
                channel,
                index,
                value,
            } => {
                let index = index & 0x7F;
                self.channel(tick, group, channel, EventBody::Cc { index, value });
            }
            Decoded::ProgramChange {
                group,
                channel,
                program,
                bank,
            } => {
                let body = EventBody::Program {
                    program: program & 0x7F,
                    bank: bank.map(|(m, l)| (m & 0x7F, l & 0x7F)),
                };
                self.channel(tick, group, channel, body);
            }
            Decoded::ChannelPressure {
                group,
                channel,
                value,
            } => {
                self.channel(tick, group, channel, EventBody::ChannelPressure { value });
            }
            Decoded::PitchBend {
                group,
                channel,
                value,
            } => {
                self.channel(tick, group, channel, EventBody::PitchBend { value });
            }
            Decoded::PerNoteRegistered {
                group,
                channel,
                note,
                index,
                value,
            } => {
                let key = (group, channel, note & 0x7F);
                self.per_note(tick, key, PerNoteKind::Registered(index), value, r);
            }
            Decoded::PerNoteAssignable {
                group,
                channel,
                note,
                index,
                value,
            } => {
                let key = (group, channel, note & 0x7F);
                self.per_note(tick, key, PerNoteKind::Assignable(index), value, r);
            }
            Decoded::PerNotePitchBend {
                group,
                channel,
                note,
                value,
            } => {
                let key = (group, channel, note & 0x7F);
                self.per_note(tick, key, PerNoteKind::PitchBend, value, r);
            }
            _ => return false,
        }
        true
    }

    /// The events, every note still sounding ended at `end` (Changed).
    fn finish(mut self, end: i64, r: &mut Report) -> Vec<EventSpec> {
        let open: Vec<Open> = std::mem::take(&mut self.open)
            .into_values()
            .flatten()
            .collect();
        for o in open {
            r.add(ItemKind::Changed, NO_END, None);
            self.close(o, end, 0);
        }
        self.slots.into_iter().flatten().collect()
    }
}
