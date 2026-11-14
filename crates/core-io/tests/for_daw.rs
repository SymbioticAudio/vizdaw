//! Export for another DAW: the folder's layout, names, MIDI files, report
//! and About text (plan 6b Task 1).

use core_engine::input_fx::up7;
use core_io::IoError;
use core_io::for_daw::{
    AboutAudio, Layout, about_text, default_strips, layout, midi_files, midi_tracks, route_report,
    start_sentence,
};
use core_io::midi_out::{file_start, to_tempo_map_smf};
use core_io::names::safe_part;
use core_io::report::{ItemKind, Report};
use core_io::smf::SmfEvent;
use core_model::command::{
    AudioCmd, Command, EventCmd, LaneCmd, MediaCmd, MixerCmd, RoutingCmd, SequenceCmd, SettingsCmd,
    apply as run,
};
use core_model::document::{DestKind, Document};
use core_model::event::{EventBody, EventSpec, Fade, FadeShape};
use core_model::ids::{AnyId, DestId, LaneId, MediaId, OutputPathId, SeqId, StripId};
use core_model::mixer::{AuxSend, BUILTIN_SYNTH_REF, Output, StripKind, Swatch};
use core_model::time::Tick;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

// ---- building documents ----

/// A document with one sequence, built by commands.
struct Doc {
    doc: Document,
    seq: SeqId,
}

fn created<T>(ids: &[AnyId], f: impl Fn(&AnyId) -> Option<T>) -> T {
    ids.iter().find_map(f).unwrap()
}

impl Doc {
    /// A document whose one sequence `name` has no tracks.
    fn new(name: &str) -> Doc {
        let a = run(
            &Document::new(),
            &Command::Sequence(SequenceCmd::New { name: name.into() }),
        )
        .unwrap();
        let seq = created(&a.report.created, |id| match id {
            AnyId::Seq(s) => Some(*s),
            _ => None,
        });
        let mut d = Doc { doc: a.doc, seq };
        let lane = d.doc.sequence(seq).unwrap().lanes.front().unwrap().id;
        d.exec(Command::Lane(LaneCmd::Remove { seq, lane }));
        d
    }

    fn exec(&mut self, cmd: Command) -> Vec<AnyId> {
        let a = run(&self.doc, &cmd).unwrap();
        self.doc = a.doc;
        a.report.created
    }

    fn add_dest(&mut self, name: &str, kind: DestKind) -> DestId {
        let ids = self.exec(Command::Routing(RoutingCmd::AddDestination {
            name: name.into(),
            kind,
            default_patch: None,
        }));
        created(&ids, |id| match id {
            AnyId::Dest(d) => Some(*d),
            _ => None,
        })
    }

    fn builtin(&mut self, channel: u8) -> DestId {
        let name = format!("Built-in Synth (ch {})", channel + 1);
        self.add_dest(
            &name,
            DestKind::MidiEndpoint {
                endpoint_ref: BUILTIN_SYNTH_REF.into(),
                group: 0,
                channel,
            },
        )
    }

    fn external(&mut self, name: &str) -> DestId {
        self.add_dest(
            name,
            DestKind::MidiEndpoint {
                endpoint_ref: "ext:port".into(),
                group: 0,
                channel: 0,
            },
        )
    }

    fn plugin(&mut self, name: &str) -> DestId {
        self.add_dest(name, DestKind::plugin(name))
    }

    fn output(&mut self, name: &str) -> OutputPathId {
        let ids = self.exec(Command::Routing(RoutingCmd::AddOutput {
            name: name.into(),
            reference: format!("bus:{name}"),
        }));
        created(&ids, |id| match id {
            AnyId::Output(o) => Some(*o),
            _ => None,
        })
    }

    fn bus(&mut self, name: &str) -> StripId {
        let ids = self.exec(Command::Mixer(MixerCmd::AddBus {
            name: name.into(),
            index: None,
        }));
        created(&ids, |id| match id {
            AnyId::Strip(s) => Some(*s),
            _ => None,
        })
    }

    fn strip(&self, kind: StripKind) -> StripId {
        self.doc.mixer().find_kind(kind).unwrap()
    }

    fn strip_name(&self, s: StripId) -> String {
        self.doc.mixer().strip(s).unwrap().name.clone()
    }

    fn track(&mut self, name: &str, dest: Option<DestId>) -> LaneId {
        let seq = self.seq;
        let ids = self.exec(Command::Lane(LaneCmd::Add {
            seq,
            name: name.into(),
            index: None,
        }));
        let lane = created(&ids, |id| match id {
            AnyId::Lane(l) => Some(*l),
            _ => None,
        });
        if dest.is_some() {
            self.exec(Command::Lane(LaneCmd::SetDest { seq, lane, dest }));
        }
        lane
    }

