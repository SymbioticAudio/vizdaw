//! The store's new, open routing and made projects (slice 7a Task 4;
//! Swift `DocumentStoreTests`, the `InterchangeTests` open rows,
//! `RecoveryTests` and the made-project rows of
//! `InterchangeCommandsTests`): new in a format, the demo with audio,
//! Co-Producer copies, open by extension (MIDI into a new project,
//! DAWproject and AAF read first, a package's `project.json`), the
//! recovery questions and their channel answers, the return prompt, the
//! slow reads and their generations, and the projects an import makes
//! and closes again.

mod support;

use std::path::Path;
use std::sync::Arc;

use core_control::error::ErrorCode;
use core_control::registry::ArgValue;
use core_control::request::{CrashAnswer, RecoveryAnswers};
use support::{FakeSession, demo_fake};
use vizdaw_app::interchange_commands::{
    self as ic, DawprojectNext, InterchangeRequest, MidiIntoNewProject, request,
};
use vizdaw_app::store::{
    CrashChoice, OpenQuestion, Opened, ReturnChoice, ReturnPrompt, is_aaf, is_copy, is_dawproject,
    is_midi, project_path,
};
use vizdaw_app::testing::{FakeCore, JobRequest, JobScript, seq};
use vizdaw_app::{DocId, DocumentStore, WindowToken};
use vizdaw_ffi::snapshot::{PluginFormatTag, PluginRefRow};
use vizdaw_ffi::{
    AafImportRequestRow, AafInfoRow, BitDepthRow, DawprojectImportRequestRow, DawprojectInfoRow,
    MergeStepRow, MidiFileInfoRow, MidiFileKindRow, MidiImportRequestRow, ProjectFormatRow,
    RecoveryInfo, VizError,
};

const PID: &str = "0f8b6f8e-3c1a-4d2e-9b7a-5e6f7a8b9c0d";
const EID: &str = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
const BUSY: &str = "Wait for the running import, export or bounce to finish.";

fn format(rate: u32, depth: BitDepthRow) -> ProjectFormatRow {
    ProjectFormatRow {
        sample_rate: rate,
        depth,
    }
}

/// A store over a fake core.
fn store() -> (DocumentStore, Arc<FakeCore>) {
    let core = Arc::new(FakeCore::new());
    (DocumentStore::with_core(core.clone()), core)
}

/// A new project's session: one empty top-level sequence (9).
fn fresh() -> Arc<FakeSession> {
    Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]))
}

/// `fresh`, handed out by the next `new_project`.
fn next_project(core: &FakeCore) -> Arc<FakeSession> {
    let f = fresh();
    core.hand_out("new_project", f.clone());
    f
}

fn plain_file(rate: u32) -> DawprojectInfoRow {
    DawprojectInfoRow {
        sample_rate: rate,
        tracks: 2,
        audio_files: 0,
        from_vizdaw: false,
        export_id: None,
        project_id: None,
    }
}

fn returned(project: Option<&str>, export: Option<&str>) -> DawprojectInfoRow {
    DawprojectInfoRow {
        sample_rate: 44_100,
        tracks: 2,
        audio_files: 0,
        from_vizdaw: true,
        export_id: export.map(str::to_owned),
        project_id: project.map(str::to_owned),
    }
}

fn new_projects(core: &FakeCore) -> Vec<String> {
    core.calls()
        .into_iter()
        .filter(|c| c.starts_with("new_project"))
        .collect()
}

fn started(f: &FakeSession) -> Vec<JobRequest> {
    f.jobs_started().into_iter().map(|j| j.request).collect()
}

fn poll(s: &mut DocumentStore, id: DocId) {
    s.with_document(id, |d, _| d.poll_jobs());
}

/// Opens `path` through the store (an open package): its id and session.
fn open_project(s: &mut DocumentStore, core: &FakeCore, path: &str) -> (DocId, Arc<FakeSession>) {
    let f = Arc::new(demo_fake());
    f.set_path(Some(path));
    let mut snap = f.snap();
    snap.title = Path::new(path)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    core.hand_out("open", f.clone());
    let Ok(Opened::Doc(id)) = s.open(Path::new(path)) else {
        panic!("{path} did not open");
    };
    s.document_mut(id).unwrap().apply(snap);
    (id, f)
}

fn alert(s: &mut DocumentStore) -> (String, String) {
    let a = s.take_alert().expect("an alert");
    (a.title, a.message)
}

// ---- new ----

#[test]
fn new_project_makes_a_project_in_its_format() {
    let (mut s, core) = store();
    assert_eq!(s.default_format(), format(48_000, BitDepthRow::Int24));
    let id = s.new_project(format(96_000, BitDepthRow::Float32)).unwrap();
    assert!(s.document(id).is_some());
    assert_eq!(new_projects(&core), ["new_project(96000,Float32)"]);
    s.set_default_format(format(44_100, BitDepthRow::Int16));
    assert_eq!(s.default_format(), format(44_100, BitDepthRow::Int16));
    // A refusal is the alert "New Document Failed".
    core.fail_next(
        "new_project",
        VizError::Io {
            message: "disk full".into(),
        },
    );
    assert!(s.new_project(format(48_000, BitDepthRow::Int24)).is_none());
    assert_eq!(
        alert(&mut s),
        ("New Document Failed".into(), "disk full".into())
    );
}

#[test]
fn the_demo_with_audio_is_its_own_constructor() {
    let (mut s, core) = store();
    let id = s.new_demo_audio().unwrap();
    assert_eq!(s.document(id).unwrap().snapshot().sequences.len(), 3);
    assert_eq!(core.calls(), ["new_demo_audio"]);
    core.fail_next(
        "new_demo_audio",
        VizError::Io {
            message: "no audio".into(),
        },
    );
    assert!(s.new_demo_audio().is_none());
    assert_eq!(alert(&mut s).0, "Demo Project Failed");
}

#[test]
fn a_store_from_makers_makes_new_projects_empty() {
    let mut s = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(demo_fake()))),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let id = s.new_project(format(44_100, BitDepthRow::Int16)).unwrap();
    assert!(s.document(id).unwrap().snapshot().sequences.is_empty());
}

// ---- routing ----

#[test]
fn open_path_routes_by_extension() {
    assert!(is_dawproject(Path::new("/x/y.DawProject")) && !is_dawproject(Path::new("/x/y.zip")));
    assert!(is_aaf(Path::new("/x/y.AAF")) && is_midi(Path::new("/x/a.MID")));
    assert!(
        ["mid", "midi", "smf", "midi2"]
            .iter()
            .all(|e| is_midi(&Path::new("/a").with_extension(e)))
    );
    let (mut s, core) = store();
    core.set_dawproject_info("/tmp/a.dawproject", plain_file(48_000));
    core.set_dawproject_info("/tmp/B.DAWPROJECT", plain_file(48_000));
    assert_eq!(s.open(Path::new("/tmp/a.dawproject")), Ok(Opened::Reading));
    assert_eq!(s.open(Path::new("/tmp/B.DAWPROJECT")), Ok(Opened::Reading));
    assert_eq!(s.count(), 0, "nothing is made before the files are read");
    s.run_reads();
    let mut paths: Vec<String> = core
        .made()
        .iter()
        .flat_map(|(_, f)| started(f))
        .map(|r| match r {
            JobRequest::ImportDawproject(r) => r.path,
            other => panic!("{other:?}"),
        })
        .collect();
    paths.sort();
    assert_eq!(paths, ["/tmp/B.DAWPROJECT", "/tmp/a.dawproject"]);
    assert_eq!(s.count(), 2);
    assert!(s.alert().is_none());
    assert_eq!(
        s.open(Path::new("/tmp/a.zip")),
        Err("a.zip is not a .vizdaw project.".into())
    );
    assert_eq!(
        alert(&mut s),
        (
            "Open Failed".into(),
            "a.zip is not a .vizdaw project.".into()
        )
    );
    assert_eq!(core.made().len(), 2);
}

