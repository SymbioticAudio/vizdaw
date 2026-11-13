//! The return record and the snapshot (plan 6d Ruling 16, spec §7.8): what
//! an export keeps so that plan 6e can merge a returning file.
//!
//! The record (`vizdaw/record.json` in the package) holds the whole
//! document; the snapshot (`Interchange/<export id>.json` in the project)
//! holds the [`Projection`] of what was exported. Both map every XML ID to
//! the VizDAW object it came from.

use super::export::{Layout, RegionGain, Ticker, layout};
use super::export_timeline::use_span;
use super::units::swatch_hex;
use crate::IoError;
use crate::limits::{MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_ENTRIES, MAX_XML_NODES};
use core_model::automation::AutomationTarget;
use core_model::document::{Document, check_project_id};
use core_model::event::{EventBody, PerNoteKind};
use core_model::ids::SeqId;
use core_model::mixer::PluginFormat;
use core_model::query::ResolvedEvent;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// The record's package entry.
pub const RECORD_ENTRY: &str = "vizdaw/record.json";
/// The project folder holding the snapshots.
pub const SNAPSHOT_DIR: &str = "Interchange";
/// How many snapshots a project keeps.
pub const KEEP_SNAPSHOTS: usize = 10;
/// The record's format (and 6d's snapshots').
pub const FORMAT: u32 = 1;
/// The snapshot format an export writes (plan 6e Ruling 3); formats 1 and
/// 2 are read.
pub const SNAPSHOT_FORMAT: u32 = 2;
/// The refusal of a snapshot that cannot be read (plan 6e Ruling 17).
pub const SNAPSHOT_DAMAGED: &str =
    "The project's copy of that export is damaged, so the file can't be merged.";

/// The VizDAW object an XML ID stands for. Every ID the export writes has
/// one. `kind` and what `id` (and `slot`, `index`) name:
///
/// - `track`: a track's `Track` (`t<lane>`, `t<lane>a`); `id` is the lane;
/// - `strip`: a strip's channel or its copy (`c<strip>`, `c<strip>t<lane>`),
///   a bus track (`b<strip>`) or the master track; `id` is the strip;
/// - `channel`: a plain channel (`k<lane>`, `k<lane>a`); `id` is its track;
/// - `volume`, `pan`, `mute`: a strip channel's parameter (`id` the strip);
///   `channel_volume`, `channel_pan`, `channel_mute`: a plain channel's
///   (`id` its track);
/// - `send`, `send_volume`, `send_enable`: a send and its parameters; `id`
///   is the strip, `index` the send's position;
/// - `device`, `device_enable`: a plug-in and its on/off; `id` is the
///   instance;
/// - `parameter`: a plug-in parameter; `id` is the parameter, `slot` its
///   instance;
/// - `tempo`, `meter`: the transport's tempo and time signature; `id` is the
///   sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectRef {
    /// What it is (see above).
    pub kind: String,
    /// Its ID's number.
    pub id: u64,
    /// The plug-in instance of a `parameter`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<u64>,
    /// The position of a send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
}

/// `vizdaw/record.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    /// [`FORMAT`].
    pub format: u32,
    /// The project's ID.
    pub project_id: String,
    /// The export's ID.
    pub export_id: String,
    /// When, in Unix seconds.
    pub exported_at: u64,
    /// VizDAW's version.
    pub app_version: String,
    /// The exported sequence's ID.
    pub seq: u64,
    /// The tick at file time 0.
    pub origin: i64,
    /// XML ID → VizDAW object.
    pub ids: BTreeMap<String, ObjectRef>,
    /// The whole `project.json`, as JSON.
    pub document: serde_json::Value,
}

/// The part of a record an import reads (plan 6d).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct RecordHead {
    /// [`FORMAT`].
    pub format: u32,
    /// The project's ID.
    pub project_id: String,
    /// The export's ID.
    pub export_id: String,
}

/// `Interchange/<export id>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// [`SNAPSHOT_FORMAT`] (or 6d's [`FORMAT`]).
    pub format: u32,
    /// The project's ID.
    pub project_id: String,
    /// The export's ID.
    pub export_id: String,
    /// When, in Unix seconds.
    pub exported_at: u64,
    /// The exported sequence's ID.
    pub seq: u64,
    /// The tick at file time 0.
    pub origin: i64,
    /// XML ID → VizDAW object; at most the reader's `max_ids` entries,
    /// counted while parsing.
    #[serde(deserialize_with = "bounded_ids")]
    pub ids: BTreeMap<String, ObjectRef>,
    /// What was exported.
    pub projection: Projection,
    /// The export's region-gain option: "apply" or "leave_out" (format 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region_gain: Option<String>,
    /// Who each projection row is (format 2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<Identity>,
}

