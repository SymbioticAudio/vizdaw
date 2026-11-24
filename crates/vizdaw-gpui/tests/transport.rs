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

/// The default window's width (Swift's 1440): the whole standard bar
/// fits.
const WIDE: f32 = 1440.;

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
    s.resize(WIDE);
    assert_eq!(s.value("textfield", "Edit In"), "");
    assert!(!s.focused("Edit In"));
    // A folded field's key opens » at it (slice 8b controller ruling).
    s.resize(600.);
    s.keys("p");
    assert_eq!(s.status(), None);
    assert!(s.has("group", "Position Field"), "{:?}", s.tree());
    assert!(s.focused("Position"));
    s.keys("escape");
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
    s.resize(WIDE);
    assert!(!s.has("button", "More Transport Items"));
    assert!(s.has("textfield", "Edit Out"));
}

/// A file panel the test keeps up until it answers.
struct HeldPanel {
    done: std::cell::RefCell<
        Vec<futures::channel::oneshot::Sender<vizdaw_gpui::file_panels::Chosen>>,
    >,
}

impl vizdaw_gpui::file_panels::PanelBackend for HeldPanel {
    fn show(
        &self,
        _spec: &vizdaw_app::panels::PanelSpec,
        _place: &vizdaw_gpui::file_panels::PanelPlace,
        _window: Option<gpui::AnyWindowHandle>,
        done: futures::channel::oneshot::Sender<vizdaw_gpui::file_panels::Chosen>,
        _cx: &mut gpui::App,
    ) {
        self.done.borrow_mut().push(done);
    }
}

#[gpui::test]
fn held_keys_do_nothing_while_a_file_panel_is_up(cx: &mut TestAppContext) {
    use vizdaw_gpui::file_panels::{PanelBackend, PanelPlace, choose, set_backend};
    let mut s = setup(cx, demo_fake());
    let held = std::rc::Rc::new(HeldPanel {
        done: Default::default(),
    });
    let backend: std::rc::Rc<dyn PanelBackend> = held.clone();
    s.cx.update(|_, cx| {
        set_backend(backend, cx);
        choose(
            &vizdaw_app::panels::IMPORT_MIDI,
            PanelPlace::default(),
            None,
            cx,
        )
        .detach();
    });
    s.cx.run_until_parked();
    // The window's own key-down handler (held Fast Forward) is off too.
    s.keys("shift-.");
    assert!(s.fake.key_steps().is_empty());
    held.done
        .borrow_mut()
        .pop()
        .unwrap()
        .send(Ok(None))
        .unwrap();
    s.cx.run_until_parked();
    s.keys("shift-.");
    assert_eq!(s.fake.key_steps(), vec![true]);
}

impl Setup {
    /// Lets the poll and the idle tick run (it waits at least 50 ms of
    /// real time) until `done`.
    fn poll_until(&mut self, what: &str, done: impl Fn(&mut Setup) -> bool) {
        for _ in 0..40 {
            if done(self) {
                return;
            }
            std::thread::sleep(Duration::from_millis(60));
            self.cx.executor().advance_clock(Duration::from_millis(250));
            self.cx.run_until_parked();
        }
        panic!("no {what} in {:?}", self.tree());
    }

    fn faults_open(&mut self) -> bool {
        self.with_doc(vizdaw_app::DocumentModel::shows_faults)
    }
}

const PROBLEM: &str = "Media/a.wav: the file could not be read";
const FAULT: &str = "stream-underrun";
const REPORT: &str = "Song: a note past the end was dropped";