#[test]
fn a_project_json_inside_a_package_opens_the_package() {
    assert_eq!(
        project_path(Path::new("/m/Song.vizdaw/project.json")),
        Path::new("/m/Song.vizdaw")
    );
    assert_eq!(
        project_path(Path::new("/m/Song/project.json")),
        Path::new("/m/Song/project.json")
    );
    let (mut s, core) = store();
    let Ok(Opened::Doc(id)) = s.open(Path::new("/m/Song.vizdaw/project.json")) else {
        panic!("not opened");
    };
    assert!(core.calls().contains(&"open(/m/Song.vizdaw)".to_string()));
    // Already open by its package: the same document.
    assert_eq!(
        s.open(Path::new("/m/Song.vizdaw/project.json")),
        Ok(Opened::Doc(id))
    );
    assert_eq!(s.count(), 1);
    // A `project.json` elsewhere is no project.
    assert!(s.open(Path::new("/m/Song/project.json")).is_err());
    assert_eq!(alert(&mut s).1, "project.json is not a .vizdaw project.");
}

#[test]
fn an_aaf_file_is_read_then_imports_with_the_defaults() {
    let (mut s, core) = store();
    s.set_default_format(format(96_000, BitDepthRow::Int16));
    core.set_aaf_info(
        "/tmp/a.aaf",
        AafInfoRow {
            sample_rate: 44_100,
            ..aaf_info()
        },
    );
    let f = next_project(&core);
    assert_eq!(s.open(Path::new("/tmp/a.aaf")), Ok(Opened::Reading));
    assert!(new_projects(&core).is_empty());
    s.run_reads();
    assert_eq!(new_projects(&core), ["new_project(44100,Int16)"]);
    assert_eq!(
        started(&f),
        [JobRequest::ImportAaf(AafImportRequestRow {
            path: "/tmp/a.aaf".into(),
            new_project: true,
            midi_path: None,
            media_folder: None,
            join_pairs: true,
        })]
    );
    let id = s.take_pending().expect("its window opens");
    assert!(s.document(id).is_some() && s.take_pending().is_none());
    // A refused file makes nothing.
    assert_eq!(s.open(Path::new("/tmp/b.aaf")), Ok(Opened::Reading));
    s.run_reads();
    assert_eq!(new_projects(&core).len(), 1);
    assert_eq!(alert(&mut s).0, "Import Failed");
}

fn aaf_info() -> AafInfoRow {
    AafInfoRow {
        sample_rate: 48_000,
        tracks: 1,
        audio_files: 0,
        offline: 0,
        from_vizdaw: false,
        midi_beside: None,
    }
}

#[test]
fn a_co_producer_copy_opens_as_a_new_project() {
    assert!(is_copy(Path::new(
        "/m/Song.vizdaw/Co-Producer/Copies/Take.vizdaw"
    )));
    assert!(is_copy(Path::new(
        "/m/Song.vizdaw/co-producer/COPIES/Take.VIZDAW"
    )));
    assert!(
        !is_copy(Path::new("/Co-Producer/Copies/Take.vizdaw")),
        "needs a package"
    );
    assert!(!is_copy(Path::new("/m/Song.vizdaw/Copies/Take.vizdaw")));
    let (mut s, core) = store();
    let copy = Path::new("/m/Song.vizdaw/Co-Producer/Copies/Take.vizdaw");
    let Ok(Opened::Doc(_)) = s.open(copy) else {
        panic!("not opened");
    };
    assert_eq!(
        core.calls(),
        ["open_copy(/m/Song.vizdaw/Co-Producer/Copies/Take.vizdaw)"]
    );
    assert!(s.open_copy(copy).is_ok());
    assert_eq!(
        s.open_copy(Path::new("/m/Song.vizdaw")),
        Err("That is not a Co-Producer copy.".into())
    );
    assert_eq!(
        alert(&mut s),
        (
            "Open Failed".into(),
            "That is not a Co-Producer copy.".into()
        )
    );
    assert_eq!(core.made().len(), 2);
}

// ---- MIDI files into a new project (InterchangeTests) ----

#[test]
fn open_routes_midi_files() {
    let (mut s, core) = store();
    s.set_default_format(format(96_000, BitDepthRow::Int16));
    let f = next_project(&core);
    let Ok(Opened::Doc(id)) = s.open(Path::new("/tmp/a.MID")) else {
        panic!("not opened");
    };
    assert!(s.document(id).is_some());
    assert_eq!(
        new_projects(&core),
        ["new_project(48000,Int16)"],
        "48 kHz at the default depth"
    );
    assert_eq!(
        started(&f),
        [JobRequest::ImportMidi(MidiImportRequestRow {
            path: "/tmp/a.MID".into(),
            new_project: true,
            seq: 9,
            lane: None,
            tick: 0,
            channel_map: vec![],
            split_channels: None,
            mpe: false,
            use_tempo: true,
        })]
    );
    assert!(s.alert().is_none());
    assert!(s.open(Path::new("/tmp/a.txt")).is_err());
    assert_eq!(alert(&mut s).0, "Open Failed");
    assert_eq!(core.made().len(), 1);
}

#[test]
fn a_refused_open_leaves_nothing() {
    let (mut s, core) = store();
    let f = next_project(&core);
    f.set_call_error(
        "import_midi",
        Some(VizError::Model {
            message: "not a MIDI file".into(),
        }),
    );
    assert_eq!(
        s.open(Path::new("/tmp/a.mid")),
        Err("not a MIDI file".into())
    );
    assert_eq!(s.count(), 0);
    assert_eq!(
        alert(&mut s),
        ("Import Failed".into(), "not a MIDI file".into())
    );
    assert!(
        f.calls().contains(&"close".to_string()),
        "its session closed"
    );
}

/// A project made for a file the job then refuses goes, with the alert
/// owned by the store; cancelled, it goes quietly.
#[test]
fn a_failed_open_job_closes_the_untouched_project() {
    let (mut s, core) = store();
    let f = next_project(&core);
    f.script_job(JobScript::failed("not a MIDI file").steps(&[0.5]));
    let Ok(Opened::Doc(id)) = s.open(Path::new("/tmp/a.mid")) else {
        panic!("not opened");
    };
    assert_eq!(s.document(id).unwrap().jobs().len(), 1, "running");
    poll(&mut s, id);
    assert!(s.document(id).is_none() && s.count() == 0);
    assert_eq!(s.take_closed(), [id], "its windows close");
    assert_eq!(
        alert(&mut s),
        ("Import Failed".into(), "not a MIDI file".into())
    );
    // Cancelled: closes quietly.
    let g = next_project(&core);
    g.script_job(JobScript::cancelled().steps(&[0.5]));
    let Ok(Opened::Doc(id2)) = s.open(Path::new("/tmp/b.mid")) else {
        panic!("not opened");
    };
    poll(&mut s, id2);
    assert!(s.count() == 0 && s.alert().is_none());
    // Failed at once: the open answers the failure and leaves nothing.
    let h = next_project(&core);
    h.script_job(JobScript::failed("bad file"));
    assert_eq!(s.open(Path::new("/tmp/c.mid")), Err("bad file".into()));
    assert!(s.count() == 0);
    assert_eq!(alert(&mut s), ("Import Failed".into(), "bad file".into()));
}

