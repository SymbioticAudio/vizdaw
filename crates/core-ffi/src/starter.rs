//! Co-Producer's starter projects (plan 8e Task 4; Ruling 5): every plug-in
//! reference of a checked spec resolved by 8d Ruling 3, or its built-in
//! fallback ([`resolve`], which never fails), and the build of a new song
//! from them as one transaction ([`build`]): tempo and meter, markers,
//! buses, tracks, then inserts, outputs and sends through 8d's
//! `mixer_chores::commands_for`.
//!
//! A starter sets up sounds and routing only. It writes no notes, regions
//! or automation (owner, 2026-11-16).
//!
//! Tracks on the Built-in Synth share its one strip, and audio tracks the
//! song's one output path's strip. The first track on a shared strip sets
//! it up; a later one's effects, output and sends are left out, with a
//! line, rather than stacked on the first's.

use crate::error::VizError;
use crate::mixer_chores::{self, PluginLookup, plugin_ref_text};
use crate::snapshot::ParamRow;
use crate::tracks::{TrackKindRow, build_plugin_instrument_track, build_track};
use core_control::registry::ArgValue;
use core_control::starter::{StarterKind, StarterSpec};
use core_host::PluginKind;
use core_host::bundled::MULTIQ_CLAP_ID;
use core_model::command::lane::LaneCmd;
use core_model::command::{Command, MixerCmd, SequenceCmd};
use core_model::diff::shown_name;
use core_model::document::Document;
use core_model::history::History;
use core_model::ids::{AnyId, LaneId, PluginInstanceId, SeqId, StripId};
use core_model::lane::MonitorMode;
use core_model::marks::lane_strip;
use core_model::meter::{Meter, MeterEntry, MeterMap};
use core_model::mixer::{PluginFormat, PluginRef, StripKind};
use core_model::selection::ViewFilter;
use core_model::sequence::Marker;
use core_model::tempo::{Bpm, TempoMap};
use core_model::time::Tick;

/// The refusal for a song that is not new.
pub const NEEDS_A_NEW_SONG: &str = "A starter project needs a new song.";

/// The line added when any track plays the Built-in Synth.
pub const SHARED_SYNTH: &str = "Tracks on the Built-in Synth share its one sound for now.";

/// The Built-in Synth's reference text.
const BUILT_IN_SYNTH: &str = mixer_chores::BUILT_IN_SYNTH;

/// What a starter track plays.
#[derive(Debug, Clone, PartialEq)]
pub enum PluginChoice {
    /// A usable plug-in instrument.
    Plugin(PluginRef),
    /// The Built-in Synth: asked for, or in place of an unusable plug-in.
    BuiltinSynth,
    /// Nothing: an audio track.
    Audio,
}

/// Each spec plug-in resolved, or its fallback, with Ruling 5's lines.
#[derive(Debug, Clone, PartialEq)]
pub struct Resolved {
    /// One per spec track, in order.
    pub instruments: Vec<PluginChoice>,
    /// The inserts of each strip, in insert order: the spec's buses, then
    /// its tracks, each in order. A track that shares a strip with an
    /// earlier one has none.
    pub effects: Vec<Vec<PluginRef>>,
    /// The fallback lines, in spec order (buses, then tracks), then
    /// [`SHARED_SYNTH`] when any track plays the Built-in Synth.
    pub lines: Vec<String>,
}

/// Whether `p` is the bundled Multi-Q.
fn is_multiq(p: &PluginRef) -> bool {
    p.format == PluginFormat::Clap && p.id == MULTIQ_CLAP_ID
}

/// Which strip a track of `choice` plays through, when it is shared: the
/// Built-in Synth's, or the one output path's.
fn shared_group(choice: &PluginChoice) -> Option<u8> {
    match choice {
        PluginChoice::Plugin(_) => None,
        PluginChoice::BuiltinSynth => Some(0),
        PluginChoice::Audio => Some(1),
    }
}