/// Who each projection row is in the project it was taken from, parallel to
/// the projection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// One per [`Projection::tracks`] entry.
    #[serde(deserialize_with = "counted")]
    pub tracks: Vec<TrackIds>,
    /// One per [`Projection::strips`] entry.
    #[serde(deserialize_with = "counted")]
    pub strips: Vec<StripIds>,
}

/// The identities behind one [`PTrack`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackIds {
    /// The track (both halves of a split track name it).
    pub lane: u64,
    /// One per [`PTrack::notes_played`] entry.
    #[serde(deserialize_with = "counted")]
    pub notes: Vec<EventAt>,
    /// One per [`PTrack::midi`] entry.
    #[serde(deserialize_with = "counted")]
    pub midi: Vec<EventAt>,
    /// One per [`PTrack::regions`] entry.
    #[serde(deserialize_with = "counted")]
    pub regions: Vec<EventAt>,
    /// One per [`PTrack::automation`] curve, one per point of it.
    #[serde(deserialize_with = "counted_lists")]
    pub automation: Vec<Vec<EventAt>>,
    /// The track's top-level reference uses, by tick (on the half that
    /// holds the notes).
    #[serde(deserialize_with = "counted")]
    pub uses: Vec<UseAt>,
}

/// An event, and the top-level event it plays through (itself, or its
/// reference use).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventAt {
    /// The source event.
    pub event: u64,
    /// The exported sequence's event it plays through.
    pub top: u64,
}

/// One top-level reference use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UseAt {
    /// The reference event.
    pub event: u64,
    /// The sequence it uses.
    pub seq: u64,
    /// Where it starts playing (the track's play shift included).
    pub tick: i64,
    /// Where its last pass ends.
    pub end: i64,
    /// Its transpose.
    pub transpose: i8,
    /// How many passes it plays.
    pub passes: u32,
}

/// The identities behind one [`PStrip`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StripIds {
    /// The strip it is (or a copy of); none for a plain channel.
    pub strip: Option<u64>,
    /// The plug-in instances, in [`PStrip::devices`] order.
    #[serde(deserialize_with = "counted")]
    pub devices: Vec<u64>,
}

/// The limits a snapshot is read under (tests lower them, as 6d's
/// `record_size` lowers `max_record`).
#[derive(Debug, Clone, Copy)]
pub struct SnapshotLimits {
    /// Largest snapshot, in bytes.
    pub max_bytes: usize,
    /// Most `ids` entries.
    pub max_ids: usize,
    /// Most list entries in the projection and identity together, nested
    /// lists included, counted while parsing.
    pub max_entries: usize,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        SnapshotLimits {
            max_bytes: MAX_SNAPSHOT_BYTES,
            max_ids: MAX_XML_NODES,
            max_entries: MAX_SNAPSHOT_ENTRIES,
        }
    }
}

thread_local! {
    /// The most `ids` a snapshot parsed on this thread may hold:
    /// [`read_snapshot_with`]'s, else [`MAX_XML_NODES`].
    static MAX_IDS: Cell<usize> = const { Cell::new(MAX_XML_NODES) };
    /// The list entries a snapshot parsed on this thread may still hold:
    /// set by [`read_snapshot_with`], else unbounded.
    static ENTRIES_LEFT: Cell<usize> = const { Cell::new(usize::MAX) };
}

/// Restores [`MAX_IDS`] and [`ENTRIES_LEFT`] when dropped.
struct Budgets {
    ids: usize,
    entries: usize,
}

impl Budgets {
    fn arm(limits: &SnapshotLimits) -> Budgets {
        Budgets {
            ids: MAX_IDS.with(|m| m.replace(limits.max_ids)),
            entries: ENTRIES_LEFT.with(|m| m.replace(limits.max_entries)),
        }
    }
}

impl Drop for Budgets {
    fn drop(&mut self) {
        MAX_IDS.with(|m| m.set(self.ids));
        ENTRIES_LEFT.with(|m| m.set(self.entries));
    }
}