#[test]
fn a_failed_open_job_keeps_a_project_that_was_edited() {
    let (mut s, core) = store();
    let f = next_project(&core);
    f.script_job(JobScript::failed("bad").steps(&[0.5]));
    let Ok(Opened::Doc(id)) = s.open(Path::new("/tmp/a.mid")) else {
        panic!("not opened");
    };
    f.set_undo_label(Some("Rename"));
    poll(&mut s, id);
    assert!(s.document(id).is_some() && s.take_closed().is_empty());
    assert_eq!(alert(&mut s), ("Import Failed".into(), "bad".into()));
    // A later failure of the same project is the job's own alert.
    f.script_job(JobScript::failed("again").steps(&[0.5]));
    s.with_document(id, |d, _| {
        d.start_job(
            vizdaw_ffi::JobKindRow::ImportMidi,
            "MIDI Import Failed",
            |x| x.import_midi(midi_req("/tmp/b.mid")),
        )
    });
    poll(&mut s, id);
    assert_eq!(alert(&mut s), ("MIDI Import Failed".into(), "again".into()));
}

fn midi_req(path: &str) -> MidiImportRequestRow {
    MidiImportRequestRow {
        path: path.into(),
        new_project: false,
        seq: 9,
        lane: None,
        tick: 0,
        channel_map: vec![],
        split_channels: None,
        mpe: false,
        use_tempo: false,
    }
}

#[test]
fn a_finished_open_job_keeps_its_project_for_good() {
    let (mut s, core) = store();
    let f = next_project(&core);
    f.script_job(JobScript::done().steps(&[0.5]));
    let Ok(Opened::Doc(id)) = s.open(Path::new("/tmp/a.mid")) else {
        panic!("not opened");
    };
    poll(&mut s, id);
    assert!(s.document(id).is_some() && s.alert().is_none());
    // A later import that fails no longer takes the project.
    f.script_job(JobScript::failed("bad").steps(&[0.5]));
    s.with_document(id, |d, _| {
        d.start_job(
            vizdaw_ffi::JobKindRow::ImportMidi,
            "MIDI Import Failed",
            |x| x.import_midi(midi_req("/tmp/b.mid")),
        )
    });
    poll(&mut s, id);
    assert!(s.document(id).is_some());
    assert_eq!(alert(&mut s).0, "MIDI Import Failed");
}

// ---- DAWproject opens (DocumentStoreTests) ----

#[test]
fn opening_a_dawproject_makes_a_project_at_its_rate() {
    let (mut s, core) = store();
    s.set_default_format(format(96_000, BitDepthRow::Int16));
    core.set_dawproject_info("/tmp/a.dawproject", plain_file(44_100));
    let f = next_project(&core);
    s.open_dawproject("/tmp/a.dawproject");
    assert!(
        s.count() == 0 && new_projects(&core).is_empty(),
        "nothing before the read"
    );
    assert!(
        core.calls().is_empty(),
        "the read waits for the app to run it"
    );
    let reads = s.take_reads();
    assert_eq!(reads.len(), 1);
    // The read runs on another thread (as the app's background executor).
    let landed: Vec<_> = reads
        .into_iter()
        .map(|r| std::thread::spawn(move || r.run()).join().unwrap())
        .collect();
    assert_eq!(core.calls(), ["dawproject_info(/tmp/a.dawproject)"]);
    assert!(s.count() == 0 && s.reading());
    for l in landed {
        s.land(l);
    }
    assert!(!s.reading());
    assert_eq!(new_projects(&core), ["new_project(44100,Int16)"]);
    assert_eq!(
        started(&f),
        [JobRequest::ImportDawproject(DawprojectImportRequestRow {
            path: "/tmp/a.dawproject".into()
        })]
    );
    let id = s.take_pending().expect("its window opens");
    assert!(s.count() == 1 && s.document(id).is_some() && s.alert().is_none());
}

#[test]
fn a_refused_dawproject_makes_no_project() {
    let (mut s, core) = store();
    // Unscripted: the fake core cannot read it.
    s.open_dawproject("/tmp/a.dawproject");
    s.run_reads();
    assert!(s.count() == 0 && core.made().is_empty() && s.take_pending().is_none());
    let (title, message) = alert(&mut s);
    assert_eq!(title, "Import Failed");
    assert!(message.contains("could not be read"), "{message}");
    // Refused by the new project's session as the job starts: it goes.
    core.set_dawproject_info("/tmp/b.dawproject", plain_file(48_000));
    let f = next_project(&core);
    f.set_call_error(
        "import_dawproject",
        Some(VizError::Busy {
            message: BUSY.into(),
        }),
    );
    s.open_dawproject("/tmp/b.dawproject");
    s.run_reads();
    assert!(core.made().len() == 1 && s.count() == 0 && s.take_pending().is_none());
    assert_eq!(alert(&mut s), ("Import Failed".into(), BUSY.into()));
}

#[test]
fn a_cancelled_dawproject_import_closes_the_untouched_project() {
    let (mut s, core) = store();
    core.set_dawproject_info("/tmp/a.dawproject", plain_file(48_000));
    let f = next_project(&core);
    f.script_job(JobScript::cancelled().steps(&[0.5]));
    s.open_dawproject("/tmp/a.dawproject");
    s.run_reads();
    let id = s.take_pending().unwrap();
    poll(&mut s, id);
    assert!(s.count() == 0 && s.alert().is_none(), "closed quietly");
    // Edited meanwhile: it stays.
    core.set_dawproject_info("/tmp/b.dawproject", plain_file(48_000));
    let g = next_project(&core);
    g.script_job(JobScript::cancelled().steps(&[0.5]));
    s.open_dawproject("/tmp/b.dawproject");
    s.run_reads();
    let id2 = s.take_pending().unwrap();
    g.set_undo_label(Some("Rename"));
    poll(&mut s, id2);
    assert!(s.document(id2).is_some() && s.alert().is_none());
}

/// Controller ruling: a failed UI import leaves no empty project open.
#[test]
fn a_failed_dawproject_import_closes_the_untouched_project() {
    let (mut s, core) = store();
    core.set_dawproject_info("/tmp/a.dawproject", plain_file(48_000));
    let f = next_project(&core);
    f.script_job(JobScript::failed("the file is damaged").steps(&[0.5]));
    s.open_dawproject("/tmp/a.dawproject");
    s.run_reads();
    let id = s.take_pending().unwrap();
    poll(&mut s, id);
    assert_eq!(s.count(), 0);
    assert_eq!(
        alert(&mut s),
        ("Import Failed".into(), "the file is damaged".into())
    );
    // Edited meanwhile: it stays, with the alert.
    let g = next_project(&core);
    g.script_job(JobScript::failed("bad").steps(&[0.5]));
    s.open_dawproject("/tmp/a.dawproject");
    s.run_reads();
    let id2 = s.take_pending().unwrap();
    g.set_undo_label(Some("Rename"));
    poll(&mut s, id2);
    assert!(s.document(id2).is_some());
    assert_eq!(alert(&mut s), ("Import Failed".into(), "bad".into()));
}

