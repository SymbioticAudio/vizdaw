//! The plug-in window's model (Swift `PluginWindowModelTests`, all 20, and
//! `PluginIdleTests.aGestureSnapshotIsApplied`; the slots half of
//! `slotChangesAndProblemsAreKept` is `mixer_slots.rs`'s
//! `the_mixer_is_read_again_when_the_slots_change`). Parameter drags go
//! through the document's mixer gestures, so a drag's touch carries no
//! value (the value heard now) and its first move sends the first value.

mod support;

use std::sync::Arc;
use std::time::Instant;

use support::{FakeSession, mixer_fake, slot_row};
use vizdaw_app::DocumentModel;
use vizdaw_app::plugin_window::{
    BYPASS_FAILED, HEADER_HEIGHT, LEARN_HELP, LOADING, NO_PARAMETERS, PARAMETER_FAILED,
    PluginWindowModel,
};
use vizdaw_ffi::automation::{AutomationStateRow, GesturePhaseRow};
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::snapshot::{ParamRow, SlotStatus};
use vizdaw_ffi::{EditorResize, EditorSize, VizError};

type Phase = GesturePhaseRow;

const T: Phase = Phase::Touch { value: None };
const R: Phase = Phase::Release;

fn v(value: f64) -> Phase {
    Phase::Value { value }
}

fn click_touch(value: f64) -> Phase {
    Phase::Touch { value: Some(value) }
}

fn param(id: u64, name: &str) -> ParamRow {
    ParamRow {
        id,
        name: name.into(),
        module: String::new(),
        min: 0.,
        max: 1.,
        default: 0.5,
        value: 0.5,
        stepped: false,
        automatable: true,
        hidden: false,
        read_only: false,
    }
}

fn ranged(id: u64, name: &str, min: f64, max: f64, value: f64) -> ParamRow {
    ParamRow {
        min,
        max,
        default: value,
        value,
        ..param(id, name)
    }
}

/// Swift `mixerFake`: Synth (1) has insert Gain (10).
fn fake() -> Arc<FakeSession> {
    let f = mixer_fake();
    f.edit_mixer(|m| m.strips[0].inserts = vec![slot_row(10, "Gain")]);
    Arc::new(f)
}

fn window(f: &Arc<FakeSession>) -> (DocumentModel, PluginWindowModel) {
    let mut d = DocumentModel::new(f.clone()).unwrap();
    let w = PluginWindowModel::new(&mut d, 10);
    (d, w)
}

fn phases(f: &FakeSession) -> Vec<Phase> {
    f.gestures().into_iter().map(|g| g.1).collect()
}

fn busy() -> VizError {
    VizError::Busy {
        message: "busy".into(),
    }
}

fn fixed(width: u32, height: u32) -> EditorSize {
    EditorSize {
        width,
        height,
        resizable: false,
    }
}

fn now() -> Instant {
    Instant::now()
}

#[test]
fn a_read_only_parameter_is_shown_but_not_editable() {
    let f = fake();
    let mut meter = param(0, "Level Meter");
    meter.automatable = false;
    meter.read_only = true;
    let mut manual = param(1, "Manual Only");
    manual.automatable = false;
    f.set_params(10, vec![meter, manual, param(2, "Gain")]);
    let (_d, w) = window(&f);
    let shown = w.visible_params();
    let names: Vec<&str> = shown.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Level Meter", "Manual Only", "Gain"]);
    assert!(!PluginWindowModel::is_editable(shown[0]));
    assert!(PluginWindowModel::is_editable(shown[1]));
    assert!(PluginWindowModel::is_editable(shown[2]));
}

