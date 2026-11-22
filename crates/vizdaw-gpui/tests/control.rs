//! The control channel in the GPUI app: slices 1 and 2's commands answered
//! through the pump, with the scripted session; and one real socket.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_control::protocol::{RawRequest, RequestId};
use core_control::server::{Call, Config, Server};
use core_control::{ControlError, ErrorCode};
use gpui::TestAppContext;
use serde_json::{Map, Value, json};
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_ffi::VizError;
use vizdaw_ffi::snapshot::DocSnapshot;
use vizdaw_gpui::AppState;
use vizdaw_gpui::control::{self, CallSource, MAX_CALLS_PER_TICK};

/// Calls queued by the test; replies kept in order.
#[derive(Default)]
struct FakeCalls {
    queue: RefCell<VecDeque<Call>>,
    replies: RefCell<Vec<(u64, Result<Value, ControlError>)>>,
    next: RefCell<u64>,
}

impl FakeCalls {
    fn push(&self, cmd: &str, params: Value) -> u64 {
        self.try_push(cmd, params).expect("a valid request")
    }

    /// Queues the call, or answers the server's own refusal (it validates
    /// a request against the registry before the app sees it).
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

    fn reply_to(&self, token: u64) -> Option<Result<Value, ControlError>> {
        self.replies
            .borrow()
            .iter()
            .find(|(t, _)| *t == token)
            .map(|(_, r)| r.clone())
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
}

struct Setup {
    fake: Arc<FakeSession>,
    calls: FakeCalls,
}

/// The app installed over `fake` (the demo's session); no window yet.
fn setup(cx: &mut TestAppContext, fake: FakeSession) -> Setup {
    let fake = Arc::new(fake);
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|path| {
            if path.contains("Missing") {
                Err(VizError::Io {
                    message: "no such project".into(),
                })
            } else {
                Ok(Arc::new(demo_fake()))
            }
        }),
    );
    cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
    });
    Setup {
        fake,
        calls: FakeCalls::default(),
    }
}

/// Sends one call through the pump and answers its reply.
fn call(
    s: &Setup,
    cx: &mut TestAppContext,
    cmd: &str,
    params: Value,
) -> Result<Value, ControlError> {
    let token = s.calls.try_push(cmd, params)?;
    cx.update(|cx| control::pump(&s.calls, cx));
    cx.run_until_parked();
    s.calls.reply_to(token).expect("the call was answered")
}

fn ok(r: Result<Value, ControlError>) -> Value {
    r.unwrap_or_else(|e| panic!("call failed: {e:?}"))
}

fn documents(cx: &mut TestAppContext) -> usize {
    cx.update(|cx| AppState::global(cx).read(cx).store.count())
}

#[gpui::test]
fn new_demo_then_play_reports_playing(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let window = opened["window"].as_i64().expect("a window number");
    let doc = opened["doc"].as_str().expect("a doc").to_owned();

    let before = ok(call(&s, cx, "state.get", json!({"target": "transport"})));
    assert_eq!(before["playing"], false);
    assert_eq!(before["doc"], doc.as_str());

    let ran = ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "transport.play_stop"}),
    ));
    assert_eq!(ran["command"], "transport.play_stop");
    assert_eq!(ran["window"], window);
    assert_eq!(s.fake.count("play(3)"), 1);

    let after = ok(call(
        &s,
        cx,
        "state.get",
        json!({"target": "transport", "window": window}),
    ));
    assert_eq!(after["playing"], true);
    assert_eq!(after["seq"], 3);
    for field in [
        "tick",
        "position",
        "seconds",
        "sample_rate",
        "device",
        "faults",
        "report",
    ] {
        assert!(after.get(field).is_some(), "no {field} in {after}");
    }

    // S: stop and return to where playing started.
    ok(call(&s, cx, "command.run", json!({"id": "transport.stop"})));
    assert_eq!(s.fake.count("stop"), 1);
    assert_eq!(s.fake.count("locate(0)"), 1);
    let stopped = ok(call(&s, cx, "state.get", json!({"target": "transport"})));
    assert_eq!(stopped["playing"], false);
}

