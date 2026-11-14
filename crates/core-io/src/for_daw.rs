//! Export for another DAW (plan 6b Task 1): the folder's layout and file
//! names, its MIDI files, the report of what did not transfer, and the
//! About file.
//!
//! The folder holds `MIDI/NN <track>.mid` for every MIDI track, `MIDI/All
//! MIDI.mid`, `Tempo map.mid`, `Audio/NN <name>.<ext>` for every chosen
//! strip and `About these files.txt`. Every file starts at
//! [`midi_out::file_start`].
//!
//! What a track plays is read with its mute and solo cleared (the files
//! are taken before mute and solo, Ruling 3); muted events and inactive
//! takes do not play.

use crate::IoError;
use crate::limits::Limits;
use crate::midi_out::{self, OTHER_INFO, SmfOptions, to_smf_until, to_tempo_map_smf};
use crate::names::safe_part;
use crate::report::{ItemKind, Report, group};
use crate::smf;
use crate::text::sanitize;
use core_model::command::{Command, LaneCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::event::EventBody;
use core_model::ids::{DestId, LaneId, SeqId, StripId};
use core_model::mixer::{Output, Strip, StripKind};
use core_model::query::flatten;
use core_model::selection::ViewFilter;
use core_model::sequence::Sequence;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, Ordering};

const NO_SEQ: &str = "The sequence to export is not in this project.";
const FOLDER_SUFFIX: &str = " for another DAW";
const ALL_MIDI: &str = "MIDI/All MIDI.mid";
const TEMPO_MAP: &str = "Tempo map.mid";
const ABOUT: &str = "About these files.txt";
/// The MIDI files' resolution (Ruling 6).
const PPQ: u16 = 960;

const EXTERNAL: &str = "No audio for tracks playing external MIDI (their MIDI files are written)";
const NOTHING: &str = "Tracks with nothing to play: no files";
const NOT_CHOSEN: &str = "Strips not chosen";
const COLOURS: &str = "Strip colours";
const UNROUTED: &str = "Audio regions with no output";
const UNREACHED: &str = "Strips no track of this sequence plays through";
const BEFORE_BUSES: &str = "Stems are taken before the buses they play through";
const MIX: &str = "Mix, effects and automation are part of the audio files";
const SONG_INFO: &str = "Song info is in About these files.txt";

/// The song-info labels, in [`core_model::document::SongInfo::fields`]
/// order.
const INFO_LABELS: [&str; 13] = [
    "Title",
    "Artist",
    "Album",
    "Original artist",
    "Composer",
    "Songwriter",
    "Producer",
    "Arranger",
    "Year",
    "Genre",
    "Copyright",
    "Website",
    "Comment",
];

/// The files of one export, by path inside the export folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The export folder's name, unnumbered: "Song for another DAW".
    pub folder: String,
    /// The stems, "Audio/01 Bass.wav": the chosen strips in mixer order.
    pub audio: Vec<(StripId, String)>,
    /// The MIDI files, "MIDI/01 Bass.mid": the MIDI tracks in track order.
    pub midi: Vec<(LaneId, String)>,
    /// "MIDI/All MIDI.mid" when there is a MIDI track.
    pub all_midi: Option<String>,
    /// "Tempo map.mid".
    pub tempo_map: String,
    /// "About these files.txt".
    pub about: String,
}

impl Layout {
    /// Every file, in the order they are written: the MIDI files, All
    /// MIDI, the tempo map, the stems, then the About file.
    pub fn files(&self) -> Vec<String> {
        self.midi
            .iter()
            .map(|(_, p)| p.clone())
            .chain(self.all_midi.clone())
            .chain([self.tempo_map.clone()])
            .chain(self.audio.iter().map(|(_, p)| p.clone()))
            .chain([self.about.clone()])
            .collect()
    }
}

/// Files as `(path inside the export folder, bytes)`.
pub type Files = Vec<(String, Vec<u8>)>;

/// The stems' format and length, for the About file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AboutAudio {
    /// "WAV", "AIFF" or "CAF".
    pub container: &'static str,
    /// The sample format: "24-bit".
    pub bits: String,
    /// The sample rate, in Hz.
    pub rate: u32,
    /// Every stem's length, in frames at `rate`.
    pub frames: u64,
}

