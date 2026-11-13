//! An opened `.dawproject` package (plan 6d Task 6): its project and
//! metadata read and checked, what it says about returning to VizDAW, the
//! audio it uses, and what the app asks before importing it.
//!
//! [`open`] reads, in order, the record (`vizdaw/record.json`: only the
//! first 64 KiB, for its head), `project.xml` and `metadata.xml`. It never
//! looks at files outside the package (a preview must not reach the paths a
//! file names, such as network mounts): [`DpPackage::needed_bytes`], in the
//! confirmed import, does. The project's tree is dropped
//! before its IDs are checked ([`project_of`] takes it by value), so one
//! byte budget ([`MAX_MODEL_BYTES`](crate::limits::MAX_MODEL_BYTES)) bounds
//! the read.

use super::record::{FORMAT, RECORD_ENTRY, RecordHead};
use super::schema::*;
use super::units::{EXPORT_LINE, TagKind, strip_tags};
use crate::IoError;
use crate::limits::PackageLimits;
use crate::report::{ItemKind, Report};
use crate::xml::Tree;
use crate::zip::{NOT_DAWPROJECT, ZipReader};
use core_model::document::MAX_INFO_BYTES;
use core_model::format::AudioFormat;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// The package entry of the project.
pub const PROJECT_ENTRY: &str = "project.xml";
/// The package entry of the song info.
pub const METADATA_ENTRY: &str = "metadata.xml";

/// Left out: a `metadata.xml` that could not be read (the reason as the
/// detail); the song info is left out.
pub const SONG_INFO_UNREADABLE: &str = "Song info that could not be read";
/// Note: a `vizdaw/record.json` that could not be read (the reason as the
/// detail); the file is treated as having no record.
pub const RECORD_UNREADABLE: &str = "The file's VizDAW record could not be read";
/// Note (Ruling 17): the file's audio has no project rate.
pub const ODD_RATE: &str = "The file's audio rate isn't a project rate; the project is 48 kHz";

/// The refusal for a project with more tracks than an import can build in
/// a bounded time (the limit in the sentence is the track limit).
fn too_many_tracks(limit: usize) -> IoError {
    IoError::Refused(format!(
        "This DAWproject file has more than {limit} tracks, more than VizDAW can open."
    ))
}

/// The refusal for a project with more buses than an import can build in a
/// bounded time.
fn too_many_buses(limit: usize) -> IoError {
    IoError::Refused(format!(
        "This DAWproject file has more than {limit} buses, more than VizDAW can open."
    ))
}

/// The rate of a project when the file gives none.
const DEFAULT_RATE: u32 = 48_000;
/// How many items the walks count between looks at the cancel flag.
const CANCEL_EVERY: usize = 65_536;
/// How much of `vizdaw/record.json` is read for its head (the export
/// writes the head's three fields first).
const RECORD_HEAD_BYTES: usize = 64 * 1024;

/// The line the app shows before an import for the files outside the
/// package, which are not looked at until the import is confirmed; `None`
/// for none.
pub fn external_files_note(n: u32) -> Option<String> {
    match n {
        0 => None,
        1 => Some("1 external audio file (checked when you import)".to_owned()),
        n => Some(format!(
            "{n} external audio files (checked when you import)"
        )),
    }
}

#[cfg(feature = "test-hooks")]
thread_local! {
    static EXTERNAL_STATS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Test hook: how many external files this thread has looked at since the
/// last call, which resets the count.
#[cfg(feature = "test-hooks")]
pub fn debug_take_external_stats() -> usize {
    EXTERNAL_STATS.with(|c| c.replace(0))
}

/// What a package says about where it came from (spec §7.8).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReturnInfo {
    /// The record's head, if the package has a readable one.
    pub record: Option<RecordHead>,
    /// The export ID of the metadata comment's last line
    /// (`VizDAW export <id>`).
    pub export_id: Option<String>,
    /// Note and audio tracks whose comment holds a `vz:t` tag.
    pub tagged_tracks: u32,
    /// Note and audio tracks (not folders, buses, VCAs or the master).
    pub tracks: u32,
}

