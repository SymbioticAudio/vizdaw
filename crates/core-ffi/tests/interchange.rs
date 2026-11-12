//! MIDI file import and export through the FFI (plan 6a Task 7): the
//! jobs, their reports, song info and every hostile request. Temporary
//! folders only; one test at a time (the engine host is process-wide).

mod common;

use core_engine::fixtures::{Builder, Q};
use core_engine::midi1::Midi1Msg;
use core_io::smf::{self, Division, SmfEvent, SmfFile, SmfTrack};
use core_model::command::{Command, LaneCmd};
use core_model::lane::LaneInput;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use vizdaw_ffi::control::result_json;
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    ChannelMapRow, ClipExportRequestRow, ControlResult, JobKindRow, JobRow, JobStateRow,
    MidiFileKindRow, MidiImportRequestRow, RecordSpecRow, ReportRow, Session, SmfExportRequestRow,
    SongInfoRow, VizError,
};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    reset_host();
    g
}

// ---- files ----

fn on(c: u8, pitch: u8, vel: u8) -> SmfEvent {
    SmfEvent::Channel(Midi1Msg::three(0x90 | c, pitch, vel))
}

fn off(c: u8, pitch: u8) -> SmfEvent {
    SmfEvent::Channel(Midi1Msg::three(0x80 | c, pitch, 0x40))
}

fn meta(kind: u8, data: &[u8]) -> SmfEvent {
    SmfEvent::Meta {
        kind,
        data: data.to_vec(),
    }
}

/// 100 BPM in microseconds per quarter.
fn tempo_100() -> SmfEvent {
    meta(0x51, &[0x09, 0x27, 0xC0])
}

/// Two notes on channel `c` (0-based) at 960 per quarter.
fn notes_on(c: u8) -> Vec<(u64, SmfEvent)> {
    vec![
        (0, on(c, 60, 100)),
        (480, off(c, 60)),
        (960, on(c, 62, 90)),
        (1440, off(c, 62)),
    ]
}

/// A type 1 file at 960 per quarter: a conductor at 100 BPM, then one
/// track per channel in `channels`.
fn type1(channels: &[u8]) -> Vec<u8> {
    let mut tracks = vec![vec![(0, meta(0x03, b"Song")), (0, tempo_100())]];
    tracks.extend(channels.iter().map(|c| notes_on(*c)));
    let tracks = tracks
        .into_iter()
        .map(|mut events| {
            let end = events.last().map_or(0, |(t, _)| *t);
            events.push((end, meta(0x2F, &[])));
            SmfTrack { events }
        })
        .collect();
    smf::write(&SmfFile {
        format: 1,
        division: Division::Ppq(960),
        tracks,
    })
    .unwrap()
}

fn write(dir: &Path, name: &str, bytes: &[u8]) -> String {
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    p.display().to_string()
}

fn text(p: &Path) -> String {
    p.display().to_string()
}

// ---- sessions ----

