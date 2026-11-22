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
use vizdaw_app::testing::{FakeSession, demo_fake, lane_row};
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
    /// Where a screenshot without `path` goes.
    shots: RefCell<Option<std::path::PathBuf>>,
    /// Calls whose client left.
    left: RefCell<Vec<u64>>,
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

    fn screenshot_path(&self) -> Option<std::path::PathBuf> {
        self.shots.borrow().clone()
    }

    fn is_waiting(&self, token: u64) -> bool {
        !self.left.borrow().contains(&token)
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
    // A point is clicked as the mouse would there.
    let point = ok(call(&s, cx, "ui.click", json!({"point": {"x": 1, "y": 1}})));
    assert_eq!(point["events"], 2);
    assert_eq!(
        (point["x"].as_f64(), point["y"].as_f64()),
        (Some(1.), Some(1.))
    );
}

#[gpui::test]
fn commands_outside_the_slice_are_unavailable(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    for (cmd, params) in [
        ("command.run", json!({"id": "mixer.new_bus"})),
        ("state.get", json!({"target": "mixer"})),
        (
            "ui.drag",
            json!({"from": {"x": 1, "y": 1}, "to": {"x": 2, "y": 2}}),
        ),
    ] {
        let e = call(&s, cx, cmd, params).unwrap_err();
        assert_eq!(e.code, ErrorCode::Unavailable, "{cmd}");
        assert!(e.message.contains("slice 4"), "{cmd}: {}", e.message);
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

    // `state.get editor`: the Swift fields this slice has.
    let e = ok(call(&s, cx, "state.get", json!({"target": "editor"})));
    assert_eq!(
        e,
        json!({
            "doc": opened["doc"], "detail": null, "windows": [], "tool": "arrow",
            "snap": {"enabled": true, "division": "quarter"}, "exact_mode": false,
            "gesture": false, "status": null, "step": null,
            "transport": {
                "locators": [], "punch_in": 0, "punch_out": 0, "punch": false,
                "overdub": false,
            },
        })
    );
    // Swift's keys, no more and no fewer.
    let keys: Vec<&str> = e.as_object().unwrap().keys().map(String::as_str).collect();
    let mut keys = keys;
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "detail",
            "doc",
            "exact_mode",
            "gesture",
            "snap",
            "status",
            "step",
            "tool",
            "transport",
            "windows"
        ]
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

// ---- Slice 3: wait, keys, modifier clicks, screenshots, transport ----

/// One bar of 4/4, in ticks.
const BAR: i64 = 61_440;

/// One pump tick, then the app settles.
fn tick(s: &Setup, cx: &mut TestAppContext) -> control::Tick {
    let t = cx.update(|cx| control::pump(&s.calls, cx));
    cx.run_until_parked();
    t
}

/// The node of `tree` with `role` and `label`.
fn node(tree: &Value, role: &str, label: &str) -> Value {
    tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["role"] == role && n["label"] == label)
        .cloned()
        .unwrap_or_else(|| panic!("no {role} {label:?} in {tree}"))
}

fn node_id(s: &Setup, cx: &mut TestAppContext, role: &str, label: &str) -> String {
    let tree = ok(call(s, cx, "ui.tree", json!({})));
    node(&tree, role, label)["id"].as_str().unwrap().to_owned()
}

fn tick_now(s: &Setup, cx: &mut TestAppContext) -> i64 {
    ok(call(s, cx, "state.get", json!({"target": "transport"})))["tick"]
        .as_i64()
        .unwrap()
}

#[gpui::test]
fn wait_playing_answers_when_play_starts(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let token = s.calls.push("wait", json!({"condition": "playing"}));
    tick(&s, cx);
    assert!(s.calls.reply_to(token).is_none(), "still waiting");
    tick(&s, cx);
    assert!(s.calls.reply_to(token).is_none(), "still waiting");
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "transport.play_stop"}),
    ));
    let waited = ok(s.calls.reply_to(token).expect("answered once playing"));
    assert_eq!(waited["condition"], "playing");
    assert!(waited["elapsed_ms"].is_u64(), "{waited}");

    // A condition that already holds answers in the tick it is taken.
    let now = ok(call(
        &s,
        cx,
        "wait",
        json!({"condition": "tick_at_least", "tick": 0, "window": opened["window"]}),
    ));
    assert_eq!(now["condition"], "tick_at_least");
    let docs = ok(call(
        &s,
        cx,
        "wait",
        json!({"condition": "documents_at_least", "count": 1}),
    ));
    assert_eq!(docs["condition"], "documents_at_least");
    // A window number that names no window: `no_window` at once.
    let e = call(
        &s,
        cx,
        "wait",
        json!({"condition": "playing", "window": 999_999}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NoWindow);
}