impl ReturnInfo {
    /// Record, export line, or `vz:t` tags on at least half the tracks (§7.8).
    pub fn from_vizdaw(&self) -> bool {
        self.record.is_some()
            || self.export_id.is_some()
            || (self.tracks > 0 && u64::from(self.tagged_tracks) * 2 >= u64::from(self.tracks))
    }
}

/// What the app asks of a package before importing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DpInfo {
    /// The project rate to make (Ruling 17).
    pub sample_rate: u32,
    /// Note and audio tracks (S14).
    pub tracks: u32,
    /// Distinct audio files.
    pub audio_files: u32,
    /// How many of them are outside the package (not looked at before the
    /// import: see [`external_files_note`]).
    pub external_files: u32,
    /// Where it came from.
    pub ret: ReturnInfo,
}

/// Where an audio file's bytes are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioSource {
    /// A package entry, by name.
    Entry(String),
    /// A file outside the package (Ruling 19): the path as written, made
    /// absolute against the package's folder when relative. It may be
    /// anywhere, or a URL; the import job checks it before reading.
    External(PathBuf),
}

/// One audio file the project uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioRef {
    /// Its number, from 0 in document order; the key of its media.
    pub key: usize,
    /// Where it is.
    pub source: AudioSource,
    /// Its channels, as the file's `Audio` says (0 when unreadable).
    pub channels: u16,
    /// Its rate, as the file's `Audio` says (0 when unreadable).
    pub sample_rate: u32,
    /// Its length in frames, from the `Audio`'s duration and rate.
    pub frames: u64,
    /// Its name: the file name without its extension, at most 4 096 bytes.
    pub name: String,
}

/// An opened package. The ZIP stays open for [`DpPackage::extract`] and
/// [`DpPackage::state`].
pub struct DpPackage {
    /// The project.
    pub project: DpProject,
    /// The song info (empty when the package has none, or it could not be
    /// read).
    pub metadata: DpMetaData,
    /// Where it came from.
    pub ret: ReturnInfo,
    /// What reading changed, left out or noted.
    pub report: Report,
    zip: ZipReader,
    limits: PackageLimits,
    stem: String,
    audio: Vec<AudioRef>,
    /// Each audio file's key, by its path and whether it is outside.
    keys: HashMap<(String, bool), usize>,
    /// Per `Audio` element: its rate.
    rates: Vec<u32>,
}

fn stop(cancel: &AtomicBool) -> Result<(), IoError> {
    if cancel.load(Ordering::Relaxed) {
        Err(IoError::Cancelled)
    } else {
        Ok(())
    }
}

/// Reads `tree` as a project and checks its IDs. It takes the tree by
/// value and drops it before the check, which continues the reader's
/// byte budget and assumes the tree is gone.
pub fn project_of(tree: Tree, report: &mut Report) -> Result<DpProject, IoError> {
    let project = read_project(&tree, report)?;
    drop(tree);
    check_ids(&project, report)?;
    Ok(project)
}

