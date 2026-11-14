//! The merge tests' project (plan 6e): one document that exercises every
//! identity the snapshot keeps, and the merge harness (Task 4): an export
//! of it ([`trip`]), the file the other DAW sends back ([`return_file`])
//! and that file as a project of its own ([`theirs_of`]).

use super::package::{TestPlist, media_of, package_bytes_with};
use core_engine::fixtures::{BAR, Builder, Q};
use core_io::dawproject::export::{
    ClipRule, DpExport, ExportEnv, ExportOptions, RegionGain, export,
};
use core_io::dawproject::import::{ImportEnv, map};
use core_io::dawproject::merge::theirs::{Theirs, theirs};
use core_io::dawproject::package::open;
use core_io::dawproject::record::{Snapshot, read_snapshot};
use core_io::dawproject::{
    DpMetaData, DpProject, read_metadata, read_project, write_metadata, write_project,
};
use core_io::limits::PackageLimits;
use core_io::report::Report;
use core_io::xml::Tree;
use core_model::command::{
    AudioCmd, Command, CrossfadeLength, LaneCmd, MediaCmd, MixerCmd, SequenceCmd,
};
use core_model::document::{Document, MediaOrigin};
use core_model::event::{Fade, FadeShape};
use core_model::ids::{AnyId, MediaId, SeqId};
use core_model::mixer::{AuxSend, PluginFormat, PluginRef, StripKind};
use core_model::sequence::{Marker, SeqLength};
use core_model::time::Tick;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tempfile::TempDir;

/// The project ID every harness export carries.
pub const PROJECT: &str = "00000000-0000-4000-8000-000000000001";
/// The export ID every harness export carries.
pub const EXPORT: &str = "00000000-0000-4000-8000-0000000000e1";

/// A CLAP plug-in `id`.
pub fn clap(id: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: id.into(),
        name: id.into(),
        vendor: "Example".into(),
        version: "1.0".into(),
    }
}

/// A media item `Media/<name>.caf` with the content hash `hash-<name>`.
pub fn media(b: &mut Builder, name: &str, frames: u64, channels: u16) -> MediaId {
    let r = b
        .run(Command::Media(MediaCmd::Add {
            path: format!("Media/{name}.caf"),
            sample_rate: 48_000,
            channels,
            length_frames: frames,
            content_hash: Some(format!("hash-{name}")),
            name: Some(name.into()),
            origin: Some(MediaOrigin::Imported {
                original_name: format!("{name}.wav"),
            }),
        }))
        .unwrap();
    r.created
        .iter()
        .find_map(|i| match i {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
        .unwrap()
}

fn no_fade() -> Fade {
    Fade {
        length_frames: 0,
        shape: FadeShape::Linear,
    }
}

/// Sequence "Song" with two tracks:
///
/// - "Bass" on CLAP instrument "Bass Synth" (`org.example.Bass`): a note at
///   tick 0, then sequence "Riff" (two notes, one bar) used at bar 1 once
///   and at bar 3 looped twice. Its strip is at −6 dB, sends to bus
///   "Reverb" at −10 dB and has the insert `org.example.Comp`;
/// - "Keys" on CLAP instrument "Keys Synth" (`org.example.Keys`): a note at
///   tick 0 and, so it is split, two regions of media "loop" (at 0 and one
///   beat, the second crossfaded into the first by 4 800 frames).
///
/// Markers "Verse" at 0 and "Chorus" at bar 2.
pub fn fixture() -> (Document, SeqId) {
    let mut b = Builder::new();
    let (bass_dest, _) = b
        .plugin_dest("Bass Synth", clap("org.example.Bass"))
        .unwrap();
    let (seq, bass) = b.sequence("Song", "Bass", Some(bass_dest)).unwrap();
    let (keys_dest, _) = b
        .plugin_dest("Keys Synth", clap("org.example.Keys"))
        .unwrap();
    let keys = b.lane(seq, "Keys", Some(keys_dest)).unwrap();

    b.notes(seq, bass, &[(0, 36, 0x8000, Q)]).unwrap();
    let (riff, rl) = b.sequence("Riff", "Riff", Some(bass_dest)).unwrap();
    b.notes(riff, rl, &[(0, 40, 0x8000, Q), (Q, 43, 0x9000, Q)])
        .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq: riff,
        length: SeqLength::Locked(Tick(BAR)),
    }))
    .unwrap();
    b.reference(seq, bass, BAR, riff, 1, 0).unwrap();
    b.reference(seq, bass, 3 * BAR, riff, 2, 0).unwrap();

    b.notes(seq, keys, &[(0, 60, 0x8000, 2 * Q)]).unwrap();
    let m = media(&mut b, "loop", 96_000, 2);
    b.region(seq, keys, 0, m, 0, 96_000, 0.0, no_fade(), no_fade())
        .unwrap();
    let second = b
        .region(seq, keys, Q, m, 0, 96_000, 0.0, no_fade(), no_fade())
        .unwrap();
    b.run(Command::Audio(AudioCmd::Crossfade {
        event: second,
        length: CrossfadeLength::Frames(4_800),
        shape: FadeShape::EqualPower,
    }))
    .unwrap();

    let strip = b.strip_of(StripKind::Instrument(bass_dest)).unwrap();
    b.mixer(MixerCmd::SetGain {
        strip,
        gain_db: -6.0,
    })
    .unwrap();
    let reverb = b.bus("Reverb").unwrap();
    b.mixer(MixerCmd::AddSend {
        strip,
        send: AuxSend {
            target: reverb,
            level_db: -10.0,
            pre_fader: false,
            enabled: true,
        },
    })
    .unwrap();
    b.insert(strip, clap("org.example.Comp")).unwrap();

    b.run(Command::Sequence(SequenceCmd::SetMarkers {
        seq,
        markers: vec![
            Marker {
                tick: Tick(0),
                name: "Verse".into(),
            },
            Marker {
                tick: Tick(2 * BAR),
                name: "Chorus".into(),
            },
        ]
        .into_iter()
        .collect(),
    }))
    .unwrap();
    (b.finish(), seq)
}