/// The first track before `i` whose strip track `i` shares, if any.
fn shares_with(instruments: &[PluginChoice], i: usize) -> Option<usize> {
    let g = shared_group(instruments.get(i)?)?;
    instruments
        .iter()
        .take(i)
        .position(|c| shared_group(c) == Some(g))
}

/// `refs` resolved as effects on strip `strip`: an unusable one gets
/// Multi-Q in its place while the strip has no Multi-Q insert yet, and is
/// left out otherwise (or when Multi-Q is not usable either).
fn effects_of(
    strip: &str,
    refs: &[String],
    host: &dyn PluginLookup,
    lines: &mut Vec<String>,
) -> Vec<PluginRef> {
    let mut out: Vec<PluginRef> = Vec::with_capacity(refs.len());
    for r in refs {
        match host.resolve(r, PluginKind::Effect) {
            Ok(p) => out.push(p),
            Err(e) => {
                let mq = if out.iter().any(is_multiq) {
                    None
                } else {
                    host.resolve("multi-q", PluginKind::Effect).ok()
                };
                match mq {
                    Some(mq) => {
                        out.push(mq);
                        lines.push(format!("{strip}: {e}; Multi-Q is in its place."));
                    }
                    None => lines.push(format!("{strip}: {e}; left out.")),
                }
            }
        }
    }
    out
}

/// Ruling 5's "using what is installed": every reference of `spec`
/// resolved through `host` (8d Ruling 3), with the Built-in Synth in place
/// of an unusable instrument and Multi-Q, or nothing, in place of an
/// unusable effect, each with its line. Never fails.
pub fn resolve(spec: &StarterSpec, host: &dyn PluginLookup) -> Resolved {
    let mut lines = Vec::new();
    let mut effects = Vec::with_capacity(spec.buses.len() + spec.tracks.len());
    for b in &spec.buses {
        effects.push(effects_of(&b.name, &b.effects, host, &mut lines));
    }
    let mut instruments: Vec<PluginChoice> = Vec::with_capacity(spec.tracks.len());
    for (i, t) in spec.tracks.iter().enumerate() {
        let choice = match (t.kind, t.plugin.as_deref()) {
            (StarterKind::Audio, _) => PluginChoice::Audio,
            (StarterKind::Instrument, None) => PluginChoice::BuiltinSynth,
            (StarterKind::Instrument, Some(r)) if r == BUILT_IN_SYNTH => PluginChoice::BuiltinSynth,
            (StarterKind::Instrument, Some(r)) => match host.resolve(r, PluginKind::Instrument) {
                Ok(p) => PluginChoice::Plugin(p),
                Err(e) => {
                    lines.push(format!(
                        "{}: {e}; it plays the Built-in Synth instead.",
                        t.name
                    ));
                    PluginChoice::BuiltinSynth
                }
            },
        };
        instruments.push(choice);
        match shares_with(&instruments, i).and_then(|f| spec.tracks.get(f)) {
            Some(first) => {
                if !t.effects.is_empty() || t.output.is_some() || !t.sends.is_empty() {
                    lines.push(format!(
                        "{}: shares its strip with {}; its effects, output and sends are left out.",
                        t.name, first.name
                    ));
                }
                effects.push(Vec::new());
            }
            None => effects.push(effects_of(&t.name, &t.effects, host, &mut lines)),
        }
    }
    if instruments.contains(&PluginChoice::BuiltinSynth) {
        lines.push(SHARED_SYNTH.into());
    }
    Resolved {
        instruments,
        effects,
        lines,
    }
}

/// One strip of a starter's answer.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StarterStripRow {
    /// The track's or bus's name as made, safe to show.
    pub name: String,
    /// What a track plays: its instrument's catalog name, "Built-in Synth"
    /// or "audio"; empty for a bus.
    pub plays: String,
    /// Each insert of its strip as loaded, safe to show.
    pub effects: Vec<String>,
}

/// What `co_producer.starter` made (Ruling 5).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CoProducerStarterRow {
    /// The spec's title.
    pub title: String,
    /// The undo step's label: "Co-Producer: Start <Title>" in its request.
    pub undo_label: String,
    /// The tracks, in spec order.
    pub tracks: Vec<StarterStripRow>,
    /// The buses, in spec order.
    pub buses: Vec<StarterStripRow>,
    /// The fallback lines ([`Resolved::lines`]).
    pub lines: Vec<String>,
}