#[test]
fn a_read_for_a_store_that_reset_is_dropped() {
    let (mut s, core) = store();
    core.set_dawproject_info("/tmp/a.dawproject", plain_file(48_000));
    s.open_dawproject("/tmp/a.dawproject");
    let r = s.take_reads().pop().unwrap();
    let landed = r.run();
    // Landed twice (or once for the wrong store): only the first counts.
    let (mut other, other_core) = store();
    other.land(landed);
    assert!(other.count() == 0 && other_core.made().is_empty());
}

// ---- the return prompt ----

/// An open project whose ID is `PID`, titled "Song".
fn open_match(s: &mut DocumentStore, core: &FakeCore) -> (DocId, Arc<FakeSession>) {
    let (id, f) = open_project(s, core, "/tmp/Song.vizdaw");
    f.set_project_id(Some(PID));
    (id, f)
}

fn merges(f: &FakeSession) -> usize {
    started(f)
        .iter()
        .filter(|r| matches!(r, JobRequest::MergeDawproject(_)))
        .count()
}

#[test]
fn a_file_from_vizdaw_prompts_instead_of_opening() {
    let (mut s, core) = store();
    core.set_dawproject_info("/tmp/back.dawproject", returned(Some(PID), Some(EID)));
    s.open_dawproject("/tmp/back.dawproject");
    s.run_reads();
    assert_eq!(
        s.return_prompt(),
        Some(&ReturnPrompt {
            path: "/tmp/back.dawproject".into(),
            info: returned(Some(PID), Some(EID)),
            matched: None,
            match_name: None,
        })
    );
    assert!(core.made().is_empty() && s.count() == 0 && s.take_pending().is_none());
    assert!(s.alert().is_none());
    let p = s.return_prompt().unwrap();
    assert_eq!(
        ReturnPrompt::MESSAGE,
        "This DAWproject came from a VizDAW project."
    );
    assert_eq!(
        p.informative(),
        "Merge it into the project it came from, or open it as a new project."
    );
    assert_eq!(
        [
            ReturnPrompt::NEW_PROJECT,
            ReturnPrompt::MERGE,
            ReturnPrompt::CANCEL
        ],
        [
            "New Project",
            "Merge With Existing Project\u{2026}",
            "Cancel"
        ]
    );
    // A plain file still makes its project at once.
    s.answer_return(ReturnChoice::Cancel);
    core.set_dawproject_info("/tmp/plain.dawproject", plain_file(48_000));
    let f = next_project(&core);
    s.open_dawproject("/tmp/plain.dawproject");
    s.run_reads();
    assert!(s.return_prompt().is_none() && core.made().len() == 1);
    assert_eq!(
        started(&f),
        [JobRequest::ImportDawproject(DawprojectImportRequestRow {
            path: "/tmp/plain.dawproject".into()
        })]
    );
}

#[test]
fn the_prompt_names_the_open_match() {
    let (mut s, core) = store();
    open_project(&mut s, &core, "/tmp/Other.vizdaw");
    let (id, _) = open_match(&mut s, &core);
    core.set_dawproject_info("/tmp/back.dawproject", returned(Some(PID), Some(EID)));
    s.open_dawproject("/tmp/back.dawproject");
    s.run_reads();
    let p = s.return_prompt().unwrap();
    assert_eq!(
        (p.matched, p.match_name.as_deref()),
        (Some(id), Some("Song"))
    );
    assert_eq!(
        p.informative(),
        "It matches \u{201c}Song\u{201d}, which is open."
    );
    // Matched by snapshot when no open project has its ID.
    let (mut t, tcore) = store();
    let (by_export, g) = open_project(&mut t, &tcore, "/tmp/Kept.vizdaw");
    g.set_snapshot_ids(&[EID]);
    tcore.set_dawproject_info("/tmp/back.dawproject", returned(Some(PID), Some(EID)));
    t.open_dawproject("/tmp/back.dawproject");
    t.run_reads();
    let p = t.return_prompt().unwrap();
    assert_eq!(
        (p.matched, p.match_name.as_deref()),
        (Some(by_export), Some("Kept"))
    );
}

#[test]
fn the_new_project_answer_opens_it_at_the_files_rate() {
    let (mut s, core) = store();
    s.set_default_format(format(96_000, BitDepthRow::Int16));
    let (_, matched) = open_match(&mut s, &core);
    core.set_dawproject_info("/tmp/back.dawproject", returned(Some(PID), Some(EID)));
    s.open_dawproject("/tmp/back.dawproject");
    s.run_reads();
    let f = next_project(&core);
    s.answer_return(ReturnChoice::NewProject);
    assert!(s.return_prompt().is_none());
    assert_eq!(new_projects(&core), ["new_project(44100,Int16)"]);
    assert_eq!(
        started(&f),
        [JobRequest::ImportDawproject(DawprojectImportRequestRow {
            path: "/tmp/back.dawproject".into()
        })]
    );
    assert_eq!(merges(&matched), 0);
    assert!(s.take_pending().is_some() && s.take_pending().is_none());
}

#[test]
fn the_cancel_answer_does_nothing() {
    let (mut s, core) = store();
    let (_, matched) = open_match(&mut s, &core);
    core.set_dawproject_info("/tmp/back.dawproject", returned(Some(PID), Some(EID)));
    s.open_dawproject("/tmp/back.dawproject");
    s.run_reads();
    s.answer_return(ReturnChoice::Cancel);
    assert!(s.return_prompt().is_none() && new_projects(&core).is_empty());
    assert!(started(&matched).is_empty() && s.count() == 1 && s.alert().is_none());
    // An answer with no prompt does nothing either.
    s.answer_return(ReturnChoice::NewProject);
    assert!(new_projects(&core).is_empty());
}

/// A second file read while a prompt shows waits its turn (never dropped).
#[test]
fn return_prompts_queue() {
    let (mut s, core) = store();
    core.set_dawproject_info("/tmp/one.dawproject", returned(None, None));
    core.set_dawproject_info("/tmp/two.dawproject", returned(None, None));
    s.open_dawproject("/tmp/one.dawproject");
    s.run_reads();
    s.open_dawproject("/tmp/two.dawproject");
    s.run_reads();
    assert_eq!(s.return_prompt().unwrap().path, "/tmp/one.dawproject");
    s.answer_return(ReturnChoice::Cancel);
    assert_eq!(s.return_prompt().unwrap().path, "/tmp/two.dawproject");
    s.answer_return(ReturnChoice::Cancel);
    assert!(s.return_prompt().is_none());
}

#[test]
fn under_the_channel_the_return_prompt_is_answered_cancel() {
    let (mut s, core) = store();
    core.set_dawproject_info("/tmp/one.dawproject", returned(None, None));
    core.set_dawproject_info("/tmp/two.dawproject", returned(None, None));
    s.open_dawproject("/tmp/one.dawproject");
    s.open_dawproject("/tmp/two.dawproject");
    s.run_reads();
    let notes = s.answer_for_channel();
    assert_eq!(notes.len(), 2);
    assert!(notes.iter().all(|n| n.title == "DAWproject Not Merged"
        && n.message == "A DAWproject from VizDAW was opened; it was not merged."));
    assert!(s.return_prompt().is_none() && core.made().is_empty());
}

// ---- slow reads: the import sheet's MIDI read ----

fn midi_info() -> MidiFileInfoRow {
    MidiFileInfoRow {
        kind: MidiFileKindRow::Smf1,
        tracks: 2,
        channels: vec![],
        has_tempo: true,
        mpe_found: false,
    }
}