#[test]
fn title_names_strip_and_plugin_and_hidden_params_are_left_out() {
    let f = fake();
    let mut secret = param(1, "Secret");
    secret.hidden = true;
    let mut mode = param(2, "Mode");
    mode.value = 2.6;
    mode.stepped = true;
    f.set_params(10, vec![param(0, "Gain"), secret, mode]);
    let (mut d, mut w) = window(&f);
    assert_eq!(w.title(), "Synth \u{2014} Gain");
    let shown = w.visible_params();
    let names: Vec<&str> = shown.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Gain", "Mode"]);
    assert_eq!(PluginWindowModel::value_text(shown[1]), "3");
    assert_eq!(PluginWindowModel::value_text(shown[0]), "0.50");
    assert!(!w.is_gone() && w.is_live());
    // A strip without a name: the plug-in's alone.
    f.edit_mixer(|m| m.strips[0].name = String::new());
    assert!(w.refresh(&mut d));
    assert_eq!(w.title(), "Gain");
    // No row: "Plugin".
    let none = PluginWindowModel::new(&mut d, 999);
    assert_eq!(none.title(), "Plugin");
    assert!(none.is_gone());
}

#[test]
fn a_parameter_drag_is_heard_and_committed_once() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.7, now());
    w.set_param(&mut d, 0, 0.2, now());
    w.commit_param(&mut d, 0, now());
    assert_eq!(phases(&f), [T, v(0.7), v(0.2), R]);
    assert!(
        f.gestures()
            .iter()
            .all(|g| g.0 == AutomationTargetRow::PluginParam { slot: 10, param: 0 })
    );
    assert!(
        f.edits().is_empty(),
        "the core commits the step at the release"
    );
    assert_eq!(w.params()[0].value, 0.2);
}

/// Ruling 8: values are normalised by the parameter's range.
#[test]
fn slider_values_are_normalised_by_the_range() {
    let f = fake();
    f.set_params(10, vec![ranged(3, "Cutoff", 20., 20_020., 1000.)]);
    let (mut d, mut w) = window(&f);
    w.begin_param(3);
    w.set_param(&mut d, 3, 10_020., now());
    w.set_param(&mut d, 3, 99_999., now());
    w.commit_param(&mut d, 3, now());
    assert_eq!(phases(&f), [T, v(0.5), v(1.), R]);
    assert_eq!(w.params()[0].value, 20_020., "shown clamped to the range");
}

/// A range with `max <= min` is 0; a NaN never reaches the core.
#[test]
fn an_empty_range_is_zero_and_nan_is_clamped() {
    let f = fake();
    f.set_params(10, vec![ranged(1, "Fixed", 5., 5., 5.), param(2, "Gain")]);
    let (mut d, mut w) = window(&f);
    w.begin_param(1);
    w.set_param(&mut d, 1, 7., now());
    w.commit_param(&mut d, 1, now());
    w.begin_param(2);
    w.set_param(&mut d, 2, f64::NAN, now());
    w.set_param(&mut d, 2, f64::INFINITY, now());
    w.set_param(&mut d, 2, f64::NEG_INFINITY, now());
    w.commit_param(&mut d, 2, now());
    assert_eq!(phases(&f), [T, v(0.), R, T, v(0.), v(1.), v(0.), R]);
    assert!(
        w.params().iter().all(|p| !p.value.is_nan()),
        "the shown value is never NaN"
    );
}

/// The release lands the step: the document shows it at once.
#[test]
fn the_release_applies_the_snapshot() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    let before = d.snapshot().version;
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.7, now());
    w.commit_param(&mut d, 0, now());
    assert!(f.snap().version > before);
    assert_eq!(d.snapshot().version, f.snap().version);
    assert_eq!(w.params()[0].value, 0.7);
}

/// A refused touch is not kept: the drag's later moves send nothing and
/// its end sends no release; the next drag touches again.
#[test]
fn a_refused_touch_is_not_kept() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    f.fail_next(
        "gesture_touch",
        VizError::Busy {
            message: "Finish the open edit first.".into(),
        },
    );
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.7, now());
    assert_eq!(d.alert.as_ref().unwrap().title, PARAMETER_FAILED);
    w.set_param(&mut d, 0, 0.8, now());
    w.commit_param(&mut d, 0, now());
    assert!(f.gestures().is_empty());
    assert_eq!(w.params()[0].value, 0.5, "not heard, not shown");
    w.end_open_drags(&mut d, now());
    assert!(f.gestures().is_empty());
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.3, now());
    w.commit_param(&mut d, 0, now());
    assert_eq!(phases(&f), [T, v(0.3), R]);
}

