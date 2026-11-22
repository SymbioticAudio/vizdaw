mod support;

use std::sync::Arc;

use core_control::registry::{self, ArgValue};
use core_control::{ControlError, ErrorCode};
use support::{FakeSession, demo_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::commands::{FieldFocus, Ran, TRANSPORT_COMMANDS, enabled, perform_from_menu, run};
use vizdaw_ffi::VizError;
use vizdaw_ffi::selection::{SelectionRow, TickRange};

const BAR: i64 = 61_440;
const TEXT: &str = "A text field has the keyboard.";

/// The demo with the Song one bar long, so Play at its end restarts.
fn demo() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(demo_fake());
    f.set_length(3, BAR);
    let doc = DocumentModel::new(f.clone()).unwrap();
    (f, doc)
}

fn go(id: &str, doc: &mut DocumentModel) -> Result<Ran, ControlError> {
    run(id, &[], doc)
}

fn failure(r: Result<Ran, ControlError>) -> ControlError {
    r.expect_err("expected a refusal")
}

fn is(e: &ControlError, code: ErrorCode, message: &str) {
    assert_eq!((e.code, e.message.as_str()), (code, message));
}

fn tick(f: &FakeSession) -> i64 {
    use vizdaw_app::DocSession;
    f.transport().unwrap().tick
}

fn playing(f: &FakeSession) -> bool {
    use vizdaw_app::DocSession;
    f.transport().unwrap().playing
}

#[test]
fn stop_goes_back_to_where_playing_started() {
    let (f, mut m) = demo();
    f.set_length(3, 4 * BAR);
    let mut m2 = DocumentModel::new(f.clone()).unwrap();
    is(
        &failure(go("transport.stop", &mut m)),
        ErrorCode::Unavailable,
        "Nothing is playing.",
    );
    f.set_transport(|t| t.tick = 30_000);
    go("transport.play_stop", &mut m2).unwrap();
    assert_eq!(m2.play_start(), Some(30_000));
    assert!(playing(&f));
    f.set_transport(|t| t.tick = 90_000);
    go("transport.stop", &mut m2).unwrap();
    assert!(!playing(&f));
    assert_eq!(tick(&f), 30_000);
}

#[test]
fn play_only_starts() {
    let (f, mut m) = demo();
    go("transport.play", &mut m).unwrap();
    assert!(playing(&f));
    f.set_play_error(Some(VizError::Model {
        message: "played twice".into(),
    }));
    go("transport.play", &mut m).expect("Play while playing does nothing");
    assert!(playing(&f));
    go("transport.stop", &mut m).unwrap();
    f.set_play_error(None);
    go("transport.play", &mut m).unwrap();
    assert!(playing(&f), "a stopped transport plays");
}

#[test]
fn play_selection_plays_its_span_once_or_onwards() {
    let (f, mut m) = demo();
    let e = failure(go("transport.play_selection", &mut m));
    assert_eq!(e.code, ErrorCode::Unavailable, "nothing selected");
    is(
        &e,
        ErrorCode::Unavailable,
        "Select something in the shown sequence first.",
    );
    f.set_span(Some(TickRange {
        start: 1_000,
        end: 5_000,
    }));
    go("transport.play_selection", &mut m).unwrap();
    assert_eq!(f.plays_from().last(), Some(&(3, 1_000, Some(5_000))));
    assert_eq!(m.play_start(), Some(1_000));
    go("transport.play_from_selection", &mut m).unwrap();
    assert_eq!(f.plays_from().last(), Some(&(3, 1_000, None)));
    f.set_play_error(Some(VizError::Model {
        message: "the end must be after the start".into(),
    }));
    is(
        &failure(go("transport.play_selection", &mut m)),
        ErrorCode::Failed,
        "the end must be after the start",
    );
}

#[test]
fn play_from_edit_in_needs_a_range() {
    let (f, mut m) = demo();
    let e = failure(go("transport.play_from_edit_in", &mut m));
    is(
        &e,
        ErrorCode::Unavailable,
        "Set an edit-in point (select a time range) first.",
    );
    assert!(!enabled("transport.play_from_edit_in", &m));
    f.force_selection(SelectionRow {
        time_range: Some(TickRange {
            start: 7_680,
            end: 61_440,
        }),
        lanes: vec![],
        events: vec![],
    });
    let mut m = DocumentModel::new(f.clone()).unwrap();
    assert!(enabled("transport.play_from_edit_in", &m));
    go("transport.play_from_edit_in", &mut m).unwrap();
    assert_eq!(f.plays_from().last(), Some(&(3, 7_680, None)));
}

