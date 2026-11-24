//! The File commands over the control channel in the GPUI app (slice
//! 7a): job replies deferred to the job's end, background reads, made
//! projects' windows, the new projects and demos, the opens and
//! `state.get media|project`, through the real pump with a scripted core
//! and sessions.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use core_control::protocol::{RawRequest, RequestId};
use core_control::server::Call;
use core_control::{ControlError, ErrorCode};
use gpui::TestAppContext;
use serde_json::{Map, Value, json};
use vizdaw_app::testing::{FakeCore, FakeSession, JobRequest, JobScript, demo_fake, seq};
use vizdaw_app::{CoreCalls, DocSession, DocumentStore};
use vizdaw_ffi::{
    AafInfoRow, BitDepthRow, BounceSettingsRow, DawprojectInfoRow, MediaOriginRow, MediaRow,
    MidiFileInfoRow, MidiFileKindRow, OpenOptions, ProjectFormatRow, RecoveryInfo, ReportRow,
    VizError,
};
use vizdaw_gpui::AppState;
use vizdaw_gpui::control::{self, CallSource};

/// Calls queued by the test; replies (every one, in order) and timeout
/// extensions kept.
#[derive(Default)]
struct FakeCalls {
    queue: RefCell<VecDeque<Call>>,
    replies: RefCell<Vec<(u64, Result<Value, ControlError>)>>,
    extended: RefCell<Vec<(u64, Duration)>>,
    next: RefCell<u64>,
}

impl FakeCalls {
    fn push(&self, cmd: &str, params: Value) -> u64 {
        self.try_push(cmd, params).expect("a valid request")
    }

    /// Queues the call, or answers the server's own refusal (it checks a
    /// request against the registry, and its always-asked dialogs, before
    /// the app sees it).
    fn try_push(&self, cmd: &str, params: Value) -> Result<u64, ControlError> {
        let params: Map<String, Value> = match params {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let raw = RawRequest {
            id: RequestId::Num(1),
            cmd: cmd.into(),
            params,
        };
        let command = core_control::validate(&raw)?;
        let token = {
            let mut n = self.next.borrow_mut();
            *n += 1;
            *n
        };
        self.queue.borrow_mut().push_back(Call {
            token,
            command,
            conn: 1,
            audit_token: None,
        });
        Ok(token)
    }

    /// Every reply `token` got.
    fn replies_to(&self, token: u64) -> Vec<Result<Value, ControlError>> {
        self.replies
            .borrow()
            .iter()
            .filter(|(t, _)| *t == token)
            .map(|(_, r)| r.clone())
            .collect()
    }
}

impl CallSource for FakeCalls {
    fn next_call(&self) -> Option<Call> {
        self.queue.borrow_mut().pop_front()
    }

    fn reply(&self, token: u64, outcome: Result<Value, ControlError>) -> bool {
        self.replies.borrow_mut().push((token, outcome));
        true
    }

    fn extend(&self, token: u64, by: Duration) -> bool {
        self.extended.borrow_mut().push((token, by));
        true
    }
}

struct Setup {
    /// The first document's session (the demo).
    fake: Arc<FakeSession>,
    core: Arc<FakeCore>,
    calls: FakeCalls,
}

/// The app over a fake core, with the demo open in a window.
fn setup(cx: &mut TestAppContext) -> Setup {
    let core = Arc::new(FakeCore::new());
    setup_with(cx, core.clone(), core)
}

/// [`setup`] with `calls` as the store's core (`core` scripts it).
fn setup_with(cx: &mut TestAppContext, core: Arc<FakeCore>, calls: Arc<dyn CoreCalls>) -> Setup {
    let fake = Arc::new(demo_fake());
    core.hand_out("new_demo", fake.clone());
    let store = DocumentStore::with_core(calls);
    cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
    });
    let s = Setup {
        fake,
        core,
        calls: FakeCalls::default(),
    };
    ok(call(&s, cx, "app.new_demo", json!({})));
    s
}

/// One pump tick, then everything ready runs (background reads land).
fn pump(s: &Setup, cx: &mut TestAppContext) {
    cx.update(|cx| control::pump(&s.calls, cx));
    cx.run_until_parked();
}