fn wait_job(s: &Session, id: u64) -> JobRow {
    let until = Instant::now() + common::wait_limit();
    loop {
        s.idle().unwrap();
        let j = s.job(id).unwrap();
        if j.state != JobStateRow::Running {
            return j;
        }
        assert!(Instant::now() < until, "job {id} still running");
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn seq_named(s: &Session, name: &str) -> u64 {
    s.snapshot()
        .unwrap()
        .sequences
        .iter()
        .find(|q| q.name == name)
        .unwrap()
        .id
}

fn first_seq(s: &Session) -> u64 {
    s.snapshot().unwrap().sequences.first().unwrap().id
}

fn import_req(path: String, seq: u64) -> MidiImportRequestRow {
    MidiImportRequestRow {
        path,
        new_project: false,
        seq,
        lane: None,
        tick: 0,
        channel_map: Vec::new(),
        split_channels: None,
        mpe: false,
        use_tempo: false,
    }
}

fn smf_req(seq: u64, path: String) -> SmfExportRequestRow {
    SmfExportRequestRow {
        seq,
        lanes: Vec::new(),
        path,
        replace: false,
        type0: false,
        ppq: 960,
        mpe: false,
    }
}

fn clip_req(seq: u64, lanes: Vec<u64>, path: String) -> ClipExportRequestRow {
    ClipExportRequestRow {
        seq,
        lanes,
        path,
        replace: false,
        start: None,
        end: None,
    }
}

fn is_model(r: &Result<u64, VizError>) -> bool {
    matches!(r, Err(VizError::Model { .. }))
}

/// Sequence "S" with tracks "Drums", "Keys" and "Bass", a note each, on a
/// built-in synth on channel 2 (0-based 1).
fn three_tracks() -> (Arc<Session>, u64, Vec<u64>, u64) {
    let mut b = Builder::new();
    let synth = b.dest("Synth 2", "builtin:synth", 1).unwrap();
    let (seq, drums) = b.sequence("S", "Drums", Some(synth)).unwrap();
    let keys = b.lane(seq, "Keys", Some(synth)).unwrap();
    let bass = b.lane(seq, "Bass", Some(synth)).unwrap();
    for (k, l) in [drums, keys, bass].into_iter().enumerate() {
        b.notes(seq, l, &[(k as i64 * Q, 60, 0x8000, Q / 2)])
            .unwrap();
    }
    let s = Session::from_document(b.finish()).unwrap();
    (s, seq.0, vec![drums.0, keys.0, bass.0], synth.0)
}

// ---- tests ----

#[test]
fn midi_file_info_lists_channels_and_suggestions() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "three.mid", &type1(&[0, 1, 9]));
    let (s, _, _, synth) = three_tracks();
    let info = s.midi_file_info(path).unwrap();
    assert_eq!(info.kind, MidiFileKindRow::Smf1);
    assert_eq!(info.tracks, 3);
    assert!(info.has_tempo);
    assert!(!info.mpe_found);
    let rows: Vec<(u8, u8, u64, Option<u64>)> = info
        .channels
        .iter()
        .map(|c| (c.group, c.channel, c.notes, c.suggested))
        .collect();
    assert_eq!(
        rows,
        vec![(0, 0, 2, None), (0, 1, 2, Some(synth)), (0, 9, 2, None)]
    );
    s.close().unwrap();
}

#[test]
fn midi_file_info_refuses_hostile_paths() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_demo().unwrap();
    let huge = dir.path().join("huge.mid");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let not_midi = write(dir.path(), "notes.mid", b"hello, this is text");
    for p in [
        String::new(),
        "relative.mid".to_string(),
        text(dir.path()),
        text(&dir.path().join("missing.mid")),
        text(&huge),
        not_midi,
        "/dev/zero".to_string(),
    ] {
        match s.midi_file_info(p.clone()) {
            Err(VizError::Model { .. } | VizError::Io { .. }) => {}
            other => panic!("{p}: {other:?}"),
        }
    }
    s.close().unwrap();
}

#[test]
fn import_is_one_undo_step() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "two.mid", &type1(&[0, 1]));
    let s = Session::new_demo().unwrap();
    let seq = seq_named(&s, "Groove");
    let before = s.debug_document_json();
    let lanes = s.lanes(seq).unwrap().len();
    assert_eq!(s.interchange_report().unwrap(), None);
    let id = s
        .import_midi(MidiImportRequestRow {
            tick: 16 * Q,
            ..import_req(path, seq)
        })
        .unwrap();
    let j = wait_job(&s, id);
    assert_eq!(j.kind, JobKindRow::ImportMidi);
    assert_eq!(j.state, JobStateRow::Done, "{:?}", j.message);
    let report = j.report.clone().expect("a report");
    assert!(
        report.summary.starts_with("Imported 2 tracks"),
        "{report:?}"
    );
    assert_eq!(s.lanes(seq).unwrap().len(), lanes + 2);
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.undo_label.as_deref(), Some("Import MIDI File"));
    assert!(snap.dirty);
    assert_eq!(s.interchange_report().unwrap(), Some(report));
    s.undo().unwrap();
    assert_eq!(s.debug_document_json(), before, "one undo restores it");
    s.close().unwrap();
}