// ---- what plays where ----

/// What a sequence's tracks play, and through which strips.
#[derive(Debug, Default)]
struct Plays {
    /// The strips each track plays straight into (instrument and audio
    /// path strips), by strip.
    feeds: BTreeMap<StripId, BTreeSet<LaneId>>,
    /// Tracks that play a MIDI event.
    midi: BTreeSet<LaneId>,
    /// Tracks that play a MIDI event to an external destination.
    external: BTreeSet<LaneId>,
    /// Tracks that play an audio region.
    audio: BTreeSet<LaneId>,
    /// Tracks that play an audio region with no output.
    unrouted: BTreeSet<LaneId>,
}

/// The strips destination `d`'s audio comes out of: a plug-in's main and
/// extra-output strips, or the built-in synth's strip; none for an
/// external destination.
fn dest_strips(doc: &Document, d: DestId) -> Vec<StripId> {
    let m = doc.mixer();
    match doc.instruments().get(&d).map(|x| &x.kind) {
        Some(DestKind::PluginInstrument { .. }) => m
            .strips()
            .values()
            .filter(|s| {
                matches!(s.kind, StripKind::Instrument(x) | StripKind::InstrumentOutput(x, _) if x == d)
            })
            .map(|s| s.id)
            .collect(),
        Some(k) if k.is_builtin_synth() => {
            m.find_kind(StripKind::BuiltinSynth).into_iter().collect()
        }
        _ => Vec::new(),
    }
}

/// What `s`'s tracks play with their mute and solo cleared.
fn plays(doc: &Document, s: &Sequence) -> Result<Plays, IoError> {
    let mut copy: Option<Document> = None;
    for l in s.lanes.iter().filter(|l| l.mute || l.solo) {
        let cmd = Command::Lane(LaneCmd::SetFlags {
            seq: s.id,
            lane: l.id,
            mute: Some(false),
            solo: Some(false),
            arm: None,
        });
        let next = apply(copy.as_ref().unwrap_or(doc), &cmd)
            .map_err(|e| IoError::Refused(format!("This export can't be made: {e}")))?
            .doc;
        copy = Some(next);
    }
    let events = flatten(
        copy.as_ref().unwrap_or(doc),
        s.id,
        None,
        &ViewFilter::default(),
    )
    .map_err(|_| IoError::Refused(NO_SEQ.to_string()))?;
    let mut p = Plays::default();
    let mut strips_of: BTreeMap<DestId, Vec<StripId>> = BTreeMap::new();
    for r in events {
        let Some(lane) = r.path.first().map(|step| step.lane) else {
            continue;
        };
        if let EventBody::AudioRegion { .. } = r.body {
            p.audio.insert(lane);
            match r
                .audio_out
                .and_then(|o| doc.mixer().find_kind(StripKind::AudioPath(o)))
            {
                Some(strip) => {
                    p.feeds.entry(strip).or_default().insert(lane);
                }
                None if r.audio_out.is_none() => {
                    p.unrouted.insert(lane);
                }
                None => {}
            }
        } else if r.body.is_midi() {
            p.midi.insert(lane);
            let Some(d) = r.dest else { continue };
            if doc
                .instruments()
                .get(&d)
                .is_some_and(|x| x.kind.is_external())
            {
                p.external.insert(lane);
            }
            let strips = strips_of.entry(d).or_insert_with(|| dest_strips(doc, d));
            for strip in strips.iter() {
                p.feeds.entry(*strip).or_default().insert(lane);
            }
        }
    }
    Ok(p)
}

impl Plays {
    /// Every strip a track plays through: the strips it plays into, and
    /// the buses they output or send to, onward. Not the master.
    fn reached(&self, doc: &Document) -> BTreeSet<StripId> {
        let m = doc.mixer();
        let mut seen: BTreeSet<StripId> = self.feeds.keys().copied().collect();
        let mut todo: Vec<StripId> = seen.iter().copied().collect();
        while let Some(id) = todo.pop() {
            let Ok(s) = m.strip(id) else { continue };
            let out = match s.output {
                Output::Bus(b) => Some(b),
                Output::Master => None,
            };
            for next in out.into_iter().chain(s.sends.iter().map(|x| x.target)) {
                if next != m.master() && seen.insert(next) {
                    todo.push(next);
                }
            }
        }
        seen
    }