#[test]
fn marker_keys_locate_to_the_nearest_marker() {
    let (f, mut m) = demo();
    f.set_marker_ticks(vec![61_440, 122_880]);
    go("transport.next_marker", &mut m).unwrap();
    assert_eq!(tick(&f), 61_440);
    go("transport.next_marker", &mut m).unwrap();
    assert_eq!(tick(&f), 122_880);
    is(
        &failure(go("transport.next_marker", &mut m)),
        ErrorCode::Unavailable,
        "No marker after the position.",
    );
    go("transport.previous_marker", &mut m).unwrap();
    assert_eq!(tick(&f), 61_440);
    is(
        &failure(go("transport.previous_marker", &mut m)),
        ErrorCode::Unavailable,
        "No marker before the position.",
    );
}

#[test]
fn scrubbing_waits() {
    let (_f, mut m) = demo();
    is(
        &failure(go("transport.scrub", &mut m)),
        ErrorCode::Unavailable,
        "Scrubbing is not available yet.",
    );
    assert!(!enabled("transport.scrub", &m));
}

#[test]
fn stop_returns_to_where_the_engine_actually_started() {
    let (f, mut m) = demo();
    // Stopped at the sequence's end: Play restarts at bar 1.
    f.set_transport(|t| t.tick = BAR);
    go("transport.play_stop", &mut m).unwrap();
    assert_eq!(m.play_start(), Some(0));
    go("transport.stop", &mut m).unwrap();
    assert_eq!(tick(&f), 0);
    // A stale cursor in another sequence is ignored too.
    f.set_transport(|t| {
        t.seq = Some(1);
        t.tick = 30_000;
    });
    go("transport.play_stop", &mut m).unwrap();
    assert_eq!(m.play_start(), Some(0));
}

#[test]
fn bare_key_commands_wait_for_text_fields() {
    let (f, mut m) = demo();
    m.begin_text_editing("track-rename");
    for id in [
        "transport.play_selection",
        "transport.next_marker",
        "transport.play_stop",
        "transport.go_position",
    ] {
        is(&failure(go(id, &mut m)), ErrorCode::Unavailable, TEXT);
        assert!(!enabled(id, &m), "{id}");
    }
    // The channel's Loop does not wait; its key and menu item do.
    assert!(!enabled("transport.loop", &m));
    assert!(f.plays_from().is_empty());
    m.end_text_editing("track-rename");
    assert!(enabled("transport.next_marker", &m));
}

#[test]
fn busy_is_busy() {
    let (f, mut m) = demo();
    f.set_span(Some(TickRange {
        start: 1_000,
        end: 5_000,
    }));
    f.set_play_error(Some(VizError::Busy {
        message: "The core is busy.".into(),
    }));
    is(
        &failure(go("transport.play_selection", &mut m)),
        ErrorCode::Busy,
        "The core is busy.",
    );
}

#[test]
fn rewind_and_fast_forward_step_a_bar() {
    let (f, mut m) = demo();
    go("transport.fast_forward", &mut m).unwrap();
    assert_eq!(tick(&f), BAR);
    go("transport.fast_forward", &mut m).unwrap();
    assert_eq!(tick(&f), 2 * BAR);
    go("transport.rewind", &mut m).unwrap();
    assert_eq!(f.step_bars(), vec![true, true, false]);
    assert_eq!(tick(&f), BAR);
    assert!(f.winds().is_empty() && f.hold_ends() == 0);
    go("transport.rewind", &mut m).unwrap();
    go("transport.rewind", &mut m).unwrap();
    assert_eq!(tick(&f), 0, "stops at the start");
}