/// The one sequence of a new song as File › New makes it: no tracks, no
/// markers, no strip but master and no instrument or output path. `None`
/// for any other song.
pub fn new_song_seq(doc: &Document) -> Option<SeqId> {
    let mut seqs = doc.sequences().values();
    let seq = seqs.next()?;
    let fresh = seqs.next().is_none()
        && seq.lanes.is_empty()
        && seq.markers.is_empty()
        && doc.mixer().strips().len() == 1
        && doc.instruments().is_empty()
        && doc.outputs().is_empty();
    fresh.then_some(seq.id)
}

/// What [`build`] made: the tracks and buses, in spec order.
#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    /// One per spec track.
    pub lanes: Vec<LaneId>,
    /// One per spec bus.
    pub buses: Vec<StripId>,
}

/// A [`PluginLookup`] answering the one plug-in already resolved, so the
/// build loads what [`resolve`] chose even if the catalog changes now.
struct Chosen<'a>(&'a PluginRef);

impl PluginLookup for Chosen<'_> {
    fn resolve(&self, _text: &str, _want: PluginKind) -> Result<PluginRef, VizError> {
        Ok(self.0.clone())
    }

    fn params(&self, _slot: PluginInstanceId) -> Option<Vec<ParamRow>> {
        None
    }
}

/// A [`PluginLookup`] for the edits that load nothing.
struct NoPlugins;

impl PluginLookup for NoPlugins {
    fn resolve(&self, text: &str, _want: PluginKind) -> Result<PluginRef, VizError> {
        Err(VizError::Model {
            message: format!("No installed plug-in is {}.", shown_name(text)),
        })
    }

    fn params(&self, _slot: PluginInstanceId) -> Option<Vec<ParamRow>> {
        None
    }
}

/// Runs `cmd` in `h`'s open transaction; its warnings go to `notes`.
fn run(h: &mut History, notes: &mut Vec<String>, cmd: &Command) -> Result<Vec<AnyId>, VizError> {
    let r = h.execute(cmd, &ViewFilter::default())?;
    notes.extend(r.warnings);
    Ok(r.created)
}

/// 8d's mixer edit `id` on `h`'s document with `args`, through
/// [`mixer_chores::commands_for`].
fn chore(
    h: &mut History,
    notes: &mut Vec<String>,
    id: &str,
    args: &[(String, ArgValue)],
    host: &dyn PluginLookup,
) -> Result<(), VizError> {
    for c in mixer_chores::commands_for(h.doc(), id, args, host)? {
        run(h, notes, &c)?;
    }
    Ok(())
}

fn id_arg(name: &str, id: StripId) -> Result<(String, ArgValue), VizError> {
    let v = i64::try_from(id.0).map_err(|_| VizError::Model {
        message: format!("Strip {} is out of range.", id.0),
    })?;
    Ok((name.into(), ArgValue::Int(v)))
}

fn text_arg(name: &str, text: String) -> (String, ArgValue) {
    (name.into(), ArgValue::Text(text))
}

/// Strip `strip`'s inserts, output and sends: `effects` in order, then
/// `output` (a spec bus; `None`: master, as made), then `sends`.
fn set_up_strip(
    h: &mut History,
    notes: &mut Vec<String>,
    strip: StripId,
    effects: &[PluginRef],
    output: Option<StripId>,
    sends: &[(StripId, f64)],
) -> Result<(), VizError> {
    for p in effects {
        let args = [
            id_arg("strip", strip)?,
            text_arg("plugin", plugin_ref_text(p)),
        ];
        chore(h, notes, "mixer.add_insert", &args, &Chosen(p))?;
    }
    let none = NoPlugins;
    if let Some(bus) = output {
        let args = [id_arg("strip", strip)?, id_arg("bus", bus)?];
        chore(h, notes, "mixer.set_output", &args, &none)?;
    }
    for (bus, level_db) in sends {
        let args = [
            id_arg("strip", strip)?,
            id_arg("bus", *bus)?,
            // At most six decimals: the registry's own shape.
            text_arg("level_db", format!("{level_db:.6}")),
            ("pre_fader".into(), ArgValue::Flag(false)),
        ];
        chore(h, notes, "mixer.add_send", &args, &none)?;
    }
    Ok(())
}