    /// A track on `dest` holding one note.
    fn midi_track(&mut self, name: &str, dest: DestId) -> LaneId {
        let lane = self.track(name, Some(dest));
        self.put(lane, vec![note(0, 60, up7(100), Q)]);
        lane
    }

    /// An audio track holding one region, on `out` (none: unrouted).
    fn audio_track(&mut self, name: &str, out: Option<OutputPathId>) -> LaneId {
        let seq = self.seq;
        let lane = self.track(name, None);
        let media = self.media();
        let fade = Fade {
            length_frames: 0,
            shape: FadeShape::Linear,
        };
        self.exec(Command::Audio(AudioCmd::Insert {
            seq,
            lane,
            tick: Tick(0),
            media,
            source_offset: 0,
            length: 48_000,
            gain_db: 0.0,
            fade_in: fade,
            fade_out: fade,
        }));
        // A new track takes the first output: set it (or none) after.
        self.exec(Command::Lane(LaneCmd::SetOutput {
            seq,
            lane,
            output: out,
        }));
        lane
    }

    fn media(&mut self) -> MediaId {
        let n = self.doc.media().len();
        let ids = self.exec(Command::Media(MediaCmd::Add {
            path: format!("Media/m{n}.wav"),
            sample_rate: 48_000,
            channels: 2,
            length_frames: 96_000,
            content_hash: None,
            name: None,
            origin: None,
        }));
        created(&ids, |id| match id {
            AnyId::Media(m) => Some(*m),
            _ => None,
        })
    }

    fn put(&mut self, lane: LaneId, events: Vec<EventSpec>) {
        let seq = self.seq;
        self.exec(Command::Event(EventCmd::Insert { seq, lane, events }));
    }

    fn mute_track(&mut self, lane: LaneId) {
        let seq = self.seq;
        self.exec(Command::Lane(LaneCmd::SetFlags {
            seq,
            lane,
            mute: Some(true),
            solo: None,
            arm: None,
        }));
    }

    fn start_offset(&mut self, ticks: i64) {
        let seq = self.seq;
        self.exec(Command::Sequence(SequenceCmd::SetStartOffset {
            seq,
            offset: Tick(ticks),
        }));
    }

    fn layout(&self, strips: &[StripId]) -> Layout {
        layout(&self.doc, self.seq, strips, "wav").unwrap()
    }

    fn defaults(&self) -> Vec<StripId> {
        default_strips(&self.doc, self.seq).unwrap()
    }

    fn files(&self, l: &Layout) -> (Vec<(String, Vec<u8>)>, Report) {
        midi_files(&self.doc, self.seq, l, &AtomicBool::new(false)).unwrap()
    }
}

fn note(tick: i64, pitch: u8, velocity: u16, duration: i64) -> EventSpec {
    EventSpec::new(Tick(tick), EventBody::note(pitch, velocity, Tick(duration)))
}

/// 6a's small document: "Song" at 120 BPM in 4/4 and C major; "Bass" on
/// the built-in synth, channel 1, with one note.
fn small_doc() -> Doc {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    d.midi_track("Bass", dest);
    d
}

fn wav() -> AboutAudio {
    AboutAudio {
        container: "WAV",
        bits: "24-bit".into(),
        rate: 48_000,
        frames: 96_000,
    }
}

// ---- looking at results ----

/// The item of `kind` whose text is `what`, if any.
fn item<'r>(r: &'r Report, kind: ItemKind, what: &str) -> Option<&'r core_io::report::Item> {
    r.items.iter().find(|i| i.kind == kind && i.what == what)
}

/// The item must be there, once per name, with exactly these names.
#[track_caller]
fn names_in(r: &Report, kind: ItemKind, what: &str, names: &[&str]) {
    let Some(i) = item(r, kind, what) else {
        panic!("no {kind:?} {what:?} in {:#?}", r.items);
    };
    assert_eq!(i.details, names, "{what}");
    assert_eq!(i.count, names.len() as u64, "{what}");
}

#[track_caller]
fn present(r: &Report, kind: ItemKind, what: &str) {
    assert!(
        item(r, kind, what).is_some(),
        "no {kind:?} {what:?} in {:#?}",
        r.items
    );
}

#[track_caller]
fn absent(r: &Report, needle: &str) {
    assert!(
        r.items.iter().all(|i| !i.what.contains(needle)),
        "{needle:?} in {:#?}",
        r.items
    );
}

fn data_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data"))
}