/// A list whose every entry is counted against [`ENTRIES_LEFT`] as it is
/// parsed (plan 6e Task 7: a snapshot of tiny entries would otherwise grow
/// about 8× in memory).
#[derive(Debug, Clone, PartialEq)]
struct Counted<T>(Vec<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Counted<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::{Error, SeqAccess, Visitor};
        use std::marker::PhantomData;

        struct List<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for List<T> {
            type Value = Vec<T>;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("a list")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some(v) = seq.next_element::<T>()? {
                    // Entries nested in `v` came off the budget while it
                    // was parsed; this one comes off what is left.
                    let left = ENTRIES_LEFT.with(Cell::get);
                    if left == 0 {
                        return Err(A::Error::custom("too many list entries"));
                    }
                    ENTRIES_LEFT.with(|m| m.set(left - 1));
                    out.push(v);
                }
                Ok(out)
            }
        }

        d.deserialize_seq(List(PhantomData)).map(Counted)
    }
}

/// A list field, counted ([`Counted`]).
fn counted<'de, D, T>(d: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Counted::<T>::deserialize(d).map(|c| c.0)
}

/// A list of lists, both levels counted ([`Counted`]).
fn counted_lists<'de, D, T>(d: D) -> Result<Vec<Vec<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Counted::<Counted<T>>::deserialize(d).map(|c| c.0.into_iter().map(|l| l.0).collect())
}

/// One automation curve: its target and (tick, value) points.
type NamedCurve = (String, Vec<(i64, f64)>);

/// Automation curves, their points counted too ([`Counted`]).
fn counted_curves<'de, D>(d: D) -> Result<Vec<NamedCurve>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Counted::<(String, Counted<(i64, f64)>)>::deserialize(d)
        .map(|c| c.0.into_iter().map(|(t, p)| (t, p.0)).collect())
}

/// `ids`, refused at the entry past [`MAX_IDS`].
fn bounded_ids<'de, D>(d: D) -> Result<BTreeMap<String, ObjectRef>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error, MapAccess, Visitor};

    struct Ids(usize);

    impl<'de> Visitor<'de> for Ids {
        type Value = BTreeMap<String, ObjectRef>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a map of XML IDs")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut out = BTreeMap::new();
            while let Some(key) = map.next_key::<String>()? {
                if out.len() >= self.0 {
                    return Err(A::Error::custom("too many IDs"));
                }
                let value = map.next_value::<ObjectRef>()?;
                if out.insert(key, value).is_some() {
                    return Err(A::Error::custom("an ID twice"));
                }
            }
            Ok(out)
        }
    }

    d.deserialize_map(Ids(MAX_IDS.with(Cell::get)))
}

/// Reads a snapshot: at most `max_bytes` (checked before parsing), JSON,
/// format 1 or 2, UUID project and export IDs, at most `max_ids` IDs and
/// `max_entries` list entries (both counted while parsing), a known
/// region-gain word, and an identity
/// parallel to the projection (Ruling 17). A format 1 snapshot has neither
/// a region gain nor an identity; every strip index is in range. Anything
/// else is [`SNAPSHOT_DAMAGED`].
pub fn read_snapshot_with(bytes: &[u8], limits: &SnapshotLimits) -> Result<Snapshot, IoError> {
    let damaged = || IoError::Refused(SNAPSHOT_DAMAGED.to_owned());
    if bytes.len() > limits.max_bytes {
        return Err(damaged());
    }
    let snap: Snapshot = {
        let _restore = Budgets::arm(limits);
        serde_json::from_slice(bytes).map_err(|_| damaged())?
    };
    if well_formed(&snap) {
        Ok(snap)
    } else {
        Err(damaged())
    }
}

/// [`read_snapshot_with`] at the default limits.
pub fn read_snapshot(bytes: &[u8]) -> Result<Snapshot, IoError> {
    read_snapshot_with(bytes, &SnapshotLimits::default())
}

/// Whether a parsed snapshot holds together (see [`read_snapshot_with`]).
fn well_formed(s: &Snapshot) -> bool {
    let p = &s.projection;
    let n = p.strips.len();
    let strips_ok = p.tracks.iter().all(|t| t.strip.is_none_or(|i| i < n))
        && p.strips
            .iter()
            .all(|x| x.output.is_none_or(|i| i < n) && x.sends.iter().all(|send| send.0 < n));
    let ids_ok = check_project_id(&s.project_id) && check_project_id(&s.export_id);
    let extras_ok = match s.format {
        FORMAT => s.region_gain.is_none() && s.identity.is_none(),
        SNAPSHOT_FORMAT => {
            s.region_gain
                .as_deref()
                .is_none_or(|g| matches!(g, "apply" | "leave_out"))
                && s.identity.as_ref().is_none_or(|id| parallel(p, id))
        }
        _ => false,
    };
    strips_ok && ids_ok && extras_ok
}