    /// Ruling 4's strips, in mixer order: those the tracks play into,
    /// plus every bus a strip they play through sends to and no such strip
    /// outputs to (a bus the tracks' strips output to is heard in the
    /// stems' mix and is not a stem of its own).
    fn defaults(&self, doc: &Document) -> Vec<StripId> {
        let m = doc.mixer();
        let reached = self.reached(doc);
        let outputs_to: BTreeSet<StripId> = m
            .strips()
            .values()
            .filter(|s| reached.contains(&s.id))
            .filter_map(|s| match s.output {
                Output::Bus(b) => Some(b),
                Output::Master => None,
            })
            .collect();
        let sent_to: BTreeSet<StripId> = m
            .strips()
            .values()
            .filter(|s| reached.contains(&s.id))
            .flat_map(|s| s.sends.iter().map(|x| x.target))
            .collect();
        m.order()
            .iter()
            .copied()
            .filter(|id| {
                self.feeds.contains_key(id)
                    || (m.strip(*id).is_ok_and(|s| s.kind == StripKind::Bus)
                        && sent_to.contains(id)
                        && !outputs_to.contains(id))
            })
            .collect()
    }
}

fn sequence(doc: &Document, seq: SeqId) -> Result<&Sequence, IoError> {
    doc.sequence(seq)
        .map_err(|_| IoError::Refused(NO_SEQ.to_string()))
}

/// The strips exported when the user chooses none (Ruling 4), in mixer
/// order: the strips this sequence's tracks play into (the instrument
/// strips of their destinations, the audio path strips of their regions),
/// plus every bus some strip they play through sends to and no strip
/// outputs to. Refuses an unknown sequence.
pub fn default_strips(doc: &Document, seq: SeqId) -> Result<Vec<StripId>, IoError> {
    let s = sequence(doc, seq)?;
    Ok(plays(doc, s)?.defaults(doc))
}

/// The MIDI tracks (Ruling 6), in track order: tracks whose active take
/// plays a MIDI event, their mute and solo cleared. Refuses an unknown
/// sequence.
pub fn midi_tracks(doc: &Document, seq: SeqId) -> Result<Vec<LaneId>, IoError> {
    let s = sequence(doc, seq)?;
    let p = plays(doc, s)?;
    Ok(s.lanes
        .iter()
        .map(|l| l.id)
        .filter(|l| p.midi.contains(l))
        .collect())
}

/// `"<n> "`, `n` padded with zeros to the digits of `total` (at least
/// two), so the files sort in order by name in any program: "001" … "120".
fn number(n: usize, total: usize) -> String {
    let width = total.to_string().len().max(2);
    format!("{n:0width$} ")
}

/// The bytes a file part leaves for the rest of its name: the number, the
/// extension and its dot, and the partial's dot and `.partial`.
fn file_reserve(num: &str, ext: &str) -> usize {
    num.len()
        .saturating_add(ext.len())
        .saturating_add(".partial".len())
        .saturating_add(2)
}

/// The bytes the folder's name leaves: " for another DAW", a number up to
/// " 9999", and the staging folder's dot and `.partial`.
const FOLDER_RESERVE: usize = FOLDER_SUFFIX.len() + " 9999".len() + ".partial".len() + 1;