fn hex(b: &[u8]) -> String {
    b.chunks(16)
        .map(|c| c.iter().map(|x| format!("{x:02X}")).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// A hand-written golden: never blessed (a copy of 6a's `midi_out.rs`
/// helper). A missing or different file fails.
#[track_caller]
fn hand_golden(name: &str, got: &[u8]) {
    let want = std::fs::read(data_dir().join(name))
        .unwrap_or_else(|e| panic!("hand-written golden {name} is missing: {e}"));
    assert_eq!(
        hex(got),
        hex(&want),
        "{name}:\n--- got\n{}\n--- want\n{}",
        String::from_utf8_lossy(got),
        String::from_utf8_lossy(&want)
    );
}

fn paths(files: &[(String, Vec<u8>)]) -> Vec<&str> {
    files.iter().map(|(p, _)| p.as_str()).collect()
}

// ---- tests ----

#[test]
fn small_files_match_the_goldens() {
    let d = small_doc();
    let strips = d.defaults();
    assert_eq!(strips, vec![d.strip(StripKind::BuiltinSynth)]);
    let l = d.layout(&strips);
    assert_eq!(l.folder, "Song for another DAW");
    assert_eq!(
        l.files(),
        [
            "MIDI/01 Bass.mid",
            "MIDI/All MIDI.mid",
            "Tempo map.mid",
            "Audio/01 Bass.wav",
            "About these files.txt",
        ]
    );
    let (files, midi) = d.files(&l);
    assert_eq!(
        paths(&files),
        ["MIDI/01 Bass.mid", "MIDI/All MIDI.mid", "Tempo map.mid"]
    );
    hand_golden("export_small_type1_960.mid", &files[0].1);
    hand_golden("export_small_type1_960.mid", &files[1].1);
    hand_golden("for_daw_tempo_map.mid", &files[2].1);
    let r = route_report(&d.doc, d.seq, &strips, midi);
    let about = about_text(&d.doc, d.seq, &l, Some(&wav()), &r);
    hand_golden("for_daw_about.txt", about.as_bytes());
}

#[test]
fn names_are_safe_and_numbered() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    let long = "é".repeat(150);
    for name in ["A/B:C", ".hidden", long.as_str(), "Tab\there"] {
        d.midi_track(name, dest);
    }
    let l = d.layout(&[]);
    let midi: Vec<&str> = l.midi.iter().map(|(_, p)| p.as_str()).collect();
    assert_eq!(midi[0], "MIDI/01 A-B-C.mid");
    assert_eq!(midi[1], "MIDI/02 -hidden.mid");
    assert_eq!(midi[3], "MIDI/04 Tab-here.mid");
    let file = midi[2].strip_prefix("MIDI/").unwrap();
    let cut = file
        .strip_prefix("03 ")
        .and_then(|f| f.strip_suffix(".mid"))
        .unwrap();
    assert!(!cut.is_empty() && long.starts_with(cut), "{cut}");
    assert!(format!(".{file}.partial").len() <= 255, "{file}");
    assert!(
        format!(".{file}.partial").len() >= 254,
        "cut too short: {file}"
    );

    // The same name twice: told apart by the number.
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    d.midi_track("Bass", dest);
    d.midi_track("Bass", dest);
    let l = d.layout(&[]);
    let midi: Vec<&str> = l.midi.iter().map(|(_, p)| p.as_str()).collect();
    assert_eq!(midi, ["MIDI/01 Bass.mid", "MIDI/02 Bass.mid"]);

    // An empty name (a strip's): "Untitled".
    let bus = d.bus("Verb");
    d.exec(Command::Mixer(MixerCmd::RenameStrip {
        strip: bus,
        name: String::new(),
    }));
    let l = d.layout(&[bus]);
    assert_eq!(l.audio, [(bus, "Audio/01 Untitled.wav".to_string())]);
    assert_eq!(safe_part("", 0), "Untitled");
    assert_eq!(safe_part("  \u{7} ", 0), "-");
    assert_eq!(safe_part(" . ", 0), "-");
    assert_eq!(safe_part("ab", 254), "a");

    // The folder: the song title, else the sequence's name, else
    // "Untitled".
    assert_eq!(d.layout(&[]).folder, "Song for another DAW");
    let mut info = d.doc.settings().info.clone();
    info.title = Some("My/Song".into());
    d.exec(Command::Settings(SettingsCmd::SetInfo { info }));
    assert_eq!(d.layout(&[]).folder, "My-Song for another DAW");
    let mut e = Doc::new("");
    assert_eq!(e.layout(&[]).folder, "Untitled for another DAW");
    let mut info = e.doc.settings().info.clone();
    info.title = Some("x".repeat(300));
    e.exec(Command::Settings(SettingsCmd::SetInfo { info }));
    let folder = e.layout(&[]).folder;
    assert!(
        format!(".{folder} 9999.partial").len() <= 255,
        "{}",
        folder.len()
    );
    assert!(folder.ends_with(" for another DAW"));
}

/// Track strips of every kind, a send-only bus, a bus a strip outputs to,
/// and a strip nothing plays through.
struct Mixed {
    d: Doc,
    keys: StripId,
    keys_out: StripId,
    synth: StripId,
    gtr: StripId,
    verb: StripId,
    group: StripId,
    unused: StripId,
}

fn mixed() -> Mixed {
    let mut d = Doc::new("Song");
    let keys_dest = d.plugin("Keys");
    d.exec(Command::Mixer(MixerCmd::AddInstrumentOutput {
        dest: keys_dest,
        index: 1,
        name: "Keys 2".into(),
    }));
    d.midi_track("Piano", keys_dest);
    let b1 = d.builtin(0);
    let b2 = d.builtin(1);
    d.midi_track("Bass", b1);
    d.midi_track("Lead", b2);
    let out = d.output("Main");
    d.audio_track("Gtr", Some(out));
    let unused_dest = d.plugin("Unused");
    let verb = d.bus("Verb");
    let group = d.bus("Group");
    let m = Mixed {
        keys: d.strip(StripKind::Instrument(keys_dest)),
        keys_out: d.strip(StripKind::InstrumentOutput(keys_dest, 1)),
        synth: d.strip(StripKind::BuiltinSynth),
        gtr: d.strip(StripKind::AudioPath(out)),
        unused: d.strip(StripKind::Instrument(unused_dest)),
        verb,
        group,
        d,
    };
    let mut m = m;
    let (keys, synth) = (m.keys, m.synth);
    m.d.exec(Command::Mixer(MixerCmd::AddSend {
        strip: keys,
        send: AuxSend::new(verb),
    }));
    m.d.exec(Command::Mixer(MixerCmd::SetOutput {
        strip: synth,
        output: Output::Bus(group),
    }));
    m
}

#[test]
fn default_strips_are_track_strips_and_send_only_buses() {
    let m = mixed();
    let got = m.d.defaults();
    let want = [m.keys, m.keys_out, m.synth, m.gtr, m.verb];
    let order: Vec<StripId> = m.d.doc.mixer().order().iter().copied().collect();
    let in_order: Vec<StripId> = order.into_iter().filter(|s| want.contains(s)).collect();
    assert_eq!(got, in_order);
    assert_eq!(got.len(), want.len());
    for s in [m.group, m.unused, m.d.doc.mixer().master()] {
        assert!(!got.contains(&s), "{}", m.d.strip_name(s));
    }

    let refused = |strips: &[StripId]| match layout(&m.d.doc, m.d.seq, strips, "wav") {
        Err(IoError::Refused(s)) => s,
        other => panic!("{other:?}"),
    };
    let master = m.d.doc.mixer().master();
    assert!(refused(&[m.keys, master]).contains("Master"));
    assert!(refused(&[StripId(9_999)]).contains(&StripId(9_999).to_string()));
    let twice = refused(&[m.keys, m.verb, m.keys]);
    assert!(twice.contains("'Keys'"), "{twice}");
    assert!(matches!(
        layout(&m.d.doc, SeqId(9_999), &[], "wav"),
        Err(IoError::Refused(_))
    ));
    // Not a default, but accepted.
    assert!(layout(&m.d.doc, m.d.seq, &[m.unused, m.group], "wav").is_ok());
}

#[test]
fn stem_names_follow_their_track() {
    let m = mixed();
    let l = m.d.layout(&m.d.defaults());
    let names: Vec<(StripId, &str)> = l.audio.iter().map(|(s, p)| (*s, p.as_str())).collect();
    let name_of = |s: StripId| {
        names
            .iter()
            .find(|(x, _)| *x == s)
            .map(|(_, p)| p.split_once(' ').unwrap().1)
            .unwrap()
    };
    // One track plays through: the track's name.
    assert_eq!(name_of(m.keys), "Piano.wav");
    assert_eq!(name_of(m.keys_out), "Piano.wav");
    assert_eq!(name_of(m.gtr), "Gtr.wav");
    // Two tracks share it, or a bus: the strip's name.
    assert_eq!(name_of(m.synth), "Built-in Synth.wav");
    assert_eq!(name_of(m.verb), "Verb.wav");
    // Numbered in mixer order.
    for (k, (_, p)) in names.iter().enumerate() {
        assert!(p.starts_with(&format!("Audio/{:02} ", k + 1)), "{p}");
    }
}

#[test]
fn midi_tracks_are_tracks_with_midi_that_plays() {
    let mut d = Doc::new("Song");
    let ext = d.external("Ext");
    let out = d.output("Main");
    let ext_track = d.midi_track("Ext", ext);
    d.audio_track("Gtr", Some(out));
    d.track("Empty", Some(ext));
    let silent = d.track("Silent", Some(ext));
    let mut muted = note(0, 60, up7(100), Q);
    muted.mute = true;
    d.put(silent, vec![muted]);
    let quiet = d.midi_track("Quiet", ext);
    d.mute_track(quiet);
    assert_eq!(
        midi_tracks(&d.doc, d.seq).unwrap(),
        [ext_track, quiet],
        "a track muted in the project is still written"
    );
    assert!(midi_tracks(&d.doc, SeqId(9_999)).is_err());
}

#[test]
fn route_report_lists_everything_dropped() {
    let mut d = Doc::new("Song");
    let ext = d.external("Ext");
    let synth_dest = d.builtin(0);
    let keys_dest = d.plugin("Keys");
    let pad_dest = d.plugin("Pad");
    let unused_dest = d.plugin("Unused");
    d.midi_track("Ext", ext);
    d.track("Empty", None);
    let bass = d.midi_track("Bass", synth_dest);
    d.midi_track("Keys", keys_dest);
    d.midi_track("Pad", pad_dest);
    // An inactive take with a note.
    let seq = d.seq;
    d.exec(Command::Lane(LaneCmd::AddTake {
        seq,
        lane: bass,
        name: "Take 2".into(),
    }));
    d.put(bass, vec![note(Q, 62, up7(100), Q)]);
    let synth = d.strip(StripKind::BuiltinSynth);
    let keys = d.strip(StripKind::Instrument(keys_dest));
    let unused = d.strip(StripKind::Instrument(unused_dest));
    let group = d.bus("Group");
    d.exec(Command::Mixer(MixerCmd::SetMute {
        strip: synth,
        mute: true,
    }));
    d.exec(Command::Mixer(MixerCmd::SetColour {
        strip: keys,
        colour: Some(Swatch::Red),
    }));
    d.exec(Command::Mixer(MixerCmd::SetOutput {
        strip: keys,
        output: Output::Bus(group),
    }));
    let mut info = d.doc.settings().info.clone();
    info.composer = Some("Someone".into());
    d.exec(Command::Settings(SettingsCmd::SetInfo { info }));

    let chosen = [synth, keys, unused];
    let l = d.layout(&chosen);
    let (_, midi) = d.files(&l);
    let r = route_report(&d.doc, d.seq, &chosen, midi);
    use ItemKind::{LeftOut, Note};
    names_in(
        &r,
        LeftOut,
        "No audio for tracks playing external MIDI (their MIDI files are written)",
        &["Ext"],
    );
    names_in(
        &r,
        Note,
        "Tracks with nothing to play: no files",
        &["Empty"],
    );
    names_in(&r, LeftOut, "Strips not chosen", &["Pad"]);
    present(
        &r,
        Note,
        "Strip 'Built-in Synth' is muted in the project; its stem is written",
    );
    names_in(&r, LeftOut, "Strip colours", &["Keys"]);
    names_in(
        &r,
        LeftOut,
        "Strips no track of this sequence plays through",
        &["Unused"],
    );
    names_in(
        &r,
        Note,
        "Stems are taken before the buses they play through",
        &["Keys"],
    );
    present(
        &r,
        Note,
        "Mix, effects and automation are part of the audio files",
    );
    present(&r, Note, "Song info is in About these files.txt");
    absent(&r, "Song info other than title and copyright");
    names_in(&r, LeftOut, "Inactive takes", &["Bass"]);
    absent(&r, "Audio regions with no output");
    assert_eq!(r.summary.audio_files, 3);
    assert_eq!(r.summary.tracks, 4);
    // The song info is in the About file.
    let about = about_text(&d.doc, d.seq, &l, Some(&wav()), &r);
    assert!(
        about.contains("\n\nSong info\n  Composer: Someone\n\nFiles\n"),
        "{about}"
    );

    // No MIDI track; an audio track muted in the project, a region with no
    // output, and a strip silenced by another's solo.
    let mut d = Doc::new("Song");
    let main = d.output("Main");
    let aux = d.output("Aux");
    let gtr = d.audio_track("Gtr", Some(main));
    d.mute_track(gtr);
    d.audio_track("Loose", None);
    d.audio_track("Keys", Some(aux));
    let main_strip = d.strip(StripKind::AudioPath(main));
    d.exec(Command::Mixer(MixerCmd::SetSolo {
        strip: main_strip,
        solo: true,
    }));
    let chosen = d.defaults();
    assert_eq!(chosen.len(), 2);
    let l = d.layout(&chosen);
    assert!(l.midi.is_empty() && l.all_midi.is_none());
    let (files, midi) = d.files(&l);
    assert_eq!(paths(&files), ["Tempo map.mid"]);
    let r = route_report(&d.doc, d.seq, &chosen, midi);
    present(&r, Note, "Track 'Gtr' is muted in the project");
    names_in(&r, LeftOut, "Audio regions with no output", &["Loose"]);
    present(
        &r,
        Note,
        "Strip 'Aux' is silenced by a solo in the project; its stem is written",
    );
    absent(&r, "Song info is in");
    assert_eq!(r.summary.audio_files, 2);
    assert_eq!(r.summary.tracks, 0);
    assert!(r.summary.tempo_map);

    // MIDI files only: no audio Note.
    let r = route_report(&d.doc, d.seq, &[], Report::default());
    absent(&r, "Mix, effects and automation");
    names_in(&r, LeftOut, "Strips not chosen", &["Main", "Aux"]);
    let about = about_text(&d.doc, d.seq, &d.layout(&[]), None, &r);
    assert!(
        about.contains("\nAudio: none; no strips were chosen.\n"),
        "{about}"
    );
}

#[test]
fn a_muted_midi_track_is_noted_once() {
    let mut d = small_doc();
    let bass = d.doc.sequence(d.seq).unwrap().lanes[0].id;
    d.mute_track(bass);
    let strips = d.defaults();
    let l = d.layout(&strips);
    let (_, midi) = d.files(&l);
    let r = route_report(&d.doc, d.seq, &strips, midi);
    let what = "Track 'Bass' is muted in the project";
    assert_eq!(item(&r, ItemKind::Note, what).map(|i| i.count), Some(1));
}

#[test]
fn tempo_line_with_a_pickup() {
    let mut d = small_doc();
    d.start_offset(Q);
    let strips = d.defaults();
    let l = d.layout(&strips);
    let (files, midi) = d.files(&l);
    let r = route_report(&d.doc, d.seq, &strips, midi);
    let about = about_text(&d.doc, d.seq, &l, Some(&wav()), &r);
    let lines: Vec<&str> = about.lines().collect();
    assert_eq!(
        lines[2],
        "Start: Every file starts 1 beat before bar 1, where the song's pickup begins."
    );
    assert_eq!(
        lines[3],
        "Tempo: 120 BPM, 1/4 for the pickup, then 4/4 from bar 1. Tempo map.mid holds every tempo and meter change and the markers."
    );
    let (smf, _) = to_tempo_map_smf(&d.doc, d.seq, 960, &AtomicBool::new(false)).unwrap();
    assert_eq!(smf.tracks.len(), 1);
    let meters: Vec<(u64, &[u8])> = smf.tracks[0]
        .events
        .iter()
        .filter_map(|(t, e)| match e {
            SmfEvent::Meta { kind: 0x58, data } => Some((*t, data.as_slice())),
            _ => None,
        })
        .collect();
    assert_eq!(meters[0], (0, &[1, 2, 24, 8][..]));
    assert_eq!(meters[1], (960, &[4, 2, 24, 8][..]));
    let tempo_map = &files.iter().find(|(p, _)| p == "Tempo map.mid").unwrap().1;
    assert!(
        tempo_map
            .windows(8)
            .any(|w| w == [0x00, 0xFF, 0x58, 0x04, 0x01, 0x02, 0x18, 0x08]),
        "{}",
        hex(tempo_map)
    );

    // A tempo with decimals, and a padded pickup's meter.
    let mut d = small_doc();
    let seq = d.seq;
    let tempo = core_model::tempo::TempoMap::new(vec![core_model::tempo::TempoPoint {
        tick: Tick(0),
        bpm: core_model::tempo::Bpm(97_250),
        ramp: false,
    }])
    .unwrap();
    d.exec(Command::Sequence(SequenceCmd::SetTempo { seq, tempo }));
    d.start_offset(Q * 3 / 2);
    let l = d.layout(&[]);
    let about = about_text(&d.doc, d.seq, &l, None, &Report::default());
    assert!(
        about.contains("\nTempo: 97.25 BPM, 4/4 at the start. "),
        "{about}"
    );
}

#[test]
fn start_sentences() {
    let mut d = small_doc();
    let s = |d: &Doc| d.doc.sequence(d.seq).unwrap().clone();
    assert_eq!(start_sentence(&s(&d)), "Every file starts at bar 1.");
    assert_eq!(file_start(&s(&d)), Tick(0));
    d.start_offset(Q);
    assert_eq!(
        start_sentence(&s(&d)),
        "Every file starts 1 beat before bar 1, where the song's pickup begins."
    );
    assert_eq!(file_start(&s(&d)), Tick(-Q));
    d.start_offset(2 * Q);
    assert_eq!(
        start_sentence(&s(&d)),
        "Every file starts 2 beats before bar 1, where the song's pickup begins."
    );
    d.start_offset(Q * 3 / 2);
    assert_eq!(
        start_sentence(&s(&d)),
        "Every file starts 1 bar before bar 1, so the song's pickup fits."
    );
    assert_eq!(file_start(&s(&d)), Tick(-61_440));
    d.start_offset(5 * Q);
    assert_eq!(
        start_sentence(&s(&d)),
        "Every file starts 2 bars before bar 1, so the song's pickup fits."
    );
    assert_eq!(file_start(&s(&d)), Tick(-2 * BAR));
}

#[test]
fn a_thousand_tracks() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    for k in 0..1000 {
        d.midi_track(&format!("T{k}"), dest);
    }
    let l = d.layout(&[]);
    let t = Instant::now();
    let (files, r) = d.files(&l);
    let took = t.elapsed();
    println!("a_thousand_tracks: midi_files took {took:?}");
    assert_eq!(files.len(), 1002);
    assert_eq!(files[0].0, "MIDI/0001 T0.mid");
    let names: Vec<&str> = files[..1000].iter().map(|(p, _)| p.as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "numbers sort in track order");
    assert_eq!(files[999].0, "MIDI/1000 T999.mid");
    assert_eq!(files[1000].0, "MIDI/All MIDI.mid");
    assert_eq!(files[1001].0, "Tempo map.mid");
    assert_eq!(r.summary.tracks, 1000);
    assert!(took.as_secs() < 10, "{took:?}");
}