/// Whether `id` has one entry per row of `p`.
fn parallel(p: &Projection, id: &Identity) -> bool {
    id.tracks.len() == p.tracks.len()
        && id.strips.len() == p.strips.len()
        && p.tracks.iter().zip(&id.tracks).all(|(t, i)| {
            t.notes_played.len() == i.notes.len()
                && t.midi.len() == i.midi.len()
                && t.regions.len() == i.regions.len()
                && t.automation.len() == i.automation.len()
                && t.automation
                    .iter()
                    .zip(&i.automation)
                    .all(|((_, pts), at)| pts.len() == at.len())
        })
        && p.strips
            .iter()
            .zip(&id.strips)
            .all(|(x, i)| x.devices.len() == i.devices.len())
}

/// `<root>/Interchange/<export id>.json`: lstat of the folder (a real
/// folder, as the write side requires, else `Ok(None)` for missing) and of
/// the file (a regular file, else `Ok(None)` for missing), at most `MAX_SNAPSHOT_BYTES + 1` bytes read,
/// then [`read_snapshot`]. A link or a file in place of the folder, a link,
/// a folder or a FIFO in place of the file, or a snapshot
/// of another export, is [`SNAPSHOT_DAMAGED`]; an `export_id` that is not
/// a UUID names no snapshot.
pub fn read_snapshot_file(root: &Path, export_id: &str) -> Result<Option<Snapshot>, IoError> {
    let damaged = || IoError::Refused(SNAPSHOT_DAMAGED.to_owned());
    if !check_project_id(export_id) {
        return Ok(None);
    }
    let dir = root.join(SNAPSHOT_DIR);
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err(damaged()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(crate::io_err(&dir, "read", &e)),
    }
    let path = dir.join(format!("{export_id}.json"));
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(crate::io_err(&path, "read", &e)),
    };
    if !meta.is_file() {
        return Err(damaged());
    }
    let file = std::fs::File::open(&path).map_err(|e| crate::io_err(&path, "opened", &e))?;
    // The path may have been swapped since the check above. (A FIFO
    // swapped in can still block the open: `std` has no non-blocking one.)
    if !file
        .metadata()
        .map_err(|e| crate::io_err(&path, "read", &e))?
        .is_file()
    {
        return Err(damaged());
    }
    let probe = u64::try_from(MAX_SNAPSHOT_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    let mut bytes = Vec::new();
    file.take(probe)
        .read_to_end(&mut bytes)
        .map_err(|e| crate::io_err(&path, "read", &e))?;
    let snap = read_snapshot(&bytes)?;
    if snap.export_id != export_id {
        return Err(damaged());
    }
    Ok(Some(snap))
}

// ---------------------------------------------------------------- normalisers

/// The last point of a tempo map without its ramp (a ramp there leads
/// nowhere).
fn ends_flat(mut v: Vec<(i64, u32, bool)>) -> Vec<(i64, u32, bool)> {
    if let Some(last) = v.last_mut() {
        last.2 = false;
    }
    v
}

/// `n`'s per-note values as a set: by offset, then kind.
fn sorted_expression(n: &mut PNote) {
    n.expression
        .sort_by(|x, y| (x.1, &x.0, x.2).cmp(&(y.1, &y.0, y.2)));
}