/// Slice 8b Task 1 (Swift `FaultsItem`): Faults shows only with faults or
/// a report, in the warning tint for a report and the error tint with
/// faults; a click opens the popover of every line, and Esc, a click
/// elsewhere or the button again close it.
#[gpui::test]
fn faults_shows_its_lines_in_a_popover(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    assert!(!s.has("button", "Faults"), "no lines, no Faults");
    s.fake.set_idle_problems(vec![PROBLEM.into()]);
    s.poll_until("Faults", |s| s.has("button", "Faults"));
    assert_eq!(s.value("button", "Faults"), "warning");
    // Faults and a compile report come with playing (the poll reads the
    // transport then).
    s.fake.set_transport(|t| {
        t.faults = vec![FAULT.into()];
        t.report = vec![REPORT.into()];
    });
    s.click("button", "Play");
    s.poll_until("the error tint", |s| s.value("button", "Faults") == "error");
    s.click("button", "Stop");
    assert!(!s.has("text", PROBLEM), "closed until clicked");
    s.click("button", "Faults");
    assert!(s.faults_open());
    for line in [FAULT, REPORT, PROBLEM] {
        assert!(s.has("text", line), "no {line:?} in {:?}", s.tree());
    }
    // In Swift's order: faults, report, problems.
    let lines: Vec<String> = s
        .tree()
        .into_iter()
        .filter(|n| n["role"] == "text")
        .filter_map(|n| n["label"].as_str().map(String::from))
        .filter(|l| [FAULT, REPORT, PROBLEM].contains(&l.as_str()))
        .collect();
    assert_eq!(lines, [FAULT, REPORT, PROBLEM]);
    s.keys("escape");
    assert!(!s.has("text", PROBLEM) && !s.faults_open());
    // The button again closes it.
    s.click("button", "Faults");
    assert!(s.has("text", PROBLEM));
    s.click("button", "Faults");
    assert!(!s.has("text", PROBLEM));
    // A click elsewhere closes it.
    s.click("button", "Faults");
    s.cx.simulate_click(gpui::point(px(700.), px(500.)), Modifiers::none());
    s.cx.run_until_parked();
    assert!(!s.has("text", PROBLEM) && !s.faults_open());
}

/// Folded, Faults is a » action that opens the same popover.
#[gpui::test]
fn folded_faults_is_an_action_in_more(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.fake.set_idle_problems(vec![PROBLEM.into()]);
    s.poll_until("Faults", |s| s.has("button", "Faults"));
    s.resize(600.);
    assert!(!s.has("button", "Faults"), "folds");
    s.click("button", "More Transport Items");
    assert_eq!(
        s.node("menuitem", "Faults").unwrap()["enabled"],
        true,
        "enabled with a report"
    );
    s.click("menuitem", "Faults");
    assert!(!s.has("menuitem", "Faults"), "the menu closed");
    assert!(s.has("text", PROBLEM), "the popover opened");
    s.keys("escape");
    assert!(!s.has("text", PROBLEM));
}

/// Faults is the document window's only (Swift `documentOnly`).
#[gpui::test]
fn the_mixer_window_has_no_faults(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.fake.set_idle_problems(vec![PROBLEM.into()]);
    s.poll_until("Faults", |s| s.has("button", "Faults"));
    let doc = s.doc();
    let app = s.app.clone();
    let mixer =
        s.cx.update(|_, cx| DocumentWindow::open_mixer(&app, doc, cx).unwrap());
    s.cx.run_until_parked();
    let mcx = VisualTestContext::from_window(mixer.into(), &s.cx);
    mcx.run_until_parked();
    let tree = mixer
        .read_with(&mcx, |v, _| v.registry().borrow().tree())
        .unwrap();
    let nodes = tree["nodes"].as_array().unwrap();
    assert!(
        nodes
            .iter()
            .any(|n| n["role"] == "button" && n["label"] == "Loop"),
        "the Mixer window has its bar: {tree}"
    );
    assert!(
        !nodes.iter().any(|n| n["label"] == "Faults"),
        "no Faults: {tree}"
    );
}

/// Faults going (its report cleared) takes its open popover with it: the
/// next lines do not open it by themselves.
#[gpui::test]
fn faults_going_closes_its_popover(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.fake.set_transport(|t| t.report = vec![REPORT.into()]);
    s.click("button", "Play");
    s.poll_until("Faults", |s| s.has("button", "Faults"));
    s.click("button", "Faults");
    assert!(s.has("text", REPORT));
    s.fake.set_transport(|t| t.report = vec![]);
    s.poll_until("no Faults", |s| !s.has("button", "Faults"));
    assert!(!s.faults_open());
}

// Slice 8b Task 2: the Record part of the cluster, Replace/Overdub,
// Count-in and the Step Entry summary.

/// The demo with Song (3)'s two tracks armed: Groove a stereo audio
/// input, Chords a MIDI one.
fn armed_fake() -> FakeSession {
    use vizdaw_app::testing::lane_row;
    use vizdaw_ffi::edit::LaneInputRow;
    let f = demo_fake();
    let mut groove = lane_row(30, "Groove", false);
    groove.arm = true;
    groove.input = Some(LaneInputRow::Audio {
        first: 0,
        stereo: true,
        return_of: None,
    });
    let mut chords = lane_row(31, "Chords", false);
    chords.arm = true;
    chords.input = Some(LaneInputRow::Midi {
        port: None,
        channel: None,
    });
    f.set_lanes(3, vec![groove, chords]);
    f
}