#[test]
fn an_empty_sequence_takes_the_files_tempo_on_a_drop() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "one.mid", &type1(&[0]));
    let s = Session::new_empty().unwrap();
    let seq = first_seq(&s);
    // use_tempo off, as a drop sends it: an empty sequence still uses it.
    let id = s.import_midi(import_req(path, seq)).unwrap();
    assert_eq!(wait_job(&s, id).state, JobStateRow::Done);
    let tempo = s.sequence_info(seq).unwrap().tempo;
    assert!(
        (tempo.first().unwrap().bpm - 100.0).abs() < 1e-6,
        "{tempo:?}"
    );
    // One undo step all the same (not a new project).
    assert_eq!(
        s.snapshot().unwrap().undo_label.as_deref(),
        Some("Import MIDI File")
    );
    s.close().unwrap();
}

#[test]
fn new_project_import_has_nothing_to_undo() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "two.mid", &type1(&[0, 1]));
    let s = Session::new_empty().unwrap();
    let seq = first_seq(&s);
    let id = s
        .import_midi(MidiImportRequestRow {
            new_project: true,
            ..import_req(path.clone(), seq)
        })
        .unwrap();
    let j = wait_job(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{:?}", j.message);
    let info = s.sequence_info(seq).unwrap();
    assert!((info.tempo.first().unwrap().bpm - 100.0).abs() < 1e-6);
    assert_eq!(info.name, "Song", "the title names the sequence");
    assert_eq!(s.lanes(seq).unwrap().len(), 2);
    let snap = s.snapshot().unwrap();
    assert_eq!(snap.undo_label, None);
    assert!(snap.dirty, "the document reads Edited");
    assert!(s.undo().is_err());
    s.close().unwrap();

    let demo = Session::new_demo().unwrap();
    let r = demo.import_midi(MidiImportRequestRow {
        new_project: true,
        ..import_req(path, first_seq(&demo))
    });
    assert!(is_model(&r), "{r:?}");
    demo.close().unwrap();
}

#[test]
fn exports_never_leave_half_files() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_demo().unwrap();
    let seq = seq_named(&s, "Groove");
    let x = dir.path().join("x.mid");
    let id = s.export_midi(smf_req(seq, text(&x))).unwrap();
    let j = wait_job(&s, id);
    assert_eq!(j.kind, JobKindRow::ExportMidi);
    assert_eq!(j.state, JobStateRow::Done, "{:?}", j.message);
    assert_eq!(j.written, vec![text(&x)]);
    let report = j.report.clone().unwrap();
    assert!(report.summary.starts_with("Exported"), "{report:?}");
    assert_eq!(s.interchange_report().unwrap(), Some(report));
    let bytes = std::fs::read(&x).unwrap();
    assert!(bytes.starts_with(b"MThd"));

    // Again without replace: refused at once, the file as it was.
    let r = s.export_midi(smf_req(seq, text(&x)));
    assert!(is_model(&r), "{r:?}");
    assert_eq!(std::fs::read(&x).unwrap(), bytes);
    // With replace it is written again.
    let id = s
        .export_midi(SmfExportRequestRow {
            replace: true,
            ppq: 480,
            ..smf_req(seq, text(&x))
        })
        .unwrap();
    assert_eq!(wait_job(&s, id).state, JobStateRow::Done);
    assert_ne!(std::fs::read(&x).unwrap(), bytes);

    // Cancelled: no file and no partial.
    let other = dir.path().join("held");
    std::fs::create_dir(&other).unwrap();
    let y = other.join("x.mid");
    s.debug_hold_jobs(true).unwrap();
    let id = s.export_midi(smf_req(seq, text(&y))).unwrap();
    s.cancel_job(id).unwrap();
    s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&s, id).state, JobStateRow::Cancelled);
    assert!(!y.exists());
    assert!(!other.join(".x.mid.partial").exists());
    assert_eq!(std::fs::read_dir(&other).unwrap().count(), 0);
    s.close().unwrap();

    // Three clips into a folder where the second's name is taken: nothing
    // is written.
    let (s, seq, lanes, _) = three_tracks();
    let folder = dir.path().join("clips");
    std::fs::create_dir(&folder).unwrap();
    std::fs::write(folder.join("02 Keys.midi2"), b"mine").unwrap();
    let r = s.export_midi_clip(clip_req(seq, lanes.clone(), text(&folder)));
    match &r {
        Err(VizError::Model { message }) => assert!(message.contains("02 Keys.midi2"), "{message}"),
        other => panic!("{other:?}"),
    }
    let names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["02 Keys.midi2".to_string()]);
    assert_eq!(
        std::fs::read(folder.join("02 Keys.midi2")).unwrap(),
        b"mine"
    );

    // With replace, all three, numbered in order.
    let id = s
        .export_midi_clip(ClipExportRequestRow {
            replace: true,
            ..clip_req(seq, lanes.clone(), text(&folder))
        })
        .unwrap();
    let j = wait_job(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{:?}", j.message);
    let want: Vec<String> = ["01 Drums.midi2", "02 Keys.midi2", "03 Bass.midi2"]
        .iter()
        .map(|n| text(&folder.join(n)))
        .collect();
    assert_eq!(j.written, want);
    for w in &want {
        assert!(std::fs::read(w).unwrap().starts_with(b"SMF2CLIP"));
    }

    // One track: the path is the file.
    let one = dir.path().join("keys.midi2");
    let id = s
        .export_midi_clip(ClipExportRequestRow {
            start: Some(0),
            end: Some(4 * Q),
            ..clip_req(seq, vec![lanes[1]], text(&one))
        })
        .unwrap();
    let j = wait_job(&s, id);
    assert_eq!(j.state, JobStateRow::Done, "{:?}", j.message);
    assert_eq!(j.written, vec![text(&one)]);
    s.close().unwrap();
}

