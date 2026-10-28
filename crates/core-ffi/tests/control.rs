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

#[test]
fn an_empty_label_falls_back_to_the_title_in_the_id() {
    let r = ControlResult::Tree {
        window: 1,
        nodes: vec![UiNodeRow {
            path: vec![0],
            role: "AXButton".into(),
            subrole: None,
            label: Some(String::new()),
            title: Some("Play".into()),
            value: None,
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
            enabled: true,
            selected: false,
            focused: false,
            actions: vec![],
        }],
        truncated: false,
    };
    let v = result_json(&r);
    let e = core_control::element::parse(v["nodes"][0]["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        e.check,
        control_element_check("AXButton".into(), Some(String::new()), Some("Play".into()))
    );
}

#[test]
fn the_editor_state_shows_step_entry_and_the_transport_marks() {
    let editor = |step| vizdaw_ffi::EditorStateRow {
        detail: None,
        windows: vec![],
        tool: "arrow".into(),
        snap_enabled: true,
        snap_division: "thirty_second".into(),
        exact_mode: false,
        gesture: false,
        status: None,
        step,
        marks: vizdaw_ffi::TransportMarksRow {
            locators: vec![None, Some(61_440)],
            punch_in: 0,
            punch_out: 245_760,
            punch: true,
            overdub: false,
        },
    };
    let off = result_json(&ControlResult::Editor {
        doc: "D".into(),
        editor: editor(None),
    });
    assert_eq!(off["step"], Value::Null);
    assert_eq!(off["snap"]["division"], "thirty_second");
    assert_eq!(
        off["transport"],
        json!({"locators": [null, 61_440], "punch_in": 0, "punch_out": 245_760, "punch": true, "overdub": false})
    );
    let on = result_json(&ControlResult::Editor {
        doc: "D".into(),
        editor: editor(Some(vizdaw_ffi::StepEntryStateRow {
            seq: 3,
            lane: 31,
            tick: 7_680,
            value: "eighth".into(),
            dotted: true,
            triplet: false,
            steps: 2,
        })),
    });
    assert_eq!(
        on["step"],
        json!({"seq": 3, "lane": 31, "tick": 7_680, "value": "eighth", "dotted": true, "triplet": false, "steps": 2})
    );
}

#[test]
fn media_and_project_results_as_json() {
    use vizdaw_ffi::{
        BitDepthRow, MediaOriginRow, MediaRow, OfflineReasonRow, ProjectFormatRow, ProjectInfoRow,
    };
    let v = result_json(&ControlResult::Media {
        doc: "D".into(),
        items: vec![MediaRow {
            id: 2,
            name: "kick".into(),
            path: "Media/kick.caf".into(),
            channels: 2,
            frames: 10,
            sample_rate: 48_000,
            origin: MediaOriginRow::Imported,
            original_name: Some("kick.wav".into()),
            offline: Some(OfflineReasonRow::Format),
            uses: 3,
            used_percent: 40.0,
            peak: Some(0.25),
        }],
    });
    assert_eq!(v["items"][0]["offline_reason"], "format");
    assert_eq!(v["items"][0]["offline"], true);
    assert_eq!(v["items"][0]["used_percent"], json!(40.0));
    assert_eq!(v["items"][0]["peak"], json!(0.25));
    let changed = result_json(&ControlResult::Media {
        doc: "D".into(),
        items: vec![MediaRow {
            id: 3,
            name: "pad".into(),
            path: "Media/pad.caf".into(),
            channels: 2,
            frames: 10,
            sample_rate: 48_000,
            origin: MediaOriginRow::Imported,
            original_name: None,
            offline: Some(OfflineReasonRow::Changed),
            uses: 0,
            used_percent: 0.0,
            peak: None,
        }],
    });
    assert_eq!(changed["items"][0]["offline_reason"], "changed");
    assert_eq!(changed["items"][0]["peak"], Value::Null);
    let p = result_json(&ControlResult::Project {
        doc: "D".into(),
        info: ProjectInfoRow {
            format: ProjectFormatRow {
                sample_rate: 44_100,
                depth: BitDepthRow::Float32,
            },
            mismatched: 1,
            rate_mismatch: true,
            device_rate: 48_000.0,
        },
    });
    assert_eq!(p["rate"], json!(44_100));
    assert_eq!(p["depth"], json!("32f"));
    assert_eq!(p["label"], json!("44.1 kHz, 32-bit float"));
    assert_eq!(p["rate_mismatch"], json!(true));
}

/// A job outlives the app timeout once the app extends its call (fix round 1,
/// I-1); an unextended call still times out.
#[test]
fn an_extended_call_outlives_the_app_timeout() {
    let d = private_dir();
    let s = ControlServer::start(socket(&d).to_string_lossy().into_owned(), 300).unwrap();
    let params = json!({"id": "media.import", "params": {"path": "/tmp/x.wav"}});
    let h = send(&socket(&d), "command.run", params.clone());
    let call = next(&s);
    assert!(s.extend_timeout(call.token, 20_000).unwrap());
    thread::sleep(Duration::from_millis(900));
    assert!(
        s.is_waiting(call.token),
        "still waiting past the app timeout"
    );
    let r = ControlResult::Ran {
        command: "media.import".into(),
        doc: Some("D".into()),
        window: None,
        undo_label: None,
        status: None,
    };
    assert!(s.reply(call.token, r).unwrap());
    assert_eq!(h.join().unwrap().unwrap()["command"], "media.import");
    // Without the extension the same call times out.
    let h = send(&socket(&d), "command.run", params);
    let call = next(&s);
    thread::sleep(Duration::from_millis(900));
    let e = h.join().unwrap().unwrap_err();
    assert!(format!("{e:?}").contains("imeout"), "{e:?}");
    assert!(!s.extend_timeout(call.token, 1000).unwrap());
    s.stop().unwrap();
}

/// Plan 5c Task 14: `state.get record` and `input` as JSON.
#[test]
fn record_and_input_results_as_json() {
    use vizdaw_ffi::{InputInfoRow, RecordLaneRow, RecordSpecRow, RecordStateRow, TickRange};
    let state = RecordStateRow {
        recording: true,
        seq: Some(3),
        spec: Some(RecordSpecRow {
            replace: false,
            punch: Some(TickRange { start: 0, end: 10 }),
            looped: true,
            count_in_bars: 0,
            pre_roll_bars: 1,
            passes_as_takes: true,
        }),
        pass: 2,
        kept: 1,
        discarded: 0,
        lanes: vec![RecordLaneRow {
            lane: 7,
            audio: true,
            path: Some("Media/Recorded/Mic.caf".into()),
            clipped: 4,
            peak: 0.25,
            return_frames: 0,
        }],
        frames: 96_000,
        overflows: 0,
        gap_frames: 0,
        free_bytes: Some(1 << 30),
        underruns: 5,
        late_starts: 6,
        recovered: vec!["Media/Recorded/old.caf".into()],
        recovered_files: vec![],
        pass_start_tick: Some(7680),
        pass_start_file_frame: 48_000,
        range_start_tick: 0,
    };
    let v = result_json(&ControlResult::Record {
        doc: "D".into(),
        state: state.clone(),
    });
    assert_eq!(v["recording"], json!(true));
    assert_eq!(v["spec"]["loop"], json!(true));
    assert_eq!(v["spec"]["punch"], json!({"start": 0, "end": 10}));
    assert_eq!(v["pass"], json!(2));
    assert_eq!(v["lanes"][0]["clipped"], json!(4));
    assert_eq!(v["recovered"][0], json!("Media/Recorded/old.caf"));
    // Pre-flight M11: the stream counters ride along.
    assert_eq!(v["underruns"], json!(5));
    assert_eq!(v["late_starts"], json!(6));
    assert_eq!(v["overflows"], json!(0));
    assert_eq!(v["recovered_files"], json!([]));
    let i = result_json(&ControlResult::InputState {
        doc: "D".into(),
        info: InputInfoRow {
            device: "Offline".into(),
            channels: 3,
            names: vec!["Fake In 1".into()],
            input_latency_frames: 512,
            output_latency_frames: 512,
            buffer_frames: 512,
            round_trip_frames: 2048,
            opening: false,
            sample_rate: 48_000.0,
        },
        levels: vec![0.25, 1.0, 0.0],
    });
    assert_eq!(i["channels"], json!(3));
    assert_eq!(i["input_latency"], json!(512));
    // Plan 5d-1 Ruling 26: the settings, the round trip and the meters.
    assert_eq!(i["device_buffer_frames"], json!(512));
    assert_eq!(i["round_trip_frames"], json!(2048));
    assert_eq!(i["round_trip_ms"], json!(42.67));
    assert_eq!(i["levels"], json!([0.25, 1.0, 0.0]));
    assert_eq!(i["opening"], json!(false));
    assert!(i["software_monitoring"].is_boolean() && i["buffer_frames"].is_u64());
    assert!(i["latency_trim"].is_i64(), "{i}");
    assert_eq!(v["lanes"][0]["peak"], json!(0.25));
    assert_eq!(v["lanes"][0]["return_frames"], json!(0));
    assert_eq!(v["pass_start_tick"], json!(7680));
    assert_eq!(v["pass_start_file_frame"], json!(48_000));
    assert_eq!(v["range_start_tick"], json!(0));
    // Task 10: the anchor is signed (a file that starts after the pass's
    // tick), and a synth return lane's take lands `return_frames` earlier.
    let mut early = state.clone();
    early.pass_start_file_frame = -256;
    early.pass_start_tick = None;
    if let Some(l) = early.lanes.first_mut() {
        l.return_frames = 1200;
    }
    let e = result_json(&ControlResult::Record {
        doc: "D".into(),
        state: early,
    });
    assert_eq!(e["pass_start_file_frame"], json!(-256));
    assert_eq!(e["pass_start_tick"], Value::Null);
    assert_eq!(e["lanes"][0]["return_frames"], json!(1200));
}

/// Plan 5c-2 Task 10: `destination.measure_return` answers with the
/// milliseconds it set and the onsets it found.
#[test]
fn a_measurement_answers_with_its_milliseconds() {
    let v = result_json(&ControlResult::Measured {
        doc: "D".into(),
        dest: 4,
        return_latency_ms: 12.5,
        onsets: 5,
    });
    assert_eq!(
        v,
        json!({"doc": "D", "dest": 4, "return_latency_ms": 12.5, "onsets": 5})
    );
    // An f32 shows as the thousandths it means, not its rounding.
    let v = result_json(&ControlResult::Measured {
        doc: "D".into(),
        dest: 4,
        return_latency_ms: 24.9,
        onsets: 3,
    });
    assert_eq!(v["return_latency_ms"], json!(24.9));
}

/// Plan 5c Task 14: document lanes show their takes, input and monitor
/// (channels counted from 1), and the transport carries the record faults.
#[test]
fn lanes_show_takes_input_and_monitor() {
    use vizdaw_ffi::{LaneInputRow, MonitorModeRow};
    let session = Session::new_demo().unwrap();
    let snapshot = session.snapshot().unwrap();
    let seq = snapshot.sequences[0].id;
    let one = session.lanes(seq).unwrap().remove(0);
    let mut lanes = vec![one.clone(), one.clone(), one.clone(), one];
    lanes[0].input = Some(LaneInputRow::Audio {
        first: 2,
        stereo: true,
        return_of: Some(7),
    });
    lanes[3].input = Some(LaneInputRow::Audio {
        first: 0,
        stereo: false,
        return_of: None,
    });
    lanes[0].monitor = MonitorModeRow::Input;
    lanes[1].input = Some(LaneInputRow::Midi {
        port: Some("Keys".into()),
        channel: Some(15),
    });
    lanes[2].input = None;
    let v = result_json(&ControlResult::Document {
        doc: "D".into(),
        path: None,
        snapshot,
        selected_seq: Some(seq),
        lanes,
        alert: None,
    });
    let l = v["lanes"].as_array().unwrap();
    assert_eq!(
        l[0]["input"],
        json!({"kind": "audio", "channel": 3, "stereo": true, "return_of": 7})
    );
    // Plan 5c-2 Task 10: plain audio links no synth.
    assert_eq!(
        l[3]["input"],
        json!({"kind": "audio", "channel": 1, "stereo": false, "return_of": null})
    );
    assert_eq!(l[0]["monitor"], json!("input"));
    assert_eq!(
        l[1]["input"],
        json!({"kind": "midi", "port": "Keys", "midi_channel": 16})
    );
    assert_eq!(l[2]["input"], Value::Null);
    assert_eq!(l[2]["monitor"], json!("auto"));
    assert!(l[2]["takes"].is_number() && l[2]["active_take"].is_string());
}

/// Plan 5d-1 Task 12 (Ruling 26): `state.get transport` says whether a
/// count-in is playing.
#[test]
fn the_transport_says_when_it_counts_in() {
    let state = vizdaw_ffi::TransportState {
        playing: false,
        seq: Some(1),
        tick: 0,
        position_bbu: "1.1.000".into(),
        seconds: 0.0,
        sample_rate: 48_000.0,
        device: "Offline".into(),
        faults: vec![],
        report: vec![],
        host_time_ns: 0,
        ticks_per_second: 960.0,
        rate_mismatch: false,
        counting_in: true,
    };
    let t = result_json(&ControlResult::Transport {
        doc: "D".into(),
        state: state.clone(),
    });
    assert_eq!(t["counting_in"], json!(true), "{t}");
    let t = result_json(&ControlResult::Transport {
        doc: "D".into(),
        state: vizdaw_ffi::TransportState {
            counting_in: false,
            ..state
        },
    });
    assert_eq!(t["counting_in"], json!(false), "{t}");
}