#[gpui::test]
fn quit_with_unsaved_changes_answers_unsaved_changes(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.set_dirty(true);
    let s = setup(cx, fake);
    ok(call(&s, cx, "app.new_demo", json!({})));

    let refused = call(&s, cx, "app.quit", json!({})).expect_err("refused");
    assert_eq!(refused.code, ErrorCode::UnsavedChanges);
    assert!(
        refused.message.contains("\u{201c}Untitled\u{201d}"),
        "the message names the song: {}",
        refused.message
    );
    // Still running: nothing closed, and nobody was asked.
    assert_eq!(s.fake.count("close"), 0);
    assert_eq!(documents(cx), 1);
    assert_eq!(cx.windows().len(), 1);
    assert!(!cx.has_pending_prompt());
    // An explicit `discard: false` is the same refusal.
    let again = call(&s, cx, "app.quit", json!({"discard": false}));
    assert_eq!(again.unwrap_err().code, ErrorCode::UnsavedChanges);
    assert!(!cx.has_pending_prompt());
    assert_eq!(s.fake.count("close"), 0);
}

#[gpui::test]
fn quit_with_discard_quits(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.set_dirty(true);
    let s = setup(cx, fake);
    ok(call(&s, cx, "app.new_demo", json!({})));

    let quitting = ok(call(&s, cx, "app.quit", json!({"discard": true})));
    assert_eq!(quitting, json!({"quitting": true}));
    // Quit ran without asking: the session closed, no document is left.
    assert_eq!(s.fake.count("close"), 1);
    assert_eq!(s.fake.count("save"), 0);
    assert_eq!(documents(cx), 0);
}

#[gpui::test]
fn quit_without_unsaved_changes_quits(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let quitting = ok(call(&s, cx, "app.quit", json!({})));
    assert_eq!(quitting, json!({"quitting": true}));
    assert_eq!(s.fake.count("close"), 1);
    assert_eq!(documents(cx), 0);
}

#[gpui::test]
fn save_as_then_state_windows_names_the_file(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));

    // Untitled: Save needs a path (the Save dialog), and nothing is written.
    let save = call(&s, cx, "command.run", json!({"id": "file.save"})).unwrap_err();
    assert_eq!(save.code, ErrorCode::DialogRequired);
    assert_eq!(save.dialog.as_deref(), Some("Save"));
    assert_eq!(s.fake.count("save"), 0);

    let ran = ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "file.save_as", "params": {"path": "/tmp/songs/Tune.vizdaw"}}),
    ));
    assert_eq!(ran["command"], "file.save_as");
    assert_eq!(s.fake.count("save_as(/tmp/songs/Tune.vizdaw)"), 1);

    let state = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    let windows = state["windows"].as_array().expect("windows");
    assert_eq!(windows.len(), 1);
    let w = &windows[0];
    assert_eq!(w["title"], "Tune");
    assert_eq!(w["kind"], "document");
    assert_eq!(w["doc"], opened["doc"]);
    assert_eq!(w["number"], opened["window"]);
    for field in ["key", "main", "visible", "frame"] {
        assert!(w.get(field).is_some(), "no {field} in {w}");
    }
    assert!(state["alerts"].is_array());

    // Saved: Save now saves in place.
    ok(call(&s, cx, "command.run", json!({"id": "file.save"})));
    assert_eq!(s.fake.count("save"), 1);
}

