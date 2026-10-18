//! The control channel through the FFI (control spec §4, §5.5): calls
//! reach the app as typed requests and typed results become the reply's
//! JSON. Success paths wait generously (slow CI).

use core_control::client::{CallError, Client};
use core_control::request::{Command, CrashAnswer, RecoveryAnswers};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use vizdaw_ffi::control::{request_of, result_json};
use vizdaw_ffi::{
    AlertRow, ControlCall, ControlErrorCode, ControlFailure, ControlRequest, ControlResult,
    ControlServer, CrashAnswerRow, DocFactsRow, Session, UiNodeRow, WaitConditionRow, WaitFactsRow,
    control_command_ids, control_element_check, control_wait_met,
};

const LONG: Duration = Duration::from_secs(20);

fn private_dir() -> tempfile::TempDir {
    let d = tempfile::Builder::new()
        .prefix("vzf")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

fn socket(d: &tempfile::TempDir) -> PathBuf {
    d.path().join("s.sock")
}

/// The next call, waiting for it (generously).
fn next(s: &ControlServer) -> ControlCall {
    let deadline = Instant::now() + LONG;
    loop {
        if let Some(c) = s.next_call().unwrap() {
            return c;
        }
        assert!(Instant::now() < deadline, "no call arrived");
        thread::sleep(Duration::from_millis(1));
    }
}

/// Sends `cmd` from another thread; returns its outcome's handle.
fn send(
    path: &Path,
    cmd: &'static str,
    params: Value,
) -> thread::JoinHandle<Result<Value, CallError>> {
    let path = path.to_path_buf();
    thread::spawn(move || Client::connect(&path).unwrap().call(cmd, params, LONG))
}

#[test]
fn a_state_call_round_trips_as_typed_rows() {
    let d = private_dir();
    let s = ControlServer::start(socket(&d).to_string_lossy().into_owned(), 60_000).unwrap();
    let h = send(
        &socket(&d),
        "state.get",
        json!({"target": "document", "window": 9}),
    );
    let call = next(&s);
    assert!(matches!(
        call.request,
        ControlRequest::State {
            target: vizdaw_ffi::StateTargetRow::Document,
            window: Some(vizdaw_ffi::WindowRefRow::Number { number: 9 })
        }
    ));
    let session = Session::new_demo().unwrap();
    let snapshot = session.snapshot().unwrap();
    let r = ControlResult::Document {
        doc: "D1".into(),
        path: None,
        snapshot,
        selected_seq: None,
        lanes: vec![],
        alert: Some(AlertRow {
            title: "Play Failed".into(),
            message: "no device".into(),
        }),
    };
    assert!(s.reply(call.token, r).unwrap());
    let v = h.join().unwrap().unwrap();
    assert_eq!(v["doc"], "D1");
    let names: Vec<&str> = v["sequences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"Song"), "{names:?}");
    assert_eq!(v["alert"]["title"], "Play Failed");
    assert!(!s.is_waiting(call.token));
    s.stop().unwrap();
    assert!(!socket(&d).exists());
}

#[test]
fn failures_carry_their_code_and_dialog() {
    let d = private_dir();
    let s = ControlServer::start(socket(&d).to_string_lossy().into_owned(), 60_000).unwrap();
    let h = send(&socket(&d), "command.run", json!({"id": "file.save"}));
    let call = next(&s);
    s.fail(
        call.token,
        ControlFailure {
            code: ControlErrorCode::DialogRequired,
            message: "this would open the \"Save\" dialog; pass `path` instead".into(),
            dialog: Some("Save".into()),
        },
    )
    .unwrap();
    match h.join().unwrap() {
        Err(CallError::Reply(e)) => {
            assert_eq!(e.code.as_str(), "dialog_required");
            assert_eq!(e.dialog.as_deref(), Some("Save"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_wait_that_times_out_reports_its_last_facts() {
    let d = private_dir();
    let s = ControlServer::start(socket(&d).to_string_lossy().into_owned(), 60_000).unwrap();
    let h = send(
        &socket(&d),
        "wait",
        json!({"condition": "playing", "timeout_ms": 50}),
    );
    let call = next(&s);
    let ControlRequest::Wait {
        condition,
        timeout_ms,
        ..
    } = call.request
    else {
        panic!("{:?}", call.request)
    };
    let facts = WaitFactsRow {
        document: Some(DocFactsRow {
            playing: false,
            tick: 120,
            detail_kind: "none".into(),
        }),
        documents: 1,
        window_titles: vec!["Untitled".into()],
    };
    assert!(!control_wait_met(condition.clone(), facts.clone()));
    s.wait_timed_out(call.token, condition, facts, timeout_ms)
        .unwrap();
    match h.join().unwrap() {
        Err(CallError::Reply(e)) => {
            assert_eq!(e.code.as_str(), "timeout");
            let d = e.details.unwrap();
            assert_eq!(d["facts"]["document"]["tick"], 120);
            assert_eq!(d["timeout_ms"], 50);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn screenshots_default_to_the_socket_folder_and_answers_map() {
    let d = private_dir();
    let s =
        Arc::new(ControlServer::start(socket(&d).to_string_lossy().into_owned(), 60_000).unwrap());
    let h = send(&socket(&d), "ui.screenshot", json!({}));
    let call = next(&s);
    let ControlRequest::Screenshot { path, .. } = &call.request else {
        panic!("{:?}", call.request)
    };
    assert!(
        path.starts_with(&d.path().to_string_lossy().into_owned()) && path.ends_with(".png"),
        "{path}"
    );
    s.reply(
        call.token,
        ControlResult::Screenshot {
            window: 3,
            path: path.clone(),
            width: 20,
            height: 10,
        },
    )
    .unwrap();
    assert_eq!(h.join().unwrap().unwrap()["width"], 20);
    let open = request_of(
        Command::AppOpen {
            path: "/tmp/a.vizdaw".into(),
            answers: RecoveryAnswers {
                plugins: Some(CrashAnswer::Off),
                restore_autosave: Some(true),
            },
        },
        || unreachable!(),
    );
    match open {
        Some(ControlRequest::Open { answers, .. }) => {
            assert_eq!(answers.plugins, Some(CrashAnswerRow::PluginsOff));
            assert_eq!(answers.restore_autosave, Some(true));
        }
        other => panic!("{other:?}"),
    }
    assert!(
        request_of(Command::Ping, || unreachable!()).is_none(),
        "the server answers it"
    );
}

#[test]
fn tree_nodes_nest_by_path_and_carry_checked_ids() {
    let node = |path: Vec<u32>, role: &str, label: Option<&str>| UiNodeRow {
        path,
        role: role.into(),
        subrole: None,
        label: label.map(str::to_string),
        title: None,
        value: None,
        x: 1.0,
        y: 2.0,
        width: 3.0,
        height: 4.0,
        enabled: true,
        selected: false,
        focused: false,
        actions: vec![],
    };
    let r = ControlResult::Tree {
        window: 5,
        nodes: vec![
            node(vec![0], "AXGroup", Some("Arrangement")),
            node(vec![0, 0], "AXButton", Some("Block Chords")),
            node(vec![0, 1], "AXButton", Some("Block Bass")),
            node(vec![1], "AXToolbar", None),
            node(vec![0, 10], "AXButton", Some("Block Late")),
        ],
        truncated: false,
    };
    let v = result_json(&r);
    let roots = v["nodes"].as_array().unwrap();
    assert_eq!(roots.len(), 2);
    let kids: Vec<&str> = roots[0]["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["label"].as_str().unwrap())
        .collect();
    assert_eq!(
        kids,
        ["Block Chords", "Block Bass", "Block Late"],
        "siblings in index order"
    );
    let id = roots[0]["children"][1]["id"].as_str().unwrap();
    let e = core_control::element::parse(id).unwrap();
    assert_eq!(e.path, vec![0, 1]);
    assert_eq!(
        e.check,
        control_element_check("AXButton".into(), Some("Block Bass".into()), None)
    );
    assert_eq!(
        control_element_check("AXWindow".into(), None, Some("Untitled".into())),
        control_element_check(
            "AXWindow".into(),
            Some(String::new()),
            Some("Untitled".into())
        ),
        "no label: the title counts"
    );
    assert!(
        roots[1].get("label").is_none(),
        "absent fields are left out"
    );
}

#[test]
fn the_registry_ids_reach_swift() {
    let ids = control_command_ids();
    assert!(ids.contains(&"transport.play_stop".to_string()));
    assert_eq!(ids.len(), core_control::registry::COMMANDS.len());
    assert!(control_wait_met(
        WaitConditionRow::DocumentsAtLeast { count: 2 },
        WaitFactsRow {
            document: None,
            documents: 2,
            window_titles: vec![]
        }
    ));
}