#[test]
fn hostile_import_paths_are_refused() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_demo().unwrap();
    let seq = seq_named(&s, "Groove");
    let before = s.debug_document_json();
    let p = dir.path();
    let folder = p.join("folder.mid");
    std::fs::create_dir(&folder).unwrap();
    let fifo = p.join("fifo.mid");
    let have_fifo = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|s| s.success());
    let (a, b) = (p.join("a.mid"), p.join("b.mid"));
    std::os::unix::fs::symlink(&b, &a).unwrap();
    std::os::unix::fs::symlink(&a, &b).unwrap();
    let huge = p.join("huge.mid");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    let long = format!("/{}", "x".repeat(4_999));
    // (path, the text its refusal names)
    let mut cases: Vec<(String, Option<String>)> = vec![
        (String::new(), None),
        ("relative.mid".into(), Some("relative.mid".into())),
        (long, None),
        (format!("{}/n\0l.mid", p.display()), None),
        (text(&folder), Some("folder.mid".into())),
        ("/dev/zero".into(), Some("zero".into())),
        (text(&a), Some("a.mid".into())),
        (text(&p.join("missing.mid")), Some("missing.mid".into())),
        (text(&huge), Some("huge.mid".into())),
    ];
    if have_fifo {
        cases.push((text(&fifo), Some("fifo.mid".into())));
    }
    for (path, named) in cases {
        let why = match s.import_midi(import_req(path.clone(), seq)) {
            Err(VizError::Model { message }) => message,
            Ok(id) => {
                let j = wait_job(&s, id);
                assert_eq!(j.state, JobStateRow::Failed, "{path:?}");
                assert_eq!(j.report, None);
                j.message.unwrap()
            }
            Err(e) => panic!("{path:?}: {e:?}"),
        };
        assert!(!why.contains("internal error"), "{path:?}: {why}");
        if let Some(n) = named {
            assert!(why.contains(&n), "{path:?}: {why}");
        }
        assert_eq!(s.debug_document_json(), before, "{path:?}");
    }
    assert_eq!(s.interchange_report().unwrap(), None);
    s.close().unwrap();
}