#[gpui::test]
fn ui_tree_and_click_press_play(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    assert_eq!(tree["window"], opened["window"]);
    let play = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["role"] == "button" && n["label"] == "Play")
        .unwrap_or_else(|| panic!("no Play in {tree}"))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let clicked = ok(call(&s, cx, "ui.click", json!({"element": play})));
    assert_eq!(clicked["window"], opened["window"]);
    assert_eq!(s.fake.count("play(3)"), 1);
    // The button now reads Stop: the old ID names nothing.
    let gone = call(&s, cx, "ui.click", json!({"element": play})).unwrap_err();
    assert_eq!(gone.code, ErrorCode::NoElement);
    // Points are not answered this slice.
    let point = call(&s, cx, "ui.click", json!({"point": {"x": 1, "y": 1}})).unwrap_err();
    assert_eq!(point.code, ErrorCode::Unavailable);
}

#[gpui::test]
fn commands_outside_the_slice_are_unavailable(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    for (cmd, params) in [
        ("command.run", json!({"id": "mixer.new_bus"})),
        ("state.get", json!({"target": "mixer"})),
        ("ui.key", json!({"key": "space"})),
    ] {
        let e = call(&s, cx, cmd, params).unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable, "{cmd}");
        assert!(e.message.contains("slice 2"), "{cmd}: {}", e.message);
    }
}

/// The track names of `state.get document`, in order.
fn lane_names(doc: &Value) -> Vec<String> {
    doc["lanes"]
        .as_array()
        .expect("lanes")
        .iter()
        .map(|l| l["name"].as_str().unwrap().to_owned())
        .collect()
}

/// The fake's snapshot as the next edit leaves it: edited, with `undo`.
fn edited(f: &FakeSession, undo: &str) -> DocSnapshot {
    let mut n = f.snap();
    n.version += 1;
    n.dirty = true;
    n.undo_label = Some(undo.into());
    n
}

#[gpui::test]
fn state_get_document_matches_the_swift_fields(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let d = ok(call(&s, cx, "state.get", json!({"target": "document"})));
    for key in [
        "doc",
        "title",
        "path",
        "dirty",
        "version",
        "undo",
        "redo",
        "selected_seq",
        "sequences",
        "lanes",
        "markers",
        "destinations",
        "warnings",
        "edit_notes",
        "alert",
    ] {
        assert!(d.get(key).is_some(), "no {key} in {d}");
    }
    assert_eq!(d["doc"], opened["doc"]);
    assert_eq!(d["title"], "Untitled");
    assert_eq!(d["path"], Value::Null);
    assert_eq!(d["dirty"], false);
    assert_eq!(d["selected_seq"], 3);
    assert_eq!(d["sequences"].as_array().unwrap().len(), 3);
    assert_eq!(d["sequences"][2]["name"], "Song");
    assert_eq!(lane_names(&d), ["Groove", "Chords"]);
    assert_eq!(d["lanes"][0]["kind"], "midi");
    assert_eq!(d["markers"], json!([]));
    assert_eq!(d["destinations"], json!([]));
    assert_eq!(d["alert"], Value::Null);
    // By window number too.
    let by_window = ok(call(
        &s,
        cx,
        "state.get",
        json!({"target": "document", "window": opened["window"]}),
    ));
    assert_eq!(by_window, d);

    // `state.get editor`: this slice's subset.
    let e = ok(call(&s, cx, "state.get", json!({"target": "editor"})));
    assert_eq!(
        e,
        json!({"doc": opened["doc"], "status": null, "gesture": null})
    );
}

#[gpui::test]
fn a_track_edit_marks_the_document_dirty(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    s.fake.bump(edited(&s.fake, "New MIDI Track"));
    ok(call(&s, cx, "command.run", json!({"id": "track.new_midi"})));

    let d = ok(call(&s, cx, "state.get", json!({"target": "document"})));
    assert_eq!(d["dirty"], true);
    assert_eq!(lane_names(&d).len(), 3);
    let w = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    let title = w["windows"][0]["title"].as_str().unwrap();
    assert!(title.ends_with("\u{2014} Edited"), "{title}");
    // The MIDI track has no destination: the status says so.
    let e = ok(call(&s, cx, "state.get", json!({"target": "editor"})));
    assert!(
        e["status"]
            .as_str()
            .unwrap()
            .contains("No external MIDI destination"),
        "{e}"
    );
    // The close and quit guards read it live.
    let quit = call(&s, cx, "app.quit", json!({})).unwrap_err();
    assert_eq!(quit.code, ErrorCode::UnsavedChanges);
}

