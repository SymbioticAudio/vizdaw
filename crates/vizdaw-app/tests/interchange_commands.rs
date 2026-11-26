//! The interchange File commands over the control channel (slice 7a
//! Task 3; Swift `InterchangeCommandsTests`, the request and refusal
//! rows): the requests as built, hostile arguments as `invalid_params`
//! before any session call, the Open, Save and Return dialogs, dry runs
//! as checks the app runs off the main thread, the reads that come
//! before a made project, the merge's rules and its date; and a real
//! MIDI export job.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use core_control::error::{ControlError, ErrorCode};
use core_control::registry::ArgValue;
use support::{FakeSession, demo_fake};
use vizdaw_app::editor::BlockRuleSetting;
use vizdaw_app::interchange_commands::{
    self as ic, DawprojectMode, DawprojectNext, DawprojectRead, FileCheck, InterchangeRequest,
    MidiIntoNewProject, check_merge_project, is_export_id_shape, merge_date, request,
};
use vizdaw_app::jobs::FileJob;
use vizdaw_app::mixer::commands::thrown;
use vizdaw_app::testing::JobRequest;
use vizdaw_app::{DocSession, DocumentModel, JobAnswer};
use vizdaw_ffi::{
    AafDepthRow, AafExportRequestRow, AafFpsRow, AafImportRequestRow, AafStereoRow, BitDepthRow,
    ChannelMapRow, ClipExportRequestRow, ContainerRow, DawprojectExportRequestRow,
    DawprojectInfoRow, DawprojectMergeRequestRow, ForDawCheckRow, ForDawRequestRow, JobKindRow,
    JobStateRow, MergeStepRow, MidiImportRequestRow, RegionGainRow, ReportRow, SmfExportRequestRow,
    VizError,
};