#[gpui::test]
fn a_wait_that_never_comes_true_times_out_with_details(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let token = s.calls.push(
        "wait",
        json!({"condition": "tick_at_least", "tick": 999_999, "timeout_ms": 500}),
    );
    tick(&s, cx);
    cx.executor().advance_clock(Duration::from_millis(200));
    tick(&s, cx);
    assert!(s.calls.reply_to(token).is_none(), "not yet timed out");
    cx.executor().advance_clock(Duration::from_millis(400));
    tick(&s, cx);
    let e = s.calls.reply_to(token).expect("timed out").unwrap_err();
    assert_eq!(e.code, ErrorCode::Timeout);
    assert_eq!(e.message, "tick >= 999999 did not happen within 500 ms");
    assert_eq!(
        e.details,
        Some(json!({
            "condition": "tick_at_least",
            "timeout_ms": 500,
            "facts": {
                "document": {"playing": false, "tick": 0, "detail_kind": "none"},
                "documents": 1,
                "window_titles": ["Untitled"],
            },
        }))
    );
    // Answered once: later ticks send nothing more.
    tick(&s, cx);
    let answers = s
        .calls
        .replies
        .borrow()
        .iter()
        .filter(|(t, _)| *t == token)
        .count();
    assert_eq!(answers, 1);
}

#[gpui::test]
fn many_waits_do_not_stall_other_calls(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let waits: Vec<u64> = (0..40)
        .map(|_| s.calls.push("wait", json!({"condition": "playing"})))
        .collect();
    // Taking them is capped as any call is; then they only wait.
    assert_eq!(tick(&s, cx).handled, MAX_CALLS_PER_TICK);
    assert_eq!(tick(&s, cx).handled, MAX_CALLS_PER_TICK);
    assert_eq!(tick(&s, cx).handled, 8);
    // Forty pending waits take no share of the next tick's calls.
    for _ in 0..MAX_CALLS_PER_TICK {
        s.calls.push("state.get", json!({"target": "windows"}));
    }
    let t = tick(&s, cx);
    assert_eq!(t.handled, MAX_CALLS_PER_TICK);
    assert_eq!(t.waiting, 40);
    assert!(waits.iter().all(|w| s.calls.reply_to(*w).is_none()));
    // Once playing, every wait is answered in one tick.
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "transport.play_stop"}),
    ));
    for w in &waits {
        let r = ok(s.calls.reply_to(*w).expect("answered"));
        assert_eq!(r["condition"], "playing");
    }
    assert_eq!(tick(&s, cx).waiting, 0);

    // A wait whose client left is dropped, unanswered.
    let gone = s.calls.push(
        "wait",
        json!({"condition": "tick_at_least", "tick": 999_999}),
    );
    assert_eq!(tick(&s, cx).waiting, 1);
    s.calls.left.borrow_mut().push(gone);
    assert_eq!(tick(&s, cx).waiting, 0);
    assert!(s.calls.reply_to(gone).is_none());
}

#[gpui::test]
fn ui_key_space_plays_and_stops(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let pressed = ok(call(&s, cx, "ui.key", json!({"key": "space"})));
    assert_eq!(
        pressed,
        json!({"window": opened["window"], "events": 1, "x": 0.0, "y": 0.0})
    );
    assert_eq!(s.fake.count("play(3)"), 1);
    let playing = ok(call(&s, cx, "state.get", json!({"target": "transport"})));
    assert_eq!(playing["playing"], true);
    ok(call(&s, cx, "ui.key", json!({"key": "space"})));
    assert_eq!(s.fake.count("stop"), 1);
    // A key no command has goes to the window as a key: down and up.
    let plain = ok(call(&s, cx, "ui.key", json!({"key": "f12"})));
    assert_eq!(plain["events"], 2);
    // Hostile keys are the request's refusal.
    for params in [
        json!({"key": "nosuchkey"}),
        json!({"key": "a", "text": "a"}),
        json!({"text": "a", "modifiers": ["shift"]}),
        json!({"key": "a", "modifiers": ["hyper"]}),
        json!({"text": "\u{7}"}),
    ] {
        let e = call(&s, cx, "ui.key", params.clone()).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{params}");
    }
}