#[test]
fn hostile_export_paths_are_refused() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_demo().unwrap();
    let pkg = dir.path().join("P.vizdaw");
    s.save_as(text(&pkg)).unwrap();
    let seq = seq_named(&s, "Groove");
    let lane = s.lanes(seq).unwrap()[0].id;
    let other = dir.path().join("Other.vizdaw");
    std::fs::create_dir(&other).unwrap();
    std::fs::create_dir(dir.path().join("a")).unwrap();
    let files = [
        pkg.join("x.mid"),
        pkg.join("Media").join("x.mid"),
        other.join("x.mid"),
        dir.path().join("a").join("..").join("x.mid"),
        dir.path().join("nope").join("x.mid"),
    ];
    for f in &files {
        let r = s.export_midi(smf_req(seq, text(f)));
        assert!(is_model(&r), "{f:?}: {r:?}");
        let r = s.export_midi_clip(clip_req(seq, vec![lane], text(f)));
        assert!(is_model(&r), "{f:?}: {r:?}");
    }
    // Folders for several clips.
    let lanes: Vec<u64> = s.lanes(seq).unwrap().iter().map(|l| l.id).collect();
    assert!(lanes.len() > 1);
    for folder in [
        pkg.clone(),
        other.clone(),
        dir.path().join("nope"),
        dir.path().join("a").join(".."),
    ] {
        let r = s.export_midi_clip(clip_req(seq, lanes.clone(), text(&folder)));
        assert!(is_model(&r), "{folder:?}: {r:?}");
    }
    // A file where a folder is wanted, and a folder where a file is.
    let file = write(dir.path(), "file", b"x");
    assert!(is_model(&s.export_midi_clip(clip_req(seq, lanes, file))));
    assert!(is_model(&s.export_midi(SmfExportRequestRow {
        replace: true,
        ..smf_req(seq, text(&dir.path().join("a")))
    })));
    assert!(!pkg.join("x.mid").exists());
    assert!(!other.join("x.mid").exists());
    assert!(!dir.path().join("x.mid").exists());
    s.close().unwrap();
}

#[test]
fn hostile_requests_are_refused() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "two.mid", &type1(&[0, 1]));
    let s = Session::new_demo().unwrap();
    let seq = seq_named(&s, "Groove");
    let beat = seq_named(&s, "Beat");
    let lane = s.lanes(seq).unwrap()[0].id;
    let foreign = s.lanes(beat).unwrap()[0].id;
    let dest = s.destinations().unwrap()[0].id;
    let before = s.debug_document_json();
    let x = text(&dir.path().join("x.mid"));
    let named = |r: Result<u64, VizError>, word: &str, what: &str| match r {
        Err(VizError::Model { message }) => {
            assert!(message.contains(word), "{what}: {message}")
        }
        other => panic!("{what}: {other:?}"),
    };

    for ppq in [0u16, 1, 961, u16::MAX] {
        named(
            s.export_midi(SmfExportRequestRow {
                ppq,
                ..smf_req(seq, x.clone())
            }),
            "ticks per quarter",
            "ppq",
        );
    }
    named(s.export_midi(smf_req(9_999, x.clone())), "sequence", "seq");
    named(
        s.export_midi(SmfExportRequestRow {
            lanes: vec![foreign],
            ..smf_req(seq, x.clone())
        }),
        "track",
        "a track of another sequence",
    );
    named(
        s.export_midi(SmfExportRequestRow {
            lanes: vec![lane, lane],
            ..smf_req(seq, x.clone())
        }),
        "track",
        "a track twice",
    );
    named(
        s.export_midi_clip(clip_req(seq, vec![], x.clone())),
        "track",
        "no track",
    );
    named(
        s.export_midi_clip(clip_req(seq, vec![foreign], x.clone())),
        "track",
        "a clip track of another sequence",
    );
    named(
        s.export_midi_clip(clip_req(9_999, vec![lane], x.clone())),
        "sequence",
        "clip seq",
    );
    for (start, end) in [
        (Some(Q), Some(Q)),
        (Some(2 * Q), Some(Q)),
        (Some(-(1 << 41)), None),
    ] {
        named(
            s.export_midi_clip(ClipExportRequestRow {
                start,
                end,
                ..clip_req(seq, vec![lane], x.clone())
            }),
            "range",
            "range",
        );
    }

    for tick in [1i64 << 41, -(1i64 << 41)] {
        named(
            s.import_midi(MidiImportRequestRow {
                tick,
                ..import_req(path.clone(), seq)
            }),
            "tick",
            "tick",
        );
    }
    named(
        s.import_midi(import_req(path.clone(), 9_999)),
        "sequence",
        "seq",
    );
    named(
        s.import_midi(MidiImportRequestRow {
            lane: Some(foreign),
            ..import_req(path.clone(), seq)
        }),
        "track",
        "a drop track of another sequence",
    );
    let map = |group: u8, channel: u8, dest: Option<u64>| ChannelMapRow {
        group,
        channel,
        dest,
    };
    for (m, word) in [
        (vec![map(0, 0, None), map(0, 0, Some(dest))], "twice"),
        (vec![map(0, 16, None)], "channel"),
        (vec![map(16, 0, None)], "group"),
        (vec![map(0, 0, Some(9_999))], "destination"),
    ] {
        named(
            s.import_midi(MidiImportRequestRow {
                channel_map: m,
                ..import_req(path.clone(), seq)
            }),
            word,
            word,
        );
    }
    // The checks refuse the same things.
    assert!(matches!(
        s.export_midi_check(SmfExportRequestRow {
            ppq: 0,
            ..smf_req(seq, String::new())
        }),
        Err(VizError::Model { .. })
    ));
    assert!(matches!(
        s.export_midi_clip_check(clip_req(seq, vec![], String::new())),
        Err(VizError::Model { .. })
    ));
    assert_eq!(s.debug_document_json(), before);
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        1,
        "only the fixture"
    );
    s.close().unwrap();
}

