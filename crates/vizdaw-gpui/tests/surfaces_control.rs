//! `surface.*` and `state.get surfaces` over the control channel in the
//! GPUI app (slice 9d Task 2), the launch's `surfaces_load`, and the
//! input picker's claimed ports, through the real pump with a fake core
//! (no CoreMIDI client, no `surfaces.json`).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use core_control::protocol::{RawRequest, RequestId};
use core_control::server::Call;
use core_control::{ControlError, ErrorCode};
use gpui::TestAppContext;
use serde_json::{Map, Value, json};
use vizdaw_app::testing::{FakeCore, FakeSession, demo_fake};
use vizdaw_app::{DocumentStore, Prefs};
use vizdaw_ffi::surfaces::{SurfaceKindRow, SurfaceStateRow};
use vizdaw_gpui::AppState;
use vizdaw_gpui::control::{self, CallSource};

#[derive(Default)]
struct FakeCalls {
    queue: RefCell<VecDeque<Call>>,
    replies: RefCell<Vec<(u64, Result<Value, ControlError>)>>,
    next: RefCell<u64>,
}

impl FakeCalls {
    fn push(&self, cmd: &str, params: Value) -> u64 {
        let params: Map<String, Value> = match params {
            Value::Object(m) => m,
            _ => Map::new(),
        };
        let raw = RawRequest {
            id: RequestId::Num(1),
            cmd: cmd.into(),
            params,
        };
        let command = core_control::validate(&raw).expect("a valid request");
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
        token
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

    fn extend(&self, _token: u64, _by: Duration) -> bool {
        true
    }

    fn is_waiting(&self, _token: u64) -> bool {
        true
    }
}

struct Setup {
    core: Arc<FakeCore>,
    calls: FakeCalls,
}

/// The app over a fake core, with no document open.
fn setup(cx: &mut TestAppContext) -> Setup {
    let core = Arc::new(FakeCore::new());
    let store = DocumentStore::with_core(core.clone());
    cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
    });
    Setup {
        core,
        calls: FakeCalls::default(),
    }
}

/// [`setup`] with the demo (`fake`) open in a window.
fn with_demo(cx: &mut TestAppContext) -> (Setup, Arc<FakeSession>) {
    let s = setup(cx);
    let fake = Arc::new(demo_fake());
    s.core.hand_out("new_demo", fake.clone());
    ok(call(&s, cx, "app.new_demo", json!({})));
    (s, fake)
}

fn call(
    s: &Setup,
    cx: &mut TestAppContext,
    cmd: &str,
    params: Value,
) -> Result<Value, ControlError> {
    let token = s.calls.push(cmd, params);
    for _ in 0..20 {
        cx.update(|cx| control::pump(&s.calls, cx));
        cx.run_until_parked();
        let reply = s
            .calls
            .replies
            .borrow()
            .iter()
            .rev()
            .find(|(t, _)| *t == token)
            .map(|(_, r)| r.clone());
        if let Some(r) = reply {
            return r;
        }
    }
    panic!("{cmd} was never answered");
}

fn run(s: &Setup, cx: &mut TestAppContext, id: &str, params: Value) -> Result<Value, ControlError> {
    call(s, cx, "command.run", json!({"id": id, "params": params}))
}

fn surfaces(
    s: &Setup,
    cx: &mut TestAppContext,
    window: Option<i64>,
) -> Result<Value, ControlError> {
    let mut params = json!({"target": "surfaces"});
    if let Some(n) = window {
        params["window"] = json!(n);
    }
    call(s, cx, "state.get", params)
}

fn ok(r: Result<Value, ControlError>) -> Value {
    r.unwrap_or_else(|e| panic!("call failed: {e:?}"))
}

fn code(r: Result<Value, ControlError>) -> ErrorCode {
    r.expect_err("the call ran").code
}

#[gpui::test]
fn state_get_surfaces_answers_app_wide_without_a_document(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.core.set_surfaces_state(SurfaceStateRow {
        problems: vec!["surfaces.json could not be read".into()],
        ..SurfaceStateRow::default()
    });
    let r = ok(surfaces(&s, cx, None));
    assert_eq!(r["doc"], Value::Null);
    assert_eq!(r["feedback"], json!([]));
    assert_eq!(r["surfaces"], json!([]));
    assert_eq!(r["problems"], json!(["surfaces.json could not be read"]));
    // A named window without a document is `no_document`
    // (`SurfaceCommandsTests.swift:168`).
    assert_eq!(code(surfaces(&s, cx, Some(424_242))), ErrorCode::NoDocument);
}

