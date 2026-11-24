//! The record popover (Swift `RecordPopover`, `RecordOptionsButton`; the
//! view rows of `RecordOptionsTests`): the chevron in each window's bar,
//! the options it sets, the punch fields' drafts and focus rules, the
//! close rules (and the one press that keeps it open), and the Mixer
//! window's own popover, driven through GPUI's test platform with the
//! scripted session.

use std::sync::Arc;

use core_control::protocol::{RawRequest, RequestId};
use gpui::{
    Entity, Modifiers, Pixels, Point, TestAppContext, VisualTestContext, WindowHandle, point, px,
    size,
};
use serde_json::Value;
use vizdaw_app::DocumentStore;
use vizdaw_app::record::{MIDI_TAKES_HELP, OPEN_DRAFT, OVERDUB_NOTE};
use vizdaw_app::testing::{FakeSession, demo_fake, lane_row};
use vizdaw_ffi::edit::LaneInputRow;
use vizdaw_gpui::control;
use vizdaw_gpui::menus::RunCommand;
use vizdaw_gpui::{AppState, DocumentWindow};

/// One bar of 4/4, in ticks.
const BAR: i64 = 61_440;
const OPTIONS: &str = "Record Options";

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

/// The demo with Song (3)'s two tracks armed: Groove a stereo audio
/// input, Chords a MIDI one.
fn armed_fake() -> FakeSession {
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

fn tree_of(window: WindowHandle<DocumentWindow>, cx: &VisualTestContext) -> Vec<Value> {
    let tree = window
        .read_with(cx, |v, _| v.registry().borrow().tree())
        .unwrap();
    tree["nodes"].as_array().unwrap().clone()
}

fn find(nodes: &[Value], role: &str, label: &str) -> Option<Value> {
    nodes
        .iter()
        .find(|n| n["role"] == role && n["label"] == label)
        .cloned()
}

fn bounds_of(
    window: WindowHandle<DocumentWindow>,
    cx: &VisualTestContext,
    role: &str,
    label: &str,
) -> gpui::Bounds<Pixels> {
    let nodes = tree_of(window, cx);
    let id = find(&nodes, role, label)
        .unwrap_or_else(|| panic!("no {role} {label:?} in {nodes:?}"))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    window
        .read_with(cx, |v, _| {
            v.registry().borrow().resolve(&id).unwrap().bounds
        })
        .unwrap()
}

impl Setup {
    fn tree(&mut self) -> Vec<Value> {
        tree_of(self.window, &self.cx)
    }

    fn node(&mut self, role: &str, label: &str) -> Option<Value> {
        find(&self.tree(), role, label)
    }

    fn has(&mut self, role: &str, label: &str) -> bool {
        self.node(role, label).is_some()
    }

    fn get(&mut self, role: &str, label: &str) -> Value {
        self.node(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {:?}", self.tree()))
    }

    fn enabled(&mut self, role: &str, label: &str) -> bool {
        self.get(role, label)["enabled"] == true
    }

    fn selected(&mut self, role: &str, label: &str) -> bool {
        self.get(role, label)["selected"] == true
    }

    fn focused(&mut self, label: &str) -> bool {
        self.get("textfield", label)["focused"] == true
    }

    fn value(&mut self, role: &str, label: &str) -> Value {
        self.get(role, label)["value"].clone()
    }

    fn open(&mut self) -> bool {
        self.has("group", OPTIONS)
    }

    fn click_at(&mut self, at: Point<Pixels>) {
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let at = bounds_of(self.window, &self.cx, role, label).center();
        self.click_at(at);
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

    fn draft(&mut self) -> bool {
        self.with_doc(|d| d.has_open_punch_draft())
    }

    fn punch_in(&mut self) -> i64 {
        self.with_doc(|d| d.layout().transport.punch_in)
    }

    fn records(&self) -> usize {
        self.fake.count("record(3)")
    }

    /// Opens the popover with its chevron and turns Punch on.
    fn open_with_punch(&mut self) {
        self.click("button", OPTIONS);
        assert!(self.open(), "{:?}", self.tree());
        if !self.selected("checkbox", "Punch") {
            self.click("checkbox", "Punch");
        }
        assert!(self.with_doc(|d| d.record_punches()));
    }

    /// A point inside the popover that is none of its controls (its
    /// padding).
    fn popover_background(&mut self) -> Point<Pixels> {
        let b = bounds_of(self.window, &self.cx, "group", OPTIONS);
        point(b.origin.x + px(4.), b.origin.y + px(4.))
    }

    fn channel(&mut self, cmd: &str, params: Value) -> control::Handled {
        let Value::Object(params) = params else {
            panic!("params are an object")
        };
        let raw = RawRequest {
            id: RequestId::Num(1),
            cmd: cmd.into(),
            params,
        };
        let command = core_control::validate(&raw).expect("a valid request");
        let handled = self.cx.cx.update(|cx| control::handle(&command, cx));
        self.cx.run_until_parked();
        handled
    }
}

/// The chevron opens the popover under it (340 wide, no title); its
/// Mode segments set the transport's mode, the same one the bar's switch
/// shows; the steppers clamp and read Swift's texts; MIDI passes as takes
/// carries its help.
#[gpui::test]
fn the_popover_sets_the_modes_and_agrees_with_the_bar_switch(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    assert_eq!(
        vizdaw_gpui::transport_bar::RECORD_OPTIONS_HELP,
        "Replace or overdub, punch, loop, count-in, pre-roll and MIDI takes"
    );
    assert!(!s.open());
    s.click("button", OPTIONS);
    assert!(s.open(), "{:?}", s.tree());
    let chevron = bounds_of(s.window, &s.cx, "button", OPTIONS);
    let popover = bounds_of(s.window, &s.cx, "group", OPTIONS);
    // 340 wide, its border inside.
    assert!(
        (f32::from(popover.size.width) - 340.).abs() <= 2.,
        "{popover:?}"
    );
    assert!(
        popover.origin.y >= chevron.origin.y + chevron.size.height,
        "under the chevron: {popover:?} {chevron:?}"
    );
    // A new document overdubs.
    assert!(s.selected("button", "Overdub") && !s.selected("button", "Replace"));
    s.click("button", "Replace");
    assert!(s.with_doc(|d| d.record_replaces()));
    assert_eq!(s.value("button", "Record mode: Replace"), "off");
    assert!(s.selected("button", "Replace"));
    // The bar's switch is the same mode (a press on it, outside the
    // popover, closes the popover).
    s.click("button", "Record mode: Replace");
    assert!(!s.open() && !s.with_doc(|d| d.record_replaces()));
    s.click("button", OPTIONS);
    assert!(s.selected("button", "Overdub") && !s.selected("button", "Replace"));
    // Count-in: off, 1 bar, N bars, at most 4.
    assert!(s.has("stepper", "Count-in: off"), "{:?}", s.tree());
    assert!(!s.enabled("button", "Decrement Count-in"), "at its least");
    s.click("button", "Increment Count-in");
    assert!(s.has("stepper", "Count-in: 1 bar"));
    for _ in 0..3 {
        s.click("button", "Increment Count-in");
    }
    assert!(s.has("stepper", "Count-in: 4 bars"));
    assert!(!s.enabled("button", "Increment Count-in"), "at its most");
    s.click("button", "Increment Count-in");
    assert_eq!(s.with_doc(|d| d.record().count_in_bars()), 4);
    s.click("button", "Decrement Count-in");
    assert!(s.has("stepper", "Count-in: 3 bars"));
    // Pre-roll: 0 to 8.
    assert!(s.has("stepper", "Pre-roll: off"));
    for _ in 0..9 {
        if s.enabled("button", "Increment Pre-roll") {
            s.click("button", "Increment Pre-roll");
        }
    }
    assert!(s.has("stepper", "Pre-roll: 8 bars"));
    assert_eq!(s.with_doc(|d| d.record().pre_roll_bars()), 8);
    assert!(!s.enabled("button", "Increment Pre-roll"));
    // MIDI passes as takes, with its help.
    assert!(!s.selected("checkbox", "MIDI passes as takes"));
    assert_eq!(s.value("checkbox", "MIDI passes as takes"), MIDI_TAKES_HELP);
    s.click("checkbox", "MIDI passes as takes");
    assert!(s.with_doc(|d| d.record().midi_passes_as_takes()));
    assert!(s.selected("checkbox", "MIDI passes as takes"));
    // Punch is off: its fields are disabled.
    assert!(!s.selected("checkbox", "Punch"));
    assert!(!s.enabled("textfield", "Punch In") && !s.enabled("textfield", "Punch Out"));
    s.click("checkbox", "Punch");
    assert!(s.with_doc(|d| d.record_punches()));
    assert!(s.enabled("textfield", "Punch In") && s.enabled("textfield", "Punch Out"));
    // The chevron again closes it.
    s.click("button", OPTIONS);
    assert!(!s.open());
}

/// The popover's Loop toggles the document's loop (the bar's Loop shows
/// it).
#[gpui::test]
fn loop_toggles_the_documents_loop(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("button", OPTIONS);
    assert!(!s.selected("checkbox", "Loop"));
    s.click("checkbox", "Loop");
    assert!(s.with_doc(|d| d.looping()));
    assert!(s.selected("checkbox", "Loop"));
    assert_eq!(s.value("button", "Loop"), "on");
    s.click("checkbox", "Loop");
    assert!(!s.with_doc(|d| d.looping()));
    assert!(s.open(), "a toggle leaves it open");
}

/// Folded, Record Options is a » entry that opens the popover at ».
#[gpui::test]
fn folded_record_options_opens_the_popover_from_more(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let bar = vizdaw_app::transport_bar::TransportLayout::parse(
        "cluster,loop,bar,time,tempo,signature,record_options",
    )
    .unwrap();
    s.app.update(&mut s.cx, |a, cx| {
        a.prefs.set_transport_bar(&bar).unwrap();
        cx.notify();
    });
    s.cx.simulate_resize(size(px(560.), px(640.)));
    s.cx.run_until_parked();
    assert!(!s.has("button", OPTIONS), "folds");
    s.click("button", "More Transport Items");
    assert!(s.enabled("menuitem", OPTIONS));
    s.click("menuitem", OPTIONS);
    assert!(!s.has("menuitem", OPTIONS), "the menu closed");
    assert!(s.open(), "{:?}", s.tree());
    s.keys("escape");
    assert!(!s.open());
}

/// The Mixer window's bar has its own chevron: opening there closes the
/// document window's popover and drops its drafts.
#[gpui::test]
fn opening_in_the_mixer_window_closes_the_documents_popover(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.open_with_punch();
    s.click("textfield", "Punch In");
    s.keys("3");
    assert!(s.draft());
    let doc = s.doc();
    let app = s.app.clone();
    let mixer =
        s.cx.update(|_, cx| DocumentWindow::open_mixer(&app, doc, cx).unwrap());
    s.cx.run_until_parked();
    let mut mcx = VisualTestContext::from_window(mixer.into(), &s.cx);
    mcx.run_until_parked();
    let at = bounds_of(mixer, &mcx, "button", OPTIONS).center();
    mcx.simulate_click(at, Modifiers::none());
    mcx.run_until_parked();
    s.cx.run_until_parked();
    assert!(
        find(&tree_of(mixer, &mcx), "group", OPTIONS).is_some(),
        "open in the Mixer window"
    );
    assert!(!s.open(), "closed in the document window");
    assert!(!s.draft(), "its draft dropped");
    assert_eq!(s.punch_in(), 0);
    assert!(s.with_doc(|d| d.transport_keys_enabled()));
    // Its chevron again closes it.
    let at = bounds_of(mixer, &mcx, "button", OPTIONS).center();
    mcx.simulate_click(at, Modifiers::none());
    mcx.run_until_parked();
    assert!(!s.with_doc(|d| d.record().shows_popover()));
}

/// Esc, a click outside and the chevron close it, and each drops the
/// drafts: the fields show the punch in use, and the keys come back.
#[gpui::test]
fn closing_drops_the_drafts(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    let shown = s.with_doc(|d| {
        vizdaw_app::transport_fields::field_text(
            d,
            vizdaw_app::transport_fields::TransportField::PunchIn,
        )
    });
    // Esc in the field.
    s.open_with_punch();
    s.click("textfield", "Punch In");
    assert!(s.focused("Punch In"));
    assert!(
        !s.with_doc(|d| d.transport_keys_enabled()),
        "the field has the keys"
    );
    s.keys("3");
    assert!(s.draft());
    s.keys("escape");
    assert!(!s.open() && !s.draft());
    assert!(s.with_doc(|d| d.transport_keys_enabled()));
    // Esc with no field focused.
    s.click("button", OPTIONS);
    s.keys("escape");
    assert!(!s.open());
    // A click outside.
    s.click("button", OPTIONS);
    s.click("textfield", "Punch In");
    s.keys("3");
    s.click_at(point(px(700.), px(500.)));
    assert!(!s.open() && !s.draft());
    assert!(s.with_doc(|d| d.transport_keys_enabled()));
    // The chevron.
    s.click("button", OPTIONS);
    s.click("textfield", "Punch In");
    s.keys("3");
    s.click("button", OPTIONS);
    assert!(!s.open() && !s.draft());
    assert!(s.with_doc(|d| d.transport_keys_enabled()));
    assert_eq!(s.punch_in(), 0, "nothing was set");
    s.click("button", OPTIONS);
    assert_eq!(s.value("textfield", "Punch In"), shown.as_str());
    assert_eq!(s.status(), None, "nothing is said");
}

/// With a punch draft open, Record from the bar button, the menu and the
/// channel refuses in Swift's words, and the popover stays open with the
/// field holding the keyboard; the R key types into the field. Return
/// then sets the punch, and Record records at it.
#[gpui::test]
fn a_draft_refuses_record_and_keeps_the_popover_and_the_keyboard(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.open_with_punch();
    s.click("textfield", "Punch In");
    s.keys("5");
    assert!(s.draft());
    let still = |s: &mut Setup, how: &str| {
        assert!(s.open(), "{how}: the popover stays open");
        assert!(s.focused("Punch In"), "{how}: the field keeps the keyboard");
        assert_eq!(s.value("textfield", "Punch In"), "5", "{how}");
        assert!(s.draft(), "{how}: the draft stays");
        assert_eq!(s.records(), 0, "{how}: nothing records");
    };
    // The bar's button.
    s.click("button", "Record");
    assert_eq!(s.status().as_deref(), Some(OPEN_DRAFT));
    still(&mut s, "the button");
    // The menu (Transport › Record).
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.with_document(doc, |d, _| d.set_status(None));
        cx.notify();
    });
    s.cx.dispatch_action(RunCommand::new("transport.record"));
    s.cx.run_until_parked();
    assert_eq!(s.status().as_deref(), Some(OPEN_DRAFT));
    still(&mut s, "the menu");
    // The channel.
    match s.channel("command.run", serde_json::json!({"id": "transport.record"})) {
        control::Handled::Reply(Err(e)) => assert_eq!(e.message, OPEN_DRAFT),
        other => panic!("refused: {other:?}"),
    }
    still(&mut s, "the channel");
    // The R key goes to the field, which has the keyboard.
    s.keys("r");
    assert_eq!(s.value("textfield", "Punch In"), "5r");
    assert_eq!(s.records(), 0);
    s.keys("backspace");
    still(&mut s, "the R key");
    // Return sets the punch; then Record records at it (and the popover
    // closes for the press).
    s.keys("enter");
    assert_eq!(s.punch_in(), 4 * BAR);
    assert!(!s.draft() && s.open());
    s.click("button", "Record");
    assert_eq!(s.records(), 1);
    assert!(s.with_doc(|d| d.record().is_recording()));
    assert!(!s.open());
}

/// Moving from Punch In to Punch Out commits In; leaving the pair (a
/// click on the popover elsewhere) drops the draft and ends the edit.
#[gpui::test]
fn moving_within_the_pair_commits_and_leaving_it_drops(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.open_with_punch();
    let out = s.with_doc(|d| d.layout().transport.punch_out);
    s.click("textfield", "Punch In");
    s.keys("3");
    s.click("textfield", "Punch Out");
    assert_eq!(s.punch_in(), 2 * BAR, "moving within the pair commits");
    assert!(s.focused("Punch Out") && !s.focused("Punch In"));
    s.keys("9");
    assert!(s.draft());
    let at = s.popover_background();
    s.click_at(at);
    assert!(s.open(), "a click inside leaves it open");
    assert!(!s.draft(), "leaving the pair drops");
    assert_eq!(s.with_doc(|d| d.layout().transport.punch_out), out);
    assert!(!s.focused("Punch Out"));
    assert!(s.with_doc(|d| d.transport_keys_enabled()));
    // Tab moves within the pair too, committing.
    s.click("textfield", "Punch In");
    s.keys("2");
    s.keys("tab");
    assert_eq!(s.punch_in(), BAR);
    assert!(s.focused("Punch Out"));
}

/// The overdub note needs an armed audio track.
#[gpui::test]
fn no_armed_audio_track_no_overdub_note(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.click("button", OPTIONS);
    assert!(!s.with_doc(|d| d.record_replaces()));
    assert!(!s.has("text", OVERDUB_NOTE));
}

/// The overdub note shows only with overdub on (and an armed audio
/// track); while recording, everything in the popover is disabled.
#[gpui::test]
fn the_note_follows_overdub_and_recording_disables_everything(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.click("button", OPTIONS);
    assert!(s.has("text", OVERDUB_NOTE), "{:?}", s.tree());
    s.click("button", "Replace");
    assert!(!s.has("text", OVERDUB_NOTE));
    s.click("button", "Overdub");
    assert!(s.has("text", OVERDUB_NOTE));
    s.click("checkbox", "Punch");
    s.click("button", "Record");
    assert!(s.with_doc(|d| d.record().is_recording()));
    s.click("button", OPTIONS);
    assert!(s.open());
    for (role, label) in [
        ("button", "Replace"),
        ("button", "Overdub"),
        ("checkbox", "Punch"),
        ("textfield", "Punch In"),
        ("textfield", "Punch Out"),
        ("checkbox", "Loop"),
        ("button", "Increment Count-in"),
        ("button", "Increment Pre-roll"),
        ("checkbox", "MIDI passes as takes"),
    ] {
        assert!(!s.enabled(role, label), "{role} {label} is disabled");
    }
    s.click("checkbox", "Loop");
    s.click("button", "Replace");
    assert!(!s.with_doc(|d| d.looping() || d.record_replaces()));
}

/// The Mixer window closing with its popover open (a draft typed) takes
/// the popover and its draft with it: Record is not left refusing.
#[gpui::test]
fn closing_the_mixer_window_drops_its_popover(cx: &mut TestAppContext) {
    let mut s = setup(cx, armed_fake());
    s.click("button", OPTIONS);
    s.click("checkbox", "Punch");
    s.click("button", OPTIONS);
    let doc = s.doc();
    let app = s.app.clone();
    let mixer =
        s.cx.update(|_, cx| DocumentWindow::open_mixer(&app, doc, cx).unwrap());
    s.cx.run_until_parked();
    let mut mcx = VisualTestContext::from_window(mixer.into(), &s.cx);
    mcx.run_until_parked();
    let at = bounds_of(mixer, &mcx, "button", OPTIONS).center();
    mcx.simulate_click(at, Modifiers::none());
    mcx.run_until_parked();
    let at = bounds_of(mixer, &mcx, "textfield", "Punch In").center();
    mcx.simulate_click(at, Modifiers::none());
    mcx.simulate_keystrokes("5");
    mcx.run_until_parked();
    assert!(s.draft());
    mixer
        .update(&mut mcx, |_, window, _| window.remove_window())
        .unwrap();
    s.cx.run_until_parked();
    assert!(!s.draft());
    assert!(!s.with_doc(|d| d.record().shows_popover()));
    s.click("button", "Record");
    assert_eq!(s.records(), 1);
}