#[gpui::test]
fn track_commands_answer_with_the_undo_label(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));

    s.fake.bump(edited(&s.fake, "Rename Track"));
    let ran = ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "track.rename", "params": {"lane": 31, "name": "Keys"}}),
    ));
    assert_eq!(
        ran,
        json!({
            "command": "track.rename",
            "doc": opened["doc"],
            "window": opened["window"],
            "undo": "Rename Track",
            "status": null,
        })
    );
    let d = ok(call(&s, cx, "state.get", json!({"target": "document"})));
    assert_eq!(lane_names(&d), ["Groove", "Keys"]);
    assert_eq!(d["undo"], "Rename Track");

    // Undo through the channel: the label it leaves.
    let mut back = s.fake.snap();
    back.version += 1;
    back.undo_label = None;
    back.redo_label = Some("Rename Track".into());
    s.fake.bump(back);
    let undone = ok(call(&s, cx, "command.run", json!({"id": "edit.undo"})));
    assert_eq!(undone["command"], "edit.undo");
    assert_eq!(undone["undo"], Value::Null);
    assert_eq!(s.fake.count("undo"), 1);
    // Nothing left to undo: refused, not run.
    let e = call(&s, cx, "command.run", json!({"id": "edit.undo"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(s.fake.count("undo"), 1);
    let redone = ok(call(&s, cx, "command.run", json!({"id": "edit.redo"})));
    assert_eq!(redone["command"], "edit.redo");
    assert_eq!(s.fake.count("redo"), 1);

    // A command's status is its own: an earlier one's does not linger.
    ok(call(&s, cx, "command.run", json!({"id": "track.new_midi"})));
    let after = ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "track.duplicate", "params": {"lane": 30}}),
    ));
    assert_eq!(after["status"], Value::Null, "{after}");
}