/// Bytes a second the armed stereo track records (48 kHz, 24-bit).
const STEREO_RATE: u64 = 288_000;

impl Setup {
    fn update_model(&mut self, f: impl FnOnce(&mut vizdaw_app::DocumentModel)) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| f(d));
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    /// Every window's bar is `items` (`ui.set_transport_bar`'s text).
    fn set_bar(&mut self, items: &str) {
        let bar = vizdaw_app::transport_bar::TransportLayout::parse(items).unwrap();
        self.app.update(&mut self.cx, |a, cx| {
            a.prefs.set_transport_bar(&bar).unwrap();
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    fn enabled(&mut self, role: &str, label: &str) -> bool {
        self.node(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {:?}", self.tree()))["enabled"]
            == true
    }

    fn recording(&mut self) -> bool {
        self.with_doc(|d| d.record().is_recording())
    }

    /// A text node whose label starts with `prefix`.
    fn text_starting(&mut self, prefix: &str) -> Option<String> {
        self.tree()
            .into_iter()
            .filter(|n| n["role"] == "text")
            .filter_map(|n| n["label"].as_str().map(String::from))
            .find(|l| l.starts_with(prefix))
    }
}

const RECORD_BAR: &str = "cluster,loop,replace_overdub,count_in,separator,summary,space,faults";

/// The cluster's Record and the record items as the Swift bar names them
/// (`V/RecordControls.swift:12-64`, `V/TransportItemViews.swift:182-205`).
#[gpui::test]
fn the_record_items_name_their_controls_as_the_swift_bar(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    // The standard bar: Record after Play, and Replace/Overdub, which a
    // new document reads as Overdub (on).
    assert!(s.enabled("button", "Record"), "{:?}", s.tree());
    assert!(!s.has("button", "Stop Recording"));
    assert_eq!(s.value("button", "Record mode: Overdub"), "on");
    let labels: Vec<String> = s
        .tree()
        .into_iter()
        .filter(|n| n["role"] == "button")
        .filter_map(|n| n["label"].as_str().map(String::from))
        .collect();
    let at = |l: &str| labels.iter().position(|x| x == l).unwrap();
    assert_eq!(
        at("Record"),
        at("Play") + 1,
        "Record follows Play: {labels:?}"
    );
    assert!(
        !s.has("button", "More Transport Items"),
        "the bar still fits"
    );
    s.set_bar(RECORD_BAR);
    assert_eq!(s.value("button", "Count-in"), "off");
    assert!(!s.has("text", "Count-in"), "no count-in readout while idle");
    assert_eq!(s.text_starting("Step Entry"), None, "no summary while off");
    assert_eq!(s.text_starting("Pass "), None);
    // The texts the Swift views show and help.
    use vizdaw_gpui::transport_bar as bar;
    assert_eq!(bar::RECORD_HELP, "Record the armed tracks (R)");
    assert_eq!(bar::STOP_RECORDING_HELP, "Stop recording (R)");
    assert_eq!(
        bar::REMAINING_HELP,
        "Recording time left on the project's disk for the armed audio tracks"
    );
    assert_eq!(
        bar::COUNT_IN_HELP,
        "Count in before recording (the bars are in Record Options)"
    );
    assert_eq!(
        vizdaw_app::record::MODE_HELP,
        "Replace removes the notes in the recorded range; Overdub keeps them (transport.overdub)"
    );
}

/// Separators are drawn between items (and never at an edge or next to
/// »), and an absent summary takes no room.
#[gpui::test]
fn separators_are_drawn(_cx: &mut TestAppContext) {
    use vizdaw_app::transport_bar::TransportLayout;
    use vizdaw_gpui::transport_bar::{Piece, Plan, Shows};
    let layout = TransportLayout::parse(RECORD_BAR).unwrap();
    let plan = Plan::new(&layout, WIDE, Shows::default());
    assert!(
        plan.shown().contains(&Piece::Separator),
        "{:?}",
        plan.shown()
    );
    assert!(!plan.shown().contains(&Piece::Summary));
    assert!(!plan.folded().contains(&Piece::Summary));
    let shows = Shows {
        summary: true,
        ..Shows::default()
    };
    let plan = Plan::new(&layout, WIDE, shows);
    assert!(plan.shown().contains(&Piece::Summary));
    let lone = TransportLayout::parse("cluster,separator").unwrap();
    assert_eq!(
        Plan::new(&lone, WIDE, Shows::default()).shown(),
        vec![Piece::Cluster],
        "not at an edge"
    );
}

/// Record stays enabled while a field types (a click is no keystroke),
/// refuses into the status line, and with a punch draft open the field
/// keeps the keyboard.
#[gpui::test]
fn record_is_enabled_while_a_field_types_and_refuses_into_the_status_line(cx: &mut TestAppContext) {
    use vizdaw_app::transport_fields::TransportField;
    let mut s = setup(cx, armed_fake());
    s.click("textfield", "Position");
    assert!(s.focused("Position") && !s.keys_enabled());
    assert!(s.enabled("button", "Record"));
    // A typed punch time (the bar's Punch In, slice 8b Task 4) refuses
    // Record, and the press leaves the edit and its keyboard alone.
    s.update_model(|d| d.bar_punch_typed(TransportField::PunchIn, "5"));
    s.click("button", "Record");
    assert_eq!(
        s.status().as_deref(),
        Some("Press Return to set the punch, or Esc to discard it.")
    );
    assert!(s.focused("Position"), "the field keeps the keyboard");
    assert!(!s.keys_enabled());
    assert_eq!(s.fake.count("record(3)"), 0);
    s.update_model(|d| d.bar_punch_ended());
    // Step entry on refuses Record, in Swift's words.
    s.keys("escape");
    s.update_model(|d| d.start_step_entry().unwrap());
    s.click("button", "Record");
    assert_eq!(
        s.status().as_deref(),
        Some("Step entry is on: turn it off to record.")
    );
    assert_eq!(s.fake.count("record(3)"), 0);
    s.update_model(|d| d.stop_step_entry());
    // With no draft, a press ends the edit, then records.
    s.click("textfield", "Position");
    s.keys("7");
    s.click("button", "Record");
    assert!(!s.focused("Position") && s.keys_enabled());
    assert!(s.locates().is_empty(), "the typed position was cancelled");
    assert_eq!(s.fake.count("record(3)"), 1);
    assert!(s.recording());
}

/// Stop Recording shows while a pass records and stops it; the pass text
/// and the time left come from the record state; Count-in shows while
/// counting in (the transport reads `playing: false`).
#[gpui::test]
fn stop_recording_shows_the_pass_and_stops_it(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.fake
        .set_free_bytes(Some(STEREO_RATE * 600 + vizdaw_app::record::STOP_BYTES));
    s.update_model(|d| {
        d.refresh_project_info();
        d.refresh_record(true);
    });
    assert_eq!(s.text_starting("10 min"), Some("10 min left".into()));
    s.click("button", "Loop");
    s.click("button", "Record");
    assert!(s.recording());
    assert!(s.has("button", "Stop Recording"), "{:?}", s.tree());
    assert!(!s.has("button", "Record"));
    assert_eq!(s.text_starting("Pass "), Some("Pass 1".into()));
    s.fake.next_pass();
    s.update_model(|d| d.press_pass(true).unwrap());
    s.fake.next_pass();
    s.update_model(|d| d.press_pass(false).unwrap());
    assert_eq!(
        s.text_starting("Pass "),
        Some("Pass 3 \u{b7} 1 kept \u{b7} 1 discarded".into())
    );
    s.click("button", "Stop Recording");
    assert!(!s.recording());
    assert_eq!(s.fake.count("stop"), 1, "{:?}", s.fake.calls());
    assert!(s.has("button", "Record"));
    assert_eq!(s.text_starting("Pass "), None);
    // Counting in: Count-in in the record colour, Stop Recording.
    s.click("button", "Loop");
    s.update_model(|d| d.record_mut().set_count_in(1));
    s.click("button", "Record");
    assert!(s.with_doc(|d| d.counting_in() && !d.is_playing()));
    assert!(s.has("text", "Count-in"), "{:?}", s.tree());
    assert!(s.has("button", "Stop Recording"));
    s.fake.count_in_done();
    s.poll_until("no count-in", |s| !s.has("text", "Count-in"));
    assert!(s.has("button", "Stop Recording"));
}

/// Replace/Overdub runs `transport.overdub` and is disabled while
/// recording; Count-in goes off and back to its last bars.
#[gpui::test]
fn replace_overdub_and_count_in_switch_the_record_modes(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.set_bar(RECORD_BAR);
    s.click("button", "Record mode: Overdub");
    assert!(!s.with_doc(|d| d.layout().transport.overdub));
    assert_eq!(s.value("button", "Record mode: Replace"), "off");
    s.click("button", "Record mode: Replace");
    assert!(s.with_doc(|d| d.layout().transport.overdub));
    // Count-in: off, then its last bars.
    s.update_model(|d| {
        d.record_mut().set_count_in(3);
        d.record_mut().set_count_in(0);
    });
    assert_eq!(s.value("button", "Count-in"), "off");
    s.click("button", "Count-in");
    assert_eq!(s.with_doc(|d| d.record().count_in_bars()), 3);
    assert_eq!(s.value("button", "Count-in"), "on");
    s.click("button", "Count-in");
    assert_eq!(s.with_doc(|d| d.record().count_in_bars()), 0);
    // Recording: both are disabled, and a press does nothing.
    s.click("button", "Record");
    assert!(s.recording());
    assert!(!s.enabled("button", "Record mode: Overdub"));
    assert!(!s.enabled("button", "Count-in"));
    s.click("button", "Record mode: Overdub");
    s.click("button", "Count-in");
    assert!(s.with_doc(|d| d.layout().transport.overdub));
    assert_eq!(s.with_doc(|d| d.record().count_in_bars()), 0);
}

/// The Step Entry summary follows step entry's value, dot and triplet.
#[gpui::test]
fn the_step_summary_follows_the_value(cx: &mut TestAppContext) {
    use vizdaw_ffi::step::StepInputRow;
    let mut s = setup(cx, demo_fake());
    s.set_bar(RECORD_BAR);
    s.update_model(|d| d.start_step_entry().unwrap());
    let summary = s.text_starting("Step Entry").expect("a summary");
    assert!(
        summary.starts_with("Step Entry \u{b7} Groove \u{b7} "),
        "{summary}"
    );
    let value = summary.rsplit(" \u{b7} ").next().unwrap().to_owned();
    s.update_model(|d| d.step_input(StepInputRow::Dot).unwrap());
    assert_eq!(
        s.text_starting("Step Entry"),
        Some(format!("Step Entry \u{b7} Groove \u{b7} {value} dotted"))
    );
    s.update_model(|d| d.step_input(StepInputRow::Triplet).unwrap());
    assert_eq!(
        s.text_starting("Step Entry"),
        Some(format!(
            "Step Entry \u{b7} Groove \u{b7} {value} dotted triplet"
        ))
    );
    s.update_model(|d| d.stop_step_entry());
    assert_eq!(s.text_starting("Step Entry"), None);
}

/// Folded, the items are Swift's » entries
/// (`M/TransportItemEntries.swift:111-123`): Replace/Overdub and Count-in
/// are check actions, disabled while recording; the summary is a text.
#[gpui::test]
fn folded_record_items_are_swifts_entries_in_more(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.set_bar("cluster,loop,bar,time,tempo,signature,replace_overdub,count_in,summary");
    s.update_model(|d| d.start_step_entry().unwrap());
    let summary = s.text_starting("Step Entry").expect("a summary");
    s.resize(600.);
    assert!(!s.has("button", "Record mode: Overdub"), "folds");
    assert!(s.has("button", "Record"), "the cluster never folds");
    s.click("button", "More Transport Items");
    let item = s.node("menuitem", "Replace/Overdub").expect("an entry");
    assert_eq!(
        (item["enabled"].clone(), item["selected"].clone()),
        (true.into(), true.into())
    );
    let item = s.node("menuitem", "Count-in").expect("an entry");
    assert_eq!(item["enabled"], true);
    assert_eq!(item.get("selected"), None, "off");
    let item = s.node("menuitem", "Step Entry").expect("an entry");
    assert_eq!(item["enabled"], false, "a text");
    assert_eq!(item["value"], format!("Step Entry: {summary}"));
    // Choosing an entry acts as the button and closes the menu.
    s.click("menuitem", "Replace/Overdub");
    assert!(!s.has("menuitem", "Count-in"));
    assert!(!s.with_doc(|d| d.layout().transport.overdub));
    s.click("button", "More Transport Items");
    s.click("menuitem", "Count-in");
    assert!(s.with_doc(|d| d.record().count_in_bars() > 0));
    // Recording: both are disabled.
    s.update_model(|d| d.stop_step_entry());
    s.click("button", "Record");
    assert!(s.recording());
    s.click("button", "More Transport Items");
    assert_eq!(
        s.node("menuitem", "Replace/Overdub").unwrap()["enabled"],
        false
    );
    assert_eq!(s.node("menuitem", "Count-in").unwrap()["enabled"], false);
}

// Slice 8b Task 4: Punch In and Out on the bar, the field keys and » at
// the field.

const PUNCH_BAR: &str = "cluster,bar,position_field,edit_fields,punch_fields";
const DRAFT: &str = "Press Return to set the punch, or Esc to discard it.";

impl Setup {
    fn editing_field(&mut self) -> Option<vizdaw_app::transport_fields::TransportField> {
        self.window
            .read_with(&self.cx, |v, _| v.editing_field())
            .unwrap()
    }

    fn draft(&mut self) -> bool {
        self.with_doc(|d| d.has_open_punch_draft())
    }

    fn punch_in(&mut self) -> i64 {
        self.with_doc(|d| d.layout().transport.punch_in)
    }

    /// Record, then Stop Recording: Record is not refused.
    fn records_and_stops(&mut self, how: &str) {
        let before = self.fake.count("record(3)");
        self.update_model(|d| d.set_status(None));
        self.click("button", "Record");
        assert_eq!(self.status(), None, "{how}: Record is not refused");
        assert_eq!(self.fake.count("record(3)"), before + 1, "{how}");
        self.click("button", "Stop Recording");
        assert!(!self.recording(), "{how}");
    }
}

/// Tab goes Position → Edit In → Edit Out → Punch In → Punch Out and
/// round (Swift `tabMovesBetweenTheFields`, `punchFieldsKeepTheirOrder`).
#[gpui::test]
fn tab_goes_through_the_punch_fields(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.set_bar(PUNCH_BAR);
    s.click("textfield", "Position");
    for label in ["Edit In", "Edit Out", "Punch In", "Punch Out", "Position"] {
        s.keys("tab");
        assert!(s.focused(label), "Tab reaches {label}");
    }
    s.keys("shift-tab");
    assert!(s.focused("Punch Out"));
    s.keys("shift-tab");
    assert!(s.focused("Punch In"));
    s.keys("escape");
    assert!(s.keys_enabled());
}

/// A typed bar Punch In refuses Record with the draft message; the field
/// keeps the keyboard and the draft, and Return then sets the punch.
#[gpui::test]
fn a_typed_bar_punch_in_refuses_record_until_return(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.set_bar(PUNCH_BAR);
    s.click("textfield", "Punch In");
    s.keys("3");
    assert!(s.draft());
    s.click("button", "Record");
    assert_eq!(s.status().as_deref(), Some(DRAFT));
    assert_eq!(s.fake.count("record(3)"), 0);
    assert!(s.focused("Punch In"), "the field keeps the keyboard");
    assert_eq!(s.value("textfield", "Punch In"), "3");
    assert_eq!(s.punch_in(), 0, "not set yet");
    s.keys("enter");
    assert_eq!(s.punch_in(), 2 * BAR);
    assert!(!s.draft() && s.keys_enabled());
    s.records_and_stops("after Return");
}

/// A way a field's edit ends.
type End = fn(&mut Setup);

/// Every end of a bar punch edit clears its draft: Return, Esc, Tab,
/// ⇧Tab, a click elsewhere, the field folding away and the window losing
/// the keyboard. Record is then not refused.
#[gpui::test]
fn every_end_of_a_bar_punch_edit_clears_its_draft(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.set_bar(PUNCH_BAR);
    let ends: [(&str, End); 7] = [
        ("Return", |s| s.keys("enter")),
        ("Esc", |s| s.keys("escape")),
        ("Tab", |s| s.keys("tab escape")),
        ("Shift-Tab", |s| s.keys("shift-tab escape")),
        ("a click elsewhere", |s| s.click("text", "Bar Readout")),
        ("folding away", |s| {
            s.resize(600.);
            assert!(!s.has("textfield", "Punch In"), "folded");
            s.resize(WIDE);
        }),
        ("the window losing the keyboard", |s| {
            s.cx.deactivate_window();
            s.cx.run_until_parked();
            s.cx.update(|window, _| window.activate_window());
            s.cx.run_until_parked();
        }),
    ];
    for (how, end) in ends {
        s.click("textfield", "Punch Out");
        // Return commits a refused text: the draft goes all the same.
        s.keys("x");
        assert!(s.draft(), "{how}: typed");
        end(&mut s);
        assert!(!s.draft(), "{how}: the draft is cleared");
        assert!(s.keys_enabled(), "{how}: the keys are back");
        s.records_and_stops(how);
    }
}

/// The Mixer window closing with its bar's Punch In typed in leaves no
/// draft behind.
#[gpui::test]
fn closing_the_mixer_window_ends_its_punch_edit(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.set_bar(PUNCH_BAR);
    let doc = s.doc();
    let app = s.app.clone();
    let mixer =
        s.cx.update(|_, cx| DocumentWindow::open_mixer(&app, doc, cx).unwrap());
    s.cx.run_until_parked();
    let mut mcx = VisualTestContext::from_window(mixer.into(), &s.cx);
    mcx.simulate_resize(size(px(WIDE), px(640.)));
    mcx.run_until_parked();
    let tree = mixer
        .read_with(&mcx, |v, _| v.registry().borrow().tree())
        .unwrap();
    let id = tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["role"] == "textfield" && n["label"] == "Punch In")
        .expect("the Mixer window's Punch In")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let at = mixer
        .read_with(&mcx, |v, _| {
            v.registry().borrow().resolve(&id).unwrap().bounds.center()
        })
        .unwrap();
    mcx.simulate_click(at, Modifiers::none());
    mcx.simulate_keystrokes("5");
    mcx.run_until_parked();
    assert!(s.draft());
    mixer
        .update(&mut mcx, |_, window, _| window.remove_window())
        .unwrap();
    s.cx.run_until_parked();
    assert!(!s.draft());
    assert!(s.keys_enabled());
    s.records_and_stops("after the Mixer window closed");
}

/// U focuses Punch In on the row; folded, it opens » at Punch In, where
/// typing and Return set it; with no punch item it says so.
#[gpui::test]
fn u_goes_to_punch_in_on_the_row_or_in_more(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.set_bar(PUNCH_BAR);
    s.keys("u");
    assert!(s.focused("Punch In"));
    s.keys("3 enter");
    assert_eq!(s.punch_in(), 2 * BAR);
    // Folded: » opens at the field, live and with the keyboard.
    s.resize(600.);
    assert!(!s.has("textfield", "Punch In"));
    s.keys("u");
    assert!(s.has("group", "Punch In and Out"), "{:?}", s.tree());
    assert!(s.focused("Punch In"));
    assert!(s.has("textfield", "Punch Out"));
    s.keys("5");
    // Record refuses there too, and the field keeps the keyboard.
    s.click("button", "Record");
    assert_eq!(s.status().as_deref(), Some(DRAFT));
    assert!(s.focused("Punch In"));
    s.keys("enter");
    assert_eq!(s.punch_in(), 4 * BAR);
    assert!(!s.draft());
    // Esc closes it.
    s.keys("escape");
    assert!(!s.has("group", "Punch In and Out"));
    // P with Position folded opens » at Position the same way.
    s.resize(300.);
    s.keys("p");
    assert!(s.has("group", "Position Field"), "{:?}", s.tree());
    assert!(s.focused("Position"));
    s.keys("5 enter");
    assert_eq!(s.locates(), vec![format!("locate({})", 4 * BAR)]);
    s.keys("escape");
    // No punch item: the status says so.
    s.resize(WIDE);
    s.set_bar("cluster,position_field");
    s.keys("u");
    assert_eq!(
        s.status().as_deref(),
        Some("The window shows no transport fields.")
    );
    assert!(s.keys_enabled());
}

/// In », the field items are popovers with live fields: a click on the
/// entry opens it; a click on a field edits it as on the bar; a click
/// outside closes it, cancelling the edit.
#[gpui::test]
fn folded_fields_are_popovers_in_more(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.set_bar(PUNCH_BAR);
    s.resize(600.);
    s.click("button", "More Transport Items");
    s.click("menuitem", "Punch In and Out");
    assert!(!s.has("menuitem", "Punch In and Out"), "the menu closed");
    assert!(s.has("group", "Punch In and Out"));
    assert!(!s.focused("Punch In"), "opened, not editing");
    s.click("textfield", "Punch Out");
    assert!(s.focused("Punch Out"));
    s.keys("7");
    assert!(s.draft());
    s.click("text", "Bar Readout");
    assert!(
        !s.has("group", "Punch In and Out"),
        "a click outside closes it"
    );
    assert!(!s.draft() && s.keys_enabled());
    // The window losing the keyboard closes it too.
    s.keys("u");
    s.keys("4");
    assert!(s.draft());
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    assert!(!s.has("group", "Punch In and Out"));
    assert!(!s.draft() && s.keys_enabled());
    s.cx.update(|window, _| window.activate_window());
    s.cx.run_until_parked();
    // Main View stays a text entry.
    s.set_bar("cluster,bar,position_field,main_view");
    s.resize(300.);
    s.click("button", "More Transport Items");
    let entry = s.node("menuitem", "Main View").expect("an entry");
    assert_eq!(entry["enabled"], false, "{entry}");
}

/// The bar's Punch In and Out share their labels with the record
/// popover's: the popover's are found through its group, the bar's
/// outside it, and the bar's edit ends when the chevron is pressed.
#[gpui::test]
fn the_bar_and_popover_punch_fields_stay_apart(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.set_bar("cluster,edit_fields,punch_fields,record_options");
    s.click("textfield", "Punch In");
    s.keys("3");
    s.click("button", "Record Options");
    assert!(!s.draft(), "the chevron ended the bar's edit");
    s.click("checkbox", "Punch");
    let tree = s.tree();
    let group = tree
        .iter()
        .find(|n| n["role"] == "group" && n["label"] == "Record Options")
        .expect("the popover")["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let inside: Vec<&Value> = tree
        .iter()
        .filter(|n| n["role"] == "textfield" && n["label"] == "Punch In")
        .collect();
    assert_eq!(inside.len(), 2, "the bar's and the popover's");
    let path = |n: &Value| {
        n["id"]
            .as_str()
            .unwrap()
            .split(':')
            .next()
            .unwrap()
            .to_owned()
    };
    let group = group.split(':').next().unwrap().to_owned();
    let in_group = |n: &&&Value| path(n).starts_with(&format!("{group}."));
    assert_eq!(
        inside.iter().filter(in_group).count(),
        1,
        "one under the popover: {inside:?} {group}"
    );
    // The popover's is the one it edits.
    let ours = inside.iter().find(in_group).unwrap()["id"].clone();
    let at = s
        .window
        .read_with(&s.cx, |v, _| {
            v.registry()
                .borrow()
                .resolve(ours.as_str().unwrap())
                .unwrap()
                .bounds
                .center()
        })
        .unwrap();
    s.cx.simulate_click(at, Modifiers::none());
    s.cx.run_until_parked();
    let focused: Vec<bool> = s
        .tree()
        .iter()
        .filter(|n| n["role"] == "textfield" && n["label"] == "Punch In")
        .map(|n| n["focused"] == true)
        .collect();
    assert_eq!(focused.iter().filter(|f| **f).count(), 1, "{focused:?}");
    assert!(
        s.editing_field().is_none(),
        "the bar's field is not editing"
    );
}

/// The labels do not clip: each label's box is at least its text's width
/// at the caption size.
#[gpui::test]
fn the_field_labels_do_not_clip(cx: &mut TestAppContext) {
    use vizdaw_app::transport_fields::TransportField;
    let mut s = setup(cx, demo_fake());
    s.set_bar(PUNCH_BAR);
    let fields: Vec<(TransportField, f32, f32)> = TransportField::ALL
        .into_iter()
        .map(|f| {
            let n = s.node("textfield", f.title()).unwrap();
            let x = n["frame"]["x"].as_f64().unwrap() as f32;
            let w = n["frame"]["width"].as_f64().unwrap() as f32;
            (f, x, x + w)
        })
        .collect();
    let caption = |text: &'static str, s: &mut Setup| {
        s.cx.update(|window, _| {
            let font = window.text_style().font();
            let size = window.rem_size() * 0.75;
            let run = gpui::TextRun {
                len: text.len(),
                font,
                color: gpui::black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            f32::from(
                window
                    .text_system()
                    .shape_line(text.into(), size, &[run], None)
                    .width,
            )
        })
    };
    // From the end of the field before (or the Bar readout) to the start
    // of this one: the gap, the label and the gap after it.
    let bar = s.node("text", "Bar Readout").unwrap();
    let mut end =
        (bar["frame"]["x"].as_f64().unwrap() + bar["frame"]["width"].as_f64().unwrap()) as f32;
    for (f, x, right) in fields {
        // The gap between pieces (8) and between label and field (4).
        let label = x - end - 12.;
        let text = caption(f.title(), &mut s);
        assert!(label >= text, "{} has {label} for {text}", f.title());
        end = right;
    }
}