/// The export's files (Ruling 5): `strips` as `Audio/NN <name>.<ext>` in
/// mixer order, each named after the one track that plays into it, else
/// after the strip; the MIDI tracks as `MIDI/NN <track>.mid`; the folder
/// named after the song title, else the sequence, else "Untitled". Every
/// part passes [`safe_part`]. Refuses an unknown sequence or strip, a
/// strip listed twice, and the master.
pub fn layout(
    doc: &Document,
    seq: SeqId,
    strips: &[StripId],
    ext: &str,
) -> Result<Layout, IoError> {
    let s = sequence(doc, seq)?;
    let m = doc.mixer();
    let mut chosen: BTreeSet<StripId> = BTreeSet::new();
    for id in strips {
        let strip = m
            .strip(*id)
            .map_err(|_| IoError::Refused(format!("Strip {id} is not in this project.")))?;
        if *id == m.master() {
            return Err(IoError::Refused(format!(
                "'{}' is the master: the master is the mix; choose track and bus strips.",
                strip.name
            )));
        }
        if !chosen.insert(*id) {
            return Err(IoError::Refused(format!(
                "Strip '{}' is chosen twice.",
                strip.name
            )));
        }
    }
    let p = plays(doc, s)?;
    let lane_name = |l: LaneId| s.lane(l).map(|x| x.name.as_str());
    let mut audio = Vec::new();
    for id in m.order().iter().filter(|id| chosen.contains(id)) {
        let Ok(strip) = m.strip(*id) else { continue };
        let fed = p.feeds.get(id);
        let name = match fed.filter(|f| f.len() == 1).and_then(|f| f.first()) {
            Some(l) => lane_name(*l).unwrap_or(strip.name.as_str()),
            None => strip.name.as_str(),
        };
        let num = number(audio.len().saturating_add(1), chosen.len());
        let part = safe_part(name, file_reserve(&num, ext));
        audio.push((*id, format!("Audio/{num}{part}.{ext}")));
    }
    let mut midi = Vec::new();
    let tracks = s.lanes.iter().filter(|l| p.midi.contains(&l.id)).count();
    for l in s.lanes.iter().filter(|l| p.midi.contains(&l.id)) {
        let num = number(midi.len().saturating_add(1), tracks);
        let part = safe_part(&l.name, file_reserve(&num, "mid"));
        midi.push((l.id, format!("MIDI/{num}{part}.mid")));
    }
    let title = doc
        .settings()
        .info
        .title
        .as_deref()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or(s.name.as_str());
    Ok(Layout {
        folder: format!("{}{FOLDER_SUFFIX}", safe_part(title, FOLDER_RESERVE)),
        audio,
        all_midi: (!midi.is_empty()).then(|| ALL_MIDI.to_string()),
        midi,
        tempo_map: TEMPO_MAP.to_string(),
        about: ABOUT.to_string(),
    })
}

/// Drops the item the About file makes untrue: it carries the song info.
fn drop_other_info(r: &mut Report) {
    r.items
        .retain(|i| !(i.kind == ItemKind::LeftOut && i.what == OTHER_INFO));
}

/// The MIDI files of `l` (Ruling 6), as `(path, bytes)` in write order:
/// each MIDI track's file, All MIDI and the tempo map, all type 1 at 960
/// ticks per quarter with MPE. The report is All MIDI's (the tempo map's
/// without MIDI tracks), without "Song info other than title and
/// copyright". Stops with `Err(Cancelled)` once `cancel` is set (looked
/// at between files and inside each).
pub fn midi_files(
    doc: &Document,
    seq: SeqId,
    l: &Layout,
    cancel: &AtomicBool,
) -> Result<(Files, Report), IoError> {
    let opts = |lanes: Vec<LaneId>| SmfOptions {
        seq,
        lanes,
        type0: false,
        ppq: PPQ,
        mpe: true,
    };
    let stop = || {
        if cancel.load(Ordering::Relaxed) {
            Err(IoError::Cancelled)
        } else {
            Ok(())
        }
    };
    let mut out = Vec::with_capacity(l.midi.len().saturating_add(2));
    for (lane, path) in &l.midi {
        stop()?;
        let (f, _) = to_smf_until(doc, &opts(vec![*lane]), cancel)?;
        out.push((path.clone(), smf::write(&f)?));
    }
    let mut report = None;
    if let Some(all) = l.all_midi.as_ref().filter(|_| !l.midi.is_empty()) {
        stop()?;
        let lanes = l.midi.iter().map(|(x, _)| *x).collect();
        let (f, r) = to_smf_until(doc, &opts(lanes), cancel)?;
        out.push((all.clone(), smf::write(&f)?));
        report = Some(r);
    }
    stop()?;
    let (f, r) = to_tempo_map_smf(doc, seq, PPQ, cancel)?;
    out.push((l.tempo_map.clone(), smf::write(&f)?));
    let mut report = report.unwrap_or(r);
    drop_other_info(&mut report);
    Ok((out, report))
}