#[test]
fn rewind_and_fast_forward_wait_and_refuse_as_the_other_transport_keys() {
    let (f, mut m) = demo();
    let ids = ["transport.rewind", "transport.fast_forward"];
    m.begin_text_editing("track-rename");
    for id in ids {
        is(&failure(go(id, &mut m)), ErrorCode::Unavailable, TEXT);
        assert!(!enabled(id, &m), "{id}");
        assert!(perform_from_menu(id, &mut m).is_err());
        assert_eq!(m.status(), Some(TEXT));
    }
    assert!(f.step_bars().is_empty());
    m.end_text_editing("track-rename");
    for id in ids {
        assert!(enabled(id, &m), "{id}");
    }
    m.select(None);
    for id in ids {
        is(
            &failure(go(id, &mut m)),
            ErrorCode::Unavailable,
            "Select a sequence first.",
        );
        assert!(!enabled(id, &m), "{id}");
    }
    assert!(f.step_bars().is_empty());
    m.select(Some(3));
    f.set_jump_error(Some(VizError::Busy {
        message: "Stop recording first.".into(),
    }));
    for id in ids {
        is(
            &failure(go(id, &mut m)),
            ErrorCode::Busy,
            "Stop recording first.",
        );
    }
    assert!(f.step_bars().is_empty() && m.alert.is_none());
}

#[test]
fn a_core_refusal_is_failed_with_its_text() {
    let (f, mut m) = demo();
    f.set_jump_error(Some(VizError::Model {
        message: "no sequence to step in".into(),
    }));
    is(
        &failure(go("transport.rewind", &mut m)),
        ErrorCode::Failed,
        "no sequence to step in",
    );
}

#[test]
fn play_stop_needs_a_sequence_and_the_keys() {
    let (f, mut m) = demo();
    m.select(None);
    is(
        &failure(go("transport.play_stop", &mut m)),
        ErrorCode::Unavailable,
        "Select a sequence to play.",
    );
    m.select(Some(3));
    m.begin_text_editing("track-rename");
    is(
        &failure(go("transport.play_stop", &mut m)),
        ErrorCode::Unavailable,
        TEXT,
    );
    m.end_text_editing("track-rename");
    go("transport.play_stop", &mut m).unwrap();
    assert!(playing(&f));
    // Stop here does not locate back.
    f.set_transport(|t| t.tick = 5_000);
    go("transport.play_stop", &mut m).unwrap();
    assert!(!playing(&f) && tick(&f) == 5_000);
    assert_eq!(f.count("locate(0)"), 0);
    f.set_play_error(Some(VizError::Busy {
        message: "The core is busy.".into(),
    }));
    is(
        &failure(go("transport.play_stop", &mut m)),
        ErrorCode::Busy,
        "The core is busy.",
    );
    f.set_play_error(Some(VizError::Model {
        message: "no device".into(),
    }));
    is(
        &failure(go("transport.play_stop", &mut m)),
        ErrorCode::Failed,
        "Play Failed: no device",
    );
}

#[test]
fn the_menu_play_stop_raises_an_alert_and_a_refusal_shows_in_the_status() {
    let (f, mut m) = demo();
    f.set_play_error(Some(VizError::Model {
        message: "no device".into(),
    }));
    perform_from_menu("transport.play_stop", &mut m).unwrap();
    let a = m.alert.take().unwrap();
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("Play Failed", "no device")
    );
    f.set_play_error(None);
    m.select(None);
    assert!(perform_from_menu("transport.play_stop", &mut m).is_err());
    assert_eq!(m.status(), Some("Select a sequence to play."));
    let e = perform_from_menu("transport.stop", &mut m).unwrap_err();
    is(&e, ErrorCode::Unavailable, "Nothing is playing.");
    assert_eq!(m.status(), Some("Nothing is playing."));
}

#[test]
fn return_to_start_needs_the_return_key() {
    let (f, mut m) = demo();
    f.set_transport(|t| t.tick = 5_000);
    m.begin_text_editing("track-rename");
    is(
        &failure(go("transport.return_to_start", &mut m)),
        ErrorCode::Unavailable,
        "A text field or list has the keyboard.",
    );
    assert!(!enabled("transport.return_to_start", &m));
    m.end_text_editing("track-rename");
    go("transport.return_to_start", &mut m).unwrap();
    assert_eq!(tick(&f), 0);
}

#[test]
fn loop_takes_on_or_toggles() {
    let (f, mut m) = demo();
    let on = |v: bool| vec![("on".to_string(), ArgValue::Flag(v))];
    run("transport.loop", &on(true), &mut m).unwrap();
    assert!(m.looping() && f.looped());
    run("transport.loop", &on(true), &mut m).unwrap();
    assert_eq!(f.count("set_loop(true)"), 1, "already on: no call");
    go("transport.loop", &mut m).unwrap();
    assert!(!m.looping() && !f.looped());
    go("transport.loop", &mut m).unwrap();
    assert!(m.looping());
    run("transport.loop", &on(false), &mut m).unwrap();
    assert!(!m.looping());
}

