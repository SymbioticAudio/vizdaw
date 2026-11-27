//! MIDI Learn in the Mix view (Swift `LearnItem`, `LearnBanner`,
//! `KeyMonitor`; slice 9d Task 5), through GPUI's test platform with the
//! scripted session and core: the "MIDI Learn" menus of the fader, pan,
//! mute, solo and sends, the banner below the mixer's bar, its Cancel,
//! Esc, a refused learn's alert, and the banner going once the map lands.

mod mixer_support;

use std::sync::Arc;
use std::time::Duration;

use gpui::{TestAppContext, VisualTestContext, WindowHandle};
use mixer_support::Setup;
use vizdaw_app::AlertItem;
use vizdaw_app::DocumentStore;
use vizdaw_app::learn::{self, FAILED_TITLE, WAITING_TEXT};
use vizdaw_app::testing::{FakeCore, FakeSession, lane_row, mixer_fake, strip_row};
use vizdaw_ffi::VizError;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::{SendRow, StripKindRow};
use vizdaw_ffi::surfaces::SurfaceStateRow;
use vizdaw_gpui::{DocumentWindow, RunCommand};

/// Synth (1) sends to Bus 1 (2); Master (0).
fn fake() -> FakeSession {
    let f = mixer_fake();
    f.edit_mixer(|m| {
        m.strips[0].sends = vec![SendRow {
            target: 2,
            level_db: -6.0,
            pre_fader: false,
            enabled: true,
        }];
    });
    f
}

/// The core says a learn waits for `what` (none: nothing waits).
fn waiting(f: &FakeSession, what: Option<&str>) {
    f.set_surface_state(SurfaceStateRow {
        learning: what.map(str::to_owned),
        ..SurfaceStateRow::default()
    });
}

/// The demo on `f` over a scripted core, in the Mix view.
fn setup(cx: &mut TestAppContext, f: FakeSession) -> (Setup, Arc<FakeCore>) {
    let core = Arc::new(FakeCore::new());
    let fake = Arc::new(f);
    core.hand_out("new_demo", fake.clone());
    let store = DocumentStore::with_core(core.clone());
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let window = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, window)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    let mut s = Setup {
        fake,
        app,
        window,
        cx: vcx,
    };
    s.cx.dispatch_action(RunCommand::new("ui.view_mix"));
    s.cx.run_until_parked();
    (s, core)
}

/// The window's idle poll runs (its 50 ms throttle is the real clock).
fn poll(s: &mut Setup) {
    for _ in 0..2 {
        std::thread::sleep(Duration::from_millis(60));
        s.cx.executor().advance_clock(Duration::from_millis(300));
        s.cx.run_until_parked();
    }
}

fn learning(s: &mut Setup) -> bool {
    s.model(|d| d.learn.is_learning())
}

/// Right-clicks `role` `label` and chooses MIDI Learn.
fn learn(s: &mut Setup, role: &str, label: &str) {
    s.right_click(role, label);
    s.click("menuitem", "MIDI Learn");
}

/// The banner's text, if one is drawn.
fn banner(s: &mut Setup) -> Option<String> {
    s.elements()
        .into_iter()
        .find(|e| e.role == "text" && e.label.starts_with("Waiting for MIDI"))
        .map(|e| e.label)
}

#[gpui::test]
fn right_click_on_a_fader_offers_midi_learn(cx: &mut TestAppContext) {
    let f = fake();
    // An unreadable surface state: the banner says only what Swift's
    // plain one says.
    f.set_call_error(
        "surface_state",
        Some(VizError::Model {
            message: "no state".into(),
        }),
    );
    let (mut s, _) = setup(cx, f);
    s.right_click("slider", "Fader of Synth");
    assert_eq!(s.titles(), vec!["MIDI Learn".to_owned()]);
    s.click("menuitem", "MIDI Learn");
    assert_eq!(s.fake.learn_targets(), vec![T::StripFader { strip: 1 }]);
    assert!(s.titles().is_empty(), "the menu closed");
    let text = s.element("text", WAITING_TEXT);
    // At the top of the mixer, below its bar.
    let add_bus = s.element("button", "Add Bus").bounds;
    let fader = s.element("slider", "Fader of Synth").bounds;
    assert!(text.bounds.origin.y >= add_bus.origin.y + add_bus.size.height);
    assert!(text.bounds.origin.y < fader.origin.y);
    assert!(s.find("button", "Cancel").is_some());
    // The level box offers it too.
    s.right_click("textfield", "Level of Synth");
    assert_eq!(s.titles(), vec!["MIDI Learn".to_owned()]);
    s.click("menuitem", "MIDI Learn");
    assert_eq!(
        s.fake.learn_targets(),
        vec![T::StripFader { strip: 1 }, T::StripFader { strip: 1 }]
    );
}