/// `midi` (the MIDI files' report) with what the route itself does not
/// carry (spec §7.6), each item once per name with the names as details,
/// and `summary.audio_files` set to the stems.
pub fn route_report(doc: &Document, seq: SeqId, strips: &[StripId], midi: Report) -> Report {
    let mut r = midi;
    drop_other_info(&mut r);
    r.summary.audio_files = u64::try_from(strips.len()).unwrap_or(u64::MAX);
    let Ok(s) = doc.sequence(seq) else { return r };
    let p = plays(doc, s).unwrap_or_default();
    let m = doc.mixer();
    let named = |r: &mut Report, kind: ItemKind, what: &str, names: Vec<String>| {
        for n in names {
            r.add(kind, what, Some(n));
        }
    };
    let tracks = |keep: &dyn Fn(LaneId) -> bool| -> Vec<String> {
        s.lanes
            .iter()
            .filter(|l| keep(l.id))
            .map(|l| l.name.clone())
            .collect()
    };
    let chosen: Vec<&Strip> = m
        .order()
        .iter()
        .filter(|id| strips.contains(id))
        .filter_map(|id| m.strip(*id).ok())
        .collect();
    let strip_names = |keep: &dyn Fn(&Strip) -> bool| -> Vec<String> {
        chosen
            .iter()
            .filter(|x| keep(x))
            .map(|x| x.name.clone())
            .collect()
    };

    named(
        &mut r,
        ItemKind::LeftOut,
        EXTERNAL,
        tracks(&|l| p.external.contains(&l)),
    );
    named(
        &mut r,
        ItemKind::Note,
        NOTHING,
        tracks(&|l| !p.midi.contains(&l) && !p.audio.contains(&l)),
    );
    let unchosen: Vec<String> = p
        .defaults(doc)
        .into_iter()
        .filter(|id| !strips.contains(id))
        .filter_map(|id| m.strip(id).ok().map(|x| x.name.clone()))
        .collect();
    named(&mut r, ItemKind::LeftOut, NOT_CHOSEN, unchosen);
    let any_solo = m.strips().values().any(|x| x.solo && x.id != m.master());
    for x in &chosen {
        let what = if x.mute {
            format!(
                "Strip '{}' is muted in the project; its stem is written",
                x.name
            )
        } else if any_solo && !x.solo {
            format!(
                "Strip '{}' is silenced by a solo in the project; its stem is written",
                x.name
            )
        } else {
            continue;
        };
        r.add(ItemKind::Note, &what, None);
    }
    named(
        &mut r,
        ItemKind::LeftOut,
        COLOURS,
        strip_names(&|x| x.colour.is_some()),
    );
    named(
        &mut r,
        ItemKind::LeftOut,
        UNROUTED,
        tracks(&|l| p.unrouted.contains(&l)),
    );
    let reached = p.reached(doc);
    named(
        &mut r,
        ItemKind::LeftOut,
        UNREACHED,
        strip_names(&|x| !reached.contains(&x.id)),
    );
    for l in s.lanes.iter().filter(|l| l.mute) {
        let what = format!("Track '{}' is muted in the project", l.name);
        if !r.items.iter().any(|i| i.what == what) {
            r.add(ItemKind::Note, &what, None);
        }
    }
    named(
        &mut r,
        ItemKind::Note,
        BEFORE_BUSES,
        strip_names(&|x| matches!(x.output, Output::Bus(_))),
    );
    if !strips.is_empty() {
        r.add(ItemKind::Note, MIX, None);
    }
    if doc.settings().info.fields().iter().any(|f| f.is_some()) {
        r.add(ItemKind::Note, SONG_INFO, None);
    }
    r
}

/// "1 beat" / "3 beats".
fn count(n: i64, one: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {one}s")
    }
}

