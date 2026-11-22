//! The transport bar: its controls as the Swift bar names them, the
//! shuttle buttons, Loop, the Position and Edit fields, and folding into
//! », driven through GPUI's test platform with the scripted session.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext, VisualTestContext, WindowHandle,
    px, size,
};
use serde_json::Value;
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_gpui::{AppState, DocumentWindow};

/// One bar of 4/4, in ticks.
const BAR: i64 = 61_440;

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

/// The app with one demo document window over `fake`.
fn setup(cx: &mut TestAppContext, fake: FakeSession) -> Setup {
    let fake = Arc::new(fake);
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let window = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, window)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    Setup {
        fake,
        app,
        window,
        cx: vcx,
    }
}

impl Setup {
    fn tree(&mut self) -> Vec<Value> {
        let tree = self
            .window
            .read_with(&self.cx, |v, _| v.registry().borrow().tree())
            .unwrap();
        tree["nodes"].as_array().unwrap().clone()
    }

    fn node(&mut self, role: &str, label: &str) -> Option<Value> {
        self.tree()
            .into_iter()
            .find(|n| n["role"] == role && n["label"] == label)
    }

    fn has(&mut self, role: &str, label: &str) -> bool {
        self.node(role, label).is_some()
    }

    /// A node's `value`.
    fn value(&mut self, role: &str, label: &str) -> Value {
        self.node(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?}"))["value"]
            .clone()
    }

    fn focused(&mut self, label: &str) -> bool {
        self.node("textfield", label)
            .unwrap_or_else(|| panic!("no field {label:?}"))["focused"]
            == true
    }

    fn center(&mut self, role: &str, label: &str) -> Point<Pixels> {
        let id = self
            .node(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {:?}", self.tree()))["id"]
            .as_str()
            .unwrap()
            .to_owned();
        self.window
            .read_with(&self.cx, |v, _| {
                v.registry().borrow().resolve(&id).unwrap().bounds.center()
            })
            .unwrap()
    }

    fn click_with(&mut self, role: &str, label: &str, modifiers: Modifiers) {
        let at = self.center(role, label);
        self.cx.simulate_click(at, modifiers);
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        self.click_with(role, label, Modifiers::none());
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }

    fn doc(&self) -> vizdaw_app::DocId {
        self.window.read_with(&self.cx, |v, _| v.doc()).unwrap()
    }

    fn with_doc<R>(&mut self, f: impl FnOnce(&vizdaw_app::DocumentModel) -> R) -> R {
        let doc = self.doc();
        self.app
            .read_with(&self.cx, |a, _| f(a.store.document(doc).unwrap()))
    }

    fn status(&mut self) -> Option<String> {
        self.with_doc(|d| d.status().map(String::from))
    }

    fn keys_enabled(&mut self) -> bool {
        self.with_doc(|d| d.transport_keys_enabled())
    }

    fn locates(&self) -> Vec<String> {
        self.fake
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("locate("))
            .collect()
    }