#[test]
fn only_the_newest_midi_read_lands() {
    let fake = Arc::new(demo_fake());
    fake.set_midi_file_info("/tmp/a.mid", midi_info());
    fake.set_midi_file_info("/tmp/b.mid", midi_info());
    let (mut s, core) = store();
    core.hand_out("new_demo", fake.clone());
    let id = s.new_demo().unwrap();
    s.begin_midi_import(id, "/tmp/a.mid");
    s.begin_midi_import(id, "/tmp/b.mid");
    let mut reads = s.take_reads();
    assert_eq!(reads.len(), 2);
    let newer = reads.pop().unwrap().run();
    let older = reads.pop().unwrap().run();
    s.land(newer);
    s.land(older);
    let read = s.document(id).unwrap().midi_read().unwrap();
    assert_eq!(
        read.path, "/tmp/b.mid",
        "the older answer never lands over the newer"
    );
    // A refused file is the alert "Import Failed".
    s.begin_midi_import(id, "/tmp/missing.mid");
    s.run_reads();
    assert_eq!(alert(&mut s).0, "Import Failed");
    // A read for a document that closed is dropped.
    s.begin_midi_import(id, "/tmp/a.mid");
    let r = s.take_reads().pop().unwrap().run();
    s.close(id);
    s.land(r);
    assert!(s.alert().is_none() && s.count() == 0);
}

// ---- recovery questions (RecoveryTests) ----

fn diva() -> PluginRefRow {
    PluginRefRow {
        format: PluginFormatTag::Au,
        id: "aumu/Diva/UHfX".into(),
        name: "Diva".into(),
        vendor: "u-he".into(),
        version: "1".into(),
    }
}

fn recovery(crashed: bool, loading: Vec<PluginRefRow>, autosave: Option<&str>) -> RecoveryInfo {
    RecoveryInfo {
        crashed,
        loading,
        autosave: autosave.map(str::to_owned),
    }
}

fn opens(core: &FakeCore) -> Vec<String> {
    core.calls()
        .into_iter()
        .filter(|c| c.starts_with("open"))
        .collect()
}

const SONG: &str = "/p/Song.vizdaw";

#[test]
fn a_clean_project_opens_without_questions() {
    let (mut s, core) = store();
    assert!(matches!(s.open(Path::new(SONG)), Ok(Opened::Doc(_))));
    assert!(s.open_question().is_none());
    assert_eq!(
        core.calls(),
        ["recovery_check(/p/Song.vizdaw)", "open(/p/Song.vizdaw)"]
    );
}

#[test]
fn after_a_crash_plugins_off_or_cancel() {
    let (mut s, core) = store();
    core.set_recovery(SONG, recovery(true, vec![diva()], None));
    assert_eq!(s.open(Path::new(SONG)), Ok(Opened::Asking));
    let q = s.open_question().unwrap().clone();
    assert_eq!(
        q,
        OpenQuestion::AfterCrash {
            project: "Song".into(),
            loading: vec![diva()],
            crashed: true
        }
    );
    assert_eq!(
        q.message(),
        "VizDAW quit unexpectedly while this project was open, while loading Diva."
    );
    assert_eq!(
        q.informative(),
        "With plugins off, every plugin is a placeholder you can load one by one."
    );
    assert_eq!(
        q.buttons(),
        [
            "Open with Plugins Off",
            "Open Normally",
            "Cancel",
            "Disable Diva"
        ]
    );
    assert!(opens(&core).is_empty(), "nothing opens before the answer");
    let id = s.answer_crash(CrashChoice::PluginsOff).unwrap();
    assert!(s.document(id).is_some() && s.open_question().is_none());
    assert_eq!(
        opens(&core),
        ["open_with(/p/Song.vizdaw,plugins_off=true,autosave=None)"]
    );
    s.close(id);
    assert_eq!(s.open(Path::new(SONG)), Ok(Opened::Asking));
    assert_eq!(s.answer_crash(CrashChoice::Cancel), None);
    assert!(s.open_question().is_none() && opens(&core).len() == 1 && s.alert().is_none());
    // No crash while loading: no Disable.
    let q = OpenQuestion::AfterCrash {
        project: "Song".into(),
        loading: vec![],
        crashed: true,
    };
    assert_eq!(
        q.message(),
        "VizDAW quit unexpectedly while this project was open."
    );
    assert_eq!(q.buttons().len(), 3);
}

#[test]
fn disabling_the_crashing_plugin_then_opening() {
    let (mut s, core) = store();
    core.set_recovery(SONG, recovery(true, vec![diva()], None));
    let disabled = Arc::new(std::sync::Mutex::new(vec![]));
    let d = disabled.clone();
    s.set_disable_plugins(Box::new(move |p| {
        d.lock().unwrap().push(p.to_vec());
        true
    }));
    s.open(Path::new(SONG)).unwrap();
    assert!(s.answer_crash(CrashChoice::Disable).is_some());
    assert_eq!(*disabled.lock().unwrap(), [vec![diva()]]);
    assert_eq!(opens(&core), ["open(/p/Song.vizdaw)"], "opened normally");
}

#[test]
fn disabling_without_a_catalog_opens_with_plugins_off() {
    let (mut s, core) = store();
    core.set_recovery(SONG, recovery(true, vec![diva()], None));
    s.open(Path::new(SONG)).unwrap();
    assert!(s.answer_crash(CrashChoice::Disable).is_some());
    assert_eq!(
        opens(&core),
        ["open_with(/p/Song.vizdaw,plugins_off=true,autosave=None)"]
    );
}

#[test]
fn shift_asks_even_without_a_crash() {
    let (mut s, core) = store();
    s.set_shift_held(Box::new(|| true));
    assert_eq!(s.open(Path::new(SONG)), Ok(Opened::Asking));
    let q = s.open_question().unwrap().clone();
    assert_eq!(
        q,
        OpenQuestion::AfterCrash {
            project: "Song".into(),
            loading: vec![],
            crashed: false
        }
    );
    assert_eq!(q.message(), "Open \u{201c}Song\u{201d} with plugins off?");
    assert!(s.answer_crash(CrashChoice::PluginsOff).is_some());
    assert_eq!(
        opens(&core),
        ["open_with(/p/Song.vizdaw,plugins_off=true,autosave=None)"]
    );
}

#[test]
fn a_newer_autosave_is_offered_and_restored() {
    let (mut s, core) = store();
    let auto = "/p/Song.vizdaw/Autosave/project-1.json";
    core.set_recovery(SONG, recovery(false, vec![], Some(auto)));
    assert_eq!(s.open(Path::new(SONG)), Ok(Opened::Asking));
    let q = s.open_question().unwrap().clone();
    assert_eq!(
        q,
        OpenQuestion::RestoreAutosave {
            project: "Song".into()
        }
    );
    assert_eq!(
        q.message(),
        "\u{201c}Song\u{201d} has an autosave newer than its last save."
    );
    assert_eq!(
        q.informative(),
        "Restoring opens the autosaved version as an unsaved change."
    );
    assert_eq!(q.buttons(), ["Restore Autosave", "Open Last Saved"]);
    assert_eq!(
        s.answer_crash(CrashChoice::Normally),
        None,
        "not the question asked"
    );
    let id = s.answer_autosave(true).unwrap();
    assert_eq!(
        opens(&core),
        [format!(
            "open_with(/p/Song.vizdaw,plugins_off=false,autosave=Some(\"{auto}\"))"
        )]
    );
    s.close(id);
    s.open(Path::new(SONG)).unwrap();
    assert!(s.answer_autosave(false).is_some());
    assert_eq!(opens(&core).last().unwrap(), "open(/p/Song.vizdaw)");
}