#[gpui::test]
fn state_get_surfaces_with_a_document_names_it(cx: &mut TestAppContext) {
    let (s, fake) = with_demo(cx);
    fake.set_surface_state(SurfaceStateRow {
        learning: Some("edit.undo".into()),
        ..SurfaceStateRow::default()
    });
    s.core.set_surfaces_state(SurfaceStateRow {
        learning: Some("the core's, not the window's".into()),
        ..SurfaceStateRow::default()
    });
    let windows = ok(call(&s, cx, "state.get", json!({"target": "windows"})));
    let w = &windows["windows"][0];
    let r = ok(surfaces(&s, cx, None));
    assert_eq!(r["doc"], w["doc"]);
    assert_eq!(r["learning"], json!("edit.undo"));
    let named = ok(surfaces(&s, cx, w["number"].as_i64()));
    assert_eq!(named["doc"], w["doc"]);
    assert_eq!(named["learning"], json!("edit.undo"));
}

#[gpui::test]
fn surface_add_runs_without_a_window(cx: &mut TestAppContext) {
    let s = setup(cx);
    let r = ok(run(
        &s,
        cx,
        "surface.add",
        json!({"type": "mcu", "input": "In A", "output": "Out A"}),
    ));
    assert_eq!(r["command"], json!("surface.add"));
    assert_eq!(r["doc"], Value::Null);
    assert_eq!(r["window"], Value::Null);
    assert_eq!(r["status"], json!("surface 0"));
    assert_eq!(
        s.core.surfaces_added(),
        vec![(SurfaceKindRow::Mcu, "In A".into(), "Out A".into(), None)]
    );
    let r = ok(run(&s, cx, "surface.remove", json!({"surface": 0})));
    assert_eq!((&r["doc"], &r["window"]), (&Value::Null, &Value::Null));
    assert_eq!(s.core.surfaces_removed(), vec![0]);
    // The others check their arguments first, then need a document.
    assert_eq!(
        code(run(&s, cx, "surface.learn", json!({}))),
        ErrorCode::InvalidParams
    );
    let e = run(
        &s,
        cx,
        "surface.inject",
        json!({"surface": 0, "bytes": "90 68 7f"}),
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::NoDocument);
    assert_eq!(e.message, "No document is open.");
}

#[gpui::test]
fn surface_inject_reaches_the_windows_session(cx: &mut TestAppContext) {
    let (s, fake) = with_demo(cx);
    let r = ok(run(
        &s,
        cx,
        "surface.inject",
        json!({"surface": 1, "bytes": "90 68 7f"}),
    ));
    assert_eq!(r["command"], json!("surface.inject"));
    assert!(r["doc"].is_string() && r["window"].is_i64());
    assert_eq!(fake.surface_injections(), vec![(1, vec![0x90, 0x68, 0x7f])]);
}

#[gpui::test]
fn launch_loads_the_surfaces_before_the_first_document(cx: &mut TestAppContext) {
    let core = Arc::new(FakeCore::new());
    let store = DocumentStore::with_core(core.clone());
    cx.update(|cx| {
        let state = vizdaw_gpui::install(store, cx);
        vizdaw_gpui::launch(&state, Prefs::in_memory(), false, cx, |_| {})
            .expect("the first window");
    });
    cx.run_until_parked();
    let calls = core.calls();
    let at = |name: &str| {
        calls
            .iter()
            .position(|c| c == name)
            .unwrap_or_else(|| panic!("no {name} in {calls:?}"))
    };
    assert!(at("surfaces_load") < at("new_empty"), "{calls:?}");
}

#[gpui::test]
fn the_picker_marks_a_surface_input_as_claimed(cx: &mut TestAppContext) {
    let core = Arc::new(FakeCore::new());
    core.set_midi_sources(vec!["In A".into(), "Keys".into()]);
    let store = DocumentStore::with_core(core.clone());
    cx.update(|cx| {
        let state = vizdaw_gpui::install(store, cx);
        state.update(cx, |s, _| s.warm_sources());
    });
    cx.run_until_parked();
    let s = Setup {
        core: core.clone(),
        calls: FakeCalls::default(),
    };
    core.hand_out("new_demo", Arc::new(demo_fake()));
    ok(call(&s, cx, "app.new_demo", json!({})));
    let midi_titles = |cx: &mut TestAppContext| -> Vec<String> {
        cx.update(|cx| {
            let app = AppState::global(cx);
            let app = app.read(cx);
            let doc = app.store.front().expect("the demo");
            let d = app.store.document(doc).unwrap();
            d.lanes()
                .iter()
                .flat_map(|l| d.inputs().groups(l))
                .find(|g| g.title == "MIDI Input")
                .expect("a track that takes MIDI")
                .choices
                .into_iter()
                .map(|c| c.title)
                .collect()
        })
    };
    assert!(
        midi_titles(cx).contains(&"In A".to_owned()),
        "{:?}",
        midi_titles(cx)
    );
    ok(run(
        &s,
        cx,
        "surface.add",
        json!({"type": "mcu", "input": "In A", "output": "Out A"}),
    ));
    let titles = midi_titles(cx);
    assert!(
        titles.contains(&"In A (used by a surface)".to_owned()),
        "{titles:?}"
    );
    assert!(titles.contains(&"Keys".to_owned()));
    ok(run(&s, cx, "surface.remove", json!({"surface": 0})));
    assert!(midi_titles(cx).contains(&"In A".to_owned()));
}