#[gpui::test]
fn hostile_track_params_keep_the_list(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let before = ok(call(&s, cx, "state.get", json!({"target": "document"})));
    let calls = s.fake.calls().len();
    for (id, params, want) in [
        (
            "track.new_audio",
            json!({"seq": -1}),
            ErrorCode::InvalidParams,
        ),
        (
            "track.new_audio",
            json!({"seq": i64::MIN}),
            ErrorCode::InvalidParams,
        ),
        (
            "track.new_audio",
            json!({"seq": u64::MAX}),
            ErrorCode::InvalidParams,
        ),
        (
            "track.new_audio",
            json!({"seq": "3"}),
            ErrorCode::InvalidParams,
        ),
        ("track.new_audio", json!({"seq": 99}), ErrorCode::Failed),
        ("track.new_midi", json!({"after": 99}), ErrorCode::Failed),
        (
            "track.new_midi",
            json!({"bogus": 1}),
            ErrorCode::InvalidParams,
        ),
        ("track.duplicate", json!({"lane": 99}), ErrorCode::Failed),
        (
            "track.duplicate",
            json!({"lane": -1}),
            ErrorCode::InvalidParams,
        ),
        ("track.delete", json!({"lane": 99}), ErrorCode::Failed),
        ("track.delete", json!({"seq": 99}), ErrorCode::Failed),
        (
            "track.rename",
            json!({"lane": 31}),
            ErrorCode::DialogRequired,
        ),
        (
            "track.rename",
            json!({"lane": 99, "name": "x"}),
            ErrorCode::Failed,
        ),
        (
            "track.rename",
            json!({"lane": 31, "name": 5}),
            ErrorCode::InvalidParams,
        ),
    ] {
        let e = call(&s, cx, "command.run", json!({"id": id, "params": params})).unwrap_err();
        assert_eq!(e.code, want, "{id} {params}");
    }
    assert_eq!(s.fake.calls().len(), calls, "no session call was made");
    // A blank name reaches the core, which refuses it.
    s.fake.set_edit_error(Some(VizError::Model {
        message: "A track name cannot be blank.".into(),
    }));
    let e = call(
        &s,
        cx,
        "command.run",
        json!({"id": "track.rename", "params": {"lane": 31, "name": "  "}}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    let after = ok(call(&s, cx, "state.get", json!({"target": "document"})));
    assert_eq!(after["lanes"], before["lanes"]);
    assert_eq!(after["version"], before["version"]);
    assert!(s.fake.edits().is_empty());
}

#[gpui::test]
fn file_close_never_prompts(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.set_dirty(true);
    let s = setup(cx, fake);
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let ran = ok(call(&s, cx, "command.run", json!({"id": "file.close"})));
    assert_eq!(ran["command"], "file.close");
    assert_eq!(ran["doc"], opened["doc"]);
    assert_eq!(ran["window"], opened["window"]);
    // Gone at once, unsaved, and nobody was asked.
    assert!(!cx.has_pending_prompt());
    assert_eq!(s.fake.count("close"), 1);
    assert_eq!(s.fake.count("save"), 0);
    assert_eq!(documents(cx), 0);
    assert!(cx.windows().is_empty());
    let w = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    assert_eq!(w["windows"], json!([]));
}

#[gpui::test]
fn file_new_opens_an_empty_window(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    // No document needed first.
    let made = ok(call(&s, cx, "command.run", json!({"id": "file.new"})));
    assert_eq!(made["command"], "file.new");
    let window = made["window"].as_i64().expect("a window number");
    let doc = made["doc"].as_str().expect("a doc").to_owned();
    assert_eq!(documents(cx), 1);
    let w = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    assert_eq!(w["windows"][0]["number"], window);
    assert_eq!(w["windows"][0]["doc"], doc.as_str());
    let d = ok(call(
        &s,
        cx,
        "state.get",
        json!({"target": "document", "window": window}),
    ));
    assert_eq!(d["sequences"], json!([]));
    assert_eq!(d["lanes"], json!([]));
    assert_eq!(d["selected_seq"], Value::Null);
    // A second one is another window; the demo's session was never made.
    let again = ok(call(&s, cx, "command.run", json!({"id": "file.new"})));
    assert_ne!(again["window"], made["window"]);
    assert_eq!(documents(cx), 2);
    assert!(s.fake.calls().is_empty());
}

#[gpui::test]
fn no_document_answers_no_document(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let e = call(&s, cx, "state.get", json!({"target": "transport"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::NoDocument);
    let e = call(
        &s,
        cx,
        "state.get",
        json!({"target": "transport", "window": 999}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NoWindow);
    // `state.get windows` needs none.
    let w = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    assert_eq!(w["windows"], json!([]));
}

#[gpui::test]
fn app_open_opens_and_refuses(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(
        &s,
        cx,
        "app.open",
        json!({"path": "/tmp/x/Other.vizdaw"}),
    ));
    assert!(opened["window"].is_i64());
    assert!(opened["doc"].is_string());
    assert_eq!(documents(cx), 1);
    // A project that cannot be read: the alert is the call's failure.
    let e = call(&s, cx, "app.open", json!({"path": "/tmp/x/Missing.vizdaw"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(e.message.contains("no such project"), "{}", e.message);
    // Taken, not left showing.
    let alert = cx.update(|cx| AppState::global(cx).read(cx).store.alert().cloned());
    assert_eq!(alert, None);
}

#[gpui::test]
fn pump_handles_at_most_16_calls_per_tick(cx: &mut TestAppContext) {
    assert_eq!(MAX_CALLS_PER_TICK, 16);
    let s = setup(cx, demo_fake());
    for _ in 0..20 {
        s.calls.push("state.get", json!({"target": "windows"}));
    }
    let first = cx.update(|cx| control::pump(&s.calls, cx));
    assert_eq!(first.handled, 16);
    assert_eq!(s.calls.replies.borrow().len(), 16);
    assert_eq!(s.calls.queue.borrow().len(), 4);
    let second = cx.update(|cx| control::pump(&s.calls, cx));
    assert_eq!(second.handled, 4);
    assert_eq!(s.calls.replies.borrow().len(), 20);
    let idle = cx.update(|cx| control::pump(&s.calls, cx));
    assert_eq!(idle.handled, 0);
}

#[gpui::test]
fn a_real_socket_is_answered_by_the_pump(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let dir = tempfile::Builder::new()
        .prefix("vzg")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.path().join("ctl").join("s.sock");
    let server = Server::start(&path, Config::default()).expect("the server starts");
    let client_path = path.clone();
    let client = std::thread::spawn(move || {
        let mut c = core_control::client::Client::connect(&client_path).unwrap();
        let demo = c
            .call("app.new_demo", json!({}), Duration::from_secs(20))
            .unwrap();
        let windows = c
            .call(
                "state.get",
                json!({"target": "windows"}),
                Duration::from_secs(20),
            )
            .unwrap();
        (demo, windows)
    });
    let deadline = Instant::now() + Duration::from_secs(20);
    while !client.is_finished() {
        assert!(Instant::now() < deadline, "the client was never answered");
        cx.update(|cx| control::pump(&server, cx));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    let (demo, windows) = client.join().unwrap();
    assert!(demo["window"].is_i64(), "{demo}");
    assert_eq!(windows["windows"][0]["number"], demo["window"]);
    assert_eq!(s.fake.count("close"), 0);
}

#[test]
fn launch_arguments_take_the_socket_and_ignore_the_rest() {
    let args = [
        "-ApplePersistenceIgnoreState",
        "YES",
        "--control",
        "/tmp/x/s.sock",
        "--no-scan",
        "--demo",
    ]
    .map(String::from);
    let o = control::LaunchOptions::parse(args, None);
    assert_eq!(
        o.control.as_deref(),
        Some(std::path::Path::new("/tmp/x/s.sock"))
    );
    assert!(o.demo);
    // The environment names the socket when `--control` does not.
    let o = control::LaunchOptions::parse(["--no-scan".to_owned()], Some("/tmp/y.sock".into()));
    assert_eq!(
        o.control.as_deref(),
        Some(std::path::Path::new("/tmp/y.sock"))
    );
    let o = control::LaunchOptions::parse(
        ["--control".to_owned(), "/tmp/a.sock".to_owned()],
        Some("/tmp/b.sock".into()),
    );
    assert_eq!(
        o.control.as_deref(),
        Some(std::path::Path::new("/tmp/a.sock"))
    );
    assert!(!o.demo);
}

#[gpui::test]
fn a_channel_click_on_more_names_the_bar_command(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let window = cx.update(|cx| AppState::global(cx).read(cx).windows()[0].handle);
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(600.), gpui::px(640.)));
    cx.run_until_parked();
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    let node = |label: &str| {
        tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["label"] == label)
            .cloned()
    };
    let more = node("More Transport Items").unwrap_or_else(|| panic!("no » in {tree}"))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    ok(call(&s, cx, "ui.click", json!({"element": more})));
    let status = cx.update(|cx| {
        let s = AppState::global(cx).read(cx);
        let doc = s.windows()[0].doc;
        s.store.document(doc).unwrap().status().map(String::from)
    });
    assert_eq!(
        status.as_deref(),
        Some("The \u{bb} menu does not open for the control channel; use ui.set_transport_bar.")
    );
    // The menu stays shut.
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    assert!(
        !tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["role"] == "menuitem" && n["label"] == "Position Field")
    );
}