/// Sends one call and pumps until it is answered (at most 20 ticks).
fn call(
    s: &Setup,
    cx: &mut TestAppContext,
    cmd: &str,
    params: Value,
) -> Result<Value, ControlError> {
    let token = s.calls.try_push(cmd, params)?;
    for _ in 0..20 {
        pump(s, cx);
        if let Some(r) = s.calls.replies_to(token).pop() {
            return r;
        }
    }
    panic!("{cmd} was never answered");
}

fn run(s: &Setup, cx: &mut TestAppContext, id: &str, params: Value) -> Result<Value, ControlError> {
    call(s, cx, "command.run", json!({"id": id, "params": params}))
}

fn ok(r: Result<Value, ControlError>) -> Value {
    r.unwrap_or_else(|e| panic!("call failed: {e:?}"))
}

fn documents(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| AppState::global(cx).read(cx).store.count())
}

fn window_numbers(s: &Setup, cx: &mut TestAppContext) -> Vec<i64> {
    let w = ok(call(s, cx, "state.get", json!({"target": "windows"})));
    w["windows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["number"].as_i64().unwrap())
        .collect()
}

fn report(summary: &str) -> ReportRow {
    ReportRow {
        summary: summary.into(),
        items: vec![],
    }
}

fn midi_info() -> MidiFileInfoRow {
    MidiFileInfoRow {
        kind: MidiFileKindRow::Smf1,
        tracks: 2,
        channels: vec![],
        has_tempo: true,
        mpe_found: false,
    }
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

/// A new project's session, handed out by the next `new_project`.
fn next_project(core: &FakeCore) -> Arc<FakeSession> {
    let f = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    core.hand_out("new_project", f.clone());
    f
}

// ---------------------------------------------------------------------
// Job replies

#[gpui::test]
fn a_done_job_answers_once_when_it_ends(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.script_job(JobScript::done().steps(&[0.2, 0.6]));
    let token = s.calls.push(
        "command.run",
        json!({"id": "media.import", "params": {"path": "/tmp/a.wav"}}),
    );
    pump(&s, cx);
    // Running: not answered yet, and the call may wait 600 s + 5 s.
    assert!(s.calls.replies_to(token).is_empty());
    assert_eq!(
        *s.calls.extended.borrow(),
        vec![(token, Duration::from_millis(605_000))]
    );
    for _ in 0..5 {
        pump(&s, cx);
    }
    let replies = s.calls.replies_to(token);
    assert_eq!(replies.len(), 1, "{replies:?}");
    let r = ok(replies[0].clone());
    assert_eq!(r["command"], "media.import");
    assert!(r["window"].is_i64());
    assert_eq!(
        s.fake.jobs_started()[0].request,
        JobRequest::Import {
            paths: vec!["/tmp/a.wav".into()],
            target: None,
        }
    );
    // The document's own job list never had it.
    let listed = cx.update(|cx| {
        let s = AppState::global(cx);
        let s = s.read(cx);
        let doc = s.store.front().unwrap();
        s.store.document(doc).unwrap().jobs().len()
    });
    assert_eq!(listed, 0);
}

#[gpui::test]
fn a_failed_job_answers_its_message_once(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake
        .script_job(JobScript::failed("the file is damaged").problems(&["a.wav: bad header"]));
    let token = s.calls.push(
        "command.run",
        json!({"id": "media.import", "params": {"path": "/tmp/a.wav"}}),
    );
    for _ in 0..4 {
        pump(&s, cx);
    }
    let replies = s.calls.replies_to(token);
    assert_eq!(replies.len(), 1);
    let e = replies[0].clone().unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "the file is damaged; a.wav: bad header");
}

#[gpui::test]
fn a_cancelled_job_answers_once(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.script_job(JobScript::cancelled().steps(&[0.5]));
    let token = s.calls.push("command.run", json!({"id": "media.convert"}));
    for _ in 0..4 {
        pump(&s, cx);
    }
    let replies = s.calls.replies_to(token);
    assert_eq!(replies.len(), 1);
    let e = replies[0].clone().unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "the job was cancelled");
}