type Pairs = Vec<(&'static str, ArgValue)>;

fn t(s: &str) -> ArgValue {
    ArgValue::Text(s.into())
}

fn yes(b: bool) -> ArgValue {
    ArgValue::Flag(b)
}

fn n(i: i64) -> ArgValue {
    ArgValue::Int(i)
}

fn args(pairs: &[(&str, ArgValue)]) -> Vec<(String, ArgValue)> {
    pairs
        .iter()
        .map(|(n, v)| ((*n).to_owned(), v.clone()))
        .collect()
}

fn demo() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(demo_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    (f, m)
}

fn run(
    id: &str,
    pairs: &[(&str, ArgValue)],
    m: &DocumentModel,
) -> Result<InterchangeRequest, ControlError> {
    request(id, &args(pairs), m)
}

fn refused(id: &str, pairs: &[(&str, ArgValue)], m: &DocumentModel) -> ControlError {
    match run(id, pairs, m) {
        Err(e) => e,
        Ok(r) => panic!("{id} {pairs:?}: not refused: {r:?}"),
    }
}

fn job(r: Result<InterchangeRequest, ControlError>) -> FileJob {
    match r {
        Ok(InterchangeRequest::Job(j)) => j,
        other => panic!("not a job: {other:?}"),
    }
}

fn check(r: Result<InterchangeRequest, ControlError>) -> FileCheck {
    match r {
        Ok(InterchangeRequest::Check(c)) => c,
        other => panic!("not a check: {other:?}"),
    }
}

fn assert_dialog(e: &ControlError, dialog: &str, param: &str) {
    assert_eq!(e.code, ErrorCode::DialogRequired, "{e:?}");
    assert_eq!(e.dialog.as_deref(), Some(dialog), "{e:?}");
    assert!(e.message.contains(&format!("`{param}`")), "{e:?}");
}

fn assert_invalid(e: &ControlError, id: &str, param: &str) {
    assert_eq!(e.code, ErrorCode::InvalidParams, "{e:?}");
    assert!(e.message.contains(id), "{e:?}");
    assert!(e.message.contains(param), "{e:?}");
}

fn report(summary: &str) -> ReportRow {
    ReportRow {
        summary: summary.into(),
        items: vec![],
    }
}

// ---- MIDI ----

#[test]
fn import_midi_builds_the_request() {
    let (_, m) = demo();
    let seq = m.selected_seq().unwrap();
    let map = "[{\"group\":0,\"channel\":9,\"dest\":null}]";
    let j = job(run(
        ic::IMPORT_MIDI,
        &[
            ("path", t("/tmp/a.mid")),
            ("tick", n(7680)),
            ("lane", n(2)),
            ("channel_map", t(map)),
            ("mpe", yes(true)),
        ],
        &m,
    ));
    assert_eq!(j.kind(), JobKindRow::ImportMidi);
    assert_eq!(j.refusal_title(), "Import Failed");
    assert_eq!(
        j,
        FileJob::ImportMidi(MidiImportRequestRow {
            path: "/tmp/a.mid".into(),
            new_project: false,
            seq,
            lane: Some(2),
            tick: 7680,
            channel_map: vec![ChannelMapRow {
                group: 0,
                channel: 9,
                dest: None
            }],
            split_channels: None,
            mpe: true,
            use_tempo: false,
        })
    );
    let j = job(run(
        ic::IMPORT_MIDI,
        &[
            ("path", t("/tmp/b.mid")),
            ("seq", n(1)),
            ("split_channels", yes(false)),
            ("use_tempo", yes(true)),
            (
                "channel_map",
                t("[{\"group\":15,\"channel\":0,\"dest\":4},{\"group\":1,\"channel\":0}]"),
            ),
        ],
        &m,
    ));
    assert_eq!(
        j,
        FileJob::ImportMidi(MidiImportRequestRow {
            path: "/tmp/b.mid".into(),
            new_project: false,
            seq: 1,
            lane: None,
            tick: 0,
            channel_map: vec![
                ChannelMapRow {
                    group: 15,
                    channel: 0,
                    dest: Some(4)
                },
                ChannelMapRow {
                    group: 1,
                    channel: 0,
                    dest: None
                },
            ],
            split_channels: Some(false),
            mpe: false,
            use_tempo: true,
        })
    );
}

#[test]
fn import_midi_checks_its_arguments() {
    let (f, m) = demo();
    let one = "{\"group\":0,\"channel\":9,\"dest\":null}";
    let many = format!(
        "[{}]",
        (0..300)
            .map(|i| format!(
                "{{\"group\":{},\"channel\":{},\"dest\":null}}",
                i % 16,
                i / 16 % 16
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let deep = format!("[{}", "[".repeat(5000));
    let maps: Vec<String> = vec![
        "not json".into(),
        "{\"group\":0}".into(),
        "7".into(),
        "[7]".into(),
        format!("[{one},{one}]"),
        "[{\"group\":0,\"channel\":16,\"dest\":null}]".into(),
        "[{\"group\":-1,\"channel\":0,\"dest\":null}]".into(),
        "[{\"group\":0,\"channel\":0,\"dest\":1.5}]".into(),
        "[{\"group\":0,\"channel\":0,\"dest\":\"3\"}]".into(),
        "[{\"group\":0,\"channel\":0,\"dest\":-3}]".into(),
        "[{\"group\":0,\"channel\":0,\"dest\":true}]".into(),
        "[{\"group\":true,\"channel\":0,\"dest\":null}]".into(),
        "[{\"group\":0.5,\"channel\":0,\"dest\":null}]".into(),
        "[{\"group\":0,\"dest\":null}]".into(),
        "[{\"group\":0,\"channel\":0,\"x\":1}]".into(),
        "".into(),
        "[{\"group\":0,\"channel\":0,\"dest\":1e400}]".into(),
        many,
        deep,
    ];
    for map in &maps {
        let e = refused(
            ic::IMPORT_MIDI,
            &[("path", t("/tmp/a.mid")), ("channel_map", t(map))],
            &m,
        );
        assert_invalid(&e, "file.import_midi", "channel_map");
    }
    let exports: Vec<(&str, Pairs, &str)> = vec![
        (ic::EXPORT_MIDI, vec![("lanes", t("1,,2"))], "lanes"),
        (ic::EXPORT_MIDI, vec![("lanes", t("a"))], "lanes"),
        (ic::EXPORT_MIDI, vec![("lanes", t("1,1"))], "lanes"),
        (ic::EXPORT_MIDI, vec![("lanes", t("-1"))], "lanes"),
        (ic::EXPORT_MIDI, vec![("lanes", t(""))], "lanes"),
        (
            ic::EXPORT_MIDI,
            vec![("lanes", t("99999999999999999999999"))],
            "lanes",
        ),
        (ic::EXPORT_MIDI, vec![("lanes", t("١"))], "lanes"),
        (ic::EXPORT_MIDI, vec![("seq", n(-1))], "seq"),
        (ic::EXPORT_MIDI, vec![("ppq", t("96"))], "ppq"),
        (ic::EXPORT_CLIP, vec![("lanes", t("1,,2"))], "lanes"),
        (ic::EXPORT_CLIP, vec![("lanes", t("3,3"))], "lanes"),
        (ic::EXPORT_CLIP, vec![("start", n(0))], "start"),
        (
            ic::EXPORT_CLIP,
            vec![("start", n(100)), ("end", n(100))],
            "start",
        ),
        (
            ic::EXPORT_CLIP,
            vec![("start", n(200)), ("end", n(100))],
            "start",
        ),
    ];
    for (cmd, a, name) in &exports {
        for extra in [("path", t("/tmp/a.mid")), ("dry_run", yes(true))] {
            let mut all = a.clone();
            all.push(extra);
            let e = refused(cmd, &all, &m);
            assert_invalid(&e, cmd, name);
        }
    }
    assert!(f.jobs_started().is_empty(), "nothing reached the core");
    assert_eq!(f.reads("export_midi_check"), 0);
}

#[test]
fn the_midi_refusals_say_why_as_swift_does() {
    let (_, m) = demo();
    let e = |id: &str, a: &[(&str, ArgValue)]| refused(id, a, &m).message;
    let p = ("path", t("/tmp/a.mid"));
    assert_eq!(
        e(ic::IMPORT_MIDI, &[p.clone(), ("channel_map", t("x"))]),
        "file.import_midi: `channel_map` is a JSON array like [{\"group\":0,\"channel\":9,\"dest\":3}]"
    );
    assert_eq!(
        e(
            ic::IMPORT_MIDI,
            &[
                p.clone(),
                (
                    "channel_map",
                    t("[{\"group\":2,\"channel\":3},{\"group\":2,\"channel\":3}]")
                )
            ]
        ),
        "file.import_midi: `channel_map` lists group 2 channel 3 twice"
    );
    assert_eq!(
        e(
            ic::IMPORT_MIDI,
            &[
                p.clone(),
                ("channel_map", t("[{\"group\":16,\"channel\":0}]"))
            ]
        ),
        "file.import_midi: `channel_map`: group and channel are 0 to 15"
    );
    assert_eq!(
        e(
            ic::IMPORT_MIDI,
            &[
                p.clone(),
                (
                    "channel_map",
                    t("[{\"group\":0,\"channel\":0,\"dest\":-1}]")
                )
            ]
        ),
        "file.import_midi: `channel_map`: `dest` is a destination ID or null"
    );
    assert_eq!(
        e(ic::EXPORT_MIDI, &[p.clone(), ("lanes", t("4,4"))]),
        "file.export_midi: `lanes` lists track 4 twice"
    );
    assert_eq!(
        e(ic::EXPORT_MIDI, &[p.clone(), ("lanes", t("x"))]),
        "file.export_midi: `lanes` is track IDs separated by commas, e.g. \"1,2\""
    );
    assert_eq!(
        e(ic::EXPORT_MIDI, &[p.clone(), ("ppq", t("1"))]),
        "file.export_midi: `ppq` is 480, 960 or 15360"
    );
    assert_eq!(
        e(ic::EXPORT_MIDI, &[p.clone(), ("seq", n(-1))]),
        "file.export_midi: `seq` is a sequence ID"
    );
    assert_eq!(
        e(ic::EXPORT_CLIP, &[p.clone(), ("start", n(1))]),
        "file.export_midi_clip: `start` needs `end`"
    );
    assert_eq!(
        e(
            ic::EXPORT_CLIP,
            &[p.clone(), ("start", n(1)), ("end", n(1))]
        ),
        "file.export_midi_clip: `start` must be before `end`"
    );
    assert_eq!(
        e(ic::IMPORT_MIDI, &[p.clone(), ("lane", n(-1))]),
        "file.import_midi: `lane` is a track ID"
    );
    // A wrong type and an unknown parameter are refused too.
    assert_invalid(
        &refused(ic::EXPORT_MIDI, &[p.clone(), ("dry_run", t("yes"))], &m),
        "file.export_midi",
        "dry_run",
    );
    assert_invalid(
        &refused(ic::EXPORT_MIDI, &[p.clone(), ("bogus", yes(true))], &m),
        "file.export_midi",
        "bogus",
    );
}

#[test]
fn dry_runs_are_checks_that_write_nothing() {
    let (f, m) = demo();
    let seq = m.selected_seq().unwrap();
    f.set_check_report("export_midi_check", report("Would export 2 tracks"));
    f.set_check_report("export_midi_clip_check", report("Would export 2 tracks"));
    for cmd in [ic::EXPORT_MIDI, ic::EXPORT_CLIP] {
        for with_path in [false, true] {
            let mut a = vec![("dry_run", yes(true))];
            if with_path {
                a.push(("path", t("/tmp/a.mid")));
            }
            let before = f.reads("export_midi_check") + f.reads("export_midi_clip_check");
            let c = check(run(cmd, &a, &m));
            // Nothing ran yet: the app runs the check off the main thread.
            assert_eq!(
                f.reads("export_midi_check") + f.reads("export_midi_clip_check"),
                before
            );
            let session: Arc<dyn DocSession> = m.session().clone();
            let got = std::thread::spawn(move || c.run(session.as_ref()))
                .join()
                .unwrap()
                .unwrap();
            assert_eq!(got.summary, "Would export 2 tracks");
        }
    }
    assert_eq!(f.reads("export_midi_check"), 2);
    assert_eq!(f.reads("export_midi_clip_check"), 2);
    assert!(f.jobs_started().is_empty(), "nothing was written");
    // The defaults: the shown sequence, type 1, 960 ppq, every track.
    let FileCheck::ExportMidi(smf) = check(run(ic::EXPORT_MIDI, &[("dry_run", yes(true))], &m))
    else {
        panic!("not an SMF check")
    };
    assert_eq!(
        smf,
        SmfExportRequestRow {
            seq,
            lanes: vec![30, 31],
            path: String::new(),
            replace: false,
            type0: false,
            ppq: 960,
            mpe: false,
        }
    );
    let FileCheck::ExportMidiClip(clip) =
        check(run(ic::EXPORT_CLIP, &[("dry_run", yes(true))], &m))
    else {
        panic!("not a clip check")
    };
    assert_eq!(clip.lanes, vec![30, 31]);
    assert_eq!((clip.start, clip.end), (None, None));
    // A refused check answers its reason.
    let c = check(run(ic::EXPORT_MIDI, &[("dry_run", yes(true))], &m));
    f.fail_next(
        "export_midi_check",
        VizError::Model {
            message: "no tracks".into(),
        },
    );
    let e = thrown(&c.run(m.session().as_ref()).unwrap_err(), ic::CHECK_FAILED);
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "Export Check Failed: no tracks");
}

#[test]
fn exports_without_a_path_ask_for_save() {
    let (f, m) = demo();
    for cmd in [ic::EXPORT_MIDI, ic::EXPORT_CLIP] {
        assert_dialog(&refused(cmd, &[], &m), "Save", "path");
        assert_dialog(
            &refused(cmd, &[("dry_run", yes(false))], &m),
            "Save",
            "path",
        );
    }
    assert_dialog(&refused(ic::IMPORT_MIDI, &[], &m), "Open", "path");
    assert!(f.jobs_started().is_empty());
}

#[test]
fn exports_build_their_requests() {
    let (_, m) = demo();
    let seq = m.selected_seq().unwrap();
    let smf = job(run(
        ic::EXPORT_MIDI,
        &[
            ("path", t("/tmp/a.mid")),
            ("type", t("0")),
            ("ppq", t("15360")),
            ("lanes", t("31,30")),
            ("mpe", yes(true)),
            ("replace", yes(true)),
        ],
        &m,
    ));
    assert_eq!(
        smf,
        FileJob::ExportMidi(SmfExportRequestRow {
            seq,
            lanes: vec![31, 30],
            path: "/tmp/a.mid".into(),
            replace: true,
            type0: true,
            ppq: 15360,
            mpe: true,
        })
    );
    assert_eq!(smf.kind(), JobKindRow::ExportMidi);
    assert_eq!(smf.refusal_title(), "Export Failed");
    let clip = job(run(
        ic::EXPORT_CLIP,
        &[
            ("path", t("/tmp/dir")),
            ("start", n(0)),
            ("end", n(960)),
            ("seq", n(2)),
        ],
        &m,
    ));
    assert_eq!(
        clip,
        FileJob::ExportMidiClip(ClipExportRequestRow {
            seq: 2,
            lanes: vec![20],
            path: "/tmp/dir".into(),
            replace: false,
            start: Some(0),
            end: Some(960),
        })
    );
    assert_eq!(clip.kind(), JobKindRow::ExportMidi);
}

#[test]
fn a_new_project_import_reads_the_file_first() {
    let (f, m) = demo();
    let r = run(
        ic::IMPORT_MIDI,
        &[("path", t("/tmp/a.mid")), ("new_project", yes(true))],
        &m,
    )
    .unwrap();
    let InterchangeRequest::MidiIntoNewProject(made) = r else {
        panic!("not a new-project import: {r:?}")
    };
    assert!(made.pick_seq);
    assert!(made.request.new_project);
    assert_eq!(made.request.seq, 0, "replaced once the project exists");
    assert_eq!(made.request_for(Some(9)).seq, 9);
    assert_eq!(ic::MIDI_PROJECT_RATE, 48_000);
    assert_eq!(f.reads("midi_file_info"), 0, "the app reads it off-main");
    // A `seq` given is kept.
    let InterchangeRequest::MidiIntoNewProject(kept) = run(
        ic::IMPORT_MIDI,
        &[
            ("path", t("/tmp/a.mid")),
            ("new_project", yes(true)),
            ("seq", n(4)),
        ],
        &m,
    )
    .unwrap() else {
        panic!("not a new-project import")
    };
    assert_eq!(
        kept,
        MidiIntoNewProject {
            request: MidiImportRequestRow {
                seq: 4,
                ..made.request.clone()
            },
            pick_seq: false,
        }
    );
    assert_eq!(kept.request_for(Some(9)).seq, 4);
    // A drop track cannot exist in a new project.
    let l = refused(
        ic::IMPORT_MIDI,
        &[
            ("path", t("/tmp/a.mid")),
            ("new_project", yes(true)),
            ("lane", n(2)),
        ],
        &m,
    );
    assert_eq!(
        l.message,
        "file.import_midi: `lane` names a track of this project; a new project has none"
    );
    // A new project needs no sequence shown in this one.
    let bare = DocumentModel::new(Arc::new(FakeSession::new(vec![]))).unwrap();
    assert!(bare.selected_seq().is_none());
    assert!(matches!(
        run(
            ic::IMPORT_MIDI,
            &[("path", t("/tmp/a.mid")), ("new_project", yes(true))],
            &bare
        ),
        Ok(InterchangeRequest::MidiIntoNewProject(_))
    ));
    let none = refused(ic::IMPORT_MIDI, &[("path", t("/tmp/a.mid"))], &bare);
    assert_eq!(none.code, ErrorCode::Failed);
    assert_eq!(none.message, "file.import_midi: no sequence is shown");
}

#[test]
fn import_refusals_never_reach_the_session() {
    let (f, mut m) = demo();
    assert!(m.begin_gesture("Move", vec![]));
    let g = refused(ic::IMPORT_MIDI, &[("path", t("/tmp/a.mid"))], &m);
    assert_eq!(g.code, ErrorCode::Unavailable);
    assert_eq!(g.message, "Finish the drag first.");
    // A new project is not this one: the open drag does not matter.
    assert!(
        run(
            ic::IMPORT_MIDI,
            &[("path", t("/tmp/a.mid")), ("new_project", yes(true))],
            &m
        )
        .is_ok()
    );
    // A track read that fails is the failure, not an empty list.
    f.set_call_error(
        "lanes",
        Some(VizError::Model {
            message: "bad seq".into(),
        }),
    );
    let b = refused(
        ic::EXPORT_CLIP,
        &[("path", t("/tmp/d")), ("seq", n(99))],
        &m,
    );
    assert_eq!(b.code, ErrorCode::Failed);
    assert_eq!(b.message, "Tracks Unavailable: bad seq");
    assert!(f.jobs_started().is_empty());
}

#[test]
fn the_report_command_is_a_read() {
    let (f, m) = demo();
    assert_eq!(
        run(ic::REPORT, &[], &m).unwrap(),
        InterchangeRequest::Report
    );
    assert_eq!(f.reads("interchange_report"), 0);
    assert!(f.jobs_started().is_empty());
    let u = refused("file.nope", &[], &m);
    assert_eq!(u.code, ErrorCode::UnknownId);
    assert_eq!(u.message, "VizDAW has no handler for file.nope.");
}

// ---- for another DAW ----

#[test]
fn for_daw_builds_the_request() {
    let (_, m) = demo();
    let seq = m.selected_seq().unwrap();
    let j = job(run(
        ic::EXPORT_FOR_DAW,
        &[
            ("path", t("/tmp/out")),
            ("strips", t("3,5")),
            ("container", t("aiff")),
            ("depth", t("16")),
            ("rate", n(44_100)),
            ("add_to_project", yes(true)),
        ],
        &m,
    ));
    assert_eq!(j.kind(), JobKindRow::ExportProject);
    assert_eq!(j.refusal_title(), "Export Failed");
    assert_eq!(
        j,
        FileJob::ForDaw(ForDawRequestRow {
            seq,
            strips: Some(vec![3, 5]),
            container: ContainerRow::Aiff,
            depth: BitDepthRow::Int16,
            rate: 44_100,
            dither: true,
            path: "/tmp/out".into(),
            add_to_project: true,
        })
    );
    // The defaults: the project's rate and depth, WAV, the usual strips.
    let FileJob::ForDaw(d) = job(run(ic::EXPORT_FOR_DAW, &[("path", t("/tmp/out"))], &m)) else {
        panic!()
    };
    assert_eq!(
        d,
        ForDawRequestRow {
            seq,
            strips: None,
            container: ContainerRow::Wav,
            depth: BitDepthRow::Int24,
            rate: 48_000,
            dither: false,
            path: "/tmp/out".into(),
            add_to_project: false,
        }
    );
    let FileJob::ForDaw(none) = job(run(
        ic::EXPORT_FOR_DAW,
        &[
            ("path", t("/tmp/out")),
            ("strips", t("none")),
            ("depth", t("32f")),
            ("dither", yes(true)),
            ("seq", n(2)),
        ],
        &m,
    )) else {
        panic!()
    };
    assert_eq!(none.strips, Some(vec![]));
    assert_eq!(none.depth, BitDepthRow::Float32);
    assert!(!none.dither, "never at 32f");
    assert_eq!(none.seq, 2);
    let FileJob::ForDaw(off) = job(run(
        ic::EXPORT_FOR_DAW,
        &[
            ("path", t("/tmp/out")),
            ("depth", t("16")),
            ("dither", yes(false)),
        ],
        &m,
    )) else {
        panic!()
    };
    assert!(!off.dither);
}

#[test]
fn for_daw_checks_its_arguments() {
    let (f, m) = demo();
    for bad in [
        "1,,2",
        "a",
        "1,1",
        "",
        "1,",
        ",1",
        "-1",
        "1 2",
        "none,1",
        "99999999999999999999",
    ] {
        let e = refused(
            ic::EXPORT_FOR_DAW,
            &[("path", t("/tmp/out")), ("strips", t(bad))],
            &m,
        );
        assert_invalid(&e, "file.export_for_daw", "strips");
    }
    assert_eq!(
        refused(
            ic::EXPORT_FOR_DAW,
            &[("path", t("/tmp/out")), ("strips", t("2,2"))],
            &m
        )
        .message,
        "file.export_for_daw: `strips` lists strip 2 twice"
    );
    assert_eq!(
        refused(
            ic::EXPORT_FOR_DAW,
            &[("path", t("/tmp/out")), ("strips", t("x"))],
            &m
        )
        .message,
        "file.export_for_daw: `strips` is strip IDs separated by commas, e.g. \"1,2\", or none"
    );
    // The same refusals on a dry run.
    let dry = refused(
        ic::EXPORT_FOR_DAW,
        &[("dry_run", yes(true)), ("strips", t("1,1"))],
        &m,
    );
    assert_eq!(dry.code, ErrorCode::InvalidParams);
    // A bad sequence or rate.
    for bad in [("seq", n(-1)), ("rate", n(7_999)), ("rate", n(1 << 40))] {
        let e = refused(
            ic::EXPORT_FOR_DAW,
            &[("path", t("/tmp/out")), bad.clone()],
            &m,
        );
        assert_invalid(&e, "file.export_for_daw", bad.0);
    }
    assert_eq!(
        refused(
            ic::EXPORT_FOR_DAW,
            &[("path", t("/tmp/out")), ("rate", n(7_999))],
            &m
        )
        .message,
        "file.export_for_daw: `rate` is 8000 to 384000 Hz"
    );
    assert!(f.jobs_started().is_empty());
    assert_eq!(f.reads("export_for_daw_check"), 0);
}

#[test]
fn for_daw_dry_run_is_a_check_with_its_path() {
    let (f, m) = demo();
    f.set_for_daw_check(ForDawCheckRow {
        report: report("Would export 3 files"),
        folder: "Song for another DAW".into(),
        files: vec!["a.mid".into()],
        strips: vec![],
        start: "Every file starts at bar 1.".into(),
        frames: 0,
        warning: None,
    });
    for (with_path, path) in [(false, ""), (true, "/tmp/out")] {
        let mut a = vec![("dry_run", yes(true))];
        if with_path {
            a.push(("path", t("/tmp/out")));
        }
        let c = check(run(ic::EXPORT_FOR_DAW, &a, &m));
        let FileCheck::ForDaw(req) = &c else { panic!() };
        assert_eq!(req.path, path);
        assert_eq!(
            c.run(m.session().as_ref()).unwrap(),
            report("Would export 3 files")
        );
    }
    assert_eq!(f.reads("export_for_daw_check"), 2);
    assert!(f.jobs_started().is_empty());
}

#[test]
fn for_daw_without_a_path_asks_for_save() {
    let (f, m) = demo();
    for a in [
        vec![],
        vec![("dry_run", yes(false))],
        vec![("container", t("caf"))],
        vec![("path", t(""))],
    ] {
        assert_dialog(&refused(ic::EXPORT_FOR_DAW, &a, &m), "Save", "path");
    }
    assert!(f.jobs_started().is_empty());
}

// ---- DAWproject ----

fn info(from_vizdaw: bool, rate: u32) -> DawprojectInfoRow {
    DawprojectInfoRow {
        sample_rate: rate,
        tracks: 2,
        audio_files: 0,
        from_vizdaw,
        export_id: from_vizdaw.then(|| EID.to_owned()),
        project_id: from_vizdaw.then(|| PID.to_owned()),
    }
}

const PID: &str = "0f8b6f8e-3c1a-4d2e-9b7a-5e6f7a8b9c0d";
const EID: &str = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";

fn read(pairs: &[(&str, ArgValue)], m: &DocumentModel) -> DawprojectRead {
    match run(ic::IMPORT_DAWPROJECT, pairs, m) {
        Ok(InterchangeRequest::ReadDawproject(r)) => r,
        other => panic!("not a read: {other:?}"),
    }
}

#[test]
fn import_dawproject_reads_the_file_then_makes_a_project_at_its_rate() {
    let (_, m) = demo();
    let r = read(&[("path", t("/tmp/a.dawproject"))], &m);
    assert_eq!(
        r,
        DawprojectRead {
            path: "/tmp/a.dawproject".into(),
            mode: None,
            project: None,
            snapshot: None,
            preview: false,
            merge_only: None,
        }
    );
    assert_eq!(
        r.after_read(&info(false, 44_100)).unwrap(),
        DawprojectNext::NewProject {
            path: "/tmp/a.dawproject".into(),
            rate: 44_100
        }
    );
    let j = ic::dawproject_import_job("/tmp/a.dawproject");
    assert_eq!(j.kind(), JobKindRow::ImportProject);
    assert_eq!(j.refusal_title(), "Import Failed");
}

#[test]
fn import_dawproject_without_a_path_asks_for_open() {
    let (_, m) = demo();
    for a in [vec![], vec![("mode", t("new"))], vec![("path", t(""))]] {
        assert_dialog(&refused(ic::IMPORT_DAWPROJECT, &a, &m), "Open", "path");
    }
}

#[test]
fn export_dawproject_without_a_path_asks_for_save() {
    let (f, m) = demo();
    for a in [vec![], vec![("dry_run", yes(false))], vec![("path", t(""))]] {
        assert_dialog(&refused(ic::EXPORT_DAWPROJECT, &a, &m), "Save", "path");
    }
    assert!(f.jobs_started().is_empty());
}

#[test]
fn export_dawproject_dry_run_is_a_check() {
    let (f, m) = demo();
    f.set_check_report("export_dawproject_check", report("Would leave out 1"));
    let c = check(run(ic::EXPORT_DAWPROJECT, &[("dry_run", yes(true))], &m));
    let FileCheck::ExportDawproject(req) = &c else {
        panic!()
    };
    assert_eq!(req.path, "");
    assert_eq!(f.reads("export_dawproject_check"), 0);
    assert_eq!(
        c.run(m.session().as_ref()).unwrap(),
        report("Would leave out 1")
    );
    f.fail_next(
        "export_dawproject_check",
        VizError::Model {
            message: "no room".into(),
        },
    );
    let e = thrown(&c.run(m.session().as_ref()).unwrap_err(), ic::CHECK_FAILED);
    assert_eq!(
        (e.code, e.message.as_str()),
        (ErrorCode::Failed, "Export Check Failed: no room")
    );
    assert!(f.jobs_started().is_empty());
}

#[test]
fn export_dawproject_builds_the_request() {
    let (_, mut m) = demo();
    m.update_layout(|l| l.block_rule = BlockRuleSetting::FixedBars { bars: 2 });
    let j = job(run(
        ic::EXPORT_DAWPROJECT,
        &[
            ("path", t("/tmp/a.dawproject")),
            ("replace", yes(true)),
            ("region_gain", t("leave_out")),
            ("seq", n(2)),
        ],
        &m,
    ));
    assert_eq!(j.kind(), JobKindRow::ExportDawproject);
    assert_eq!(
        j,
        FileJob::ExportDawproject(DawprojectExportRequestRow {
            seq: 2,
            path: "/tmp/a.dawproject".into(),
            replace: true,
            region_gain: RegionGainRow::LeaveOut,
            blocks: BlockRuleSetting::FixedBars { bars: 2 }.rule(),
        })
    );
    let FileJob::ExportDawproject(d) = job(run(
        ic::EXPORT_DAWPROJECT,
        &[("path", t("/tmp/b.dawproject"))],
        &m,
    )) else {
        panic!()
    };
    assert!(!d.replace);
    assert_eq!(d.region_gain, RegionGainRow::Apply);
    assert_eq!(Some(d.seq), m.selected_seq());
    // A `seq` that is no ID never reaches the session, dry run or not.
    for dry in [false, true] {
        let e = refused(
            ic::EXPORT_DAWPROJECT,
            &[
                ("path", t("/tmp/a.dawproject")),
                ("seq", n(-1)),
                ("dry_run", yes(dry)),
            ],
            &m,
        );
        assert_invalid(&e, "file.export_dawproject", "seq");
    }
}

/// Hostile destinations reach the real core, which refuses each with
/// its own sentence before any job starts (Swift's hostile test: the
/// parse passes them on as given).
#[test]
fn hostile_dawproject_destinations_are_the_core_s_to_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().display().to_string();
    let session: Arc<dyn DocSession> = vizdaw_ffi::session::Session::new_demo().unwrap();
    let m = DocumentModel::new(session).unwrap();
    let long = format!("{base}/{}.dawproject", "a".repeat(5_000));
    let mut sentences = std::collections::BTreeSet::new();
    for p in [
        "relative.dawproject".to_owned(),
        long,
        format!("{base}/a\0.dawproject"),
        base.clone(),
        format!("{base}/a.mid"),
    ] {
        let j = job(run(ic::EXPORT_DAWPROJECT, &[("path", t(&p))], &m));
        let FileJob::ExportDawproject(r) = &j else {
            panic!()
        };
        assert_eq!(r.path, p, "sent as given");
        let e = j.start(m.session().as_ref()).unwrap_err();
        let c = thrown(&e, j.refusal_title());
        assert_eq!(c.code, ErrorCode::Failed, "{c:?}");
        sentences.insert(c.message);
    }
    assert!(
        sentences.len() > 1,
        "the core's own sentences: {sentences:?}"
    );
    let written: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert!(written.is_empty(), "nothing written");
    // An unknown sequence is the core's refusal too.
    let j = job(run(
        ic::EXPORT_DAWPROJECT,
        &[
            ("path", t(&format!("{base}/a.dawproject"))),
            ("seq", n(9_999)),
        ],
        &m,
    ));
    assert!(j.start(m.session().as_ref()).is_err());
    assert!(!dir.path().join("a.dawproject").exists());
}

#[test]
fn from_vizdaw_without_a_mode_asks_for_return() {
    let (_, m) = demo();
    let r = read(&[("path", t("/tmp/back.dawproject"))], &m);
    let d = r.after_read(&info(true, 48_000)).unwrap_err();
    assert_dialog(&d, "Return", "mode");
    assert_eq!(
        d.message,
        "this would open the \"Return\" dialog; pass `mode` instead"
    );
}

#[test]
fn mode_new_still_makes_a_project() {
    let (_, m) = demo();
    let r = read(
        &[("path", t("/tmp/back.dawproject")), ("mode", t("new"))],
        &m,
    );
    assert_eq!(r.mode, Some(DawprojectMode::New));
    assert_eq!(
        r.after_read(&info(true, 48_000)).unwrap(),
        DawprojectNext::NewProject {
            path: "/tmp/back.dawproject".into(),
            rate: 48_000
        }
    );
}

#[test]
fn a_merge_goes_into_its_project_with_its_snapshot_and_date() {
    let (_, m) = demo();
    let r = read(
        &[("path", t("/tmp/back.dawproject")), ("mode", t("merge"))],
        &m,
    );
    assert_eq!(
        r.after_read(&info(true, 48_000)).unwrap(),
        DawprojectNext::Merge { project: None }
    );
    let j = r.merge_job("2026-11-23".into());
    assert_eq!(j.kind(), JobKindRow::MergeDawproject);
    assert_eq!(j.refusal_title(), "DAWproject Merge Failed");
    assert_eq!(
        j,
        FileJob::MergeDawproject(DawprojectMergeRequestRow {
            path: "/tmp/back.dawproject".into(),
            snapshot: None,
            step: MergeStepRow::Apply,
            date: "2026-11-23".into(),
        })
    );
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("Song.vizdaw");
    std::fs::create_dir(&project).unwrap();
    let p = project.display().to_string();
    let given = read(
        &[
            ("path", t("/tmp/back.dawproject")),
            ("mode", t("merge")),
            ("project", t(&p)),
            ("snapshot", t(EID)),
            ("preview", yes(true)),
        ],
        &m,
    );
    assert_eq!(
        given.after_read(&info(true, 48_000)).unwrap(),
        DawprojectNext::Merge {
            project: Some(p.clone())
        }
    );
    let FileJob::MergeDawproject(req) = given.merge_job("2026-01-02".into()) else {
        panic!()
    };
    assert_eq!(req.snapshot.as_deref(), Some(EID));
    assert_eq!(req.step, MergeStepRow::Preview);
    assert!(check_merge_project(&p, false).is_ok());
    let none = ic::merge_target_dialog();
    assert_dialog(&none, "Open", "project");
}

#[test]
fn a_merge_refuses_hostile_arguments() {
    let (_, m) = demo();
    let file = ("path", t("/tmp/back.dawproject"));
    let merge = ("mode", t("merge"));
    // Not from VizDAW.
    let r = read(&[file.clone(), merge.clone()], &m);
    let e = r.after_read(&info(false, 48_000)).unwrap_err();
    assert_invalid(&e, "file.import_dawproject", "`mode`");
    assert_eq!(
        e.message,
        "file.import_dawproject: `mode` merge needs a file that came from VizDAW."
    );
    // The project: already open, a link, not a folder, missing.
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("Open.vizdaw");
    std::fs::create_dir(&package).unwrap();
    let link = dir.path().join("Link.vizdaw");
    std::os::unix::fs::symlink(&package, &link).unwrap();
    let plain = dir.path().join("x.vizdaw");
    std::fs::write(&plain, "not a package").unwrap();
    let open = check_merge_project(&package.display().to_string(), true).unwrap_err();
    assert_eq!(
        open.message,
        "file.import_dawproject: `project` is already open; leave out project to merge into it."
    );
    for p in [&link, &plain, &dir.path().join("gone.vizdaw")] {
        let e = check_merge_project(&p.display().to_string(), false).unwrap_err();
        assert_invalid(&e, "file.import_dawproject", "`project`");
        assert_eq!(
            e.message,
            "file.import_dawproject: `project` is not a VizDAW project."
        );
    }
    // A snapshot that is not an export ID's shape.
    for bad in [
        "x".to_owned(),
        String::new(),
        "0F8B6F8E-3C1A-4D2E-9B7A-5E6F7A8B9C0D".into(),
        format!("{PID} "),
        format!("{PID}0"),
        "../../../../../../../../../../../../../etc/passwd".into(),
        "0f8b6f8e\u{0}3c1a-4d2e-9b7a-5e6f7a8b9c0d".into(),
        "0f8b6f8e-3c1a-4d2e-9b7a-5e6f7a8b9c0\u{0301}".into(),
        "é".repeat(36),
    ] {
        assert!(!is_export_id_shape(&bad), "{bad:?}");
        let e = refused(
            ic::IMPORT_DAWPROJECT,
            &[file.clone(), merge.clone(), ("snapshot", t(&bad))],
            &m,
        );
        assert_invalid(&e, "file.import_dawproject", "`snapshot`");
        assert_eq!(
            e.message,
            "file.import_dawproject: `snapshot` is an export ID (36 lowercase hex digits and hyphens)."
        );
    }
    assert!(is_export_id_shape(PID));
    // Merge parameters with mode new: refused before the file is read.
    for extra in [
        ("project", t(&package.display().to_string())),
        ("snapshot", t(EID)),
        ("preview", yes(true)),
    ] {
        let e = refused(
            ic::IMPORT_DAWPROJECT,
            &[file.clone(), ("mode", t("new")), extra.clone()],
            &m,
        );
        assert_eq!(e.code, ErrorCode::InvalidParams);
        assert_eq!(
            e.message,
            format!("file.import_dawproject: `{}` is for mode merge.", extra.0)
        );
    }
    // Merge parameters with no mode, on a file not from VizDAW: refused
    // once it is read.
    let r = read(&[file.clone(), ("preview", yes(false))], &m);
    assert_eq!(r.merge_only, Some("preview"));
    let e = r.after_read(&info(false, 48_000)).unwrap_err();
    assert_eq!(
        e.message,
        "file.import_dawproject: `preview` is for mode merge."
    );
    // On a file from VizDAW the Return question comes first.
    assert_eq!(
        r.after_read(&info(true, 48_000)).unwrap_err().code,
        ErrorCode::DialogRequired
    );
}

#[test]
fn an_unknown_mode_and_a_relative_project_are_refused() {
    let (_, m) = demo();
    let file = ("path", t("/tmp/back.dawproject"));
    for bad in [
        "fuse",
        "",
        "New",
        "MERGE",
        " merge",
        "new\u{0}",
        "merge\u{0301}",
    ] {
        let e = refused(ic::IMPORT_DAWPROJECT, &[file.clone(), ("mode", t(bad))], &m);
        assert_eq!(e.code, ErrorCode::InvalidParams, "{bad:?}");
        assert_eq!(e.message, "file.import_dawproject: `mode` is new or merge.");
    }
    for rel in [
        "Open.vizdaw",
        "./Open.vizdaw",
        "../x/Open.vizdaw",
        "~/Open.vizdaw",
        "",
    ] {
        let e = refused(
            ic::IMPORT_DAWPROJECT,
            &[file.clone(), ("mode", t("merge")), ("project", t(rel))],
            &m,
        );
        assert_eq!(e.code, ErrorCode::InvalidParams, "{rel:?}");
        assert_eq!(
            e.message,
            "file.import_dawproject: `project` must be an absolute path."
        );
    }
}

#[test]
fn today_is_local_iso() {
    // 2026-11-13 20:00 UTC.
    let instant = 1_794_600_000;
    assert_eq!(merge_date(instant, 0), "2026-11-13");
    assert_eq!(merge_date(instant, 14 * 3600), "2026-11-14");
    assert_eq!(merge_date(instant, -21 * 3600), "2026-11-12");
    assert_eq!(merge_date(0, 0), "1970-01-01");
    assert_eq!(merge_date(-1, 0), "1969-12-31");
    assert_eq!(merge_date(951_782_400, 0), "2000-02-29");
    assert_eq!(merge_date(4_107_542_400, 0), "2100-03-01");
    let today = ic::merge_date_today();
    assert_eq!(today.len(), 10);
    assert_eq!(&today[4..5], "-");
}

// ---- AAF ----

#[test]
fn import_aaf_makes_a_project_at_the_file_rate() {
    let (f, m) = demo();
    let r = run(ic::IMPORT_AAF, &[("path", t("/tmp/a.aaf"))], &m).unwrap();
    assert_eq!(
        r,
        InterchangeRequest::AafIntoNewProject(AafImportRequestRow {
            path: "/tmp/a.aaf".into(),
            new_project: true,
            midi_path: None,
            media_folder: None,
            join_pairs: true,
        })
    );
    assert!(f.jobs_started().is_empty());
}

#[test]
fn import_aaf_into_this_project() {
    let (_, mut m) = demo();
    let a = [
        ("path", t("/tmp/a.aaf")),
        ("new_project", yes(false)),
        ("midi_path", t("/tmp/a.mid")),
        ("media_folder", t("/tmp/media")),
        ("join_pairs", yes(false)),
    ];
    let j = job(run(ic::IMPORT_AAF, &a, &m));
    assert_eq!(j.kind(), JobKindRow::ImportAaf);
    assert_eq!(j.refusal_title(), "Import Failed");
    assert_eq!(
        j,
        FileJob::ImportAaf(AafImportRequestRow {
            path: "/tmp/a.aaf".into(),
            new_project: false,
            midi_path: Some("/tmp/a.mid".into()),
            media_folder: Some("/tmp/media".into()),
            join_pairs: false,
        })
    );
    assert!(m.begin_gesture("Move", vec![]));
    assert_eq!(refused(ic::IMPORT_AAF, &a, &m).code, ErrorCode::Unavailable);
}

#[test]
fn import_aaf_asks_for_open_and_export_for_save() {
    let (f, m) = demo();
    for a in [
        vec![],
        vec![("new_project", yes(false))],
        vec![("path", t(""))],
    ] {
        assert_dialog(&refused(ic::IMPORT_AAF, &a, &m), "Open", "path");
    }
    for a in [vec![], vec![("dry_run", yes(false))], vec![("path", t(""))]] {
        assert_dialog(&refused(ic::EXPORT_AAF, &a, &m), "Save", "path");
    }
    assert!(f.jobs_started().is_empty());
}

#[test]
fn export_aaf_builds_the_request() {
    let (_, m) = demo();
    let shown = m.selected_seq().unwrap();
    let j = job(run(ic::EXPORT_AAF, &[("path", t("/tmp"))], &m));
    assert_eq!(j.kind(), JobKindRow::ExportAaf);
    assert_eq!(
        j,
        FileJob::ExportAaf(AafExportRequestRow {
            seq: shown,
            path: "/tmp".into(),
            embed: true,
            depth: AafDepthRow::Int24,
            stereo: AafStereoRow::MonoPairs,
            hour_start: false,
            fps: AafFpsRow::Fps25,
            clip_gain: true,
            automation: true,
        })
    );
    let j = job(run(
        ic::EXPORT_AAF,
        &[
            ("path", t("/tmp")),
            ("seq", n(2)),
            ("embed", yes(false)),
            ("depth", t("16")),
            ("stereo", t("stereo")),
            ("start_tc", t("01:00:00:00")),
            ("fps", t("29.97df")),
            ("clip_gain", yes(false)),
            ("automation", yes(false)),
        ],
        &m,
    ));
    assert_eq!(
        j,
        FileJob::ExportAaf(AafExportRequestRow {
            seq: 2,
            path: "/tmp".into(),
            embed: false,
            depth: AafDepthRow::Int16,
            stereo: AafStereoRow::Stereo,
            hour_start: true,
            fps: AafFpsRow::Fps2997Drop,
            clip_gain: false,
            automation: false,
        })
    );
    for (fps, row) in [("24", AafFpsRow::Fps24), ("30", AafFpsRow::Fps30)] {
        let FileJob::ExportAaf(r) = job(run(
            ic::EXPORT_AAF,
            &[("path", t("/tmp")), ("fps", t(fps))],
            &m,
        )) else {
            panic!()
        };
        assert_eq!(r.fps, row);
    }
}

#[test]
fn export_aaf_dry_run_is_a_check() {
    let (f, m) = demo();
    f.set_check_report("export_aaf_check", report("Would leave out 1"));
    let c = check(run(ic::EXPORT_AAF, &[("dry_run", yes(true))], &m));
    let FileCheck::ExportAaf(req) = &c else {
        panic!()
    };
    assert_eq!(req.path, "");
    assert_eq!(f.reads("export_aaf_check"), 0);
    assert_eq!(
        c.run(m.session().as_ref()).unwrap(),
        report("Would leave out 1")
    );
    assert!(f.jobs_started().is_empty());
}

/// Each bad argument is `invalid_params` naming the command and the
/// parameter, or passes on for the core to refuse; never `internal`.
#[test]
fn aaf_hostile_arguments() {
    let (f, m) = demo();
    let dir = std::env::temp_dir().display().to_string();
    let long = format!("/{}", "a".repeat(5_000));
    for p in ["/tmp/a\0.aaf".to_owned(), format!("{long}.aaf")] {
        for extra in [vec![], vec![("new_project", yes(false))]] {
            let mut a = vec![("path", t(&p))];
            a.extend(extra);
            let e = refused(ic::IMPORT_AAF, &a, &m);
            assert_invalid(&e, "file.import_aaf", "`path`");
            assert_eq!(
                e.message,
                "file.import_aaf: `path` is a path of at most 4096 bytes with no NUL"
            );
        }
        let e = refused(ic::EXPORT_AAF, &[("path", t(&p))], &m);
        assert_eq!(
            e.message,
            "file.export_aaf: `path` is a folder path of at most 4096 bytes with no NUL"
        );
    }
    // A folder is a path the core refuses as a file to import.
    assert!(matches!(
        run(ic::IMPORT_AAF, &[("path", t(&dir))], &m),
        Ok(InterchangeRequest::AafIntoNewProject(_))
    ));
    let e = refused(
        ic::IMPORT_AAF,
        &[("path", t("/tmp/a.aaf")), ("media_folder", t("media"))],
        &m,
    );
    assert_eq!(
        e.message,
        "file.import_aaf: `media_folder` must be an absolute path"
    );
    // An absolute file as the folder is the core's to refuse.
    assert!(
        run(
            ic::IMPORT_AAF,
            &[("path", t("/tmp/a.aaf")), ("media_folder", t("/etc/hosts"))],
            &m
        )
        .is_ok()
    );
    let e = refused(
        ic::IMPORT_AAF,
        &[
            ("path", t("/tmp/a.aaf")),
            ("midi_path", t(&format!("{long}.mid"))),
        ],
        &m,
    );
    assert_invalid(&e, "file.import_aaf", "`midi_path`");
    let cases: Vec<(Pairs, &str, &str)> = vec![
        (
            vec![("path", t(&dir)), ("seq", n(-1))],
            "seq",
            "file.export_aaf: `seq` is a sequence ID",
        ),
        (
            vec![("path", t(&dir)), ("fps", t("0"))],
            "fps",
            "file.export_aaf: `fps` is 24, 25, 29.97df or 30",
        ),
        (
            vec![("path", t(&dir)), ("depth", t("32f"))],
            "depth",
            "file.export_aaf: `depth` is 16 or 24",
        ),
        (
            vec![("dry_run", yes(true)), ("stereo", t("quad"))],
            "stereo",
            "file.export_aaf: `stereo` is mono_pairs or stereo",
        ),
        (
            vec![("path", t(&dir)), ("start_tc", t("02:00:00:00"))],
            "start_tc",
            "file.export_aaf: `start_tc` is 00:00:00:00 or 01:00:00:00",
        ),
    ];
    for (a, name, message) in cases {
        let e = refused(ic::EXPORT_AAF, &a, &m);
        assert_invalid(&e, "file.export_aaf", name);
        assert_eq!(e.message, message);
    }
    assert!(f.jobs_started().is_empty());
}

// ---- jobs through the session ----

#[test]
fn every_job_starts_its_own_session_call() {
    let (f, m) = demo();
    let jobs = [
        job(run(ic::IMPORT_MIDI, &[("path", t("/tmp/a.mid"))], &m)),
        job(run(ic::EXPORT_MIDI, &[("path", t("/tmp/a.mid"))], &m)),
        job(run(ic::EXPORT_CLIP, &[("path", t("/tmp/d"))], &m)),
        job(run(ic::EXPORT_FOR_DAW, &[("path", t("/tmp/out"))], &m)),
        job(run(
            ic::EXPORT_DAWPROJECT,
            &[("path", t("/tmp/a.dawproject"))],
            &m,
        )),
        job(run(
            ic::IMPORT_AAF,
            &[("path", t("/tmp/a.aaf")), ("new_project", yes(false))],
            &m,
        )),
        job(run(ic::EXPORT_AAF, &[("path", t("/tmp"))], &m)),
        ic::dawproject_import_job("/tmp/a.dawproject"),
        read(
            &[("path", t("/tmp/b.dawproject")), ("mode", t("merge"))],
            &m,
        )
        .merge_job("2026-11-23".into()),
    ];
    for j in &jobs {
        let id = j.start(m.session().as_ref()).unwrap();
        let s = f.jobs_started().pop().unwrap();
        assert_eq!(s.id, id);
        assert_eq!(s.kind, j.kind(), "{j:?}");
        let same = match (j, &s.request) {
            (FileJob::ImportMidi(a), JobRequest::ImportMidi(b)) => a == b,
            (FileJob::ExportMidi(a), JobRequest::ExportMidi(b)) => a == b,
            (FileJob::ExportMidiClip(a), JobRequest::ExportMidiClip(b)) => a == b,
            (FileJob::ForDaw(a), JobRequest::ForDaw(b)) => a == b,
            (FileJob::ExportDawproject(a), JobRequest::ExportDawproject(b)) => a == b,
            (FileJob::ImportAaf(a), JobRequest::ImportAaf(b)) => a == b,
            (FileJob::ExportAaf(a), JobRequest::ExportAaf(b)) => a == b,
            (FileJob::ImportDawproject(a), JobRequest::ImportDawproject(b)) => a == b,
            (FileJob::MergeDawproject(a), JobRequest::MergeDawproject(b)) => a == b,
            _ => false,
        };
        assert!(same, "{j:?} started {:?}", s.request);
    }
    assert_eq!(f.jobs_started().len(), jobs.len());
}

/// Carried from the Task 2 review: a MIDI export the channel's request
/// starts on a real session is an `ExportMidi` job that ends Done, and
/// its answer is the export's report summary.
#[test]
fn a_real_midi_export_runs_as_an_export_midi_job() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Song.mid").display().to_string();
    let session: Arc<dyn DocSession> = vizdaw_ffi::session::Session::new_demo().unwrap();
    let mut m = DocumentModel::new(session).unwrap();
    let j = job(run(ic::EXPORT_MIDI, &[("path", t(&path))], &m));
    let id = m.try_start_job(j.kind(), |s| j.start(s)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !m.jobs().is_empty() {
        assert!(Instant::now() < deadline, "the job never ended");
        std::thread::sleep(Duration::from_millis(10));
        m.poll_jobs();
    }
    let r = m.session().job(id).unwrap();
    assert_eq!(r.kind, JobKindRow::ExportMidi);
    assert_eq!(r.state, JobStateRow::Done, "{:?}", r.message);
    let summary = r.report.as_ref().map(|r| r.summary.clone());
    assert!(
        summary.as_deref().is_some_and(|s| !s.is_empty()),
        "{summary:?}"
    );
    assert_eq!(m.job_answer(&r), JobAnswer::Ok(summary));
    assert!(m.alert.is_none(), "{:?}", m.alert);
    assert!(std::path::Path::new(&path).exists(), "the file was written");
}