    fn resize(&mut self, width: f32) {
        self.cx.simulate_resize(size(px(width), px(640.)));
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn the_bar_names_its_controls_as_the_swift_bar(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    for label in ["Return to Start", "Rewind", "Fast Forward", "Play", "Loop"] {
        assert!(s.has("button", label), "no {label} in {:?}", s.tree());
    }
    // The bar has one Play/Stop button, as Swift's.
    assert!(!s.has("button", "Stop and Return"));
    assert!(!s.has("button", "Stop"));
    for (label, value) in [
        ("Bar Readout", "1.1.0"),
        ("Time Readout", "0:00.000"),
        ("Tempo Readout", "120.00"),
        ("Signature Readout", "4/4"),
    ] {
        assert_eq!(s.value("text", label), value, "{label}");
    }
    assert_eq!(s.value("textfield", "Position"), "1.1.0");
    assert_eq!(s.value("textfield", "Edit In"), "");
    assert_eq!(s.value("textfield", "Edit Out"), "");
    assert!(!s.focused("Position"));
    assert_eq!(s.value("button", "Loop"), "off");
    // The standard bar fits the window: nothing folds.
    assert!(!s.has("button", "More Transport Items"));
    // Playing flips Play to Stop.
    s.click("button", "Play");
    assert!(s.has("button", "Stop"));
    assert!(!s.has("button", "Play"));
}

#[gpui::test]
fn fast_forward_click_steps_a_bar(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("button", "Fast Forward");
    assert_eq!(s.fake.step_bars(), vec![true]);
    s.click("button", "Rewind");
    assert_eq!(s.fake.step_bars(), vec![true, false]);
    assert_eq!(s.fake.hold_ends(), 0);
    // A press released outside the button with no wind moves nothing.
    let at = s.center("button", "Fast Forward");
    s.cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    s.cx.simulate_mouse_up(
        gpui::point(px(900.), px(600.)),
        MouseButton::Left,
        Modifiers::none(),
    );
    s.cx.run_until_parked();
    assert_eq!(s.fake.step_bars(), vec![true, false]);
    // Return to Start goes to 0.
    s.click("button", "Fast Forward");
    s.click("button", "Return to Start");
    assert_eq!(s.locates().last().map(String::as_str), Some("locate(0)"));
}

#[gpui::test]
fn shift_click_fast_forward_without_markers_says_so(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click_with("button", "Fast Forward", Modifiers::shift());
    assert_eq!(s.status().as_deref(), Some("No marker after the position."));
    assert!(s.fake.step_bars().is_empty());
    // With a marker ahead, it goes there.
    s.fake.set_marker_ticks(vec![3 * BAR]);
    s.click_with("button", "Fast Forward", Modifiers::shift());
    assert_eq!(s.locates(), vec![format!("locate({})", 3 * BAR)]);
}

#[gpui::test]
fn a_hold_winds_and_lands_once(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let at = s.center("button", "Fast Forward");
    s.cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    s.cx.run_until_parked();
    for _ in 0..10 {
        s.cx.executor().advance_clock(Duration::from_millis(100));
        s.cx.run_until_parked();
    }
    assert!(!s.fake.winds().is_empty(), "a held button winds");
    assert!(s.fake.winds().iter().all(|q| *q > 0.0));
    assert_eq!(s.fake.hold_ends(), 0);
    s.cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
    s.cx.run_until_parked();
    assert_eq!(s.fake.hold_ends(), 1, "the release lands once");
    assert!(s.fake.step_bars().is_empty(), "a hold steps no bar");
    // The hold is over: time passing winds no more.
    let winds = s.fake.winds().len();
    s.cx.executor().advance_clock(Duration::from_millis(500));
    s.cx.run_until_parked();
    assert_eq!(s.fake.winds().len(), winds);
    assert_eq!(s.fake.hold_ends(), 1);
}

#[gpui::test]
fn a_hold_ends_when_the_window_loses_the_keyboard(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let at = s.center("button", "Rewind");
    s.fake.set_transport(|t| t.tick = 8 * BAR);
    s.cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    for _ in 0..8 {
        s.cx.executor().advance_clock(Duration::from_millis(100));
        s.cx.run_until_parked();
    }
    assert!(!s.fake.winds().is_empty());
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    assert_eq!(s.fake.hold_ends(), 1);
    let winds = s.fake.winds().len();
    s.cx.executor().advance_clock(Duration::from_millis(500));
    s.cx.run_until_parked();
    assert_eq!(s.fake.winds().len(), winds);
}

#[gpui::test]
fn a_hold_lands_when_the_window_closes(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let at = s.center("button", "Fast Forward");
    s.cx.simulate_mouse_down(at, MouseButton::Left, Modifiers::none());
    for _ in 0..8 {
        s.cx.executor().advance_clock(Duration::from_millis(100));
        s.cx.run_until_parked();
    }
    assert!(!s.fake.winds().is_empty());
    s.cx.dispatch_action(vizdaw_gpui::CloseWindow);
    s.cx.run_until_parked();
    s.cx.cx.update(|_| {});
    assert_eq!(s.fake.count("close"), 1);
    assert_eq!(s.fake.hold_ends(), 1);
    let winds = s.fake.winds().len();
    s.cx.executor().advance_clock(Duration::from_millis(500));
    s.cx.run_until_parked();
    assert_eq!(s.fake.winds().len(), winds);
}

#[gpui::test]
fn held_shift_period_steps_and_its_key_up_lands(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.keys("shift-. shift-.");
    assert_eq!(s.fake.key_steps(), vec![true, true]);
    s.cx.simulate_event(gpui::KeyUpEvent {
        keystroke: gpui::Keystroke::parse("shift-.").unwrap(),
    });
    s.cx.run_until_parked();
    assert_eq!(s.fake.hold_ends(), 1);
    s.keys("shift-,");
    assert_eq!(s.fake.key_steps(), vec![true, true, false]);
}

#[gpui::test]
fn loop_toggles_and_shows_on(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("button", "Loop");
    assert!(s.fake.looped());
    assert!(s.with_doc(|d| d.looping()));
    assert_eq!(s.value("button", "Loop"), "on");
    // L, its key, turns it off again.
    s.keys("l");
    assert!(!s.fake.looped());
    assert_eq!(s.value("button", "Loop"), "off");
}

#[gpui::test]
fn the_first_click_into_a_field_selects_all(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("textfield", "Position");
    assert!(s.focused("Position"));
    assert!(!s.keys_enabled(), "the field is a text-editing owner");
    // Typing replaces the whole text.
    s.keys("5");
    assert_eq!(s.value("textfield", "Position"), "5");
    // P focuses Position with all its text selected, from the keyboard.
    s.keys("escape");
    assert!(!s.focused("Position"));
    s.keys("p");
    assert!(s.focused("Position"));
    s.keys("7");
    assert_eq!(s.value("textfield", "Position"), "7");
    s.keys("escape e");
    assert!(s.focused("Edit In"));
}

#[gpui::test]
fn return_commits_the_typed_position(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    // Unchanged text commits nothing.
    s.click("textfield", "Position");
    s.keys("enter");
    assert!(s.locates().is_empty());
    s.click("textfield", "Position");
    s.keys("5 enter");
    assert_eq!(s.locates(), vec![format!("locate({})", 4 * BAR)]);
    assert!(!s.focused("Position"));
    assert!(s.keys_enabled());
    assert_eq!(s.value("textfield", "Position"), "5.1.0");
    // The keyboard is back with the window: Space plays.
    s.keys("space");
    assert_eq!(s.fake.count("play(3)"), 1);
    // Bad text says why and moves nothing.
    s.keys("space");
    s.click("textfield", "Position");
    s.keys("x enter");
    assert_eq!(
        s.status().as_deref(),
        Some("\u{201c}x\u{201d} is not a position (bar.beat.unit).")
    );
    assert_eq!(s.locates().len(), 1);
    // Edit Out before Edit In is refused; Tab commits Edit In and moves on.
    s.click("textfield", "Edit Out");
    s.keys("3 enter");
    assert_eq!(s.status().as_deref(), Some("Set Edit In first."));
    s.click("textfield", "Edit In");
    s.keys("2 tab");
    assert!(s.focused("Edit Out"));
    assert_eq!(s.value("textfield", "Edit In"), "2.1.0");
    s.keys("1 enter");
    assert_eq!(
        s.status().as_deref(),
        Some("Edit Out must be after Edit In.")
    );
    let range = s.with_doc(|d| d.selection().time_range.map(|r| (r.start, r.end)));
    assert_eq!(range, Some((BAR, 2 * BAR)));
}

#[gpui::test]
fn escape_and_clicking_away_cancel_the_field(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("textfield", "Position");
    s.keys("7 escape");
    assert!(s.locates().is_empty());
    assert!(!s.focused("Position"));
    assert!(s.keys_enabled());
    assert_eq!(s.value("textfield", "Position"), "1.1.0");

    s.click("textfield", "Position");
    s.keys("7");
    s.click("text", "Tempo Readout");
    assert!(s.locates().is_empty());
    assert!(!s.focused("Position"));
    assert!(s.keys_enabled());
    assert_eq!(s.value("textfield", "Position"), "1.1.0");

    // A click on a bar button ends the edit, then the button acts.
    s.click("textfield", "Position");
    s.keys("7");
    s.click("button", "Play");
    assert!(s.locates().is_empty());
    assert_eq!(s.fake.count("play(3)"), 1);
    assert!(s.keys_enabled());

    // The window losing the keyboard cancels too.
    s.click("textfield", "Edit In");
    s.keys("3");
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    assert!(s.keys_enabled());
    assert_eq!(s.value("textfield", "Edit In"), "");
    assert_eq!(s.fake.count("set_selection"), 0);
}

#[gpui::test]
fn a_field_that_folds_away_cancels_its_edit(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("textfield", "Edit In");
    s.keys("3");
    s.resize(600.);
    assert!(!s.has("textfield", "Edit In"));
    assert!(s.keys_enabled(), "a hidden field gives up the keyboard");
    s.keys("enter");
    assert_eq!(s.fake.count("set_selection"), 0);
    s.resize(1000.);
    assert_eq!(s.value("textfield", "Edit In"), "");
    assert!(!s.focused("Edit In"));
    // A folded field refuses its key.
    s.resize(600.);
    s.keys("p");
    assert_eq!(
        s.status().as_deref(),
        Some("The window shows no transport fields.")
    );
    assert!(s.keys_enabled());
}

#[gpui::test]
fn bare_keys_wait_while_a_field_has_the_keyboard(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("textfield", "Position");
    s.keys("space l shift-. p");
    assert_eq!(s.fake.count("play(3)"), 0);
    assert!(!s.fake.looped());
    assert!(s.fake.key_steps().is_empty());
    assert!(s.focused("Position"));
    let text = s.value("textfield", "Position");
    assert!(text.as_str().unwrap().starts_with(" l"), "{text}");
    // The menu's Loop waits too, and says why.
    s.cx.dispatch_action(vizdaw_gpui::RunCommand::new("transport.loop"));
    s.cx.run_until_parked();
    assert!(!s.fake.looped());
    assert_eq!(
        s.status().as_deref(),
        Some("A text field has the keyboard.")
    );
}

/// ⇧Space (Play from Selection) and ⌥Space (Play Selection) have no ⌘ or
/// ⌃: a field that has the keyboard types their space (Swift's bare-key
/// rule), in the transport fields and the track rename alike.
#[gpui::test]
fn shifted_and_option_space_type_into_a_field(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("textfield", "Position");
    s.keys("x shift-space");
    // ⌥Space types a no-break space on a Mac keyboard: the test platform
    // has no character for ⌥ keys unless told.
    let option_space = gpui::Keystroke {
        modifiers: Modifiers::alt(),
        key: "space".into(),
        key_char: Some("\u{a0}".into()),
    };
    s.cx.dispatch_keystroke(s.window.into(), option_space);
    s.cx.run_until_parked();
    s.keys("y");
    assert_eq!(s.value("textfield", "Position"), "x \u{a0}y");
    assert_eq!(s.status(), None);
    assert!(s.fake.plays_from().is_empty());
    assert_eq!(s.fake.count("play(3)"), 0);
    s.keys("escape");

    s.window
        .update(&mut s.cx, |v, window, cx| v.begin_rename(30, window, cx))
        .unwrap();
    s.cx.run_until_parked();
    s.cx.simulate_input("LEAD");
    s.keys("shift-space");
    s.cx.simulate_input("VOX");
    s.keys("enter");
    assert_eq!(s.status(), None);
    assert!(s.fake.plays_from().is_empty());
    let renamed = s.with_doc(|d| d.lanes().iter().any(|l| l.name == "LEAD VOX"));
    assert!(renamed, "the space was typed into the name");
}

#[gpui::test]
fn a_narrow_window_folds_the_last_items_into_more(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.resize(600.);
    assert!(s.has("button", "More Transport Items"));
    for label in ["Position", "Edit In", "Edit Out"] {
        assert!(!s.has("textfield", label), "{label} folds");
    }
    for label in ["Return to Start", "Rewind", "Fast Forward", "Play", "Loop"] {
        assert!(s.has("button", label), "{label} stays");
    }
    assert!(s.has("text", "Bar Readout"));
    // A click opens the » menu of the folded items.
    assert!(!s.has("menuitem", "Position Field"));
    s.click("button", "More Transport Items");
    assert!(s.has("menuitem", "Position Field"));
    assert!(s.has("menuitem", "Edit In and Out"));
    s.keys("escape");
    assert!(!s.has("menuitem", "Position Field"));
    // Wide again: nothing folds.
    s.resize(1000.);
    assert!(!s.has("button", "More Transport Items"));
    assert!(s.has("textfield", "Edit Out"));
}