/// A refused mark lives only for its drag: it goes with the parameter,
/// so the parameter's next drag touches again.
#[test]
fn a_refused_mark_goes_with_its_drag_or_its_parameter() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    // Refused, then let go: the next drag is heard.
    f.fail_next("gesture_touch", busy());
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.7, now());
    w.commit_param(&mut d, 0, now());
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.4, now());
    w.commit_param(&mut d, 0, now());
    assert_eq!(phases(&f), [T, v(0.4), R]);
    // Refused, then the parameter goes and comes back before the drag
    // ends: the next drag touches.
    f.fail_next("gesture_touch", busy());
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.7, now());
    f.set_params(10, vec![]);
    w.refresh(&mut d);
    f.set_params(10, vec![param(0, "Gain")]);
    w.refresh(&mut d);
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.6, now());
    w.commit_param(&mut d, 0, now());
    assert_eq!(phases(&f), [T, v(0.4), R, T, v(0.6), R]);
    // The same when the slot goes and comes back.
    f.fail_next("gesture_touch", busy());
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.7, now());
    let inserts = f.mixer_snapshot().strips[0].inserts.clone();
    f.edit_mixer(|m| m.strips[0].inserts = vec![]);
    w.refresh(&mut d);
    f.edit_mixer(|m| m.strips[0].inserts = inserts);
    w.refresh(&mut d);
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.3, now());
    w.commit_param(&mut d, 0, now());
    let all = phases(&f);
    assert_eq!(all[all.len() - 3..], [T, v(0.3), R]);
    assert_eq!(all.len(), 9);
}

/// A move the core refuses releases the taken touch, once.
#[test]
fn a_failing_move_still_releases_the_taken_touch() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.7, now());
    f.fail_next(
        "gesture_value",
        VizError::Model {
            message: "refused".into(),
        },
    );
    w.set_param(&mut d, 0, 0.2, now());
    assert_eq!(d.alert.as_ref().unwrap().title, PARAMETER_FAILED);
    w.set_param(&mut d, 0, 0.1, now());
    w.commit_param(&mut d, 0, now());
    assert_eq!(phases(&f), [T, v(0.7), R]);
    assert_eq!(w.params()[0].value, 0.7, "the last heard value stays");
}

/// The parameter or the slot going mid-drag releases its touch once.
#[test]
fn a_parameter_or_slot_going_mid_drag_releases_once() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain"), param(1, "Pan")]);
    let (mut d, mut w) = window(&f);
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.7, now());
    w.begin_param(1);
    w.set_param(&mut d, 1, 0.1, now());
    f.set_params(10, vec![param(1, "Pan")]);
    w.refresh(&mut d);
    assert_eq!(phases(&f), [T, v(0.7), T, v(0.1), R]);
    assert_eq!(
        f.gestures().last().unwrap().0,
        AutomationTargetRow::PluginParam { slot: 10, param: 0 }
    );
    w.commit_param(&mut d, 0, now());
    assert_eq!(f.gestures().len(), 5, "released once");
    f.edit_mixer(|m| m.strips[0].inserts = vec![]);
    f.clear_params(10);
    w.refresh(&mut d);
    assert!(w.is_gone());
    assert_eq!(f.gestures().len(), 6);
    let last = f.gestures().last().cloned().unwrap();
    assert_eq!(
        last.0,
        AutomationTargetRow::PluginParam { slot: 10, param: 1 }
    );
    assert_eq!(last.1, R);
    w.end_open_drags(&mut d, now());
    assert_eq!(f.gestures().len(), 6, "released once");
}