#[test]
fn busy_states_refuse() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let path = write(dir.path(), "two.mid", &type1(&[0, 1]));
    let s = Session::new_demo().unwrap();
    let seq = seq_named(&s, "Groove");
    let x = text(&dir.path().join("x.mid"));
    s.debug_hold_jobs(true).unwrap();
    let id = s.import_midi(import_req(path.clone(), seq)).unwrap();
    for r in [
        s.import_midi(import_req(path.clone(), seq)),
        s.export_midi(smf_req(seq, x.clone())),
        s.export_midi_clip(clip_req(seq, vec![s.lanes(seq).unwrap()[0].id], x.clone())),
    ] {
        assert!(matches!(r, Err(VizError::Busy { .. })), "{r:?}");
    }
    s.cancel_job(id).unwrap();
    s.debug_hold_jobs(false).unwrap();
    assert_eq!(wait_job(&s, id).state, JobStateRow::Cancelled);

    // Inside an open edit.
    s.begin_edit("Drag".into()).unwrap();
    let r = s.import_midi(import_req(path.clone(), seq));
    assert!(matches!(r, Err(VizError::Busy { .. })), "{r:?}");
    let r = s.export_midi(smf_req(seq, x.clone()));
    assert!(matches!(r, Err(VizError::Busy { .. })), "{r:?}");
    s.end_edit().unwrap();
    s.close().unwrap();

    // While recording (offline, an armed MIDI track).
    let mut b = Builder::new();
    let synth = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (rseq, keys) = b.sequence("S", "Keys", Some(synth)).unwrap();
    b.run(Command::Lane(LaneCmd::SetInput {
        seq: rseq,
        lane: keys,
        input: Some(LaneInput::Midi {
            port: Some("Keys".into()),
            channel: None,
        }),
    }))
    .unwrap();
    b.run(Command::Lane(LaneCmd::SetFlags {
        seq: rseq,
        lane: keys,
        mute: None,
        solo: None,
        arm: Some(true),
    }))
    .unwrap();
    let r = Session::from_document(b.finish()).unwrap();
    r.debug_set_trash_dir(Some(text(&dir.path().join("Trash"))));
    r.record(
        rseq.0,
        RecordSpecRow {
            replace: true,
            punch: None,
            looped: false,
            count_in_bars: 0,
            pre_roll_bars: 0,
            passes_as_takes: true,
        },
    )
    .unwrap();
    let until = Instant::now() + common::wait_limit();
    while !r.record_state().unwrap().recording {
        assert!(Instant::now() < until, "recording did not start");
        std::thread::sleep(Duration::from_millis(5));
    }
    let res = r.import_midi(import_req(path, rseq.0));
    assert!(matches!(res, Err(VizError::Busy { .. })), "{res:?}");
    let res = r.export_midi(smf_req(rseq.0, x));
    assert!(matches!(res, Err(VizError::Busy { .. })), "{res:?}");
    r.stop().unwrap();
    r.close().unwrap();
}

#[test]
fn damaged_file_fails_the_job_with_its_sentence() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let mut bytes = type1(&[0]);
    bytes.truncate(bytes.len() - 2);
    let path = write(dir.path(), "cut.mid", &bytes);
    let s = Session::new_demo().unwrap();
    let seq = seq_named(&s, "Groove");
    let before = s.debug_document_json();
    let id = s.import_midi(import_req(path.clone(), seq)).unwrap();
    let j = wait_job(&s, id);
    assert_eq!(j.state, JobStateRow::Failed);
    assert_eq!(
        j.message.as_deref(),
        Some("This MIDI file can't be read: track 2 ends in the middle of an event.")
    );
    assert_eq!(s.debug_document_json(), before);
    match s.midi_file_info(path) {
        Err(VizError::Model { message }) => assert!(message.contains("track 2"), "{message}"),
        other => panic!("{other:?}"),
    }
    s.close().unwrap();
}