#[gpui::test]
fn ui_key_text_types_into_the_focused_field(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let position = node_id(&s, cx, "textfield", "Position");
    // The element's click starts the edit, all selected; the text replaces it.
    ok(call(
        &s,
        cx,
        "ui.key",
        json!({"element": position, "text": "5"}),
    ));
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    let field = node(&tree, "textfield", "Position");
    assert_eq!(field["value"], "5");
    assert_eq!(field["focused"], true);
    // Space types while the field has the keyboard; nothing plays.
    ok(call(&s, cx, "ui.key", json!({"key": "space"})));
    assert_eq!(s.fake.count("play(3)"), 0);
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    assert_eq!(node(&tree, "textfield", "Position")["value"], "5 ");
    ok(call(&s, cx, "ui.key", json!({"key": "delete"})));
    assert!(s.fake.calls().iter().all(|c| !c.starts_with("locate(")));
    // Return commits: bar 5.
    ok(call(&s, cx, "ui.key", json!({"text": "\n"})));
    assert_eq!(s.fake.count(&format!("locate({})", 4 * BAR)), 1);
    assert_eq!(tick_now(&s, cx), 4 * BAR);
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    assert_eq!(node(&tree, "textfield", "Position").get("focused"), None);
}

#[gpui::test]
fn ui_key_shift_dot_and_keypad_minus_step_bars(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let ran = ok(call(
        &s,
        cx,
        "ui.key",
        json!({"key": ".", "modifiers": ["shift"]}),
    ));
    assert_eq!(ran["events"], 1);
    assert_eq!(tick_now(&s, cx), BAR);
    // `>` is shift-. too.
    ok(call(&s, cx, "ui.key", json!({"key": ">"})));
    assert_eq!(tick_now(&s, cx), 2 * BAR);
    ok(call(&s, cx, "ui.key", json!({"key": "keypad_minus"})));
    assert_eq!(tick_now(&s, cx), BAR);
    ok(call(&s, cx, "ui.key", json!({"key": "keypad_plus"})));
    assert_eq!(tick_now(&s, cx), 2 * BAR);
    ok(call(&s, cx, "ui.key", json!({"key": "keypad_minus"})));
    ok(call(&s, cx, "ui.key", json!({"key": "keypad_minus"})));
    assert_eq!(tick_now(&s, cx), 0);
    // The main minus key is not the keypad's.
    ok(call(&s, cx, "ui.key", json!({"key": "-"})));
    assert_eq!(s.fake.count("transport_step_bar(false)"), 3);
}

#[gpui::test]
fn ui_click_shift_on_fast_forward_steps_to_the_marker(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let ff = node_id(&s, cx, "button", "Fast Forward");
    // No marker: the status says so and the transport stays.
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"element": ff, "modifiers": ["shift"]}),
    ));
    let e = ok(call(&s, cx, "state.get", json!({"target": "editor"})));
    assert_eq!(e["status"], "No marker after the position.");
    assert_eq!(tick_now(&s, cx), 0);
    // A marker at bar 3: shift goes there, not a bar on.
    s.fake.set_marker_ticks(vec![2 * BAR]);
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"element": ff, "modifiers": ["shift"]}),
    ));
    assert_eq!(tick_now(&s, cx), 2 * BAR);
    assert_eq!(s.fake.count("transport_step_bar(true)"), 0);
    // Without shift it steps one bar.
    ok(call(&s, cx, "ui.click", json!({"element": ff})));
    assert_eq!(tick_now(&s, cx), 3 * BAR);
    // A double click on a field still starts its edit.
    let position = node_id(&s, cx, "textfield", "Position");
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"element": position, "count": 2}),
    ));
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    assert_eq!(node(&tree, "textfield", "Position")["focused"], true);
}