/// A Busy refresh mid-drag releases nothing and keeps the dragged value.
#[test]
fn a_busy_refresh_mid_drag_keeps_the_touch() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.7, now());
    f.set_call_error("mixer", Some(busy()));
    f.set_call_error("params", Some(busy()));
    assert!(!w.refresh(&mut d));
    f.set_call_error("mixer", None);
    f.set_call_error("params", None);
    assert!(w.refresh(&mut d));
    assert_eq!(phases(&f), [T, v(0.7)]);
    assert_eq!(w.params()[0].value, 0.7, "the dragged value stays shown");
    w.commit_param(&mut d, 0, now());
    assert_eq!(phases(&f), [T, v(0.7), R]);
}

/// Final review m6: a value with no editing span open (a keyboard or
/// accessibility increment) is a click: touch, value and release at
/// once, so no touch is left open to hold the document's edit.
#[test]
fn a_value_with_no_editing_span_is_a_click() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    w.set_param(&mut d, 0, 0.3, now());
    assert_eq!(phases(&f), [click_touch(0.3), v(0.3), R]);
    assert!(
        f.gestures()
            .iter()
            .all(|g| g.0 == AutomationTargetRow::PluginParam { slot: 10, param: 0 })
    );
    assert_eq!(w.params()[0].value, 0.3, "shown at once");
    assert_eq!(d.snapshot().version, f.snap().version, "its step lands");
    w.commit_param(&mut d, 0, now());
    w.end_open_drags(&mut d, now());
    assert_eq!(f.gestures().len(), 3, "nothing left to release");
    // A refused click sends nothing more.
    f.fail_next("gesture_touch", busy());
    w.set_param(&mut d, 0, 0.6, now());
    assert_eq!(f.gestures().len(), 3);
    assert_eq!(d.alert.as_ref().unwrap().title, PARAMETER_FAILED);
    w.set_param(&mut d, 0, 0.6, now());
    assert_eq!(f.gestures().len(), 6, "the next value clicks again");
}

#[test]
fn the_editor_opens_when_the_plugin_has_one_else_parameters_show() {
    let f = fake();
    let (mut d, mut w) = window(&f);
    assert!(!w.open_editor(&mut d, 0x1000, 2.));
    assert_eq!(w.editor_size(), None);
    assert!(w.editor_problem().unwrap().contains("editor unavailable"));
    assert_eq!(w.empty_text(), w.editor_problem().unwrap());
    f.set_editor(10, Some(fixed(400, 300)));
    assert!(w.open_editor(&mut d, 0x1000, 2.));
    assert_eq!(f.opened_editors(), [(10, 0x1000, 2.)]);
    assert_eq!(w.editor_size().unwrap().width, 400);
    assert_eq!(w.editor_problem(), None);
    assert_eq!(w.resize(&d, 500, 500), None);
    assert_eq!(
        w.editor_size().unwrap().width,
        400,
        "a fixed-size editor keeps its size"
    );
    assert!(f.resized_editors().is_empty(), "not even asked");
    w.opened(&d);
    assert_eq!(f.windows_open(), [10]);
    assert!(w.closed(&mut d, now()));
    assert_eq!(f.closed_editors(), [10]);
    assert!(f.windows_open().is_empty());
    assert_eq!(w.editor_size(), None);
}

#[test]
fn a_resizable_editor_takes_the_size_the_plugin_answers() {
    let f = fake();
    f.set_editor(
        10,
        Some(EditorSize {
            width: 400,
            height: 300,
            resizable: true,
        }),
    );
    f.set_resize_answer(Some((480, 320)));
    let (mut d, mut w) = window(&f);
    assert!(w.open_editor(&mut d, 0x1000, 1.));
    assert_eq!(w.content_size(), (400., 300. + HEADER_HEIGHT));
    let s = w.resize(&d, 500, 333).unwrap();
    assert_eq!(f.resized_editors(), [(10, 500, 333)]);
    assert_eq!((s.width, s.height), (480, 320), "snap to the answer");
    assert_eq!(w.content_size(), (480., 320. + HEADER_HEIGHT));
    // A refused resize keeps the size.
    f.fail_next("resize_editor", busy());
    assert_eq!(w.resize(&d, 600, 600), None);
    assert_eq!(w.editor_size().unwrap().width, 480);
}