#[gpui::test]
fn a_job_still_running_after_ten_minutes_answers_the_expiry(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.script_job(JobScript::done().steps(&[0.1; 50]));
    let token = s.calls.push("command.run", json!({"id": "media.convert"}));
    pump(&s, cx);
    cx.executor().advance_clock(Duration::from_secs(599));
    pump(&s, cx);
    assert!(s.calls.replies_to(token).is_empty(), "not yet expired");
    cx.executor().advance_clock(Duration::from_secs(1));
    pump(&s, cx);
    let id = s.fake.jobs_started()[0].id;
    let replies = s.calls.replies_to(token);
    assert_eq!(replies.len(), 1);
    let e = replies[0].clone().unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout);
    assert_eq!(
        e.message,
        format!("job {id} is still running after 10 minutes; it was not cancelled")
    );
    // Not cancelled, and never answered again.
    assert!(s.fake.cancelled_jobs().is_empty());
    pump(&s, cx);
    assert_eq!(s.calls.replies_to(token).len(), 1);
}

#[gpui::test]
fn a_window_closed_mid_job_answers_once_and_drops_the_reply(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.script_job(JobScript::done().steps(&[0.1; 50]));
    let token = s.calls.push("command.run", json!({"id": "media.convert"}));
    pump(&s, cx);
    ok(run(&s, cx, "file.close", json!({})));
    pump(&s, cx);
    let replies = s.calls.replies_to(token);
    assert_eq!(replies.len(), 1, "{replies:?}");
    let e = replies[0].clone().unwrap_err();
    assert_eq!(e.code, ErrorCode::NoDocument);
    for _ in 0..3 {
        pump(&s, cx);
    }
    assert_eq!(s.calls.replies_to(token).len(), 1);
    let tick = cx.update(|cx| control::pump(&s.calls, cx));
    assert_eq!(tick.deferred, 0);
}