/// Where every file starts, as a sentence (Ruling 2).
pub fn start_sentence(s: &Sequence) -> String {
    match midi_out::pickup_shape(s) {
        (Some(beats), _) => format!(
            "Every file starts {} before bar 1, where the song's pickup begins.",
            count(i64::from(beats), "beat")
        ),
        (None, bars) if bars > 0 => format!(
            "Every file starts {} before bar 1, so the song's pickup fits.",
            count(bars, "bar")
        ),
        _ => "Every file starts at bar 1.".to_string(),
    }
}

/// `bpm` with at most two decimals, trailing zeros dropped.
fn bpm_text(bpm: f64) -> String {
    let t = format!("{bpm:.2}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `frames` at `rate` as `m:ss.s`.
fn duration(frames: u64, rate: u32) -> String {
    let rate = u128::from(rate);
    let tenths = u128::from(frames)
        .saturating_mul(10)
        .saturating_add(rate / 2)
        .checked_div(rate)
        .unwrap_or(0);
    format!("{}:{:02}.{}", tenths / 600, tenths % 600 / 10, tenths % 10)
}

/// `s` without carriage returns, other control characters (newline
/// kept) or bidirectional overrides, as [`sanitize`] cleans file text.
fn clean(s: &str) -> String {
    sanitize(
        s.as_bytes(),
        &Limits {
            max_text_bytes: usize::MAX,
            ..Limits::default()
        },
    )
    .0
}

/// `report` with every item's text and details cleaned to one line, so a
/// name holding a newline cannot break the file's layout.
fn one_line_report(report: &Report) -> Report {
    let line = |s: &str| clean(s).replace('\n', " ");
    let mut r = report.clone();
    for i in &mut r.items {
        i.what = line(&i.what);
        for d in &mut i.details {
            *d = line(d);
        }
    }
    r
}

/// The About file (`About these files.txt`): the folder, where the files
/// start, the tempo and meter there, the stems' format and length, the
/// song info, the files, and `report` as text. UTF-8 with `\n` line ends,
/// ending in one newline.
pub fn about_text(
    doc: &Document,
    seq: SeqId,
    l: &Layout,
    audio: Option<&AboutAudio>,
    report: &Report,
) -> String {
    let mut out: Vec<String> = vec![l.folder.clone(), String::new()];
    if let Ok(s) = doc.sequence(seq) {
        out.push(format!("Start: {}", start_sentence(s)));
        let bpm = bpm_text(midi_out::start_bpm(s));
        let meter = match midi_out::start_meters(s) {
            (p, Some(m)) => format!(
                "{}/{} for the pickup, then {}/{} from bar 1",
                p.num, p.den, m.num, m.den
            ),
            (m, None) => format!("{}/{} at the start", m.num, m.den),
        };
        out.push(format!(
            "Tempo: {bpm} BPM, {meter}. {} holds every tempo and meter change and the markers.",
            l.tempo_map
        ));
    }
    out.push(match audio {
        Some(a) => format!(
            "Audio: {}, {}, {} Hz. Every audio file is {} frames ({}) long: to the song's end plus the tail.",
            a.container,
            a.bits,
            group(u64::from(a.rate)),
            group(a.frames),
            duration(a.frames, a.rate)
        ),
        None => "Audio: none; no strips were chosen.".to_string(),
    });
    let info = &doc.settings().info;
    if info.fields().iter().any(|f| f.is_some()) {
        out.push(String::new());
        out.push("Song info".to_string());
        for (label, value) in INFO_LABELS.iter().zip(info.fields()) {
            if let Some(v) = value {
                let v = clean(v);
                out.push(format!("  {label}: {}", v.replace('\n', "\n    ")));
            }
        }
    }
    out.push(String::new());
    out.push("Files".to_string());
    for f in l.files().into_iter().filter(|f| *f != l.about) {
        let note = if l.all_midi.as_ref() == Some(&f) {
            " (every MIDI track)"
        } else if f == l.tempo_map {
            " (tempo, meter and markers)"
        } else {
            ""
        };
        out.push(format!("  {f}{note}"));
    }
    out.push(String::new());
    out.push("What changed or was left out".to_string());
    out.push(one_line_report(report).plain("Exported"));
    let mut text = out.join("\n");
    text.push('\n');
    text
}