/// `p` as an unchanged file exported with pickup `origin` brings it back
/// (Ruling 1); for base and ours. Every tick is shifted by `-origin`, the
/// last tempo point is flat, each note's expression is sorted and each
/// region's gain is 0. Under [`RegionGain::Apply`] a region with gain has
/// no media hash: its audio comes back as a new copy (Ruling 11). The
/// result's origin is 0.
pub fn as_returned(p: &Projection, origin: i64, region_gain: RegionGain) -> Projection {
    let by = |t: i64| t.saturating_sub(origin);
    let tracks = p
        .tracks
        .iter()
        .map(|t| {
            let mut t = t.clone();
            for n in &mut t.notes_played {
                n.tick = by(n.tick);
                sorted_expression(n);
            }
            for m in &mut t.midi {
                m.0 = by(m.0);
            }
            for r in &mut t.regions {
                r.tick = by(r.tick);
                if region_gain == RegionGain::Apply && r.gain_db != 0.0 {
                    r.media_hash = None;
                }
                r.gain_db = 0.0;
            }
            for (_, points) in &mut t.automation {
                for pt in points {
                    pt.0 = by(pt.0);
                }
            }
            t
        })
        .collect();
    Projection {
        tracks,
        strips: p.strips.clone(),
        tempo: ends_flat(p.tempo.iter().map(|&(t, b, r)| (by(t), b, r)).collect()),
        meter: p.meter.iter().map(|&(t, n, d)| (by(t), n, d)).collect(),
        markers: p.markers.iter().map(|(t, n)| (by(*t), n.clone())).collect(),
        info: p.info.clone(),
        origin: 0,
    }
}

/// The entries of a map from tick `at` on; the entry in force there counts
/// as one on it (as an import without a pickup plays it).
fn from_tick<T: Copy>(
    map: &[T],
    at: i64,
    tick: impl Fn(&T) -> i64,
    on: impl Fn(&T, i64) -> T,
) -> Vec<T> {
    let mut out: Vec<T> = map.iter().filter(|p| tick(p) >= at).copied().collect();
    if out.first().is_none_or(|p| tick(p) > at)
        && let Some(held) = map.iter().rev().find(|p| tick(p) < at)
    {
        out.insert(0, on(held, at));
    }
    out
}

/// `points` without the one-tick hold points an import adds after a step:
/// a point one tick after the last kept one, with its value.
fn without_holds(points: &[(i64, f64)]) -> Vec<(i64, f64)> {
    let mut out: Vec<(i64, f64)> = Vec::new();
    for p in points {
        if let Some(last) = out.last()
            && p.0 == last.0.saturating_add(1)
            && p.1 == last.1
        {
            continue;
        }
        out.push(*p);
    }
    out
}

/// A returning file's projection in the same frame as [`as_returned`]'s
/// (Ruling 1): the tempo and meter maps from tick `-origin` on, no hold
/// points, the last tempo point flat and each note's expression sorted.
/// Region hashes are left as they are.
pub fn returned(theirs: &Projection, origin: i64) -> Projection {
    let at = 0i64.saturating_sub(origin);
    let mut p = theirs.clone();
    for t in &mut p.tracks {
        for n in &mut t.notes_played {
            sorted_expression(n);
        }
        for (_, points) in &mut t.automation {
            *points = without_holds(points);
        }
    }
    p.tempo = ends_flat(from_tick(&theirs.tempo, at, |x| x.0, |x, t| (t, x.1, x.2)));
    p.meter = from_tick(&theirs.meter, at, |x| x.0, |x, t| (t, x.1, x.2));
    p
}

/// What the route carries (§7.1), ID-free: tracks in order, strips by index.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct Projection {
    /// The exported tracks (a split track is two).
    #[serde(deserialize_with = "counted")]
    pub tracks: Vec<PTrack>,
    /// The exported channels, in the structure's order: the tracks', then
    /// buses and submixes, then the master.
    #[serde(deserialize_with = "counted")]
    pub strips: Vec<PStrip>,
    /// The tempo map: tick, milli-BPM, ramp.
    #[serde(deserialize_with = "counted")]
    pub tempo: Vec<(i64, u32, bool)>,
    /// The meter map: tick, numerator, denominator.
    #[serde(deserialize_with = "counted")]
    pub meter: Vec<(i64, u8, u8)>,
    /// The markers: tick, name.
    #[serde(deserialize_with = "counted")]
    pub markers: Vec<(i64, String)>,
    /// The 13 song-info fields.
    #[serde(deserialize_with = "counted")]
    pub info: Vec<Option<String>>,
    /// The tick at file time 0.
    pub origin: i64,
}