#[test]
fn cancel_stops_the_midi_files() {
    let d = small_doc();
    let l = d.layout(&[]);
    assert_eq!(
        midi_files(&d.doc, d.seq, &l, &AtomicBool::new(true)).map(|_| ()),
        Err(IoError::Cancelled)
    );
}

#[test]
fn numbers_take_the_width_of_the_total() {
    let mut d = Doc::new("Song");
    let dest = d.builtin(0);
    let long = "é".repeat(150);
    for k in 0..100 {
        let name = if k == 49 {
            long.clone()
        } else {
            format!("T{k}")
        };
        d.midi_track(&name, dest);
    }
    let l = d.layout(&[]);
    let midi: Vec<&str> = l.midi.iter().map(|(_, p)| p.as_str()).collect();
    assert_eq!(midi[0], "MIDI/001 T0.mid");
    assert_eq!(midi[99], "MIDI/100 T99.mid");
    let file = midi[49].strip_prefix("MIDI/").unwrap();
    assert!(file.starts_with("050 é"), "{file}");
    let partial = format!(".{file}.partial").len();
    assert!((254..=255).contains(&partial), "{partial}");
}

#[test]
fn a_bus_a_played_strip_sends_to_is_a_default() {
    // "Verb" receives a played track's send; a strip nothing plays
    // through outputs to it. It is still a default stem.
    let mut d = Doc::new("Song");
    let keys_dest = d.plugin("Keys");
    let unused_dest = d.plugin("Unused");
    d.midi_track("Piano", keys_dest);
    let keys = d.strip(StripKind::Instrument(keys_dest));
    let unused = d.strip(StripKind::Instrument(unused_dest));
    let verb = d.bus("Verb");
    d.exec(Command::Mixer(MixerCmd::AddSend {
        strip: keys,
        send: AuxSend::new(verb),
    }));
    d.exec(Command::Mixer(MixerCmd::SetOutput {
        strip: unused,
        output: Output::Bus(verb),
    }));
    assert_eq!(d.defaults(), [keys, verb]);
    // A played strip outputting to it: heard in that stem, not its own.
    d.exec(Command::Mixer(MixerCmd::SetOutput {
        strip: keys,
        output: Output::Bus(verb),
    }));
    assert_eq!(d.defaults(), [keys]);
}