/// Opens the package at `path`: refuses what is not a DAWproject file (no
/// `project.xml`, a damaged or too large one, version 2 and later); reads
/// the song info and the record when they can be read, else lists them.
/// Looks at `cancel` before any work and while reading.
pub fn open(
    path: &Path,
    limits: &PackageLimits,
    cancel: &AtomicBool,
) -> Result<DpPackage, IoError> {
    stop(cancel)?;
    let zip = ZipReader::open(path, limits)?;
    let mut report = Report::default();
    let record = read_record(&zip, limits, cancel, &mut report)?;
    stop(cancel)?;

    let entry = zip
        .find(PROJECT_ENTRY)
        .ok_or_else(|| IoError::Refused(NOT_DAWPROJECT.to_owned()))?;
    let text = zip.read(
        entry,
        limits.max_project_xml,
        Some(limits.max_ratio),
        cancel,
    )?;
    // The text and the tree share one parse budget.
    let parse = PackageLimits {
        max_tree_bytes: limits
            .max_tree_bytes
            .min(crate::limits::MAX_PARSE_BYTES.saturating_sub(text.len())),
        ..*limits
    };
    let tree = Tree::parse(&text, &parse, cancel)?;
    drop(text);
    let project = project_of(tree, &mut report)?;
    stop(cancel)?;

    let metadata = read_metadata_entry(&zip, limits, cancel, &mut report)?;
    stop(cancel)?;

    let export_id = metadata
        .fields
        .last()
        .and_then(Option::as_deref)
        .and_then(export_id_of);
    let (tracks, tagged_tracks, buses) = count_tracks(&project, cancel)?;
    if usize::try_from(tracks).unwrap_or(usize::MAX) > limits.max_import_tracks {
        return Err(too_many_tracks(limits.max_import_tracks));
    }
    if buses > limits.max_import_buses {
        return Err(too_many_buses(limits.max_import_buses));
    }
    let ret = ReturnInfo {
        record,
        export_id,
        tagged_tracks,
        tracks,
    };

    let base = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut walk = AudioWalk::new(cancel);
    walk.project(&project)?;
    let AudioWalk {
        refs, rates, seen, ..
    } = walk;
    let keys = seen
        .into_iter()
        .map(|((path, external), key)| ((path.to_owned(), external), key))
        .collect();
    let audio: Vec<AudioRef> = refs
        .into_iter()
        .enumerate()
        .map(|(key, a)| AudioRef {
            key,
            source: if a.file.external {
                AudioSource::External(base.join(&a.file.path))
            } else {
                AudioSource::Entry(a.file.path.clone())
            },
            channels: u16::try_from(a.channels).ok().unwrap_or(0),
            sample_rate: u32::try_from(a.sample_rate).ok().unwrap_or(0),
            frames: frames(a.duration, a.sample_rate),
            name: audio_name(&a.file.path, key),
        })
        .collect();
    if !rates.is_empty() && !rates.iter().any(|r| AudioFormat::RATES.contains(r)) {
        report.add(ItemKind::Note, ODD_RATE, None);
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(DpPackage {
        project,
        metadata,
        ret,
        report,
        zip,
        limits: *limits,
        stem,
        audio,
        keys,
        rates,
    })
}

/// The export ID of a metadata comment's last line, if it is the export line.
pub(crate) fn export_id_of(comment: &str) -> Option<String> {
    let last = comment.trim_end().rsplit('\n').next()?;
    let id = last.strip_prefix(EXPORT_LINE)?.trim();
    (!id.is_empty()).then(|| id.to_owned())
}

/// The record's head; a record that cannot be read is noted and treated as
/// absent. A cancel is passed on.
fn read_record(
    zip: &ZipReader,
    limits: &PackageLimits,
    cancel: &AtomicBool,
    report: &mut Report,
) -> Result<Option<RecordHead>, IoError> {
    let Some(entry) = zip.find(RECORD_ENTRY) else {
        return Ok(None);
    };
    // Only the head is unpacked (at most `RECORD_HEAD_BYTES`), so the
    // ratio limit, which guards whole entries, does not apply.
    let read = zip.read_prefix(entry, limits.max_record, RECORD_HEAD_BYTES, None, cancel);
    let why = match read {
        Err(IoError::Cancelled) => return Err(IoError::Cancelled),
        Err(e) => e.to_string(),
        Ok(bytes) => match head_of(&bytes) {
            Ok(head) if head.format == FORMAT => return Ok(Some(head)),
            Ok(head) => format!("it is format {}", head.format),
            Err(e) => e.to_string(),
        },
    };
    report.add(ItemKind::Note, RECORD_UNREADABLE, Some(why));
    Ok(None)
}

/// A record's head from the start of its text: its `format`, `project_id`
/// and `export_id`, read up to the third of them, whatever follows.
fn head_of(bytes: &[u8]) -> Result<RecordHead, String> {
    use serde::de::{Deserializer, Error, IgnoredAny, MapAccess, Visitor};

    /// The error that stops the parse once the head is complete.
    const DONE: &str = "head read";

    struct Head<'a>(&'a mut Option<RecordHead>);

    impl<'de> Visitor<'de> for Head<'_> {
        type Value = ();

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a VizDAW record")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
            let (mut format, mut project_id, mut export_id) = (None, None, None);
            while let Some(key) = map.next_key::<String>()? {
                match key.as_str() {
                    "format" => format = Some(map.next_value::<u32>()?),
                    "project_id" => project_id = Some(map.next_value::<String>()?),
                    "export_id" => export_id = Some(map.next_value::<String>()?),
                    _ => {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                if let (Some(format), Some(p), Some(e)) = (format, &project_id, &export_id) {
                    *self.0 = Some(RecordHead {
                        format,
                        project_id: p.clone(),
                        export_id: e.clone(),
                    });
                    return Err(A::Error::custom(DONE));
                }
            }
            Err(A::Error::custom(
                "it has no format, project_id and export_id",
            ))
        }
    }

    let mut head = None;
    let mut de = serde_json::Deserializer::from_slice(bytes);
    let got = (&mut de).deserialize_map(Head(&mut head));
    match (head, got) {
        (Some(h), _) => Ok(h),
        (None, Err(e)) => Err(e.to_string()),
        (None, Ok(())) => Err("it is empty".to_owned()),
    }
}