#[gpui::test]
fn ui_screenshot_writes_a_png_or_answers_unavailable(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("nowhere").join("x.png");
    for params in [
        json!({"path": dir.path().join("x.jpg")}),
        json!({"path": missing}),
        json!({"path": dir.path().join("x.png"), "scale": 10}),
        json!({"path": dir.path().join("x.png"), "live": "yes"}),
    ] {
        let e = call(&s, cx, "ui.screenshot", params.clone()).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{params}");
    }
    // The test platform draws nothing: without a capture, unavailable.
    let path = dir.path().join("real.png");
    match call(&s, cx, "ui.screenshot", json!({"path": path})) {
        Ok(shot) => assert_eq!(shot["path"], path.to_str().unwrap()),
        Err(e) => assert_eq!(e.code, ErrorCode::Unavailable, "{e:?}"),
    }

    // Through a stub capture: the PNG, written whole, and its size.
    cx.update(|cx| {
        vizdaw_gpui::screenshot::set_capture(cx, |_, scale| {
            let k = scale.unwrap_or(1.0) as u32;
            let (width, height) = (20 * k, 10 * k);
            Ok(vizdaw_gpui::screenshot::Shot {
                width,
                height,
                rgba: vec![0x80; (width * height * 4) as usize],
            })
        })
    });
    let path = dir.path().join("shot.png");
    let shot = ok(call(
        &s,
        cx,
        "ui.screenshot",
        json!({"path": path, "scale": 2}),
    ));
    assert_eq!(
        shot,
        json!({"window": opened["window"], "path": path.to_str().unwrap(), "width": 40, "height": 20})
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        names,
        [std::ffi::OsString::from("shot.png")],
        "no temp file left"
    );
    // No `path`: the channel's next screenshot path.
    *s.calls.shots.borrow_mut() = Some(dir.path().join("screenshot-1.png"));
    let shot = ok(call(&s, cx, "ui.screenshot", json!({})));
    assert_eq!(
        shot["path"],
        dir.path().join("screenshot-1.png").to_str().unwrap()
    );
    assert!(dir.path().join("screenshot-1.png").is_file());
    // A path that cannot be written: `failed`, naming it.
    std::fs::create_dir(dir.path().join("taken.png")).unwrap();
    let e = call(
        &s,
        cx,
        "ui.screenshot",
        json!({"path": dir.path().join("taken.png")}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(e.message.starts_with("cannot write "), "{}", e.message);
    // A capture that fails says so.
    cx.update(|cx| {
        vizdaw_gpui::screenshot::set_capture(cx, |_, _| {
            Err(ControlError::new(
                ErrorCode::Unavailable,
                "Screenshots need an X11 window on Linux.",
            ))
        })
    });
    let e = call(&s, cx, "ui.screenshot", json!({"path": path})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "Screenshots need an X11 window on Linux.");
}

#[gpui::test]
fn state_get_transport_has_click(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let t = ok(call(&s, cx, "state.get", json!({"target": "transport"})));
    assert_eq!(t.get("click"), Some(&Value::Null), "{t}");
}

#[gpui::test]
fn state_get_windows_has_theme(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let w = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    assert_eq!(w["theme"], "studio-dark", "{w}");
    assert!(core_control::registry::THEME_IDS.contains(&w["theme"].as_str().unwrap()));
}

#[gpui::test]
fn transport_go_position_focuses_the_field(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let ran = ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "transport.go_position"}),
    ));
    assert_eq!(ran["command"], "transport.go_position");
    assert_eq!(ran["window"], opened["window"]);
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    assert_eq!(node(&tree, "textfield", "Position")["focused"], true);
    // While it has the keyboard the bare transport commands wait.
    let e = call(&s, cx, "command.run", json!({"id": "transport.play_stop"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "A text field has the keyboard.");
    ok(call(&s, cx, "ui.key", json!({"key": "escape"})));

    // A bar without fields: the command cannot focus one.
    let set = ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "ui.set_transport_bar", "params": {"items": "cluster,loop"}}),
    ));
    assert_eq!(set["status"], "transport bar: 2 items");
    assert_eq!(set["doc"], Value::Null);
    let e = call(&s, cx, "command.run", json!({"id": "transport.go_edit_in"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "The window shows no transport fields.");
    let reset = ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "ui.reset_transport_bar"}),
    ));
    assert_eq!(reset["status"], "transport bar: the default");
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "transport.go_edit_in"}),
    ));
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    assert_eq!(node(&tree, "textfield", "Edit In")["focused"], true);
}