#[gpui::test]
fn pan_mute_solo_and_a_send_learn_their_targets(cx: &mut TestAppContext) {
    let f = fake();
    waiting(&f, Some("Pan · Synth"));
    let (mut s, _) = setup(cx, f);
    learn(&mut s, "slider", "Pan knob of Synth");
    learn(&mut s, "textfield", "Pan of Synth");
    learn(&mut s, "button", "Mute Synth");
    learn(&mut s, "button", "Solo Synth");
    // The send's menu has it after On.
    s.right_click("slider", "Send knob 1 of Synth");
    assert_eq!(
        s.titles(),
        vec!["Pre-Fader", "On", "MIDI Learn", "Remove Send"]
    );
    s.click("menuitem", "MIDI Learn");
    // Master has a mute to learn, and no solo.
    s.right_click("button", "Mute Master");
    assert_eq!(s.titles(), vec!["MIDI Learn".to_owned()]);
    s.click("menuitem", "MIDI Learn");
    assert!(s.find("button", "Solo Master").is_none());
    assert_eq!(
        s.fake.learn_targets(),
        vec![
            T::StripPan { strip: 1 },
            T::StripPan { strip: 1 },
            T::StripMute { strip: 1 },
            T::StripSolo { strip: 1 },
            T::SendLevel { strip: 1, send: 0 },
            T::StripMute { strip: 0 },
        ]
    );
}

#[gpui::test]
fn the_banner_names_the_project_with_two_open(cx: &mut TestAppContext) {
    let f = fake();
    waiting(&f, Some("Fader · Synth"));
    let (mut s, _) = setup(cx, f);
    learn(&mut s, "slider", "Fader of Synth");
    assert_eq!(
        banner(&mut s),
        Some(learn::waiting(None, Some("Fader · Synth")))
    );
    let _other: WindowHandle<DocumentWindow> =
        s.cx.update(|_, cx| vizdaw_gpui::new_document(cx))
            .expect("a second window");
    s.cx.run_until_parked();
    let title = s.model(|d| d.title());
    s.window.update(&mut s.cx, |_, _, cx| cx.notify()).unwrap();
    s.cx.run_until_parked();
    assert_eq!(
        banner(&mut s),
        Some(learn::waiting(Some(&title), Some("Fader · Synth")))
    );
}

#[gpui::test]
fn cancel_and_escape_end_the_learn(cx: &mut TestAppContext) {
    let f = fake();
    waiting(&f, Some("Fader · Synth"));
    let (mut s, core) = setup(cx, f);
    learn(&mut s, "slider", "Fader of Synth");
    s.click("button", "Cancel");
    assert_eq!(core.learn_cancels(), 1);
    assert!(!learning(&mut s));
    assert_eq!(banner(&mut s), None);
    // Esc.
    learn(&mut s, "slider", "Fader of Synth");
    s.cx.simulate_keystrokes("escape");
    assert_eq!(core.learn_cancels(), 2);
    assert!(!learning(&mut s));
    assert_eq!(banner(&mut s), None);
    // Esc with a text field typed in is the field's: nothing is cancelled.
    learn(&mut s, "slider", "Fader of Synth");
    s.click("textfield", "Level of Synth");
    assert!(s.element("textfield", "Level of Synth").focused);
    s.cx.simulate_keystrokes("escape");
    assert_eq!(core.learn_cancels(), 2);
    assert!(learning(&mut s));
    // A modified Esc goes on as today.
    s.cx.simulate_keystrokes("shift-escape");
    assert_eq!(core.learn_cancels(), 2);
    assert!(learning(&mut s));
}

