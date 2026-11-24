//! The plug-in windows' registry and lifecycle through GPUI's test
//! platform with the scripted session. The test platform has no native
//! window, so no container is made: the editor opens with no parent view
//! (parent 0), which the scripted session takes.

mod mixer_support;

use std::time::Duration;

use core_control::protocol::{RawRequest, RequestId};
use gpui::{AnyWindowHandle, TestAppContext, VisualTestContext, size};
use mixer_support::{Setup, setup};
use serde_json::{Value, json};
use vizdaw_app::DocSession;
use vizdaw_app::plugin_window::HEADER_HEIGHT;
use vizdaw_app::testing::{FakeSession, dest_row, mixer_fake, slot_row};
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::{BuiltinSound, EditorResize, EditorSize, VizError};
use vizdaw_gpui::plugin_windows::{self, Subject, capped};
use vizdaw_gpui::{AppState, WindowKind, control};

/// Swift `mixerFake`: Synth (1) has insert Gain (10), whose editor is
/// 300 × 200 and fixed; destination 7 is a Built-in Synth.
fn fake() -> FakeSession {
    let f = mixer_fake();
    f.edit_mixer(|m| m.strips[0].inserts = vec![slot_row(10, "Gain")]);
    f.set_editor(10, Some(fixed(300, 200)));
    f.edit_destinations(|d| {
        d.push(DestRow {
            channel: Some(0),
            builtin_sound: Some(BuiltinSound::Piano),
            ..dest_row(7, "Built-in Synth")
        })
    });
    f
}

fn fixed(width: u32, height: u32) -> EditorSize {
    EditorSize {
        width,
        height,
        resizable: false,
    }
}

fn busy() -> VizError {
    VizError::Busy {
        message: "busy".into(),
    }
}

fn show(s: &mut Setup, slot: u64) -> Option<AnyWindowHandle> {
    let doc = s.doc();
    let h =
        s.cx.update(|_, cx| plugin_windows::show_plugin_window(doc, slot, cx));
    s.cx.run_until_parked();
    h
}

/// Lets the document's idle and the windows' tick run.
fn tick(s: &mut Setup) {
    for _ in 0..4 {
        s.cx.executor().advance_clock(Duration::from_millis(300));
        s.cx.run_until_parked();
    }
}

fn kinds(s: &mut Setup, kind: WindowKind) -> usize {
    s.app.read_with(&s.cx, |a, _| {
        a.windows().iter().filter(|w| w.kind == kind).count()
    })
}

fn opened_slots(f: &FakeSession) -> Vec<(u64, u64)> {
    f.opened_editors()
        .into_iter()
        .map(|(slot, parent, _)| (slot, parent))
        .collect()
}

/// The window's content size in points.
fn content(s: &mut Setup, h: AnyWindowHandle) -> (f32, f32) {
    let b =
        s.cx.update(|_, cx| h.update(cx, |_, w, _| w.bounds().size))
            .unwrap();
    (f32::from(b.width), f32::from(b.height))
}

fn call(s: &mut Setup, cmd: &str, params: Value) -> Value {
    let raw = RawRequest {
        id: RequestId::Num(1),
        cmd: cmd.into(),
        params: params.as_object().cloned().unwrap(),
    };
    let command = core_control::validate(&raw).expect("a valid request");
    let handled = s.cx.cx.update(|cx| control::handle(&command, cx));
    let control::Handled::Reply(Ok(v)) = handled else {
        panic!("{cmd} failed");
    };
    v
}

fn state_windows(s: &mut Setup) -> Vec<Value> {
    call(s, "state.get", json!({"target": "windows"}))["windows"]
        .as_array()
        .unwrap()
        .clone()
}

#[gpui::test]
fn the_window_is_registered_before_its_editor_opens(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let doc = s.doc();
    let fake = s.fake.clone();
    s.cx.update(|_, cx| {
        let h = plugin_windows::show_plugin_window(doc, 10, cx);
        assert!(h.is_some());
        let a = AppState::global(cx);
        let a = a.read(cx);
        assert!(a.plugin_windows().is_open(doc, Subject::Plugin(10)));
        assert!(a.windows().iter().any(|w| w.kind == WindowKind::Plugin));
        assert!(fake.opened_editors().is_empty(), "the open is deferred");
        assert_eq!(fake.windows_open(), vec![10], "saved as open at once");
    });
    s.cx.run_until_parked();
    assert_eq!(opened_slots(&s.fake), vec![(10, 0)]);
}