/// The song info; one that cannot be read is listed and left out. A cancel
/// is passed on.
fn read_metadata_entry(
    zip: &ZipReader,
    limits: &PackageLimits,
    cancel: &AtomicBool,
    report: &mut Report,
) -> Result<DpMetaData, IoError> {
    let Some(entry) = zip.find(METADATA_ENTRY) else {
        return Ok(DpMetaData::default());
    };
    let tree = zip
        .read(
            entry,
            limits.max_metadata_xml,
            Some(limits.max_ratio),
            cancel,
        )
        .and_then(|text| Tree::parse(&text, limits, cancel));
    match tree {
        Ok(tree) => Ok(read_metadata(&tree, report)),
        Err(IoError::Cancelled) => Err(IoError::Cancelled),
        Err(e) => {
            report.add(ItemKind::LeftOut, SONG_INFO_UNREADABLE, Some(e.to_string()));
            Ok(DpMetaData::default())
        }
    }
}

/// What a `Track` is to the import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrackRole {
    /// Its channel is the master.
    Master,
    /// Its channel is a VCA.
    Vca,
    /// It holds tracks: a folder.
    Folder,
    /// Its channel is an effect or submix channel.
    Bus,
    /// A note or audio track.
    Content,
}

/// What `t` is: the master, a VCA, a folder (it holds tracks), a bus (an
/// effect or submix channel) or a note or audio track.
pub(crate) fn role_of(t: &DpTrack) -> TrackRole {
    match t.channel.as_ref().and_then(|c| c.role) {
        Some(DpMixerRole::Master) => TrackRole::Master,
        Some(DpMixerRole::Vca) => TrackRole::Vca,
        _ if !t.children.is_empty() || t.content.contains(&DpContentType::Tracks) => {
            TrackRole::Folder
        }
        Some(DpMixerRole::Effect | DpMixerRole::Submix) => TrackRole::Bus,
        _ => TrackRole::Content,
    }
}

/// Decides each structure node's role in walk order, for both the count at
/// `open` and the structure walk of the import, so the two agree: the first
/// master (track or channel) is the master, every later master is a bus,
/// and a VCA is no bus.
#[derive(Debug, Default)]
pub(crate) struct Roles {
    master_seen: bool,
}