/// Sequence "Song" (eight bars) with the two shapes in which the export
/// repeats a tag:
///
/// - "Pad A" and "Pad B" both play CLAP instrument "Pad Synth"
///   (`org.example.Pad`), whose strip has the insert `org.example.Chorus`,
///   so its channel is copied into each track; each has a note at tick 0;
/// - "Loop" on CLAP instrument "Arp Synth" (`org.example.Arp`) loops its
///   first two bars, and uses sequence "Arp" (two notes, one bar) at bar 1
///   twice, across the loop's end, so the use is one clip per pass.
pub fn shared_fixture() -> (Document, SeqId) {
    let mut b = Builder::new();
    let (pad, _) = b.plugin_dest("Pad Synth", clap("org.example.Pad")).unwrap();
    let (seq, pad_a) = b.sequence("Song", "Pad A", Some(pad)).unwrap();
    let pad_b = b.lane(seq, "Pad B", Some(pad)).unwrap();
    b.notes(seq, pad_a, &[(0, 60, 0x8000, Q)]).unwrap();
    b.notes(seq, pad_b, &[(0, 64, 0x8000, Q)]).unwrap();
    let strip = b.strip_of(StripKind::Instrument(pad)).unwrap();
    b.insert(strip, clap("org.example.Chorus")).unwrap();

    let (arp, _) = b.plugin_dest("Arp Synth", clap("org.example.Arp")).unwrap();
    let lp = b.lane(seq, "Loop", Some(arp)).unwrap();
    let (riff, rl) = b.sequence("Arp", "Arp", Some(arp)).unwrap();
    b.notes(riff, rl, &[(0, 48, 0x8000, Q), (2 * Q, 55, 0x9000, Q)])
        .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq: riff,
        length: SeqLength::Locked(Tick(BAR)),
    }))
    .unwrap();
    b.reference(seq, lp, BAR, riff, 2, 0).unwrap();
    b.run(Command::Lane(LaneCmd::SetLength {
        seq,
        lane: lp,
        length: Some(Tick(2 * BAR)),
        looped: true,
        locked: false,
    }))
    .unwrap();
    b.run(Command::Sequence(SequenceCmd::SetLength {
        seq,
        length: SeqLength::Locked(Tick(8 * BAR)),
    }))
    .unwrap();
    (b.finish(), seq)
}

// ---------------------------------------------------------------- the harness