#[gpui::test]
fn a_refused_job_start_is_the_call_failure(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.fail_next(
        "convert_media",
        VizError::Busy {
            message: "a job is running".into(),
        },
    );
    let e = run(&s, cx, "media.convert", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    s.fake.fail_next(
        "import_audio",
        VizError::Io {
            message: "no such file".into(),
        },
    );
    let e = run(&s, cx, "media.import", json!({"path": "/tmp/x.wav"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "Import Failed: no such file");
}

#[gpui::test]
fn file_commands_ask_for_their_dialogs(cx: &mut TestAppContext) {
    let s = setup(cx);
    for (id, dialog) in [
        ("media.import", "Open"),
        ("file.project_settings", "Project Settings"),
        ("file.bounce", "Bounce"),
        ("file.import_midi", "Open"),
        ("file.export_midi", "Save"),
        ("file.import_dawproject", "Open"),
        ("file.import_aaf", "Open"),
        ("file.open", "Open"),
        ("file.open_copy", "Open"),
    ] {
        let e = run(&s, cx, id, json!({})).unwrap_err();
        assert_eq!(e.code, ErrorCode::DialogRequired, "{id}");
        assert!(e.message.contains(dialog), "{id}: {}", e.message);
    }
    assert!(s.fake.jobs_started().is_empty());
}

#[gpui::test]
fn project_settings_changes_the_format_as_a_job(cx: &mut TestAppContext) {
    let s = setup(cx);
    let r = ok(run(
        &s,
        cx,
        "file.project_settings",
        json!({"rate": "96000"}),
    ));
    assert_eq!(r["command"], "file.project_settings");
    assert_eq!(
        s.fake.jobs_started()[0].request,
        JobRequest::Format(ProjectFormatRow {
            sample_rate: 96_000,
            depth: BitDepthRow::Int24,
        })
    );
    let e = run(&s, cx, "file.project_settings", json!({"rate": "1"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

#[gpui::test]
fn a_bounce_answers_with_what_it_wrote(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake
        .script_job(JobScript::done().bounce(vizdaw_ffi::BounceResultRow {
            files: vec![vizdaw_ffi::BouncedFileRow {
                path: "/tmp/Bounce.wav".into(),
                frames: 480,
                rate: 48_000,
                bits: "24-bit".into(),
                channels: 2,
                clipped: 0,
            }],
            media: vec![],
        }));
    let r = ok(run(
        &s,
        cx,
        "file.bounce",
        json!({"path": "/tmp/Bounce.wav"}),
    ));
    assert_eq!(
        r["status"],
        "wrote /tmp/Bounce.wav (480 frames, 48000 Hz, 24-bit)"
    );
}

// ---------------------------------------------------------------------
// Interchange

/// Swift `reportCommandCallsTheSessionOnce`.
#[gpui::test]
fn report_command_calls_the_session_once(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake
        .set_interchange_report(Some(report("Imported 2 tracks")));
    let calls_before = s.fake.calls().len();
    let r = ok(run(&s, cx, "file.interchange_report", json!({})));
    assert_eq!(r["command"], "file.interchange_report");
    assert_eq!(r["window"], Value::Null);
    assert!(r["doc"].is_string());
    assert_eq!(r["report"]["summary"], "Imported 2 tracks");
    assert_eq!(r["written"], json!([]));
    assert_eq!(s.fake.reads("interchange_report"), 1);
    // Nothing but the read.
    assert_eq!(s.fake.calls().len(), calls_before);
    assert!(s.fake.jobs_started().is_empty());
    s.fake.set_interchange_report(None);
    let none = ok(run(&s, cx, "file.interchange_report", json!({})));
    assert_eq!(none["report"], Value::Null, "no report yet");
    assert_eq!(s.fake.reads("interchange_report"), 2);
}

#[gpui::test]
fn an_export_answers_as_interchange_with_what_it_wrote(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.script_job(
        JobScript::done()
            .report(report("Exported 2 tracks"))
            .written(&["/tmp/Song.mid"]),
    );
    let r = ok(run(
        &s,
        cx,
        "file.export_midi",
        json!({"path": "/tmp/Song.mid"}),
    ));
    assert_eq!(r["command"], "file.export_midi");
    assert_eq!(r["report"]["summary"], "Exported 2 tracks");
    assert_eq!(r["written"], json!(["/tmp/Song.mid"]));
    assert!(r["window"].is_i64());
}

#[gpui::test]
fn a_dry_run_reads_off_the_ui_thread(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake
        .set_check_report("export_midi_check", report("Would export 2 tracks"));
    let token = s.calls.push(
        "command.run",
        json!({"id": "file.export_midi", "params": {"dry_run": true}}),
    );
    cx.update(|cx| control::pump(&s.calls, cx));
    // Not read inside the pump's update: the background task runs later.
    assert_eq!(s.fake.reads("export_midi_check"), 0);
    assert!(s.calls.replies_to(token).is_empty());
    cx.run_until_parked();
    assert_eq!(s.fake.reads("export_midi_check"), 1);
    pump(&s, cx);
    let replies = s.calls.replies_to(token);
    assert_eq!(replies.len(), 1);
    let r = ok(replies[0].clone());
    assert_eq!(r["report"]["summary"], "Would export 2 tracks");
    assert_eq!(r["written"], json!([]));
    assert_eq!(r["window"], Value::Null);
    assert!(s.fake.jobs_started().is_empty());
    assert_eq!(s.calls.extended.borrow().len(), 1);
}

#[gpui::test]
fn a_dry_run_for_a_document_that_closed_is_dropped(cx: &mut TestAppContext) {
    let s = setup(cx);
    let token = s.calls.push(
        "command.run",
        json!({"id": "file.export_midi", "params": {"dry_run": true}}),
    );
    cx.update(|cx| control::pump(&s.calls, cx));
    // The window closes before the read lands.
    ok(run(&s, cx, "file.close", json!({})));
    pump(&s, cx);
    let replies = s.calls.replies_to(token);
    assert_eq!(replies.len(), 1);
    assert_eq!(replies[0].clone().unwrap_err().code, ErrorCode::NoDocument);
}

#[gpui::test]
fn a_midi_import_into_a_new_project_names_its_window_once_it_shows(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.set_midi_file_info("/tmp/Song.mid", midi_info());
    let made = next_project(&s.core);
    made.script_job(
        JobScript::done()
            .steps(&[0.5])
            .report(report("Imported 2 tracks")),
    );
    let before = window_numbers(&s, cx);
    let r = ok(run(
        &s,
        cx,
        "file.import_midi",
        json!({"path": "/tmp/Song.mid", "new_project": true}),
    ));
    assert_eq!(r["command"], "file.import_midi");
    assert_eq!(r["report"]["summary"], "Imported 2 tracks");
    let after = window_numbers(&s, cx);
    assert_eq!(after.len(), before.len() + 1);
    let window = r["window"].as_i64().expect("the made project's window");
    assert!(after.contains(&window) && !before.contains(&window));
    // The import went into the made project's shown sequence.
    let JobRequest::ImportMidi(req) = &made.jobs_started()[0].request else {
        panic!("not a MIDI import");
    };
    assert_eq!(req.seq, 9);
    assert!(
        s.core
            .calls()
            .iter()
            .any(|c| c == "new_project(48000,Int24)"),
        "{:?}",
        s.core.calls()
    );
}

#[gpui::test]
fn a_made_project_closed_after_a_failed_import_is_gone_from_the_windows(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.set_midi_file_info("/tmp/Song.mid", midi_info());
    let made = next_project(&s.core);
    made.script_job(JobScript::failed("the file is damaged").steps(&[0.5]));
    let before = window_numbers(&s, cx);
    let e = run(
        &s,
        cx,
        "file.import_midi",
        json!({"path": "/tmp/Song.mid", "new_project": true}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "the file is damaged");
    assert_eq!(window_numbers(&s, cx), before);
    assert_eq!(documents(cx), 1);
}

#[gpui::test]
fn an_unreadable_midi_file_makes_no_project(cx: &mut TestAppContext) {
    let s = setup(cx);
    let e = run(
        &s,
        cx,
        "file.import_midi",
        json!({"path": "/tmp/Missing.mid", "new_project": true}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(
        e.message,
        "Import Failed: /tmp/Missing.mid could not be read"
    );
    assert_eq!(documents(cx), 1);
    assert!(!s.core.calls().iter().any(|c| c.starts_with("new_project")));
}

#[gpui::test]
fn a_dawproject_import_reads_the_file_then_makes_its_project(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core
        .set_dawproject_info("/tmp/Song.dawproject", plain_file(44_100));
    let made = next_project(&s.core);
    made.script_job(JobScript::done().report(report("Imported 2 tracks")));
    let r = ok(run(
        &s,
        cx,
        "file.import_dawproject",
        json!({"path": "/tmp/Song.dawproject"}),
    ));
    assert_eq!(r["command"], "file.import_dawproject");
    assert!(r["window"].is_i64());
    assert!(
        s.core
            .calls()
            .contains(&"new_project(44100,Int24)".to_string())
    );
    assert_eq!(documents(cx), 2);
    // A file from VizDAW without `mode` asks for the Return dialog, and
    // nothing is made.
    s.core.set_dawproject_info(
        "/tmp/Back.dawproject",
        DawprojectInfoRow {
            from_vizdaw: true,
            ..plain_file(48_000)
        },
    );
    let e = run(
        &s,
        cx,
        "file.import_dawproject",
        json!({"path": "/tmp/Back.dawproject"}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::DialogRequired);
    assert!(e.message.contains("Return"), "{}", e.message);
    assert_eq!(documents(cx), 2);
}

#[gpui::test]
fn an_aaf_import_makes_a_project_at_the_file_rate(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core.set_aaf_info(
        "/tmp/Song.aaf",
        AafInfoRow {
            sample_rate: 96_000,
            tracks: 2,
            audio_files: 1,
            offline: 0,
            from_vizdaw: false,
            midi_beside: None,
        },
    );
    let made = next_project(&s.core);
    made.script_job(JobScript::done().report(report("Imported 2 tracks")));
    let r = ok(run(
        &s,
        cx,
        "file.import_aaf",
        json!({"path": "/tmp/Song.aaf"}),
    ));
    assert_eq!(r["command"], "file.import_aaf");
    assert!(r["window"].is_i64());
    assert!(
        s.core
            .calls()
            .contains(&"new_project(96000,Int24)".to_string())
    );
}

// ---------------------------------------------------------------------
// New projects, demos, opens

#[gpui::test]
fn file_new_takes_a_rate_and_depth(cx: &mut TestAppContext) {
    let s = setup(cx);
    let r = ok(run(
        &s,
        cx,
        "file.new",
        json!({"rate": "44100", "depth": "16"}),
    ));
    assert_eq!(r["command"], "file.new");
    assert!(window_numbers(&s, cx).contains(&r["window"].as_i64().unwrap()));
    assert!(
        s.core
            .calls()
            .contains(&"new_project(44100,Int16)".to_string())
    );
    // The registry refuses a depth it does not list before the app sees it.
    let e = run(&s, cx, "file.new", json!({"depth": "8"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

#[gpui::test]
fn file_new_uses_the_default_format_from_the_preferences(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| {
        AppState::global(cx).update(cx, |st, _| {
            let mut prefs = vizdaw_app::Prefs::in_memory();
            prefs
                .set_default_format(ProjectFormatRow {
                    sample_rate: 88_200,
                    depth: BitDepthRow::Float32,
                })
                .unwrap();
            st.set_prefs(prefs);
        })
    });
    ok(run(&s, cx, "file.new", json!({})));
    assert!(
        s.core
            .calls()
            .contains(&"new_project(88200,Float32)".to_string())
    );
}

#[gpui::test]
fn the_demos_open_in_windows(cx: &mut TestAppContext) {
    let s = setup(cx);
    let a = ok(run(&s, cx, "file.create_demo", json!({})));
    assert_eq!(a["command"], "file.create_demo");
    let b = ok(run(&s, cx, "file.create_demo_audio", json!({})));
    assert_eq!(b["command"], "file.create_demo_audio");
    let c = ok(call(&s, cx, "app.new_demo", json!({"audio": true})));
    let windows = window_numbers(&s, cx);
    for r in [&a, &b, &c] {
        assert!(windows.contains(&r["window"].as_i64().unwrap()), "{r}");
    }
    let audio = s
        .core
        .calls()
        .iter()
        .filter(|c| *c == "new_demo_audio")
        .count();
    assert_eq!(audio, 2);
    // A demo that cannot be made: the alert is the failure.
    s.core.fail_next(
        "new_demo_audio",
        VizError::Io {
            message: "no demo".into(),
        },
    );
    let e = run(&s, cx, "file.create_demo_audio", json!({})).unwrap_err();
    assert_eq!(e.message, "Demo Project Failed: no demo");
}

#[gpui::test]
fn app_open_asks_nothing_and_answers_the_dialog(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core.set_recovery(
        "/tmp/x/Song.vizdaw",
        RecoveryInfo {
            crashed: false,
            loading: vec![],
            autosave: Some("/tmp/x/Song.vizdaw/autosave".into()),
        },
    );
    let e = call(&s, cx, "app.open", json!({"path": "/tmp/x/Song.vizdaw"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::DialogRequired);
    assert!(e.message.contains("Restore Autosave"), "{}", e.message);
    assert_eq!(documents(cx), 1);
    let r = ok(call(
        &s,
        cx,
        "app.open",
        json!({"path": "/tmp/x/Song.vizdaw", "restore_autosave": false}),
    ));
    assert!(window_numbers(&s, cx).contains(&r["window"].as_i64().unwrap()));
    // Open already: refused, not opened twice.
    let e = run(&s, cx, "file.open", json!({"path": "/tmp/x/Song.vizdaw"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Busy);
    assert_eq!(documents(cx), 2);
    // A DAWproject has its own command (the registry refuses it first).
    let e = run(&s, cx, "file.open", json!({"path": "/tmp/x/A.dawproject"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

#[gpui::test]
fn open_copy_refuses_what_is_not_a_copy(cx: &mut TestAppContext) {
    let s = setup(cx);
    let e = run(
        &s,
        cx,
        "file.open_copy",
        json!({"path": "/tmp/Song.vizdaw"}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "That is not a Co-Producer copy.");
    let r = ok(run(
        &s,
        cx,
        "file.open_copy",
        json!({"path": "/tmp/Song.vizdaw/Co-Producer/Copies/Take.vizdaw"}),
    ));
    assert_eq!(r["command"], "file.open_copy");
    assert!(window_numbers(&s, cx).contains(&r["window"].as_i64().unwrap()));
}

// ---------------------------------------------------------------------
// State

#[gpui::test]
fn state_get_media_and_project(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.set_media(vec![MediaRow {
        id: 7,
        name: "Kick".into(),
        path: "/tmp/Kick.wav".into(),
        channels: 1,
        frames: 4_800,
        sample_rate: 48_000,
        origin: MediaOriginRow::Imported,
        original_name: None,
        offline: None,
        uses: 1,
        used_percent: 100.0,
        peak: None,
    }]);
    let m = ok(call(&s, cx, "state.get", json!({"target": "media"})));
    assert_eq!(m["items"][0]["id"], 7);
    assert_eq!(m["items"][0]["name"], "Kick");
    let p = ok(call(&s, cx, "state.get", json!({"target": "project"})));
    assert_eq!(p["rate"], 48_000);
    assert_eq!(p["depth"], "24");
}

// ---------------------------------------------------------------------
// The store's own reads (the GUI's opens)

fn open_path(cx: &mut TestAppContext, path: &str) {
    cx.update(|cx| {
        vizdaw_gpui::open_path(Path::new(path), cx);
    });
    cx.run_until_parked();
}

fn reading(cx: &mut TestAppContext) -> bool {
    cx.update(|cx| AppState::global(cx).read(cx).store.reading())
}

#[gpui::test]
fn a_gui_dawproject_open_reads_off_the_ui_thread_then_shows_its_project(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core
        .set_dawproject_info("/tmp/Song.dawproject", plain_file(44_100));
    next_project(&s.core).script_job(JobScript::done().steps(&[0.5; 3]));
    open_path(cx, "/tmp/Song.dawproject");
    assert!(!reading(cx));
    assert_eq!(documents(cx), 2);
    assert_eq!(window_numbers(&s, cx).len(), 2);
}

#[gpui::test]
fn a_failed_gui_read_lands_and_reading_ends(cx: &mut TestAppContext) {
    let s = setup(cx);
    open_path(cx, "/tmp/Missing.dawproject");
    assert!(!reading(cx));
    assert_eq!(documents(cx), 1);
    let alert = cx.update(|cx| AppState::global(cx).read(cx).store.alert().cloned());
    assert_eq!(alert.map(|a| a.title), Some("Import Failed".into()));
    assert_eq!(window_numbers(&s, cx).len(), 1);
}

#[gpui::test]
fn a_project_closed_before_its_window_opened_is_only_forgotten(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core
        .set_dawproject_info("/tmp/Song.dawproject", plain_file(44_100));
    // The import fails at its first poll: the store closes the project
    // it made before any window showed it.
    next_project(&s.core).script_job(JobScript::failed("the file is damaged"));
    open_path(cx, "/tmp/Song.dawproject");
    pump(&s, cx);
    assert!(!reading(cx));
    assert_eq!(documents(cx), 1);
    assert_eq!(window_numbers(&s, cx).len(), 1);
}

#[gpui::test]
fn a_gui_midi_open_whose_import_fails_closes_its_window(cx: &mut TestAppContext) {
    let s = setup(cx);
    next_project(&s.core).script_job(JobScript::failed("the file is damaged").steps(&[0.5]));
    open_path(cx, "/tmp/Song.mid");
    assert_eq!(window_numbers(&s, cx).len(), 2);
    // The window's poll ends the job; the store closes the untouched
    // project and its window goes.
    for _ in 0..4 {
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
    assert_eq!(documents(cx), 1);
    assert_eq!(window_numbers(&s, cx).len(), 1);
}

/// A core whose DAWproject read panics (a read that can never land).
struct PanickyCore(Arc<FakeCore>);

impl CoreCalls for PanickyCore {
    fn new_empty(&self) -> Result<Arc<dyn DocSession>, VizError> {
        self.0.new_empty()
    }
    fn new_demo(&self) -> Result<Arc<dyn DocSession>, VizError> {
        self.0.new_demo()
    }
    fn new_demo_audio(&self) -> Result<Arc<dyn DocSession>, VizError> {
        self.0.new_demo_audio()
    }
    fn new_project(&self, format: ProjectFormatRow) -> Result<Arc<dyn DocSession>, VizError> {
        self.0.new_project(format)
    }
    fn open(&self, path: &str) -> Result<Arc<dyn DocSession>, VizError> {
        self.0.open(path)
    }
    fn open_with(&self, path: &str, options: OpenOptions) -> Result<Arc<dyn DocSession>, VizError> {
        self.0.open_with(path, options)
    }
    fn open_copy(&self, path: &str) -> Result<Arc<dyn DocSession>, VizError> {
        self.0.open_copy(path)
    }
    fn dawproject_info(&self, _path: &str) -> Result<DawprojectInfoRow, VizError> {
        panic!("the read went wrong");
    }
    fn aaf_info(&self, path: &str) -> Result<AafInfoRow, VizError> {
        self.0.aaf_info(path)
    }
    fn recovery_check(&self, path: &str) -> Result<RecoveryInfo, VizError> {
        self.0.recovery_check(path)
    }
    fn project_rates(&self) -> Vec<u32> {
        self.0.project_rates()
    }
    fn format_label(&self, format: ProjectFormatRow) -> String {
        self.0.format_label(format)
    }
    fn bounce_settings(&self) -> BounceSettingsRow {
        self.0.bounce_settings()
    }
}

#[gpui::test]
fn a_read_that_panics_is_cancelled_not_left_reading(cx: &mut TestAppContext) {
    let core = Arc::new(FakeCore::new());
    let s = setup_with(cx, core.clone(), Arc::new(PanickyCore(core)));
    open_path(cx, "/tmp/Song.dawproject");
    assert!(!reading(cx));
    assert_eq!(documents(cx), 1);
    // The channel's own read of it fails the call, once.
    let e = run(
        &s,
        cx,
        "file.import_dawproject",
        json!({"path": "/tmp/Song.dawproject"}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
}

#[gpui::test(iterations = 10)]
fn an_older_background_read_never_overwrites_a_newer_one(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.fake.set_midi_file_info("/tmp/A.mid", midi_info());
    s.fake.set_midi_file_info(
        "/tmp/B.mid",
        MidiFileInfoRow {
            tracks: 5,
            ..midi_info()
        },
    );
    let read = cx.update(|cx| {
        AppState::global(cx).update(cx, |st, _| {
            let doc = st.store.front().unwrap();
            st.store.begin_midi_import(doc, "/tmp/A.mid");
            st.store.begin_midi_import(doc, "/tmp/B.mid");
            doc
        })
    });
    cx.run_until_parked();
    let landed = cx.update(|cx| {
        AppState::global(cx)
            .read(cx)
            .store
            .document(read)
            .and_then(|d| d.midi_read().cloned())
    });
    let landed = landed.expect("the newest read landed");
    assert_eq!(landed.path, "/tmp/B.mid");
    assert_eq!(landed.info.tracks, 5);
    assert!(!reading(cx));
    let _ = s;
}

#[gpui::test]
fn the_dawproject_return_prompt_is_answered_under_the_channel(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core.set_dawproject_info(
        "/tmp/Back.dawproject",
        DawprojectInfoRow {
            from_vizdaw: true,
            ..plain_file(48_000)
        },
    );
    open_path(cx, "/tmp/Back.dawproject");
    pump(&s, cx);
    let prompt = cx.update(|cx| {
        AppState::global(cx)
            .read(cx)
            .store
            .return_prompt()
            .is_some()
    });
    assert!(!prompt, "answered Cancel");
    let w = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    assert_eq!(w["alerts"][0]["title"], "DAWproject Not Merged");
    assert_eq!(documents(cx), 1);
}

#[gpui::test]
fn the_file_commands_are_no_longer_unavailable(cx: &mut TestAppContext) {
    let s = setup(cx);
    let e = call(&s, cx, "state.get", json!({"target": "record"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert!(e.message.contains("slice 7a"), "{}", e.message);
    assert!(e.message.contains("file.import_*"), "{}", e.message);
}