impl Roles {
    /// The role `t` gets, and whether it is the project's master. A master
    /// after the first is a [`TrackRole::Bus`].
    pub(crate) fn track(&mut self, t: &DpTrack) -> (TrackRole, bool) {
        let role = role_of(t);
        if role != TrackRole::Master {
            return (role, false);
        }
        if self.master_seen {
            (TrackRole::Bus, false)
        } else {
            self.master_seen = true;
            (TrackRole::Master, true)
        }
    }

    /// Whether the top-level channel `c` is the project's master, and
    /// whether it is a bus (neither the master nor a VCA).
    pub(crate) fn channel(&mut self, c: &DpChannel) -> (bool, bool) {
        let master = c.role == Some(DpMixerRole::Master) && !self.master_seen;
        self.master_seen |= master;
        let vca = c.role == Some(DpMixerRole::Vca);
        (master, !master && !vca)
    }
}

/// Whether a track with `role` becomes a bus.
pub(crate) fn is_bus(role: TrackRole) -> bool {
    matches!(role, TrackRole::Folder | TrackRole::Bus)
}

/// Note and audio tracks, how many of them carry a `vz:t` tag, and the
/// buses the import builds (folders, effect and submix channels, channels
/// outside tracks, and every master after the first), counted in the
/// structure walk's order with [`Roles`].
fn count_tracks(p: &DpProject, cancel: &AtomicBool) -> Result<(u32, u32, usize), IoError> {
    let (mut tracks, mut tagged, mut buses) = (0u32, 0u32, 0usize);
    let mut roles = Roles::default();
    let mut seen = 0usize;
    let mut stack: Vec<&DpTrack> = Vec::new();
    for item in &p.structure {
        let top = match item {
            DpStructureItem::Channel(c) => {
                if roles.channel(c).1 {
                    buses = buses.saturating_add(1);
                }
                continue;
            }
            DpStructureItem::Track(t) => t,
        };
        stack.push(top);
        while let Some(t) = stack.pop() {
            if seen % CANCEL_EVERY == 0 {
                stop(cancel)?;
            }
            seen = seen.saturating_add(1);
            stack.extend(t.children.iter().rev());
            let (role, _) = roles.track(t);
            if is_bus(role) {
                buses = buses.saturating_add(1);
                continue;
            }
            if role != TrackRole::Content {
                continue;
            }
            tracks = tracks.saturating_add(1);
            let has_tag = t
                .named
                .comment
                .as_deref()
                .is_some_and(|c| strip_tags(c).1.iter().any(|(k, _)| *k == TagKind::Track));
            if has_tag {
                tagged = tagged.saturating_add(1);
            }
        }
    }
    Ok((tracks, tagged, buses))
}

/// `seconds` at `rate` in frames, rounded; 0 for what cannot be a length.
fn frames(seconds: f64, rate: i32) -> u64 {
    let f = seconds * f64::from(rate);
    if rate > 0 && f.is_finite() && f > 0.0 {
        // `as` saturates past `u64::MAX`.
        f.round() as u64
    } else {
        0
    }
}

/// A file name without its folders and its extension; "Audio <n>" when
/// nothing is left.
fn audio_name(path: &str, key: usize) -> String {
    let file = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let stem = match file.rfind('.') {
        Some(i) if i > 0 => file.get(..i).unwrap_or(file),
        _ => file,
    };
    let mut stem = stem.trim();
    if stem.len() > MAX_INFO_BYTES {
        let mut end = MAX_INFO_BYTES;
        while !stem.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        stem = stem.get(..end).unwrap_or("").trim_end();
    }
    if stem.is_empty() {
        format!("Audio {}", key.saturating_add(1))
    } else {
        stem.to_owned()
    }
}

/// The size of an external file the import may read: a regular file, not
/// a URL (Ruling 19).
fn external_size(p: &Path) -> Option<u64> {
    if p.to_string_lossy().contains("://") {
        return None;
    }
    #[cfg(feature = "test-hooks")]
    EXTERNAL_STATS.with(|c| c.set(c.get().saturating_add(1)));
    std::fs::metadata(p)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .map(|m| m.len())
}