#[test]
fn after_a_crash_the_autosave_question_follows() {
    let (mut s, core) = store();
    let auto = "/p/Song.vizdaw/Autosave/project-1.json";
    core.set_recovery(SONG, recovery(true, vec![], Some(auto)));
    s.open(Path::new(SONG)).unwrap();
    assert_eq!(s.answer_crash(CrashChoice::PluginsOff), None);
    assert!(matches!(
        s.open_question(),
        Some(OpenQuestion::RestoreAutosave { .. })
    ));
    assert!(s.answer_autosave(true).is_some());
    assert_eq!(
        opens(&core),
        [format!(
            "open_with(/p/Song.vizdaw,plugins_off=true,autosave=Some(\"{auto}\"))"
        )]
    );
}

#[test]
fn under_the_channel_the_recovery_questions_are_noted() {
    let (mut s, core) = store();
    core.set_recovery(SONG, recovery(true, vec![], None));
    core.set_recovery(
        "/p/Two.vizdaw",
        recovery(false, vec![], Some("/p/Two.vizdaw/Autosave/a.json")),
    );
    s.open(Path::new(SONG)).unwrap();
    s.open(Path::new("/p/Two.vizdaw")).unwrap();
    let notes: Vec<(String, String)> = s
        .answer_for_channel()
        .into_iter()
        .map(|n| (n.title, n.message))
        .collect();
    assert_eq!(
        notes,
        [
            (
                "Open with Plugins Off?".to_string(),
                "Song was not opened; open it with app.open and `plugins`.".to_string()
            ),
            (
                "Restore Autosave".to_string(),
                "Two was opened from its last save; close it and app.open with \
                 `restore_autosave: true` to restore the autosave."
                    .to_string()
            ),
        ]
    );
    assert_eq!(
        opens(&core),
        ["open(/p/Two.vizdaw)"],
        "the crashed one stays closed"
    );
    assert!(s.take_pending().is_some() && s.open_question().is_none());
}

// ---- app.open over the channel ----

fn answers(plugins: Option<CrashAnswer>, restore: Option<bool>) -> RecoveryAnswers {
    RecoveryAnswers {
        plugins,
        restore_autosave: restore,
    }
}

#[test]
fn the_channels_open_refuses_dawproject_and_aaf_files() {
    let (mut s, core) = store();
    let e = s
        .open_answered(Path::new("/tmp/a.dawproject"), &RecoveryAnswers::default())
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(
        e.message,
        "Use file.import_dawproject to open a DAWproject file."
    );
    let e = s
        .open_answered(Path::new("/tmp/a.AAF"), &RecoveryAnswers::default())
        .unwrap_err();
    assert_eq!(e.message, "Use file.import_aaf to open an AAF file.");
    assert!(core.calls().is_empty() && s.take_reads().is_empty() && s.count() == 0);
}

#[test]
fn the_channels_open_answers_the_questions_from_its_args() {
    let (mut s, core) = store();
    let auto = "/p/Song.vizdaw/Autosave/a.json";
    core.set_recovery(SONG, recovery(true, vec![], Some(auto)));
    let none = RecoveryAnswers::default();
    // Crashed with an autosave and nothing answered: the crash dialog.
    let e = s.open_answered(Path::new(SONG), &none).unwrap_err();
    assert_eq!(e.code, ErrorCode::DialogRequired);
    assert!(
        e.message.contains("Open with Plugins Off?"),
        "{}",
        e.message
    );
    // Plugins answered, the autosave not: its dialog, before anything opens.
    let e = s
        .open_answered(Path::new(SONG), &answers(Some(CrashAnswer::Off), None))
        .unwrap_err();
    assert!(e.message.contains("Restore Autosave"), "{}", e.message);
    assert!(opens(&core).is_empty());
    let id = s
        .open_answered(
            Path::new(SONG),
            &answers(Some(CrashAnswer::Off), Some(true)),
        )
        .unwrap();
    assert_eq!(
        opens(&core),
        [format!(
            "open_with(/p/Song.vizdaw,plugins_off=true,autosave=Some(\"{auto}\"))"
        )]
    );
    assert!(s.open_question().is_none(), "nothing is left asking");
    // Open already: Busy, no window fronted.
    let e = s.open_answered(Path::new(SONG), &none).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert_eq!(e.message, "Song.vizdaw is already open in another window.");
    s.close(id);
    // Disable, with no catalog: plugins off; the autosave declined.
    s.open_answered(
        Path::new(SONG),
        &answers(Some(CrashAnswer::Disable), Some(false)),
    )
    .unwrap();
    assert_eq!(
        opens(&core).last().unwrap(),
        "open_with(/p/Song.vizdaw,plugins_off=true,autosave=None)"
    );
}

#[test]
fn the_channels_open_reports_a_failure_and_routes_files() {
    let (mut s, core) = store();
    core.fail_next("open", VizError::NotFound { path: SONG.into() });
    let e = s
        .open_answered(Path::new(SONG), &RecoveryAnswers::default())
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(
        e.message,
        "Open Failed: No VizDAW project at /p/Song.vizdaw."
    );
    assert!(s.alert().is_none(), "the call took the alert");
    let e = s
        .open_answered(Path::new("/tmp/a.txt"), &RecoveryAnswers::default())
        .unwrap_err();
    assert_eq!(e.message, "Open Failed: a.txt is not a .vizdaw project.");
    // A package's project.json and a MIDI file open as the UI's do.
    let a = s
        .open_answered(
            Path::new("/p/Song.vizdaw/project.json"),
            &RecoveryAnswers::default(),
        )
        .unwrap();
    assert!(s.document(a).is_some());
    next_project(&core);
    let m = s
        .open_answered(Path::new("/tmp/a.mid"), &RecoveryAnswers::default())
        .unwrap();
    assert!(s.document(m).is_some() && m != a);
}

// ---- projects the channel's imports make (InterchangeCommandsTests) ----

fn t(s: &str) -> ArgValue {
    ArgValue::Text(s.into())
}

fn args(pairs: &[(&str, ArgValue)]) -> Vec<(String, ArgValue)> {
    pairs
        .iter()
        .map(|(n, v)| ((*n).to_owned(), v.clone()))
        .collect()
}

/// A store with the demo open: the store, its core, the demo and its
/// session.
fn demo() -> (DocumentStore, Arc<FakeCore>, DocId, Arc<FakeSession>) {
    let (mut s, core) = store();
    let f = Arc::new(demo_fake());
    core.hand_out("new_demo", f.clone());
    let id = s.new_demo().unwrap();
    (s, core, id, f)
}

fn midi_new(s: &DocumentStore, id: DocId, extra: &[(&str, ArgValue)]) -> MidiIntoNewProject {
    let mut pairs = vec![
        ("path", t("/tmp/a.mid")),
        ("new_project", ArgValue::Flag(true)),
    ];
    pairs.extend_from_slice(extra);
    match request(ic::IMPORT_MIDI, &args(&pairs), s.document(id).unwrap()) {
        Ok(InterchangeRequest::MidiIntoNewProject(m)) => m,
        other => panic!("{other:?}"),
    }
}