#[gpui::test]
fn transport_commands_run_and_refuse_over_the_channel(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    for _ in 0..2 {
        ok(call(
            &s,
            cx,
            "command.run",
            json!({"id": "transport.fast_forward"}),
        ));
    }
    assert_eq!(tick_now(&s, cx), 2 * BAR);
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "transport.rewind"}),
    ));
    assert_eq!(tick_now(&s, cx), BAR);
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "transport.return_to_start"}),
    ));
    assert_eq!(tick_now(&s, cx), 0);
    for (id, params) in [
        ("transport.rewind", json!({"bars": 2})),
        ("transport.fast_forward", json!("x")),
    ] {
        let e = call(&s, cx, "command.run", json!({"id": id, "params": params})).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{id}");
    }
    let e = call(
        &s,
        cx,
        "command.run",
        json!({"id": "transport.next_marker"}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "No marker after the position.");
    let e = call(&s, cx, "command.run", json!({"id": "transport.stop"})).unwrap_err();
    assert_eq!(e.message, "Nothing is playing.");
    ok(call(&s, cx, "command.run", json!({"id": "transport.loop"})));
    assert_eq!(s.fake.count("set_loop(true)"), 1);
    assert_eq!(tick_now(&s, cx), 0);
}

#[gpui::test]
fn a_refused_loop_is_the_calls_failure_not_an_alert(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    s.fake.set_loop_error(Some(VizError::Model {
        message: "The loop cannot be set.".into(),
    }));
    let e = call(&s, cx, "command.run", json!({"id": "transport.loop"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "The loop cannot be set.");
    let w = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    assert_eq!(w["alerts"], json!([]));
    let d = ok(call(&s, cx, "state.get", json!({"target": "document"})));
    assert_eq!(d["alert"], Value::Null);
    // The locate behind Return to Start the same.
    s.fake.set_locate_error(Some(VizError::Model {
        message: "No locate now.".into(),
    }));
    let e = call(
        &s,
        cx,
        "command.run",
        json!({"id": "transport.return_to_start"}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "No locate now.");
    let w = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    assert_eq!(w["alerts"], json!([]));
}

#[gpui::test]
fn a_scrolled_out_element_is_not_clicked(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let window = cx.update(|cx| AppState::global(cx).read(cx).windows()[0].handle);
    // Too short for the second track's header.
    cx.simulate_window_resize(window.into(), gpui::size(gpui::px(1000.), gpui::px(170.)));
    cx.run_until_parked();
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    let chords = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["label"].as_str().is_some_and(|l| l.contains("Chords")))
        .cloned()
        .unwrap_or_else(|| panic!("no Chords header in {tree}"));
    let label = chords["label"].as_str().unwrap().to_owned();
    let calls = s.fake.calls().len();
    let e = call(&s, cx, "ui.click", json!({"element": chords["id"]})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, format!("{label} is scrolled out of view."));
    assert_eq!(s.fake.calls().len(), calls, "nothing was clicked");
    // The track still in view is clicked.
    let groove = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| {
            n["role"] == "button" && n["label"].as_str().is_some_and(|l| l.contains("Groove"))
        })
        .cloned()
        .unwrap();
    ok(call(&s, cx, "ui.click", json!({"element": groove["id"]})));
}

#[gpui::test]
fn a_refused_key_command_is_not_run_again_by_the_window(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    // An empty song: Space has nothing to play.
    ok(call(&s, cx, "command.run", json!({"id": "file.new"})));
    let doc_state = |s: &Setup, cx: &mut TestAppContext| {
        let w = ok(call(s, cx, "state.get", json!({"target": "windows"})));
        let d = ok(call(s, cx, "state.get", json!({"target": "document"})));
        let e = ok(call(s, cx, "state.get", json!({"target": "editor"})));
        (w["alerts"].clone(), d["alert"].clone(), e["status"].clone())
    };
    ok(call(&s, cx, "ui.key", json!({"key": "space"})));
    assert_eq!(doc_state(&s, cx), (json!([]), Value::Null, Value::Null));
    // S while stopped: refused, and the window does not run it again.
    ok(call(&s, cx, "ui.key", json!({"key": "s"})));
    assert_eq!(doc_state(&s, cx), (json!([]), Value::Null, Value::Null));
}

#[gpui::test]
fn the_tree_is_read_from_a_fresh_frame(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    // One tick, no frame drawn between the step and the reads (a locked
    // screen sends no frame callbacks).
    let step = s.calls.push("command.run", json!({"id": "transport.loop"}));
    let tree = s.calls.push("ui.tree", json!({}));
    cx.update(|cx| control::pump(&s.calls, cx));
    ok(s.calls.reply_to(step).unwrap());
    let tree = ok(s.calls.reply_to(tree).unwrap());
    assert_eq!(node(&tree, "button", "Loop")["value"], "on");
    // A new track's header shows in the same tick's tree.
    s.fake.bump(edited(&s.fake, "New Audio Track"));
    s.fake.set_lanes(
        3,
        vec![
            lane_row(30, "Groove", false),
            lane_row(31, "Chords", false),
            lane_row(32, "Vox", false),
        ],
    );
    let made = s
        .calls
        .push("command.run", json!({"id": "track.new_audio"}));
    let tree = s.calls.push("ui.tree", json!({}));
    cx.update(|cx| control::pump(&s.calls, cx));
    ok(s.calls.reply_to(made).unwrap());
    let tree = ok(s.calls.reply_to(tree).unwrap());
    assert!(
        tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["label"].as_str().is_some_and(|l| l.contains("Vox"))),
        "{tree}"
    );
}