#[test]
fn a_placeholder_shows_its_reason_and_a_removed_slot_is_gone() {
    let f = fake();
    f.edit_mixer(|m| {
        m.strips[0].inserts[0].status = SlotStatus::Placeholder {
            reason: "Disabled in Plugin Manager".into(),
        }
    });
    let (mut d, mut w) = window(&f);
    assert!(!w.open_editor(&mut d, 0x1000, 1.));
    assert_eq!(w.editor_problem(), Some("Disabled in Plugin Manager"));
    assert_eq!(w.placeholder_reason(), Some("Disabled in Plugin Manager"));
    assert!(f.opened_editors().is_empty(), "not asked");
    // Not loaded yet: "Loading…".
    f.edit_mixer(|m| m.strips[0].inserts[0].status = SlotStatus::Loading);
    w.refresh(&mut d);
    assert!(!w.open_editor(&mut d, 0x1000, 1.));
    assert_eq!(w.editor_problem(), Some(LOADING));
    f.edit_mixer(|m| m.strips[0].inserts = vec![]);
    w.refresh(&mut d);
    assert!(w.is_gone());
}

#[test]
fn an_unloaded_instance_drops_its_editor_and_the_next_one_opens_again() {
    let f = fake();
    f.set_editor(10, Some(fixed(400, 300)));
    let (mut d, mut w) = window(&f);
    assert!(w.open_editor(&mut d, 0x1000, 1.));
    // Turned off in the Plugin Manager: the slot stays, a placeholder.
    f.edit_mixer(|m| {
        m.strips[0].inserts[0].status = SlotStatus::Placeholder {
            reason: "Disabled in Plugin Manager".into(),
        }
    });
    w.refresh(&mut d);
    assert!(!w.is_gone());
    assert_eq!(w.editor_size(), None);
    assert_eq!(w.placeholder_reason(), Some("Disabled in Plugin Manager"));
    // Turned on again: the new instance's editor opens.
    f.edit_mixer(|m| m.strips[0].inserts[0].status = SlotStatus::Live);
    w.refresh(&mut d);
    assert!(w.open_editor(&mut d, 0x1000, 1.));
    assert_eq!(w.editor_size().unwrap().width, 400);
    assert!(w.closed(&mut d, now()));
}

#[test]
fn bypass_from_the_header() {
    let f = fake();
    let (mut d, mut w) = window(&f);
    w.toggle_bypass(&mut d);
    let bypasses: Vec<String> = f
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("set_bypass"))
        .collect();
    assert_eq!(bypasses, ["set_bypass(10,true)"]);
    assert!(w.row().unwrap().bypass, "the window shows it");
    f.fail_next(
        "set_bypass",
        VizError::Model {
            message: "no".into(),
        },
    );
    w.toggle_bypass(&mut d);
    let a = d.alert.as_ref().unwrap();
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        (BYPASS_FAILED, "no")
    );
    assert!(w.row().unwrap().bypass);
}

#[test]
fn a_busy_close_is_pending_and_retried_until_it_succeeds() {
    let f = fake();
    f.set_editor(10, Some(fixed(400, 300)));
    let (mut d, mut w) = window(&f);
    assert!(w.open_editor(&mut d, 0x1000, 1.));
    w.opened(&d);
    f.fail_next("close_editor", busy());
    assert!(!w.closed(&mut d, now()), "the view must be kept");
    assert!(w.close_pending());
    assert!(f.closed_editors().is_empty());
    assert!(f.windows_open().is_empty());
    assert!(w.retry_close(&d), "the next tick succeeds");
    assert!(!w.close_pending());
    assert_eq!(f.closed_editors(), [10]);
    assert!(w.retry_close(&d), "nothing left to retry");
    assert_eq!(f.closed_editors(), [10]);
}