/// One export of a project, and what it left in the project.
pub struct Trip {
    /// The exported project (base and, unless a test edits it, ours).
    pub base_doc: Document,
    /// Its exported sequence.
    pub seq: SeqId,
    /// The export's files.
    pub export: DpExport,
    /// The snapshot the export kept.
    pub snapshot: Snapshot,
    /// Where the returned files are written.
    pub dir: TempDir,
}

/// `doc`'s `seq` exported with [`TestPlist`], [`PROJECT`] and [`EXPORT`],
/// regions with gain getting their own copies.
pub fn trip(doc: &Document, seq: SeqId) -> Trip {
    trip_with(doc, seq, RegionGain::Apply)
}

/// [`trip`] with the region-gain option `gain`.
pub fn trip_with(doc: &Document, seq: SeqId, gain: RegionGain) -> Trip {
    let o = ExportOptions {
        seq,
        region_gain: gain,
        clips: ClipRule::SilenceGap(Tick(4 * Q)),
        left_out: Default::default(),
    };
    let env = ExportEnv {
        plist: &TestPlist,
        app_version: "1.2.3",
        project_id: PROJECT,
        export_id: EXPORT,
        exported_at: 1_700_000_000,
    };
    let export = export(doc, &o, &env, &AtomicBool::new(false)).expect("exports");
    let snapshot = read_snapshot(export.snapshot_json.as_bytes()).expect("reads the snapshot");
    Trip {
        base_doc: doc.clone(),
        seq,
        export,
        snapshot,
        dir: tempfile::tempdir().expect("a temp dir"),
    }
}

/// What [`return_file`] takes out of the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Strip {
    /// Nothing: the file as VizDAW wrote it.
    Nothing,
    /// The record (`vizdaw/record.json`).
    Record,
    /// Every `vz:` tag in the project's comments.
    Tags,
    /// The record and the tags.
    Both,
}