/// One exported track.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PTrack {
    /// Its name.
    pub name: String,
    /// Whether it holds notes.
    pub notes: bool,
    /// Whether it holds audio.
    pub audio: bool,
    /// Its channel in [`Projection::strips`].
    pub strip: Option<usize>,
    /// The notes it plays.
    #[serde(deserialize_with = "counted")]
    pub notes_played: Vec<PNote>,
    /// Its other MIDI events: tick, "<kind> <channel> <number> <value>".
    #[serde(deserialize_with = "counted")]
    pub midi: Vec<(i64, String)>,
    /// Its regions.
    #[serde(deserialize_with = "counted")]
    pub regions: Vec<PRegion>,
    /// Its automation: target, then (tick, value) points.
    #[serde(deserialize_with = "counted_curves")]
    pub automation: Vec<(String, Vec<(i64, f64)>)>,
}

/// One played note.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PNote {
    /// Where it starts.
    pub tick: i64,
    /// Its key.
    pub pitch: u8,
    /// Its MIDI channel.
    pub channel: u8,
    /// Its velocity.
    pub velocity: u16,
    /// Its release velocity.
    pub release: u16,
    /// Its length.
    pub duration: i64,
    /// Its per-note values the route carries: kind, offset, value.
    #[serde(deserialize_with = "counted")]
    pub expression: Vec<(String, i64, u32)>,
}

/// One region.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PRegion {
    /// Where it starts.
    pub tick: i64,
    /// Its media's content hash.
    pub media_hash: Option<String>,
    /// Its start in the media, in frames.
    pub offset: u64,
    /// Its length, in frames.
    pub length: u64,
    /// Its fade-in length, in frames.
    pub fade_in: u64,
    /// Its fade-out length, in frames.
    pub fade_out: u64,
    /// Whether it is muted.
    pub muted: bool,
    /// Its own name.
    pub name: Option<String>,
    /// Whether its fade-in is a crossfade.
    pub crossfade: bool,
    /// Its gain, in dB.
    pub gain_db: f32,
    /// Its media's name: the stem of the file it came from (plan 6e
    /// Ruling 4 matches by it when a region has no content hash). Absent
    /// in snapshots made before 6e's fix round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stem: Option<String>,
}

/// One exported channel.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct PStrip {
    /// Its name.
    pub name: String,
    /// "regular", "effect", "submix" or "master".
    pub role: String,
    /// Its colour, `#rrggbb`.
    pub colour: Option<String>,
    /// Its fader, in dB.
    pub gain_db: f64,
    /// Its pan, −1 … +1.
    pub pan: f64,
    /// Its mute.
    pub mute: bool,
    /// Its solo.
    pub solo: bool,
    /// The strip it plays into.
    pub output: Option<usize>,
    /// Its sends: target strip, level in dB, pre-fader, enabled.
    #[serde(deserialize_with = "counted")]
    pub sends: Vec<(usize, f64, bool, bool)>,
    /// Its devices: format, plug-in ID, bypass, state hash.
    #[serde(deserialize_with = "counted")]
    pub devices: Vec<(String, String, bool, Option<u64>)>,
}

/// The projection of sequence `seq` as `export` writes it.
pub fn projection(doc: &Document, seq: SeqId) -> Result<Projection, IoError> {
    let never = AtomicBool::new(false);
    let mut scratch = crate::report::Report::default();
    let l = layout(doc, seq, &mut scratch, &never)?;
    project(&l, &never)
}

/// [`projection`], with who each of its rows is.
pub fn projection_with_identity(
    doc: &Document,
    seq: SeqId,
) -> Result<(Projection, Identity), IoError> {
    let never = AtomicBool::new(false);
    let mut scratch = crate::report::Report::default();
    let l = layout(doc, seq, &mut scratch, &never)?;
    project_with_identity(&l, &never)
}

fn role_text(r: super::schema::DpMixerRole) -> &'static str {
    use super::schema::DpMixerRole::*;
    match r {
        Regular => "regular",
        Master => "master",
        Effect => "effect",
        Submix => "submix",
        Vca => "vca",
    }
}

fn per_note_kind(k: PerNoteKind) -> Option<String> {
    match k {
        PerNoteKind::PitchBend => Some("pitch_bend".to_owned()),
        PerNoteKind::Registered(n @ (7 | 10 | 74)) => Some(format!("registered {n}")),
        PerNoteKind::Registered(_) | PerNoteKind::Assignable(_) => None,
    }
}