#[test]
fn a_drag_left_open_by_the_closing_window_is_committed() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    w.begin_param(0);
    w.set_param(&mut d, 0, 0.9, now());
    assert!(w.closed(&mut d, now()));
    assert_eq!(phases(&f), [T, v(0.9), R]);
    w.end_open_drags(&mut d, now());
    assert_eq!(
        phases(&f).iter().filter(|p| **p == R).count(),
        1,
        "released once"
    );
}

#[test]
fn a_busy_refresh_keeps_the_window_and_its_parameters() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    w.opened(&d);
    f.set_call_error("mixer", Some(busy()));
    f.set_call_error("params", Some(busy()));
    assert!(!w.refresh(&mut d));
    assert!(!w.is_gone());
    assert_eq!(w.visible_params().len(), 1);
    assert_eq!(w.title(), "Synth \u{2014} Gain");
    assert_eq!(f.windows_open(), [10], "still saved as open");
    assert!(w.needs_refresh(&d), "a Busy read is read again");
    f.set_call_error("mixer", None);
    f.set_call_error("params", None);
    assert!(w.refresh(&mut d));
    assert!(!w.needs_refresh(&d));
}

#[test]
fn a_busy_editor_open_is_owed_and_retried() {
    let f = fake();
    f.set_editor(10, Some(fixed(400, 300)));
    let (mut d, mut w) = window(&f);
    f.fail_next(
        "open_editor",
        VizError::Busy {
            message: "editor opening".into(),
        },
    );
    assert!(!w.open_editor(&mut d, 0x1000, 1.));
    assert!(w.open_owed());
    assert_eq!(w.editor_size(), None);
    assert!(w.open_editor(&mut d, 0x1000, 1.));
    assert!(!w.open_owed());
    assert_eq!(w.editor_size().unwrap().width, 400);
    // Another refusal is not owed.
    assert!(w.closed(&mut d, now()));
    f.set_editor(10, None);
    assert!(!w.open_editor(&mut d, 0x1000, 1.));
    assert!(!w.open_owed());
}

/// Carry 3 (the app half): `editor_resize` is followed only by an open
/// editor; one for a slot whose editor is closed is dropped, so a later
/// open never takes a stale size.
#[test]
fn a_resize_for_a_slot_whose_editor_is_closed_is_dropped() {
    let f = fake();
    f.set_editor(10, Some(fixed(400, 300)));
    let resize = |w, h| EditorResize {
        slot: 10,
        width: w,
        height: h,
    };
    let (mut d, mut w) = window(&f);
    f.set_idle_editor_resize(vec![resize(500, 200)]);
    d.idle();
    assert_eq!(w.take_editor_resize(&mut d), None, "no editor yet");
    assert!(w.open_editor(&mut d, 0x1000, 1.));
    assert_eq!(w.take_editor_resize(&mut d), None, "dropped, not kept");
    f.set_idle_editor_resize(vec![resize(640, 480)]);
    d.idle();
    let s = w.take_editor_resize(&mut d).unwrap();
    assert_eq!((s.width, s.height), (640, 480));
    assert_eq!(w.editor_size().unwrap().width, 640);
    assert_eq!(w.content_size(), (640., 480. + HEADER_HEIGHT));
    assert_eq!(w.take_editor_resize(&mut d), None, "once");
    // Asked for after the window closed: dropped.
    assert!(w.closed(&mut d, now()));
    f.set_idle_editor_resize(vec![resize(800, 600)]);
    d.idle();
    let mut again = PluginWindowModel::new(&mut d, 10);
    assert!(again.open_editor(&mut d, 0x2000, 1.));
    assert_eq!(again.take_editor_resize(&mut d), None);
    assert_eq!(again.editor_size().unwrap().width, 400);
}