/// `xml` without its `vz:` tags (they are only ever in comments).
pub fn without_tags(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(at) = rest.find("vz:") {
        out.push_str(&rest[..at]);
        let after = &rest[at + 3..];
        let mut chars = after.char_indices();
        let tag_len = match chars.next() {
            Some((_, c)) if c.is_ascii_lowercase() => {
                let digits = after[1..].bytes().take_while(u8::is_ascii_digit).count();
                if digits > 0 { Some(1 + digits) } else { None }
            }
            _ => None,
        };
        match tag_len {
            Some(n) => {
                let mut tail = &after[n..];
                // One space after a tag goes with it.
                if let Some(t) = tail.strip_prefix(' ') {
                    tail = t;
                }
                rest = tail;
            }
            None => {
                out.push_str("vz:");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn parse(xml: &str) -> Tree {
    Tree::parse(
        xml.as_bytes(),
        &PackageLimits::default(),
        &AtomicBool::new(false),
    )
    .expect("parses")
}

/// `xml` (a project.xml) read back as a project.
pub fn project_of(xml: &str) -> DpProject {
    read_project(&parse(xml), &mut Report::default()).expect("reads")
}

/// `doc`'s `seq` as the harness exports it, read back: its project and
/// metadata (to move parts of it into a returned file).
pub fn exported(doc: &Document, seq: SeqId) -> (DpProject, DpMetaData) {
    let x = trip(doc, seq).export;
    let metadata = read_metadata(&parse(&x.metadata_xml), &mut Report::default());
    (project_of(&x.project_xml), metadata)
}

static RETURNS: AtomicUsize = AtomicUsize::new(0);

/// The returned file: `t.export`'s project.xml and metadata.xml read back,
/// edited as the other DAW would (`edit`), written again and packaged in
/// `t.dir`; `strip` takes out the record, the tags, or both and the export
/// line ([`stripped_package`]).
pub fn return_file(
    t: &Trip,
    strip: Strip,
    edit: impl FnOnce(&mut DpProject, &mut DpMetaData),
) -> PathBuf {
    let mut report = Report::default();
    let mut project = read_project(&parse(&t.export.project_xml), &mut report).expect("reads");
    let mut metadata = read_metadata(&parse(&t.export.metadata_xml), &mut report);
    edit(&mut project, &mut metadata);
    let mut x = t.export.clone();
    x.project_xml = write_project(&project);
    x.metadata_xml = write_metadata(&metadata);
    let n = RETURNS.fetch_add(1, Ordering::Relaxed);
    let path = t.dir.path().join(format!("Return {n}.dawproject"));
    std::fs::write(&path, stripped_package(&x, strip)).expect("writes");
    path
}

/// `meta` without the export line its comment ends with.
pub fn without_export_line(meta: &mut DpMetaData) {
    use core_io::dawproject::units::EXPORT_LINE;
    if let Some(c) = meta.fields.last_mut() {
        if let Some(text) = c.take() {
            let kept: Vec<&str> = text
                .split('\n')
                .filter(|l| !l.starts_with(EXPORT_LINE))
                .collect();
            let kept = kept.join("\n");
            *c = (!kept.is_empty()).then_some(kept);
        }
    }
}

/// `x` packaged as `strip` says: without the tags ([`Strip::Tags`]), the
/// record ([`Strip::Record`]), or both and the metadata's export line
/// ([`Strip::Both`]: the file lost its export ID).
pub fn stripped_package(x: &DpExport, strip: Strip) -> Vec<u8> {
    let mut x = x.clone();
    if matches!(strip, Strip::Tags | Strip::Both) {
        x.project_xml = without_tags(&x.project_xml);
    }
    if strip == Strip::Both {
        let mut meta = read_metadata(&parse(&x.metadata_xml), &mut Report::default());
        without_export_line(&mut meta);
        x.metadata_xml = write_metadata(&meta);
    }
    let record = matches!(strip, Strip::Nothing | Strip::Tags);
    package_bytes_with(&x, record).expect("packages")
}

/// The file at `path` as a project of its own: opened, mapped (media from
/// [`media_of`], at the project's rate as a conversion would make them;
/// the snapshot's IDs trusted when the file's record names its export and
/// its project),
/// then [`theirs`].
pub fn theirs_of(t: &Trip, path: &Path) -> Theirs {
    let pkg = open(path, &PackageLimits::default(), &AtomicBool::new(false)).expect("opens");
    let media = media_of(&pkg, &t.export, &t.base_doc);
    let trusted = pkg.ret.record.as_ref().is_some_and(|r| {
        r.export_id == t.snapshot.export_id && r.project_id == t.snapshot.project_id
    });
    let installed = |_: PluginFormat, _: &str| true;
    let format = t.base_doc.settings().format;
    let imp = map(
        &pkg,
        &media,
        &ImportEnv {
            installed: &installed,
            plist: &TestPlist,
            rate: format.sample_rate,
            ids: trusted.then_some(&t.snapshot.ids),
            eq_plugin: None,
        },
        &AtomicBool::new(false),
    )
    .expect("maps");
    theirs(imp, format, t.snapshot.origin, &AtomicBool::new(false)).expect("theirs")
}

/// The hostile file of plan 6e Task 2: hostile tags and IDs, and `clips`
/// one-note clips on track A all tagged `vz:e<event>` (with no pass tags,
/// so they are not one use's passes). Tracks A and B both claim
/// `vz:t<lane>`, C claims `vz:t999`, D a strip tag, and E has the ID
/// `t<lane>`.
pub fn hostile(clips: usize, lane: u64, event: u64) -> String {
    let structure = format!(
        concat!(
            r#"<Track id="ta" name="A" comment="vz:t{l}" contentType="notes"/>"#,
            r#"<Track id="tb" name="B" comment="vz:t{l}" contentType="notes"/>"#,
            r#"<Track id="tc" name="C" comment="vz:t999" contentType="notes"/>"#,
            r#"<Track id="td" name="D" comment="vz:s3" contentType="notes"/>"#,
            r#"<Track id="t{l}" name="E" contentType="notes"/>"#,
        ),
        l = lane
    );
    let mut s = String::with_capacity(clips * 160);
    for i in 0..clips {
        s.push_str(&format!(
            r#"<Clip time="{}" duration="1" comment="vz:e{event}"><Notes><Note time="0" duration="0.5" channel="0" key="60" vel="0.5"/></Notes></Clip>"#,
            i as f64 / 2.0
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Project version="1.0">
  <Application name="Fixture" version="1"/>
  <Structure>
    <Track id="m" name="Master" contentType="audio notes"><Channel id="mc" role="master"/></Track>
    {structure}
  </Structure>
  <Arrangement><Lanes timeUnit="beats"><Clips track="ta">{s}</Clips></Lanes></Arrangement>
</Project>"#
    )
}

/// What a merge always keeps from ours (spec §7.8's keep list), as JSON,
/// for a project merged into sequence `seq`: every other sequence (with
/// its keyboard slot), `seq`'s keyboard slot, and per track of `seq` that
/// has any: its inactive takes, input effects, input (with `return_of`)
/// and monitoring, MPE input settings and drum rows; each strip's
/// automation mode other than the default; the MIDI-learn maps; the audio
/// format; the templates. Control surfaces live in the app's preferences,
/// not the project, so there is nothing of theirs here to compare.
pub fn vizdaw_only(doc: &Document, seq: SeqId) -> String {
    vizdaw_only_except(doc, seq, &Except::default())
}

/// What [`vizdaw_only_except`] leaves out: what a merge changes by design,
/// each checked on its own by the caller.
#[derive(Debug, Clone, Default)]
pub struct Except {
    /// Sequences the exported one plays through its uses that the merge
    /// edited, or made as private copies (Ruling 8).
    pub seqs: std::collections::BTreeSet<SeqId>,
    /// The conflict takes the merge added (Ruling 6).
    pub takes: std::collections::BTreeSet<core_model::ids::TakeId>,
}

/// Every sequence `seq` plays through its tracks' active takes, nested
/// uses included.
pub fn played_sequences(doc: &Document, seq: SeqId) -> std::collections::BTreeSet<SeqId> {
    use core_model::event::EventBody;
    let mut out = std::collections::BTreeSet::new();
    let mut todo = vec![seq];
    while let Some(s) = todo.pop() {
        let Ok(sq) = doc.sequence(s) else { continue };
        for l in &sq.lanes {
            for e in l.active().into_iter().flat_map(|t| t.events.values()) {
                if let EventBody::Reference { seq: used, .. } = e.body {
                    if out.insert(used) {
                        todo.push(used);
                    }
                }
            }
        }
    }
    out
}

/// [`vizdaw_only`], without what `except` names.
pub fn vizdaw_only_except(doc: &Document, seq: SeqId, except: &Except) -> String {
    use core_model::automation::AutomationMode;
    use serde_json::{Value, json};
    let mut others = Vec::new();
    let mut lanes = Vec::new();
    let mut slot = Value::Null;
    for s in doc.sequences().values() {
        if s.id != seq {
            if !except.seqs.contains(&s.id) {
                others.push(serde_json::to_value(s).expect("serialises"));
            }
            continue;
        }
        slot = serde_json::to_value(&s.keyboard_slot).expect("serialises");
        for l in &s.lanes {
            let inactive: Vec<_> = l
                .takes
                .iter()
                .filter(|t| t.id != l.active_take)
                .filter(|t| !except.takes.contains(&t.id))
                .collect();
            let has = !inactive.is_empty()
                || !l.input_fx.is_empty()
                || l.input.is_some()
                || !l.monitor.is_auto()
                || l.mpe.is_some()
                || l.drum.is_some();
            if has {
                lanes.push(json!({
                    "lane": l.id.0,
                    "inactive_takes": serde_json::to_value(&inactive).expect("serialises"),
                    "input_fx": serde_json::to_value(&l.input_fx).expect("serialises"),
                    "input": serde_json::to_value(&l.input).expect("serialises"),
                    "monitor": serde_json::to_value(l.monitor).expect("serialises"),
                    "mpe": serde_json::to_value(l.mpe).expect("serialises"),
                    "drum": serde_json::to_value(&l.drum).expect("serialises"),
                }));
            }
        }
    }
    let modes: Vec<Value> = doc
        .mixer()
        .strips()
        .values()
        .filter(|s| s.automation != AutomationMode::default())
        .map(|s| {
            json!([
                s.id.0,
                serde_json::to_value(s.automation).expect("serialises")
            ])
        })
        .collect();
    let settings = doc.settings();
    serde_json::to_string_pretty(&json!({
        "other_sequences": others,
        "keyboard_slot": slot,
        "tracks": lanes,
        "automation_modes": modes,
        "learn": serde_json::to_value(&settings.learn).expect("serialises"),
        "format": serde_json::to_value(settings.format).expect("serialises"),
        "templates": serde_json::to_value(&settings.templates).expect("serialises"),
    }))
    .expect("serialises")
}