/// The first meta of `kind` in the tempo map file: its tick and data.
fn first_meta(d: &Doc, kind: u8) -> (u64, Vec<u8>) {
    let (smf, _) = to_tempo_map_smf(&d.doc, d.seq, 960, &AtomicBool::new(false)).unwrap();
    smf.tracks[0]
        .events
        .iter()
        .find_map(|(t, e)| match e {
            SmfEvent::Meta { kind: k, data } if *k == kind => Some((*t, data.clone())),
            _ => None,
        })
        .unwrap()
}

fn tempo_line(d: &Doc) -> String {
    let l = d.layout(&[]);
    let about = about_text(&d.doc, d.seq, &l, None, &Report::default());
    about.lines().nth(3).unwrap().to_string()
}

#[test]
fn every_tempo_map_has_a_meter_at_0() {
    let mut d = small_doc();
    let seq = d.seq;
    let meter = core_model::meter::MeterMap::new(vec![
        core_model::meter::MeterEntry {
            tick: Tick(0),
            meter: core_model::meter::Meter { num: 3, den: 4 },
            key: core_model::meter::KeySig {
                sharps: 0,
                minor: false,
            },
        },
        core_model::meter::MeterEntry {
            tick: Tick(3 * Q),
            meter: core_model::meter::Meter { num: 4, den: 4 },
            key: core_model::meter::KeySig {
                sharps: 0,
                minor: false,
            },
        },
    ])
    .unwrap();
    d.exec(Command::Sequence(SequenceCmd::SetMeter { seq, meter }));
    for (offset, line_meter) in [
        (0, "3/4 at the start"),
        (Q, "1/4 for the pickup, then 3/4 from bar 1"),
        (Q / 2, "3/4 at the start"),
    ] {
        d.start_offset(offset);
        let (tick, data) = first_meta(&d, 0x58);
        assert_eq!(tick, 0, "offset {offset}");
        let want = if offset == Q { 1 } else { 3 };
        assert_eq!(data[0], want, "offset {offset}");
        let line = tempo_line(&d);
        assert!(line.contains(&format!(", {line_meter}. ")), "{line}");
    }
}