fn bad(message: impl Into<String>) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

/// Ruling 5's build of `spec` in sequence `seq`, inside `h`'s open
/// transaction (the caller arms `h`, begins, and commits or aborts), in
/// Ruling 5's order: tempo and meter, markers, the buses (each with the
/// default EQ), the tracks (as `track.new_plugin_instrument` and
/// `track.new_audio` make them, named as the spec names them), then each
/// strip's inserts, output and sends through
/// [`mixer_chores::commands_for`]. A strip used by one track alone takes
/// its name. Writes no notes, regions or automation. The steps' warning
/// lines go to `notes`.
pub fn build(
    h: &mut History,
    seq: SeqId,
    spec: &StarterSpec,
    resolved: &Resolved,
    monitor: MonitorMode,
    notes: &mut Vec<String>,
) -> Result<Built, VizError> {
    // 1. Tempo and meter.
    let tempo = TempoMap::constant(Bpm(spec.tempo_milli))?;
    run(
        h,
        notes,
        &Command::Sequence(SequenceCmd::SetTempo { seq, tempo }),
    )?;
    let key = h
        .doc()
        .sequence(seq)?
        .meter
        .entries()
        .front()
        .map(|e| e.key)
        .unwrap_or_default();
    let meter = Meter {
        num: spec.meter.0,
        den: spec.meter.1,
    };
    let map = MeterMap::new(vec![MeterEntry {
        tick: Tick::ZERO,
        meter,
        key,
    }])?;
    run(
        h,
        notes,
        &Command::Sequence(SequenceCmd::SetMeter { seq, meter: map }),
    )?;

    // 2. Markers, each bar's tick from the new meter.
    let bar = meter.bar_ticks();
    let mut markers: Vec<Marker> = spec
        .markers
        .iter()
        .map(|(b, name)| Marker {
            tick: Tick(i64::from(b.saturating_sub(1)).saturating_mul(bar)),
            name: name.clone(),
        })
        .collect();
    markers.sort_by_key(|m| m.tick);
    run(
        h,
        notes,
        &Command::Sequence(SequenceCmd::SetMarkers {
            seq,
            markers: markers.into_iter().collect(),
        }),
    )?;

    // 3. The buses.
    let mut buses = Vec::with_capacity(spec.buses.len());
    for b in &spec.buses {
        let made = run(
            h,
            notes,
            &Command::Mixer(MixerCmd::AddBus {
                name: b.name.clone(),
                index: None,
            }),
        )?;
        let doc = h.doc();
        let bus = made
            .iter()
            .find_map(|id| match id {
                AnyId::Strip(s)
                    if doc
                        .mixer()
                        .strip(*s)
                        .is_ok_and(|x| x.kind == StripKind::Bus) =>
                {
                    Some(*s)
                }
                _ => None,
            })
            .ok_or_else(|| bad("no bus was added"))?;
        buses.push(bus);
    }

    // 4. The tracks.
    let mut lanes = Vec::with_capacity(spec.tracks.len());
    for (t, choice) in spec.tracks.iter().zip(&resolved.instruments) {
        let lane = match choice {
            PluginChoice::Plugin(p) => {
                build_plugin_instrument_track(h, seq, p.clone(), None, monitor, notes)?
            }
            PluginChoice::BuiltinSynth => {
                build_track(h, seq, TrackKindRow::Instrument, None, monitor, notes)?
            }
            PluginChoice::Audio => build_track(h, seq, TrackKindRow::Audio, None, monitor, notes)?,
        };
        run(
            h,
            notes,
            &Command::Lane(LaneCmd::Rename {
                seq,
                lane,
                name: t.name.clone(),
            }),
        )?;
        lanes.push(lane);
    }

    // 5. Inserts, outputs and sends.
    let bus_named = |name: &str| -> Result<StripId, VizError> {
        spec.buses
            .iter()
            .position(|b| b.name == name)
            .and_then(|i| buses.get(i))
            .copied()
            .ok_or_else(|| {
                bad(format!(
                    "No bus {} in the starter project.",
                    shown_name(name)
                ))
            })
    };
    let mut effects = resolved.effects.iter();
    for (b, strip) in spec.buses.iter().zip(&buses) {
        let fx = effects.next().map(Vec::as_slice).unwrap_or_default();
        let output = b.output.as_deref().map(bus_named).transpose()?;
        set_up_strip(h, notes, *strip, fx, output, &[])?;
    }
    for (i, (t, lane)) in spec.tracks.iter().zip(&lanes).enumerate() {
        let fx = effects.next().map(Vec::as_slice).unwrap_or_default();
        if shares_with(&resolved.instruments, i).is_some() {
            continue;
        }
        let strip = lane_strip(h.doc(), *lane)
            .ok_or_else(|| bad(format!("Track {} has no strip.", shown_name(&t.name))))?;
        let group = resolved.instruments.get(i).and_then(shared_group);
        let alone = group.is_none()
            || resolved
                .instruments
                .iter()
                .filter(|c| shared_group(c) == group)
                .count()
                == 1;
        if alone {
            run(
                h,
                notes,
                &Command::Mixer(MixerCmd::RenameStrip {
                    strip,
                    name: t.name.clone(),
                }),
            )?;
        }
        let output = t.output.as_deref().map(bus_named).transpose()?;
        let sends = t
            .sends
            .iter()
            .map(|s| Ok((bus_named(&s.bus)?, s.level_db)))
            .collect::<Result<Vec<_>, VizError>>()?;
        set_up_strip(h, notes, strip, fx, output, &sends)?;
    }
    Ok(Built { lanes, buses })
}