#[gpui::test]
fn a_second_open_brings_the_window_to_the_front(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let first = show(&mut s, 10).unwrap();
    let again = show(&mut s, 10).unwrap();
    assert_eq!(first, again);
    assert_eq!(kinds(&mut s, WindowKind::Plugin), 1);
    tick(&mut s);
    assert_eq!(opened_slots(&s.fake), vec![(10, 0)], "opened once");
}

#[gpui::test]
fn a_slot_that_is_gone_opens_no_window(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    assert!(show(&mut s, 99).is_none());
    assert_eq!(kinds(&mut s, WindowKind::Plugin), 0);
}

#[gpui::test]
fn a_busy_close_is_retried_and_the_slot_reopens_only_after(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let doc = s.doc();
    show(&mut s, 10).unwrap();
    s.fake.fail_next("close_editor", busy());
    s.cx.update(|_, cx| plugin_windows::close_window(doc, Subject::Plugin(10), cx));
    s.cx.run_until_parked();
    // The window went; the close is pending, and the slot is refused.
    assert_eq!(kinds(&mut s, WindowKind::Plugin), 0);
    let pending = |s: &mut Setup| {
        s.app
            .read_with(&s.cx, |a, _| a.plugin_windows().is_close_pending(doc, 10))
    };
    assert!(pending(&mut s));
    assert!(show(&mut s, 10).is_none());
    assert!(s.fake.closed_editors().is_empty());
    // The tick retries it.
    tick(&mut s);
    assert!(!pending(&mut s));
    assert_eq!(s.fake.closed_editors(), vec![10]);
    assert!(show(&mut s, 10).is_some());
}

#[gpui::test]
fn the_close_button_closes_the_editor_then_the_window(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let h = show(&mut s, 10).unwrap();
    let mut pcx = VisualTestContext::from_window(h, &s.cx.cx);
    // Not inside the window's own update: the close runs deferred.
    assert!(!pcx.simulate_close());
    pcx.run_until_parked();
    assert_eq!(s.fake.closed_editors(), vec![10]);
    assert!(
        s.fake.windows_open().is_empty(),
        "not reopened with the project"
    );
    assert_eq!(kinds(&mut s, WindowKind::Plugin), 0);
    assert!(
        s.cx.update(|_, cx| h.update(cx, |_, _, _| ())).is_err(),
        "the GPUI window is gone"
    );
}

#[gpui::test]
fn the_window_closes_when_its_slot_goes(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let h = show(&mut s, 10).unwrap();
    s.fake.edit_mixer(|m| m.strips[0].inserts = vec![]);
    s.fake.set_idle_slots_changed();
    tick(&mut s);
    assert_eq!(kinds(&mut s, WindowKind::Plugin), 0);
    assert_eq!(s.fake.closed_editors(), vec![10]);
    assert!(s.cx.update(|_, cx| h.update(cx, |_, _, _| ())).is_err());
}

#[gpui::test]
fn closing_the_document_closes_its_editors_before_the_session_goes(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let doc = s.doc();
    let h = show(&mut s, 10).unwrap();
    s.cx.update(|_, cx| {
        AppState::global(cx).update(cx, |a, cx| {
            a.close_document(doc);
            cx.notify();
        })
    });
    s.cx.run_until_parked();
    // The editor was closed through the document's own session.
    assert_eq!(s.fake.closed_editors(), vec![10]);
    assert_eq!(s.app.read_with(&s.cx, |a, _| a.plugin_windows().len()), 0);
    assert!(s.cx.update(|_, cx| h.update(cx, |_, _, _| ())).is_err());
}

#[gpui::test]
fn quitting_closes_every_editor_first(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.fake.set_editor(11, Some(fixed(100, 100)));
    s.fake
        .edit_mixer(|m| m.strips[0].inserts.push(slot_row(11, "Delay")));
    show(&mut s, 10).unwrap();
    show(&mut s, 11).unwrap();
    s.cx.update(|_, cx| {
        AppState::global(cx).update(cx, |a, _| a.close_all());
    });
    s.cx.run_until_parked();
    let mut closed = s.fake.closed_editors();
    closed.sort();
    assert_eq!(closed, vec![10, 11]);
    assert_eq!(s.app.read_with(&s.cx, |a, _| a.windows().len()), 0);
}