/// "97.25" from 97.25, "120" from 120.0: the About file's BPM text.
fn bpm_text(b: f64) -> String {
    format!("{b:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[test]
fn the_tempo_line_matches_the_tempo_map_file() {
    use core_model::tempo::{Bpm, TempoMap, TempoPoint};
    let point = |t: i64, bpm: u32, ramp: bool| TempoPoint {
        tick: Tick(t),
        bpm: Bpm(bpm),
        ramp,
    };
    for (points, offset) in [
        // A ramp from the start: the file's first step, not bpm_at(0).
        (vec![point(0, 100_000, true), point(BAR, 140_000, false)], 0),
        // A step change, with a pickup before it.
        (vec![point(0, 97_250, false), point(BAR, 90_000, false)], Q),
        // A ramp from bar 1, with a pickup before it.
        (
            vec![point(0, 80_000, true), point(BAR, 160_000, false)],
            2 * Q,
        ),
    ] {
        let mut d = small_doc();
        let seq = d.seq;
        let tempo = TempoMap::new(points).unwrap();
        d.exec(Command::Sequence(SequenceCmd::SetTempo { seq, tempo }));
        d.start_offset(offset);
        let (tick, data) = first_meta(&d, 0x51);
        assert_eq!(tick, 0);
        let micros = u32::from_be_bytes([0, data[0], data[1], data[2]]);
        let file_bpm = bpm_text(60_000_000.0 / f64::from(micros));
        let line = tempo_line(&d);
        assert!(
            line.starts_with(&format!("Tempo: {file_bpm} BPM, ")),
            "{line} vs {file_bpm}"
        );
    }
}

#[test]
fn about_text_is_clean_text() {
    let mut d = small_doc();
    let mut info = d.doc.settings().info.clone();
    // The model keeps song info clean; strip names are free text.
    info.comment = Some("one\ntwo".into());
    d.exec(Command::Settings(SettingsCmd::SetInfo { info }));
    let synth = d.strip(StripKind::BuiltinSynth);
    d.exec(Command::Mixer(MixerCmd::RenameStrip {
        strip: synth,
        name: "Big\nSynth\r\u{7}".into(),
    }));
    d.exec(Command::Mixer(MixerCmd::SetColour {
        strip: synth,
        colour: Some(Swatch::Blue),
    }));
    let strips = d.defaults();
    let l = d.layout(&strips);
    let (_, midi) = d.files(&l);
    let r = route_report(&d.doc, d.seq, &strips, midi);
    let about = about_text(&d.doc, d.seq, &l, Some(&wav()), &r);
    assert!(
        !about.chars().any(|c| c.is_control() && c != '\n'),
        "{about:?}"
    );
    assert!(about.contains("\n  Comment: one\n    two\n"), "{about}");
    assert!(about.contains("\n    Big Synth\n"), "{about}");
}