#[test]
fn new_project_makes_a_document() {
    let (mut s, core, id, f) = demo();
    s.set_default_format(format(96_000, BitDepthRow::Int16));
    f.set_midi_file_info("/tmp/a.mid", midi_info());
    let m = midi_new(&s, id, &[]);
    assert_eq!(s.count(), 1, "nothing is made before the file is read");
    // The file is read off the main thread, on this document's session.
    let session = s.document(id).unwrap().session().clone();
    let read = std::thread::spawn(move || session.midi_file_info("/tmp/a.mid".into()))
        .join()
        .unwrap();
    assert!(read.is_ok() && f.reads("midi_file_info") == 1);
    let fresh = next_project(&core);
    let (doc, _job) = s.import_midi_into_new_project(&m).unwrap();
    assert!(doc != id && s.document(doc).is_some() && s.count() == 2);
    assert!(s.made_project(doc), "the channel shows its window");
    assert_eq!(
        new_projects(&core),
        ["new_project(48000,Int16)"],
        "48 kHz at the default depth"
    );
    let jobs = started(&fresh);
    let [JobRequest::ImportMidi(r)] = jobs.as_slice() else {
        panic!("{jobs:?}");
    };
    assert!(r.new_project);
    assert_eq!(Some(r.seq), s.document(doc).unwrap().selected_seq());
    assert!(
        s.document(doc).unwrap().jobs().is_empty(),
        "the channel polls it itself"
    );
}

#[test]
fn a_refused_new_project_import_makes_no_project() {
    let (s, core, id, f) = demo();
    let _m = midi_new(&s, id, &[]);
    // The file is read first: a refusal leaves nothing behind (the store
    // is never asked).
    let e = s
        .document(id)
        .unwrap()
        .session()
        .midi_file_info("/tmp/a.mid".into());
    assert!(e.is_err());
    assert!(s.count() == 1 && core.made().len() == 1);
    // A drop track cannot exist in a new project.
    let l = request(
        ic::IMPORT_MIDI,
        &args(&[
            ("path", t("/tmp/a.mid")),
            ("new_project", ArgValue::Flag(true)),
            ("lane", ArgValue::Int(2)),
        ]),
        s.document(id).unwrap(),
    )
    .unwrap_err();
    assert!(l.code == ErrorCode::InvalidParams && l.message.contains("lane"));
    assert!(started(&f).is_empty());
    // A new project needs no sequence shown in this one.
    let (mut bare, bcore) = store();
    let bid = bare.new_empty().unwrap();
    assert_eq!(bare.document(bid).unwrap().selected_seq(), None);
    let m = midi_new(&bare, bid, &[]);
    let fresh = next_project(&bcore);
    bare.import_midi_into_new_project(&m).unwrap();
    let jobs = started(&fresh);
    let [JobRequest::ImportMidi(r)] = jobs.as_slice() else {
        panic!("{jobs:?}");
    };
    assert_eq!(r.seq, 9);
    // Refused by the made project's session: it goes again.
    let g = next_project(&bcore);
    g.set_call_error(
        "import_midi",
        Some(VizError::Busy {
            message: BUSY.into(),
        }),
    );
    let before = bare.count();
    let e = bare.import_midi_into_new_project(&m).unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::Busy, BUSY));
    assert_eq!(bare.count(), before);
}

#[test]
fn a_failed_import_closes_the_project_it_made() {
    let (mut s, core, id, _) = demo();
    let m = midi_new(&s, id, &[]);
    next_project(&core);
    let (doc, _) = s.import_midi_into_new_project(&m).unwrap();
    assert_eq!(s.count(), 2);
    s.job_ended(doc, true, |s, d| s.close(d));
    assert!(s.count() == 1 && s.document(doc).is_none() && !s.made_project(doc));
    // A successful job keeps it.
    next_project(&core);
    let (ok, _) = s.import_midi_into_new_project(&m).unwrap();
    s.job_ended(ok, false, |s, d| s.close(d));
    assert!(s.document(ok).is_some() && !s.made_project(ok));
    // Not a project the command made: left alone.
    s.job_ended(id, true, |s, d| s.close(d));
    assert!(s.document(id).is_some());
    // Edited meanwhile: kept.
    let g = next_project(&core);
    let (edited, _) = s.import_midi_into_new_project(&m).unwrap();
    g.set_undo_label(Some("Rename"));
    let snap = g.snap();
    s.document_mut(edited).unwrap().apply(snap);
    s.job_ended(edited, true, |s, d| s.close(d));
    assert!(s.document(edited).is_some());
}

fn dawproject_read(s: &DocumentStore, id: DocId, pairs: &[(&str, ArgValue)]) -> ic::DawprojectRead {
    match request(ic::IMPORT_DAWPROJECT, &args(pairs), s.document(id).unwrap()) {
        Ok(InterchangeRequest::ReadDawproject(r)) => r,
        other => panic!("{other:?}"),
    }
}

fn file() -> (&'static str, ArgValue) {
    ("path", t("/tmp/back.dawproject"))
}

fn merge() -> (&'static str, ArgValue) {
    ("mode", t("merge"))
}

fn merge_requests(f: &FakeSession) -> Vec<vizdaw_ffi::DawprojectMergeRequestRow> {
    started(f)
        .into_iter()
        .filter_map(|r| match r {
            JobRequest::MergeDawproject(m) => Some(m),
            _ => None,
        })
        .collect()
}

/// A demo session added to `s` (the next `new_demo`).
fn add(s: &mut DocumentStore, core: &FakeCore) -> (DocId, Arc<FakeSession>) {
    let f = Arc::new(demo_fake());
    core.hand_out("new_demo", f.clone());
    (s.new_demo().unwrap(), f)
}

#[test]
fn merge_finds_the_open_project_by_id_then_by_snapshot() {
    let (mut s, core, id, f) = demo();
    let (a, by_id) = add(&mut s, &core);
    by_id.set_project_id(Some(PID));
    let (b, by_snapshot) = add(&mut s, &core);
    by_snapshot.set_snapshot_ids(&[EID]);
    // The snapshot holder is the most recently active: the ID still wins.
    s.focus_document(b, WindowToken(1));
    let read = dawproject_read(&s, id, &[file(), merge()]);
    let info = returned(Some(PID), Some(EID));
    let Ok(DawprojectNext::Merge { project: None }) = read.after_read(&info) else {
        panic!();
    };
    let (doc, _) = s
        .merge_dawproject(&read, &info, "2026-11-23".into())
        .unwrap();
    assert_eq!(doc, a);
    assert_eq!(
        merge_requests(&by_id),
        [vizdaw_ffi::DawprojectMergeRequestRow {
            path: "/tmp/back.dawproject".into(),
            snapshot: None,
            step: MergeStepRow::Apply,
            date: "2026-11-23".into(),
        }]
    );
    assert!(merge_requests(&f).is_empty() && merge_requests(&by_snapshot).is_empty());
    assert!(!s.made_project(a), "an open project was not made");
    // A record with no project ID: the snapshot holder.
    let info = returned(None, Some(EID));
    let (doc2, _) = s
        .merge_dawproject(&read, &info, "2026-11-23".into())
        .unwrap();
    assert_eq!(doc2, b);
    assert_eq!(merge_requests(&by_snapshot).len(), 1);
    // `snapshot` is passed through.
    let read = dawproject_read(&s, id, &[file(), merge(), ("snapshot", t(EID))]);
    s.merge_dawproject(&read, &returned(Some(PID), Some(EID)), "2026-11-23".into())
        .unwrap();
    assert_eq!(
        merge_requests(&by_id).last().unwrap().snapshot.as_deref(),
        Some(EID)
    );
    // No match and no `project`: the Open dialog.
    let e = s
        .merge_dawproject(&read, &returned(Some("other"), None), "2026-11-23".into())
        .unwrap_err();
    assert_eq!(e, ic::merge_target_dialog());
}