/// Swift `PluginIdleTests.aGestureSnapshotIsApplied`: a plug-in tick's
/// gesture step is applied, and its changed parameters are kept for the
/// slot's window, which reads them again once.
#[test]
fn a_gesture_snapshot_is_applied_and_changed_params_are_kept() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain")]);
    let (mut d, mut w) = window(&f);
    assert!(!w.needs_refresh(&d));
    let mut next = f.snap();
    next.undo_label = Some("Change Plugin Parameter".into());
    f.set_idle(Some(next));
    f.set_idle_params_changed(vec![10, 77]);
    assert!(d.idle());
    assert_eq!(d.undo_title(), "Undo Change Plugin Parameter");
    assert!(d.params_changed().contains(&10));
    assert!(
        !d.params_changed().contains(&77),
        "no window shows slot 77: nothing keeps it"
    );
    assert_eq!(d.slots_version(), 0);
    assert!(w.needs_refresh(&d));
    f.set_params(10, vec![ranged(0, "Gain", 0., 1., 0.25)]);
    assert!(w.refresh(&mut d));
    assert_eq!(w.params()[0].value, 0.25);
    assert!(!d.params_changed().contains(&10), "read once");
    assert!(!w.needs_refresh(&d));
}

#[test]
fn the_window_reads_again_when_the_document_or_slots_move() {
    let f = fake();
    let (mut d, mut w) = window(&f);
    assert!(!w.needs_refresh(&d));
    f.set_idle_slots_changed();
    d.idle();
    assert!(w.needs_refresh(&d));
    w.refresh(&mut d);
    assert!(!w.needs_refresh(&d));
    w.toggle_bypass(&mut d);
    assert!(!w.needs_refresh(&d), "its own step is read already");
    let mut next = f.snap();
    next.version += 1;
    f.set_idle(Some(next));
    d.idle();
    assert!(w.needs_refresh(&d));
}

/// The Learn button's parameter: the last one touched here, or by the
/// plug-in's own editor as the core lists it.
#[test]
fn the_last_touched_parameter_is_followed_for_learn() {
    let f = fake();
    f.set_params(10, vec![param(0, "Gain"), param(1, "Pan")]);
    f.set_editor(10, Some(fixed(400, 300)));
    let (mut d, mut w) = window(&f);
    assert_eq!(w.last_touched(), None);
    assert_eq!(w.learn_help(), LEARN_HELP);
    w.begin_param(1);
    assert_eq!(w.last_touched(), Some((10, 1)));
    assert_eq!(w.learn_help(), "MIDI Learn: Pan");
    w.commit_param(&mut d, 1, now());
    // Without an editor the core's touches are not followed.
    let touched = AutomationStateRow {
        modes: vec![],
        touched: vec![
            AutomationTargetRow::PluginParam { slot: 11, param: 4 },
            AutomationTargetRow::PluginParam { slot: 10, param: 9 },
        ],
        touched_values: vec![0.5, 0.5],
        suspended: false,
        writing: false,
    };
    f.set_automation_state(touched);
    w.follow_core_touches(&d);
    assert_eq!(w.last_touched(), Some((10, 1)));
    assert!(w.open_editor(&mut d, 0x1000, 1.));
    w.follow_core_touches(&d);
    assert_eq!(w.last_touched(), Some((10, 9)), "this slot's only");
    assert_eq!(w.learn_help(), "MIDI Learn: parameter 9");
}

#[test]
fn the_content_is_the_editor_or_the_parameter_list_under_the_header() {
    let f = fake();
    let (mut d, mut w) = window(&f);
    assert_eq!(w.empty_text(), NO_PARAMETERS);
    assert_eq!(w.content_size(), (360., 120. + HEADER_HEIGHT));
    f.set_params(10, (0..3).map(|i| param(i, "P")).collect());
    w.refresh(&mut d);
    assert_eq!(w.content_size(), (400., 120. + HEADER_HEIGHT));
    f.set_params(10, (0..20).map(|i| param(i, "P")).collect());
    w.refresh(&mut d);
    assert_eq!(w.content_size(), (400., 420. + HEADER_HEIGHT));
}