/// Carry-over from Task 4: Esc in another document's window ends the
/// learn of the one that waits, whose banner goes.
#[gpui::test]
fn escape_in_another_window_cancels_the_waiting_learn(cx: &mut TestAppContext) {
    let f = fake();
    waiting(&f, Some("Fader · Synth"));
    let (mut s, core) = setup(cx, f);
    learn(&mut s, "slider", "Fader of Synth");
    let other: WindowHandle<DocumentWindow> =
        s.cx.update(|_, cx| vizdaw_gpui::new_document(cx))
            .expect("a second window");
    s.cx.run_until_parked();
    let mut ocx = VisualTestContext::from_window(other.into(), &s.cx.cx);
    ocx.simulate_keystrokes("escape");
    ocx.run_until_parked();
    assert_eq!(core.learn_cancels(), 1);
    assert!(!learning(&mut s));
    let learning_doc = s.app.read_with(&s.cx, |a, _| a.store.learning_document());
    assert_eq!(learning_doc, None);
    s.cx.run_until_parked();
    assert_eq!(banner(&mut s), None);
}

#[gpui::test]
fn a_refused_learn_shows_midi_learn_failed(cx: &mut TestAppContext) {
    let (mut s, _) = setup(cx, fake());
    s.fake.fail_next(
        "learn_target",
        VizError::Model {
            message: "That control cannot be learned.".into(),
        },
    );
    learn(&mut s, "slider", "Fader of Synth");
    assert_eq!(
        s.cx.pending_prompt(),
        Some((
            FAILED_TITLE.to_owned(),
            "That control cannot be learned.".to_owned()
        ))
    );
    assert_eq!(banner(&mut s), None);
}

#[gpui::test]
fn the_banner_goes_when_the_map_lands(cx: &mut TestAppContext) {
    let f = fake();
    waiting(&f, Some("Fader · Synth"));
    let (mut s, _) = setup(cx, f);
    learn(&mut s, "slider", "Fader of Synth");
    assert!(banner(&mut s).is_some());
    waiting(&s.fake, None);
    poll(&mut s);
    assert!(!learning(&mut s));
    assert_eq!(banner(&mut s), None);
}

/// Review M1: Esc while an alert is up in the window is the alert's; the
/// learn still waits.
#[gpui::test]
fn escape_with_an_alert_up_cancels_nothing(cx: &mut TestAppContext) {
    let f = fake();
    waiting(&f, Some("Fader · Synth"));
    let (mut s, core) = setup(cx, f);
    learn(&mut s, "slider", "Fader of Synth");
    s.app.update(&mut s.cx, |a, cx| {
        a.store
            .post_alert(AlertItem::new("Note", "Something happened."));
        cx.notify();
    });
    s.cx.run_until_parked();
    assert!(s.cx.has_pending_prompt());
    s.cx.simulate_keystrokes("escape");
    assert_eq!(core.learn_cancels(), 0);
    assert!(learning(&mut s));
    s.cx.simulate_prompt_answer("OK");
    s.cx.run_until_parked();
    s.cx.simulate_keystrokes("escape");
    assert_eq!(core.learn_cancels(), 1);
}

/// Review M2: an open menu takes Esc first (it closes); the next Esc
/// cancels the learn.
#[gpui::test]
fn an_open_menu_takes_escape_first(cx: &mut TestAppContext) {
    let f = fake();
    let mut groove = lane_row(30, "Groove", false);
    groove.audio_out = Some(1);
    f.set_lanes(3, vec![groove]);
    f.edit_mixer(|m| {
        m.strips.insert(
            0,
            strip_row(5, "Audio 1", StripKindRow::AudioPath { path: 1 }),
        )
    });
    waiting(&f, Some("Fader · Synth"));
    let (mut s, core) = setup(cx, f);
    learn(&mut s, "slider", "Fader of Synth");
    s.click("menu", "Input of Groove");
    assert!(s.find("menuitem", "No Input").is_some());
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert!(
        s.find("menuitem", "No Input").is_none(),
        "the picker closed"
    );
    assert_eq!(core.learn_cancels(), 0);
    assert!(learning(&mut s));
    s.cx.simulate_keystrokes("escape");
    assert_eq!(core.learn_cancels(), 1);
    assert!(!learning(&mut s));
}