#[test]
fn a_failing_loop_is_an_alert_and_stays_as_it_was() {
    let (f, mut m) = demo();
    f.set_loop_error(Some(VizError::Busy {
        message: "Stop recording first.".into(),
    }));
    m.toggle_loop();
    assert!(!m.looping());
    let a = m.alert.take().unwrap();
    assert_eq!(a.title, "Loop Failed");
    assert_eq!(a.message, "Stop recording first.");
}

#[test]
fn locating_failures_are_alerts() {
    let (f, mut m) = demo();
    m.locate(1_000);
    assert!(m.alert.is_none());
    f.set_locate_error(Some(VizError::Model {
        message: "cannot locate".into(),
    }));
    m.locate(1_000);
    let a = m.alert.take().unwrap();
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("Locate Failed", "cannot locate")
    );
    m.return_to_start();
    let a = m.alert.take().unwrap();
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("Locate Failed", "cannot locate")
    );
    assert!(m.move_to(2_000).is_err());
    assert!(m.alert.is_none(), "move_to leaves the error to its caller");
    f.set_locate_error(None);
    assert!(m.move_to(2_000).is_ok());
}

#[test]
fn a_busy_transport_read_keeps_the_last_good_state() {
    let (f, mut m) = demo();
    go("transport.play_stop", &mut m).unwrap();
    assert!(playing(&f));
    f.set_transport_error(Some(VizError::Busy {
        message: "plugin host is busy".into(),
    }));
    // Playing, as last read: Stop tries the stop; it is not "Nothing is playing."
    go("transport.stop", &mut m).unwrap();
    assert_eq!(f.count("stop"), 1);
    f.set_transport_error(None);
    assert!(!playing(&f));
    go("transport.play_stop", &mut m).unwrap();
    f.set_transport_error(Some(VizError::Busy {
        message: "plugin host is busy".into(),
    }));
    go("transport.play_stop", &mut m).unwrap();
    assert_eq!(
        f.count("stop"),
        2,
        "play_stop stops, it does not start again"
    );
    assert_eq!(f.count("play(3)"), 2);
}

#[test]
fn the_field_keys_ask_the_ui_to_focus_their_field() {
    let (_f, mut m) = demo();
    let ran = go("transport.go_position", &mut m).unwrap();
    assert_eq!(ran.focus, Some(FieldFocus::Position));
    let ran = go("transport.go_edit_in", &mut m).unwrap();
    assert_eq!(ran.focus, Some(FieldFocus::EditIn));
    assert_eq!(go("transport.rewind", &mut m).unwrap().focus, None);
}

#[test]
fn the_text_editing_owners_gate_the_keys() {
    let (_f, mut m) = demo();
    assert!(m.transport_keys_enabled() && m.return_key_enabled());
    m.begin_text_editing("a");
    m.begin_text_editing("b");
    m.end_text_editing("a");
    assert!(!m.transport_keys_enabled() && !m.return_key_enabled());
    m.end_text_editing("b");
    assert!(m.transport_keys_enabled());
}

#[test]
fn every_transport_command_is_in_the_registry() {
    assert_eq!(TRANSPORT_COMMANDS.len(), 15);
    for id in TRANSPORT_COMMANDS {
        assert!(registry::find(id).is_some(), "{id}");
    }
}

#[test]
fn transport_commands_refuse_extra_params() {
    let (f, mut m) = demo();
    let bars = vec![("bars".to_string(), ArgValue::Int(2))];
    let e = failure(run("transport.rewind", &bars, &mut m));
    assert_eq!(e.code, ErrorCode::InvalidParams);
    let x = vec![("x".to_string(), ArgValue::Text("x".into()))];
    let e = failure(run("transport.fast_forward", &x, &mut m));
    assert_eq!(e.code, ErrorCode::InvalidParams);
    for id in TRANSPORT_COMMANDS
        .iter()
        .filter(|i| **i != "transport.loop")
    {
        assert_eq!(
            failure(run(id, &bars, &mut m)).code,
            ErrorCode::InvalidParams,
            "{id}"
        );
    }
    assert!(f.calls().is_empty(), "{:?}", f.calls());
}