/// A MIDI event other than a note, as the projection spells it; `None` for
/// what the route does not carry.
fn midi_text(r: &ResolvedEvent) -> Option<String> {
    let ch = r.group_channel.map_or(0, |g| g.1);
    Some(match &r.body {
        EventBody::Cc { index, value } => format!("cc {ch} {index} {value}"),
        EventBody::PitchBend { value } => format!("pitch_bend {ch} 0 {value}"),
        EventBody::ChannelPressure { value } => format!("channel_pressure {ch} 0 {value}"),
        EventBody::PolyPressure { pitch, value } => format!("poly_pressure {ch} {pitch} {value}"),
        EventBody::Program { program, bank } => match bank {
            Some((msb, lsb)) => format!(
                "program {ch} {program} {}",
                u32::from(*msb) * 128 + u32::from(*lsb)
            ),
            None => format!("program {ch} {program} none"),
        },
        _ => return None,
    })
}

/// The projection of a layout.
pub(super) fn project(l: &Layout<'_>, cancel: &AtomicBool) -> Result<Projection, IoError> {
    project_with_identity(l, cancel).map(|(p, _)| p)
}

/// The top-level reference uses of track part `p` (none on a split
/// track's audio half), by tick.
fn uses_of(l: &Layout<'_>, p: &super::export::Part<'_>) -> Vec<UseAt> {
    if p.audio && p.split {
        return Vec::new();
    }
    let lane = p.lane;
    let mut out: Vec<UseAt> = lane
        .active()
        .into_iter()
        .flat_map(|t| t.events.values())
        .filter_map(|e| {
            let EventBody::Reference {
                seq,
                length,
                loop_count,
                transpose,
            } = e.body
            else {
                return None;
            };
            // As the export writes the use's clips.
            let span = use_span(l.doc, seq, length, loop_count);
            let (passes, total) = (u32::try_from(span.passes).unwrap_or(u32::MAX), span.total);
            let tick = e.tick.0.saturating_add(lane.play_shift.0);
            Some(UseAt {
                event: e.id.0,
                seq: seq.0,
                tick,
                end: tick.saturating_add(total),
                transpose,
                passes,
            })
        })
        .collect();
    out.sort_by_key(|u| (u.tick, u.event));
    out
}

/// One automation curve's points and who each point is.
type Curve = (Vec<(i64, f64)>, Vec<EventAt>);