/// The walk that finds the audio files, in document order.
struct AudioWalk<'p, 'c> {
    refs: Vec<&'p DpAudio>,
    seen: HashMap<(&'p str, bool), usize>,
    rates: Vec<u32>,
    n: usize,
    cancel: &'c AtomicBool,
}

impl<'p, 'c> AudioWalk<'p, 'c> {
    fn new(cancel: &'c AtomicBool) -> Self {
        AudioWalk {
            refs: Vec::new(),
            seen: HashMap::new(),
            rates: Vec::new(),
            n: 0,
            cancel,
        }
    }

    fn tick(&mut self) -> Result<(), IoError> {
        self.n = self.n.wrapping_add(1);
        if self.n % CANCEL_EVERY == 0 {
            stop(self.cancel)
        } else {
            Ok(())
        }
    }

    fn project(&mut self, p: &'p DpProject) -> Result<(), IoError> {
        let mut stack: Vec<&'p DpTimeline> = Vec::new();
        for s in p.scenes.iter().rev() {
            stack.extend(&s.content);
        }
        if let Some(l) = p.arrangement.as_ref().and_then(|a| a.lanes.as_ref()) {
            stack.extend(l.items.iter().rev());
        }
        // The reader keeps timelines at most 16 deep; the stack holds what
        // is still to visit, in document order.
        while let Some(t) = stack.pop() {
            self.tick()?;
            match t {
                DpTimeline::Lanes(l) => stack.extend(l.items.iter().rev()),
                DpTimeline::Clips(c) => {
                    for clip in c.clips.iter().rev() {
                        self.tick()?;
                        stack.extend(clip.content.as_deref());
                    }
                }
                DpTimeline::Notes(n) => {
                    for note in n.notes.iter().rev() {
                        self.tick()?;
                        stack.extend(note.content.as_deref());
                    }
                }
                DpTimeline::Warps(w) => stack.push(&w.content),
                DpTimeline::ClipSlot(_, Some(clip)) => stack.extend(clip.content.as_deref()),
                DpTimeline::Audio(a) => {
                    self.rates
                        .push(u32::try_from(a.sample_rate).ok().unwrap_or(0));
                    let key = (a.file.path.as_str(), a.file.external);
                    if !self.seen.contains_key(&key) {
                        self.seen.insert(key, self.refs.len());
                        self.refs.push(a);
                    }
                }
                DpTimeline::Points(_)
                | DpTimeline::Markers(..)
                | DpTimeline::ClipSlot(_, None)
                | DpTimeline::Video(_)
                | DpTimeline::Unknown(_) => {}
            }
        }
        Ok(())
    }
}

impl DpPackage {
    /// Ruling 17's rate; `tracks` counts note and audio tracks, not folders,
    /// buses or the master (S14). The rate is the one most `Audio` elements
    /// have among the project rates; a tie goes to 48 kHz when it is one of
    /// the tied, else to the higher rate; with none at a project rate, 48 kHz
    /// (and `open` noted [`ODD_RATE`]).
    pub fn info(&self) -> DpInfo {
        let mut counts: BTreeMap<u32, u32> = BTreeMap::new();
        for r in self.rates.iter().filter(|r| AudioFormat::RATES.contains(r)) {
            let n = counts.entry(*r).or_insert(0);
            *n = n.saturating_add(1);
        }
        let best = counts.values().copied().max().unwrap_or(0);
        let tied: Vec<u32> = counts
            .iter()
            .filter(|(_, n)| **n == best)
            .map(|(r, _)| *r)
            .collect();
        let sample_rate = if tied.is_empty() || tied.contains(&DEFAULT_RATE) {
            DEFAULT_RATE
        } else {
            tied.iter().copied().max().unwrap_or(DEFAULT_RATE)
        };
        DpInfo {
            sample_rate,
            tracks: self.ret.tracks,
            audio_files: u32::try_from(self.audio.len()).unwrap_or(u32::MAX),
            external_files: u32::try_from(
                self.audio
                    .iter()
                    .filter(|a| matches!(a.source, AudioSource::External(_)))
                    .count(),
            )
            .unwrap_or(u32::MAX),
            ret: self.ret.clone(),
        }
    }