#[gpui::test]
fn saved_windows_reopen_with_the_project(cx: &mut TestAppContext) {
    let f = fake();
    f.set_window_open(10, true).unwrap();
    let mut s = setup(cx, f);
    assert_eq!(kinds(&mut s, WindowKind::Plugin), 1);
    assert_eq!(opened_slots(&s.fake), vec![(10, 0)]);
}

#[gpui::test]
fn the_windows_list_names_plugin_and_builtin_synth_windows(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let doc = s.doc();
    show(&mut s, 10).unwrap();
    let synth =
        s.cx.update(|_, cx| plugin_windows::show_builtin_synth_window(doc, 7, cx));
    s.cx.run_until_parked();
    assert!(synth.is_some());
    // Not a Built-in Synth: refused.
    assert!(
        s.cx.update(|_, cx| plugin_windows::show_builtin_synth_window(doc, 99, cx))
            .is_none()
    );
    let rows = state_windows(&mut s);
    let row = |kind: &str| {
        rows.iter()
            .find(|r| r["kind"] == kind)
            .unwrap_or_else(|| panic!("no {kind} window in {rows:?}"))
            .clone()
    };
    assert_eq!(row("plugin")["title"], "Synth \u{2014} Gain");
    assert_eq!(row("other")["title"], "Built-in Synth");
    assert_eq!(row("other")["frame"]["width"], 300.0);
    assert_eq!(row("plugin")["doc"], row("document")["doc"]);
    // The channel's window calls reach it by its number.
    let number = row("plugin")["number"].clone();
    let tree = call(&mut s, "ui.tree", json!({"window": number}));
    assert_eq!(tree["window"], number);
}

#[gpui::test]
fn the_window_is_the_editor_under_the_header(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let h = show(&mut s, 10).unwrap();
    assert_eq!(content(&mut s, h), (300.0, 200.0 + HEADER_HEIGHT as f32));
    // The editor asks for a new size: the window follows.
    s.fake.set_idle_editor_resize(vec![EditorResize {
        slot: 10,
        width: 500,
        height: 400,
    }]);
    tick(&mut s);
    assert_eq!(content(&mut s, h), (500.0, 400.0 + HEADER_HEIGHT as f32));
}

#[gpui::test]
fn a_huge_editor_is_capped_to_the_screen(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.fake.set_editor(10, Some(fixed(9000, 9000)));
    let h = show(&mut s, 10).unwrap();
    let visible =
        s.cx.update(|_, cx| {
            h.update(cx, |_, w, cx| {
                w.display(cx).map(|d| {
                    let v = d.visible_bounds().size;
                    (
                        f64::from(f32::from(v.width)),
                        f64::from(f32::from(v.height)),
                    )
                })
            })
        })
        .unwrap();
    let want = capped((9000.0, 9000.0 + HEADER_HEIGHT), visible);
    assert!(want.0 < 9000.0 && want.1 < 9000.0);
    assert_eq!(content(&mut s, h), (want.0 as f32, want.1 as f32));
    // The plug-in was told nothing: its editor keeps its size.
    assert!(s.fake.resized_editors().is_empty());
}

#[gpui::test]
fn a_user_resize_asks_the_editor_and_snaps_to_its_answer(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.fake.set_editor(
        10,
        Some(EditorSize {
            width: 300,
            height: 200,
            resizable: true,
        }),
    );
    s.fake.set_resize_answer(Some((320, 240)));
    let h = show(&mut s, 10).unwrap();
    let pcx = VisualTestContext::from_window(h, &s.cx.cx);
    pcx.simulate_resize(size(gpui::px(400.), gpui::px(337.)));
    s.cx.run_until_parked();
    assert_eq!(s.fake.resized_editors(), vec![(10, 400, 300)], "asked once");
    assert_eq!(content(&mut s, h), (320.0, 240.0 + HEADER_HEIGHT as f32));
}

#[test]
fn the_cap_leaves_a_margin_and_never_goes_under_the_least_size() {
    assert_eq!(
        capped((300.0, 237.0), Some((1440.0, 900.0))),
        (300.0, 237.0)
    );
    assert_eq!(
        capped((3000.0, 2000.0), Some((1440.0, 900.0))),
        (
            1440.0 - plugin_windows::SCREEN_MARGIN,
            900.0 - plugin_windows::SCREEN_MARGIN
        )
    );
    assert_eq!(capped((300.0, 237.0), None), (300.0, 237.0));
    assert_eq!(capped((300.0, 237.0), Some((50.0, 50.0))), (120.0, 97.0));
}