/// The demo with a two-event block on Chords at bar 1.
fn with_chords_block() -> FakeSession {
    let f = demo_fake();
    f.set_blocks(
        3,
        31,
        vec![vizdaw_ffi::blocks::BlockRow {
            start: 0,
            end: BAR,
            kind: vizdaw_ffi::blocks::BlockKind::Events,
            events: vec![1, 2],
            label: None,
            ref_seq: None,
            muted: false,
        }],
    );
    f
}

const CHORDS_BLOCK: &str = "Block on Chords, bar 1, 2 events";

#[gpui::test]
fn state_get_selection_follows_a_block_click(cx: &mut TestAppContext) {
    let s = setup(cx, with_chords_block());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let sel = ok(call(&s, cx, "state.get", json!({"target": "selection"})));
    assert_eq!(
        sel,
        json!({"doc": opened["doc"], "time_range": null, "lanes": [], "events": []})
    );
    let block = node_id(&s, cx, "button", CHORDS_BLOCK);
    ok(call(&s, cx, "ui.click", json!({"element": block})));
    let sel = ok(call(&s, cx, "state.get", json!({"target": "selection"})));
    assert_eq!(
        sel,
        json!({
            "doc": opened["doc"], "time_range": {"start": 0, "end": BAR},
            "lanes": [{"seq": 3, "lane": 31}], "events": [1, 2],
        })
    );
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    assert_eq!(node(&tree, "button", CHORDS_BLOCK)["selected"], true);
    // A double click (count 2) opens the detail on the block.
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"element": block, "count": 2}),
    ));
    let e = ok(call(&s, cx, "state.get", json!({"target": "editor"})));
    assert_eq!(
        e["detail"],
        json!({"kind": "list", "seq": 3, "lane": 31, "start": 0, "end": BAR})
    );
}