/// The projection of a layout and who each of its rows is, from one walk:
/// each row's identity is pushed where the row is.
pub(super) fn project_with_identity(
    l: &Layout<'_>,
    cancel: &AtomicBool,
) -> Result<(Projection, Identity), IoError> {
    let doc = l.doc;
    let s = l.seq;
    let by_id: BTreeMap<&str, usize> = l
        .chans
        .iter()
        .enumerate()
        .map(|(i, c)| (c.id.as_str(), i))
        .collect();
    let index = |id: &str| by_id.get(id).copied();

    let strip_ids: Vec<StripIds> = l
        .chans
        .iter()
        .map(|c| StripIds {
            strip: c.strip.map(|s| s.id.0),
            devices: c.slots().iter().map(|(slot, _, _)| slot.id.0).collect(),
        })
        .collect();
    let strips = l
        .chans
        .iter()
        .map(|c| {
            let st = c.strip;
            PStrip {
                name: c.name.clone(),
                role: role_text(c.role).to_owned(),
                colour: st.and_then(|s| s.colour).map(|w| swatch_hex(w).to_owned()),
                gain_db: st.map_or(0.0, |s| s.gain_db),
                pan: st.map_or(0.0, |s| s.pan),
                mute: st.is_some_and(|s| s.mute),
                solo: st.is_some_and(|s| s.solo),
                output: c.to.as_deref().and_then(index),
                sends: st
                    .map(|s| {
                        s.sends
                            .iter()
                            .filter_map(|x| {
                                let to = index(&format!("c{}", x.target.0))?;
                                Some((to, x.level_db, x.pre_fader, x.enabled))
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                devices: c
                    .slots()
                    .iter()
                    .map(|(slot, _, _)| {
                        let format = match slot.plugin.format {
                            PluginFormat::Au => "au",
                            PluginFormat::Clap => "clap",
                        };
                        let hash = slot
                            .state
                            .filter(|id| doc.state_blob(*id).is_some())
                            .map(|id| id.0);
                        (format.to_owned(), slot.plugin.id.clone(), slot.bypass, hash)
                    })
                    .collect(),
            }
        })
        .collect();

    let mut ticker = Ticker::new(cancel);
    let mut tracks = Vec::new();
    let mut track_ids = Vec::new();
    for (pi, p) in l.parts.iter().enumerate() {
        let mut ids = TrackIds {
            lane: p.lane.id.0,
            uses: uses_of(l, p),
            ..TrackIds::default()
        };
        let mut t = PTrack {
            name: p.name(),
            notes: !p.audio,
            audio: p.audio,
            strip: Some(p.chan),
            notes_played: Vec::new(),
            midi: Vec::new(),
            regions: Vec::new(),
            automation: Vec::new(),
        };
        let mut automation: BTreeMap<String, Curve> = BTreeMap::new();
        for r in l.played.get(&p.lane.id).map_or(&[][..], Vec::as_slice) {
            ticker.tick()?;
            let at = EventAt {
                event: r.path.last().map_or(0, |x| x.event.0),
                top: r.path.first().map_or(0, |x| x.event.0),
            };
            // What a muted reference use holds does not play.
            if r.path.len() > 1 && r.path.first().is_some_and(|x| l.muted.contains(&x.event)) {
                continue;
            }
            let tick = r.tick.0;
            match &r.body {
                EventBody::AudioRegion {
                    media,
                    source_offset,
                    length,
                    gain_db,
                    fade_in,
                    fade_out,
                    name,
                    crossfade,
                } if p.audio => {
                    ids.regions.push(at);
                    t.regions.push(PRegion {
                        tick,
                        media_hash: doc.media().get(media).and_then(|m| m.content_hash.clone()),
                        stem: doc
                            .media()
                            .get(media)
                            .map(|m| m.name.clone())
                            .filter(|n| !n.is_empty()),
                        offset: *source_offset,
                        length: *length,
                        fade_in: fade_in.length_frames,
                        fade_out: fade_out.length_frames,
                        muted: r.path.len() == 1
                            && r.path.last().is_some_and(|x| l.muted.contains(&x.event)),
                        name: name.clone(),
                        crossfade: *crossfade,
                        gain_db: *gain_db,
                    });
                }
                EventBody::AudioRegion { .. } => {}
                _ if p.audio && p.split => {}
                EventBody::Note {
                    pitch,
                    velocity,
                    release_velocity,
                    duration,
                    per_note,
                    ..
                } => {
                    ids.notes.push(at);
                    t.notes_played.push(PNote {
                        tick,
                        pitch: *pitch,
                        channel: r.group_channel.map_or(0, |g| g.1),
                        velocity: *velocity,
                        release: *release_velocity,
                        duration: duration.0,
                        expression: per_note
                            .iter()
                            .filter_map(|c| Some((per_note_kind(c.kind)?, c.offset.0, c.value)))
                            .collect(),
                    });
                }
                EventBody::Automation { target, value } => {
                    let Some((c, d)) = l.target(pi, *target) else {
                        continue;
                    };
                    let name = match (*target, d) {
                        (AutomationTarget::StripFader(_), _) => format!("fader {c}"),
                        (AutomationTarget::StripPan(_), _) => format!("pan {c}"),
                        (AutomationTarget::StripMute(_), _) => format!("mute {c}"),
                        (AutomationTarget::SendLevel(_, i), _) => format!("send {c} {}", i.0),
                        (AutomationTarget::PluginParam(_, q), Some(d)) => {
                            format!("param {c} {d} {}", q.0)
                        }
                        _ => continue,
                    };
                    let curve = automation.entry(name).or_default();
                    curve.0.push((tick, *value));
                    curve.1.push(at);
                }
                _ => {
                    if let Some(m) = midi_text(r) {
                        t.midi.push((tick, m));
                        ids.midi.push(at);
                    }
                }
            }
        }
        for (name, (points, at)) in automation {
            t.automation.push((name, points));
            ids.automation.push(at);
        }
        tracks.push(t);
        track_ids.push(ids);
    }
    let projection = Projection {
        tracks,
        strips,
        tempo: s
            .tempo
            .points()
            .iter()
            .map(|p| (p.tick.0, p.bpm.0, p.ramp))
            .collect(),
        meter: s
            .meter
            .entries()
            .iter()
            .map(|e| (e.tick.0, e.meter.num, e.meter.den))
            .collect(),
        markers: s
            .markers
            .iter()
            .map(|m| (m.tick.0, m.name.clone()))
            .collect(),
        info: doc.settings().info.fields().into_iter().cloned().collect(),
        origin: l.origin,
    };
    Ok((
        projection,
        Identity {
            tracks: track_ids,
            strips: strip_ids,
        },
    ))
}