#[test]
fn song_info_round_trips() {
    let _g = serial();
    let s = Session::new_demo().unwrap();
    let empty = s.song_info().unwrap();
    assert_eq!(empty, SongInfoRow::default());
    let info = SongInfoRow {
        title: Some("Night Drive".into()),
        artist: Some("The Band".into()),
        year: Some("2026".into()),
        comment: Some("two\nlines".into()),
        ..SongInfoRow::default()
    };
    let snap = s.set_song_info(info.clone()).unwrap();
    assert!(snap.dirty);
    assert_eq!(s.song_info().unwrap(), info);
    s.undo().unwrap();
    assert_eq!(s.song_info().unwrap(), empty);
    let before = s.debug_document_json();
    let r = s.set_song_info(SongInfoRow {
        title: Some("t".repeat(5_000)),
        ..SongInfoRow::default()
    });
    assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    let r = s.set_song_info(SongInfoRow {
        artist: Some("bad\u{0}name".into()),
        ..SongInfoRow::default()
    });
    assert!(matches!(r, Err(VizError::Model { .. })), "{r:?}");
    assert_eq!(s.debug_document_json(), before);
    s.close().unwrap();
}

#[test]
fn export_check_writes_nothing() {
    let _g = serial();
    let dir = tempfile::tempdir().unwrap();
    let s = Session::new_demo().unwrap();
    let seq = seq_named(&s, "Groove");
    let x = dir.path().join("x.mid");
    let r: ReportRow = s.export_midi_check(smf_req(seq, text(&x))).unwrap();
    assert!(r.summary.starts_with("Exported"), "{r:?}");
    assert!(!x.exists());
    // No path yet (F2), and an existing file without replace: the check
    // looks only at what is written.
    assert_eq!(s.export_midi_check(smf_req(seq, String::new())).unwrap(), r);
    std::fs::write(&x, b"mine").unwrap();
    assert_eq!(s.export_midi_check(smf_req(seq, text(&x))).unwrap(), r);
    let lane = s.lanes(seq).unwrap()[0].id;
    let c = s
        .export_midi_clip_check(clip_req(seq, vec![lane], String::new()))
        .unwrap();
    assert!(c.summary.starts_with("Exported"), "{c:?}");
    assert_eq!(std::fs::read(&x).unwrap(), b"mine");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    // Checks are not jobs and keep no report.
    assert_eq!(s.interchange_report().unwrap(), None);
    s.close().unwrap();
}

#[test]
fn interchange_results_answer_as_json() {
    let r = ControlResult::Interchange {
        command: "file.export_midi".into(),
        doc: "D".into(),
        window: None,
        undo_label: None,
        report: Some(ReportRow {
            summary: "Exported 1 track".into(),
            items: vec![vizdaw_ffi::ReportItemRow {
                kind: vizdaw_ffi::ReportKindRow::LeftOut,
                what: "Muted notes".into(),
                count: 2,
                details: vec!["Bass".into()],
            }],
        }),
        written: vec!["/tmp/x.mid".into()],
    };
    let v = result_json(&r);
    assert_eq!(v["command"], "file.export_midi");
    assert_eq!(v["doc"], "D");
    assert!(v["undo"].is_null());
    assert_eq!(v["report"]["summary"], "Exported 1 track");
    assert_eq!(v["report"]["items"][0]["kind"], "left_out");
    assert_eq!(v["report"]["items"][0]["count"], 2);
    assert_eq!(v["report"]["items"][0]["details"][0], "Bass");
    assert_eq!(v["written"][0], "/tmp/x.mid");
    let none = result_json(&ControlResult::Interchange {
        command: "file.interchange_report".into(),
        doc: "D".into(),
        window: Some(3),
        undo_label: Some("Import MIDI File".into()),
        report: None,
        written: vec![],
    });
    assert!(none["report"].is_null());
    assert_eq!(none["window"], 3);
    assert_eq!(none["undo"], "Import MIDI File");
}