#[gpui::test]
fn a_point_click_on_empty_space_clears_the_selection(cx: &mut TestAppContext) {
    let s = setup(cx, with_chords_block());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let tree = ok(call(&s, cx, "ui.tree", json!({})));
    let canvas = node(&tree, "group", "Arrangement")["frame"].clone();
    let (x, y) = (canvas["x"].as_f64().unwrap(), canvas["y"].as_f64().unwrap());
    // A point on the block selects it, as the mouse does.
    let r = ok(call(
        &s,
        cx,
        "ui.click",
        json!({"point": {"x": x + 50., "y": y + 22. + 48. + 24.}}),
    ));
    assert_eq!(r["events"], 2);
    let sel = ok(call(&s, cx, "state.get", json!({"target": "selection"})));
    assert_eq!(sel["events"], json!([1, 2]));
    // A point on empty space clears it.
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"point": {"x": x + 400., "y": y + 200.}}),
    ));
    let sel = ok(call(&s, cx, "state.get", json!({"target": "selection"})));
    assert_eq!(sel["events"], json!([]));
    assert_eq!(sel["lanes"], json!([]));
    assert_eq!(sel["time_range"], Value::Null);
}

#[gpui::test]
fn close_detail_with_nothing_open_is_unavailable(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    ok(call(&s, cx, "app.new_demo", json!({})));
    let e = call(&s, cx, "command.run", json!({"id": "editors.close_detail"})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Unavailable);
    assert_eq!(e.message, "No detail editor is open.");
    // Open, it closes.
    ok(call(&s, cx, "command.run", json!({"id": "editors.list"})));
    let r = ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "editors.close_detail"}),
    ));
    assert_eq!(r["status"], Value::Null);
    let e = ok(call(&s, cx, "state.get", json!({"target": "editor"})));
    assert_eq!(e["detail"], Value::Null);
}

#[gpui::test]
fn the_wait_reads_the_detail_kind(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let window = opened["window"].clone();
    let none = ok(call(
        &s,
        cx,
        "wait",
        json!({"condition": "detail_kind", "kind": "none", "window": window}),
    ));
    assert_eq!(none["condition"], "detail_kind");
    let token = s.calls.push(
        "wait",
        json!({"condition": "detail_kind", "kind": "list", "window": window}),
    );
    tick(&s, cx);
    assert!(s.calls.reply_to(token).is_none(), "still waiting");
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "editors.list", "window": window}),
    ));
    tick(&s, cx);
    ok(s.calls
        .reply_to(token)
        .expect("answered once the list opens"));
    // The pane hidden, the detail is still the list.
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "ui.toggle_bottom", "params": {"on": false}}),
    ));
    ok(call(
        &s,
        cx,
        "wait",
        json!({"condition": "detail_kind", "kind": "list", "timeout_ms": 100}),
    ));
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "editors.close_detail"}),
    ));
    ok(call(
        &s,
        cx,
        "wait",
        json!({"condition": "detail_kind", "kind": "none", "timeout_ms": 100}),
    ));
}

#[gpui::test]
fn state_get_editor_reports_the_detail(cx: &mut TestAppContext) {
    let s = setup(cx, demo_fake());
    let opened = ok(call(&s, cx, "app.new_demo", json!({})));
    let r = ok(call(&s, cx, "command.run", json!({"id": "editors.list"})));
    assert_eq!(r["status"], Value::Null);
    let e = ok(call(&s, cx, "state.get", json!({"target": "editor"})));
    assert_eq!(e["doc"], opened["doc"]);
    assert_eq!(
        e["detail"],
        json!({"kind": "list", "seq": 3, "lane": null, "start": null, "end": null})
    );
    // ui.toggle_bottom: `on` sets, absent flips; a bad `on` is refused.
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "ui.toggle_bottom", "params": {"on": false}}),
    ));
    let shown = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let a = AppState::global(cx).read(cx);
            let doc = a.windows()[0].doc;
            a.store
                .document(doc)
                .unwrap()
                .layout()
                .views
                .arrange
                .bottom
                .shown
        })
    };
    assert!(!shown(cx));
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "ui.toggle_bottom"}),
    ));
    assert!(shown(cx));
    let e = call(
        &s,
        cx,
        "command.run",
        json!({"id": "ui.toggle_bottom", "params": {"on": "yes"}}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert!(shown(cx));
    ok(call(
        &s,
        cx,
        "command.run",
        json!({"id": "ui.toggle_bottom", "params": {"on": true}}),
    ));
    assert!(shown(cx));
    // A key goes the same way.
    ok(call(
        &s,
        cx,
        "ui.key",
        json!({"key": "down", "modifiers": ["control", "command"]}),
    ));
    assert!(!shown(cx));
}