#[test]
fn merge_into_a_project_path_opens_it() {
    let dir = tempfile::tempdir().unwrap();
    let package = dir.path().join("Song.vizdaw");
    std::fs::create_dir(&package).unwrap();
    let project = package.to_string_lossy().into_owned();
    let (mut s, core, id, f) = demo();
    let opened = Arc::new(demo_fake());
    opened.set_path(Some(&project));
    core.hand_out("open", opened.clone());
    let read = dawproject_read(&s, id, &[file(), merge(), ("project", t(&project))]);
    let info = returned(Some(PID), Some(EID));
    let (doc, _) = s
        .merge_dawproject(&read, &info, "2026-11-23".into())
        .unwrap();
    assert!(doc != id && s.document(doc).is_some());
    assert!(core.calls().contains(&format!("open({project})")));
    assert_eq!(merge_requests(&opened).len(), 1);
    assert!(merge_requests(&f).is_empty());
    assert!(s.made_project(doc), "the channel shows its window");
    // Already open now: refused.
    let e = s
        .merge_dawproject(&read, &info, "2026-11-23".into())
        .unwrap_err();
    assert_eq!(e.message, ic::PROJECT_ALREADY_OPEN);
    // A refused job closes it again.
    s.job_ended(doc, true, |s, d| s.close(d));
    assert!(s.document(doc).is_none() && s.count() == 1);
    // Refused before its job: closed at once.
    let refusing = Arc::new(demo_fake());
    refusing.set_path(Some(&project));
    refusing.set_call_error(
        "merge_dawproject",
        Some(VizError::Busy {
            message: BUSY.into(),
        }),
    );
    core.hand_out("open", refusing);
    let e = s
        .merge_dawproject(&read, &info, "2026-11-23".into())
        .unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::Busy, BUSY));
    assert_eq!(s.count(), 1);
}

#[test]
fn a_busy_dawproject_job_is_refused_with_nothing_made_or_written() {
    let (mut s, core, id, _) = demo();
    let read = dawproject_read(&s, id, &[("path", t("/tmp/a.dawproject"))]);
    let Ok(DawprojectNext::NewProject { path, rate }) = read.after_read(&plain_file(48_000)) else {
        panic!();
    };
    let fresh = next_project(&core);
    fresh.set_call_error(
        "import_dawproject",
        Some(VizError::Busy {
            message: BUSY.into(),
        }),
    );
    let before = s.count();
    let e = s
        .import_dawproject_into_new_project(&path, rate)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert!(e.message.contains("Wait for the running"));
    assert!(s.count() == before && started(&fresh).is_empty());
    assert_eq!(
        new_projects(&core).len(),
        1,
        "one project was made, then closed"
    );
    assert!(fresh.calls().contains(&"close".to_string()));
}

#[test]
fn merge_while_the_target_records_is_busy() {
    let (mut s, _core, id, f) = demo();
    f.set_project_id(Some(PID));
    let held = "Finish or cancel the merge waiting to be confirmed.";
    f.set_call_error(
        "merge_dawproject",
        Some(VizError::Busy {
            message: held.into(),
        }),
    );
    let read = dawproject_read(&s, id, &[file(), merge()]);
    let e = s
        .merge_dawproject(&read, &returned(Some(PID), Some(EID)), "2026-11-23".into())
        .unwrap_err();
    assert_eq!((e.code, e.message.as_str()), (ErrorCode::Busy, held));
    assert!(
        s.document(id).is_some(),
        "an open project the command did not open stays"
    );
}

#[test]
fn aaf_refused_import_closes_its_project() {
    let (mut s, core, id, _) = demo();
    let r = match request(
        ic::IMPORT_AAF,
        &args(&[("path", t("/tmp/a.aaf"))]),
        s.document(id).unwrap(),
    ) {
        Ok(InterchangeRequest::AafIntoNewProject(r)) => r,
        other => panic!("{other:?}"),
    };
    let fresh = next_project(&core);
    fresh.set_call_error(
        "import_aaf",
        Some(VizError::Busy {
            message: BUSY.into(),
        }),
    );
    let before = s.count();
    let e = s
        .import_aaf_into_new_project(r.clone(), 48_000)
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert!(s.count() == before && started(&fresh).is_empty());
    assert!(core.made().len() == 2 && !s.made_project(DocId(2)));
    // Started: made, at the file's rate.
    s.set_default_format(format(96_000, BitDepthRow::Int16));
    let g = next_project(&core);
    let (doc, _) = s.import_aaf_into_new_project(r.clone(), 44_100).unwrap();
    assert!(s.made_project(doc));
    assert_eq!(
        new_projects(&core).last().unwrap(),
        "new_project(44100,Int16)"
    );
    assert_eq!(started(&g), [JobRequest::ImportAaf(r)]);
}

/// The store half: the read runs off the main thread through the store's
/// core, then the project is made at the file's rate.
#[test]
fn import_dawproject_reads_off_the_main_actor_then_makes_the_project() {
    let (mut s, core, id, _) = demo();
    s.set_default_format(format(96_000, BitDepthRow::Int16));
    core.set_dawproject_info("/tmp/a.dawproject", plain_file(44_100));
    let read = dawproject_read(&s, id, &[("path", t("/tmp/a.dawproject"))]);
    let before = s.count();
    let c = s.core();
    let info = std::thread::spawn(move || c.dawproject_info("/tmp/a.dawproject"))
        .join()
        .unwrap()
        .unwrap();
    assert_eq!(s.count(), before, "no project before the file is read");
    let Ok(DawprojectNext::NewProject { path, rate }) = read.after_read(&info) else {
        panic!();
    };
    let fresh = next_project(&core);
    let (doc, _) = s.import_dawproject_into_new_project(&path, rate).unwrap();
    assert!(doc != id && s.document(doc).is_some() && s.made_project(doc));
    assert_eq!(new_projects(&core), ["new_project(44100,Int16)"]);
    assert_eq!(
        started(&fresh),
        [JobRequest::ImportDawproject(DawprojectImportRequestRow {
            path: "/tmp/a.dawproject".into()
        })]
    );
    s.job_ended(doc, false, |s, d| s.close(d));
    // A refused file: no project, the core's sentence.
    let e = s.core().dawproject_info("/tmp/b.dawproject").unwrap_err();
    assert!(vizdaw_app::alerts::error_message(&e).contains("could not be read"));
    assert_eq!(s.count(), before + 1);
    assert_eq!(new_projects(&core).len(), 1);
}

/// A made project the user closes before its job ends is forgotten.
#[test]
fn closing_a_made_project_forgets_it() {
    let (mut s, core, id, _) = demo();
    let m = midi_new(&s, id, &[]);
    next_project(&core);
    let (doc, _) = s.import_midi_into_new_project(&m).unwrap();
    s.close(doc);
    assert!(!s.made_project(doc));
    let mut closed = false;
    s.job_ended(doc, true, |_, _| closed = true);
    assert!(!closed);
}