/// A strip's row: its name and its inserts' names, safe to show.
fn strip_row(
    doc: &Document,
    strip: Option<StripId>,
    name: String,
    plays: String,
) -> StarterStripRow {
    let s = strip.and_then(|s| doc.mixer().strip(s).ok());
    StarterStripRow {
        name,
        plays,
        effects: s
            .map(|s| {
                s.inserts
                    .iter()
                    .map(|p| shown_name(&p.plugin.name))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// The answer for what [`build`] made in `doc`: each track's and bus's
/// name as made, what each track plays and each strip's inserts; then
/// `resolved`'s lines and each distinct line of `notes`.
pub fn answer(
    doc: &Document,
    spec: &StarterSpec,
    resolved: &Resolved,
    built: &Built,
    undo_label: String,
    notes: &[String],
) -> CoProducerStarterRow {
    let lane_name = |lane: LaneId| {
        doc.sequences()
            .values()
            .find_map(|s| s.lane(lane))
            .map(|l| shown_name(&l.name))
            .unwrap_or_default()
    };
    let tracks = built
        .lanes
        .iter()
        .zip(&resolved.instruments)
        .map(|(lane, choice)| {
            let plays = match choice {
                PluginChoice::Plugin(p) => shown_name(&p.name),
                PluginChoice::BuiltinSynth => "Built-in Synth".into(),
                PluginChoice::Audio => "audio".into(),
            };
            strip_row(doc, lane_strip(doc, *lane), lane_name(*lane), plays)
        })
        .collect();
    let buses = built
        .buses
        .iter()
        .map(|b| {
            let name = doc
                .mixer()
                .strip(*b)
                .map(|s| shown_name(&s.name))
                .unwrap_or_default();
            strip_row(doc, Some(*b), name, String::new())
        })
        .collect();
    let mut lines = resolved.lines.clone();
    for n in notes {
        if !lines.contains(n) {
            lines.push(n.clone());
        }
    }
    CoProducerStarterRow {
        title: spec.title.clone(),
        undo_label,
        tracks,
        buses,
        lines,
    }
}