    /// One per distinct File path, in document order.
    pub fn audio(&self) -> &[AudioRef] {
        &self.audio
    }

    /// The key of the audio file `file` names ([`AudioRef::key`]).
    pub(crate) fn audio_key(&self, file: &DpFileRef) -> Option<usize> {
        self.keys.get(&(file.path.clone(), file.external)).copied()
    }

    /// The bytes an import writes (Ruling 18, L10): every entry's declared
    /// size (its extracted copy, which [`DpPackage::extract`] never passes)
    /// plus one converted copy per file that will be read (entries and
    /// regular external files): its frames × channels × `depth_bytes`, at
    /// least the file's size × 4 when the `Audio` element understates it.
    /// Files that will not be read (missing entries, URLs, what is not a
    /// regular file) count nothing.
    ///
    /// External files are looked at here, not in [`open`]: this runs in the
    /// import the user confirmed, never for a preview. Looks at `cancel`
    /// before each file.
    pub fn needed_bytes(&self, depth_bytes: u32, cancel: &AtomicBool) -> Result<u64, IoError> {
        let mut total = 0u64;
        for a in &self.audio {
            stop(cancel)?;
            let size = match &a.source {
                AudioSource::Entry(name) => self.zip.find(name).map(|e| e.size),
                AudioSource::External(p) => external_size(p),
            };
            let Some(size) = size else { continue };
            if matches!(a.source, AudioSource::Entry(_)) {
                total = total.saturating_add(size);
            }
            let caf = a
                .frames
                .saturating_mul(u64::from(a.channels))
                .saturating_mul(u64::from(depth_bytes))
                .max(size.saturating_mul(4));
            total = total.saturating_add(caf);
        }
        Ok(total)
    }

    /// Streams `a`'s entry into the new file `to` ([`ZipReader::extract`]),
    /// returning the bytes written. An external file is refused: it is read
    /// where it is.
    pub fn extract(&self, a: &AudioRef, to: &Path, cancel: &AtomicBool) -> Result<u64, IoError> {
        match &a.source {
            AudioSource::Entry(name) => {
                let e = self.zip.find(name).ok_or_else(|| missing(name))?;
                self.zip.extract(e, to, cancel)
            }
            AudioSource::External(p) => Err(IoError::Refused(format!(
                "{} is outside the package; it is read where it is",
                core_media::export::display_name(p)
            ))),
        }
    }

    /// The plug-in state entry `path`, at most the limits' `max_state`
    /// bytes and within their ratio; looks at `cancel` as it unpacks.
    pub fn state(&self, path: &str, cancel: &AtomicBool) -> Result<Vec<u8>, IoError> {
        let e = self.zip.find(path).ok_or_else(|| missing(path))?;
        self.zip.read(
            e,
            self.limits.max_state,
            Some(self.limits.max_ratio),
            cancel,
        )
    }

    /// The declared size of entry `name`, if the package holds it.
    pub(crate) fn entry_size(&self, name: &str) -> Option<u64> {
        self.zip.find(name).map(|e| e.size)
    }

    /// The limits the package was opened with.
    pub(crate) fn limits(&self) -> &PackageLimits {
        &self.limits
    }

    /// The project's IDs (checked when the package was opened). Each call
    /// re-runs the whole ID scan, as long as the open did: call it once.
    pub fn ids(&self) -> Ids<'_> {
        check_ids(&self.project, &mut Report::default()).unwrap_or_default()
    }

    /// The package file's name without its extension.
    pub(crate) fn stem(&self) -> &str {
        &self.stem
    }
}

/// The refusal for an entry the package does not hold.
fn missing(name: &str) -> IoError {
    let shown: String = name.chars().take(64).collect();
    IoError::Refused(format!("'{shown}' isn't in the DAWproject file"))
}
