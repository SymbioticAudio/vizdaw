//! The strips' slot rows (Swift `SlotView`, `BuiltinSynthSlotView`,
//! `EqSlotView`, `StripInserts`) and sends (`SendKnobRow`, `SendKnob`) in
//! the Mix view, through GPUI's test platform with the scripted session:
//! the Swift labels, the slot and send menus, the insert drag, the send
//! knob and box, the gestures that open the plug-in and Built-in Synth
//! windows, and the parts that wait for the catalogue (shown disabled).

use std::sync::Arc;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, WindowHandle, point, px,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::mixer::{MixerPlacement, StripEntry};
use vizdaw_app::testing::{FakeSession, dest_row, mixer_fake, slot_row, strip_row};
use vizdaw_ffi::BuiltinSound;
use vizdaw_ffi::automation::GesturePhaseRow;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::{DestRow, SendRow, StripKindRow};
use vizdaw_ffi::snapshot::SlotStatus;
use vizdaw_gpui::mixer_view::MixerView;
use vizdaw_gpui::plugin_windows::Subject;
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow, RunCommand};

/// Audio 1 (5: EQ "Q" 50, inserts Comp 51 and Verb 52 bypassed, a
/// pre-fader send to Bus 1 at −6 dB), Audio 2 (6: no slots, no sends),
/// Pad (7: a plugin instrument, slot 53), Synth (1, the Built-in Synth),
/// Bus 1 (2), then Master (0). Destinations 8 and 9 are Built-in
/// Synths.
fn fake() -> FakeSession {
    let f = mixer_fake();
    f.edit_destinations(|d| {
        for id in [8, 9] {
            d.push(DestRow {
                builtin_sound: Some(BuiltinSound::Piano),
                ..dest_row(id, "Built-in Synth")
            });
        }
    });
    f.edit_mixer(|m| {
        let mut a1 = strip_row(5, "Audio 1", StripKindRow::AudioPath { path: 1 });
        a1.eq = Some(slot_row(50, "Q"));
        let mut verb = slot_row(52, "Verb");
        verb.bypass = true;
        a1.inserts = vec![slot_row(51, "Comp"), verb];
        a1.sends = vec![SendRow {
            target: 2,
            level_db: -6.0,
            pre_fader: true,
            enabled: true,
        }];
        let a2 = strip_row(6, "Audio 2", StripKindRow::AudioPath { path: 2 });
        let mut pad = strip_row(7, "Pad", StripKindRow::Instrument { dest: 4 });
        let mut inst = slot_row(53, "Pad");
        inst.instrument = true;
        pad.instrument = Some(inst);
        m.strips.insert(0, a1);
        m.strips.insert(1, a2);
        m.strips.insert(2, pad);
    });
    f
}

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext, f: FakeSession) -> Setup {
    let fake = Arc::new(f);
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(mixer_fake()))),
    );
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
    s.fake.clear_gestures();
    s.fake.clear_calls();
    s
}

impl Setup {
    fn view(&mut self) -> Entity<MixerView> {
        self.window
            .read_with(&self.cx, |v, _| v.mixer_view(MixerPlacement::View))
            .unwrap()
            .expect("the Mix view's mixer")
    }

    fn elements(&mut self) -> Vec<Element> {
        self.window
            .read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    }

    fn find(&mut self, role: &str, label: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == role && e.label == label)
    }

    fn element(&mut self, role: &str, label: &str) -> Element {
        let all = self.elements();
        self.find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {all:#?}"))
    }

    fn value(&mut self, role: &str, label: &str) -> Option<String> {
        self.element(role, label).value
    }

    /// The open menus' items: (title, enabled, checked).
    fn items(&mut self) -> Vec<(String, bool, bool)> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "menuitem")
            .map(|e| (e.label, e.enabled, e.selected))
            .collect()
    }

    fn titles(&mut self) -> Vec<String> {
        self.items().into_iter().map(|i| i.0).collect()
    }

    fn click_at(&mut self, at: Point<Pixels>) {
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.click_at(at);
    }

    fn right_click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.cx.simulate_event(MouseDownEvent {
            button: MouseButton::Right,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        self.cx.simulate_event(MouseUpEvent {
            button: MouseButton::Right,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    fn down(&mut self, at: Point<Pixels>, count: usize) {
        self.cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::none(),
            click_count: count,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    fn move_to(&mut self, at: Point<Pixels>, pressed: bool) {
        self.cx.simulate_event(MouseMoveEvent {
            position: at,
            pressed_button: pressed.then_some(MouseButton::Left),
            modifiers: Modifiers::none(),
        });
        self.cx.run_until_parked();
    }

    fn up(&mut self, at: Point<Pixels>, count: usize) {
        self.cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::none(),
            click_count: count,
        });
        self.cx.run_until_parked();
    }

    /// Whether the document has a window for `subject`.
    fn is_open(&mut self, subject: Subject) -> bool {
        let doc = self.window.read_with(&self.cx, |v, _| v.doc()).unwrap();
        self.app
            .read_with(&self.cx, |a, _| a.plugin_windows().is_open(doc, subject))
    }

    fn open_windows(&mut self) -> usize {
        self.app
            .read_with(&self.cx, |a, _| a.plugin_windows().len())
    }

    fn double_click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        for count in [1, 2] {
            self.down(at, count);
            self.up(at, count);
        }
    }

    fn calls_like(&self, prefix: &str) -> Vec<String> {
        self.fake
            .calls()
            .into_iter()
            .filter(|c| c.starts_with(prefix))
            .collect()
    }

    fn status(&mut self) -> Option<String> {
        let doc = self.window.read_with(&self.cx, |v, _| v.doc()).unwrap();
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().status().map(String::from)
        })
    }
}

fn faders_line_up(s: &mut Setup) {
    let ys: Vec<f32> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "slider" && e.label.starts_with("Fader of "))
        .map(|e| f32::from(e.bounds.origin.y))
        .collect();
    assert!(ys.len() > 1);
    assert!(ys.iter().all(|y| *y == ys[0]), "{ys:?}");
}

#[gpui::test]
fn slot_rows_show_the_swift_labels(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    // The EQ: loaded, its plugin as the value and its bypass; empty, the
    // "EQ ＋" face, disabled until the catalogue comes.
    let eq = s.element("group", "EQ of Audio 1");
    assert_eq!(eq.value.as_deref(), Some("Q"));
    assert!(s.find("button", "Bypass EQ of Audio 1").is_some());
    let empty = s.element("button", "EQ of Audio 2");
    assert_eq!(empty.value.as_deref(), Some("empty"));
    assert!(!empty.enabled);
    // The inserts, in order, then the disabled "+".
    let comp = s.element("button", "Comp");
    let verb = s.element("button", "Verb");
    assert!(comp.bounds.origin.y < verb.bounds.origin.y);
    let add = s.element("button", "Add an effect to Audio 1");
    assert!(!add.enabled);
    assert!(add.bounds.origin.y > verb.bounds.origin.y);
    assert!(!s.element("button", "Add an effect to Master").enabled);
    // The instruments.
    assert!(s.find("button", "Pad, instrument").is_some());
    let synth = s.element("button", "Built-in Synth, instrument");
    assert!(synth.enabled, "its double-click opens its sounds");
    // Taller rows in one strip move every fader down alike.
    faders_line_up(&mut s);
}

#[gpui::test]
fn the_eq_bypass_button_turns_the_eq_off_and_on(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("button", "Bypass EQ of Audio 1");
    assert_eq!(s.calls_like("set_bypass"), ["set_bypass(50,true)"]);
    s.click("button", "Turn On EQ of Audio 1");
    assert_eq!(
        s.calls_like("set_bypass"),
        ["set_bypass(50,true)", "set_bypass(50,false)"]
    );
    assert!(s.find("button", "Bypass EQ of Audio 1").is_some());
}

#[gpui::test]
fn an_insert_menu_bypasses_sidechains_and_removes(cx: &mut TestAppContext) {
    let f = fake();
    f.edit_mixer(|m| m.strips[0].inserts[0].has_sidechain_input = true);
    let mut s = setup(cx, f);
    s.right_click("button", "Comp");
    assert_eq!(
        s.items(),
        [
            ("Open".into(), true, false),
            ("Bypass".into(), true, false),
            ("Sidechain".into(), true, false),
            ("Remove".into(), true, false),
        ]
    );
    // Open opens the insert's window.
    s.click("menuitem", "Open");
    assert!(s.is_open(Subject::Plugin(51)));
    assert!(s.titles().is_empty(), "the menu closed");
    s.right_click("button", "Comp");
    // Sidechain ▸ None, then every other strip but master.
    s.click("menuitem", "Sidechain");
    let titles = s.titles();
    assert_eq!(
        titles[4..],
        ["None", "Audio 2", "Pad", "Synth", "Bus 1"],
        "{titles:?}"
    );
    s.click("menuitem", "Audio 2");
    assert_eq!(s.calls_like("set_sidechain"), ["set_sidechain(51,Some(6))"]);
    assert!(s.titles().is_empty(), "the menu closed");
    // The source now has the tick.
    s.right_click("button", "Comp");
    s.click("menuitem", "Sidechain");
    assert!(s.titles().contains(&"\u{2713} Audio 2".to_owned()));
    s.click("menuitem", "\u{2713} Audio 2");
    s.right_click("button", "Comp");
    s.click("menuitem", "Bypass");
    assert_eq!(s.calls_like("set_bypass"), ["set_bypass(51,true)"]);
    // A bypassed insert offers Turn On.
    s.right_click("button", "Verb");
    assert_eq!(s.titles(), ["Open", "Turn On", "Remove"]);
    s.click("menuitem", "Remove");
    assert_eq!(s.calls_like("remove_insert"), ["remove_insert(52)"]);
    assert!(s.find("button", "Verb").is_none());
}

#[gpui::test]
fn the_eq_menu_offers_remove_eq_and_waits_for_replace(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.right_click("group", "EQ of Audio 1");
    assert_eq!(
        s.items(),
        [
            ("Open".into(), true, false),
            ("Replace\u{2026}".into(), false, false),
            ("Bypass".into(), true, false),
            ("Remove EQ".into(), true, false),
        ]
    );
    s.click("menuitem", "Remove EQ");
    assert_eq!(s.calls_like("set_eq"), ["set_eq(5,None)"]);
    assert_eq!(s.value("button", "EQ of Audio 1").as_deref(), Some("empty"));
}

#[gpui::test]
fn the_instrument_slot_menu_bypasses_and_loads_a_held_slot(cx: &mut TestAppContext) {
    let f = fake();
    f.set_held(vec![53]);
    let mut s = setup(cx, f);
    s.right_click("button", "Pad, instrument");
    assert_eq!(
        s.items(),
        [
            ("Open".into(), true, false),
            ("Load".into(), true, false),
            ("Bypass".into(), true, false),
        ]
    );
    s.click("menuitem", "Load");
    assert_eq!(s.fake.loaded(), [vec![53]]);
    // A click on the slot or the Built-in Synth's (their instrument menu
    // comes with the catalogue) does nothing.
    s.click("button", "Pad, instrument");
    s.click("button", "Built-in Synth, instrument");
    s.right_click("button", "Built-in Synth, instrument");
    assert!(s.titles().is_empty());
    s.click("button", "Add an effect to Audio 1");
    s.click("button", "EQ of Audio 2");
    assert!(s.titles().is_empty());
    assert_eq!(s.fake.calls().len(), 1, "{:?}", s.fake.calls());
}

#[gpui::test]
fn a_faulted_or_placeholder_slot_still_has_its_menu(cx: &mut TestAppContext) {
    let f = fake();
    f.edit_mixer(|m| {
        m.strips[0].inserts[0].status = SlotStatus::Placeholder {
            reason: "Missing".into(),
        };
    });
    let mut s = setup(cx, f);
    s.right_click("button", "Comp");
    assert_eq!(s.titles(), ["Open", "Bypass", "Remove"]);
}

#[gpui::test]
fn dragging_an_insert_moves_it_in_one_step(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let comp = s.element("button", "Comp").bounds.center();
    // A click is no drag.
    s.down(comp, 1);
    s.up(comp, 1);
    assert!(s.calls_like("move_insert").is_empty());
    s.down(comp, 1);
    s.move_to(point(comp.x, comp.y + px(1.)), true);
    let v = s.view();
    assert_eq!(
        v.read_with(&s.cx, |v, _| v.insert_drag().and_then(|d| d.over)),
        None,
        "not moved far enough"
    );
    s.move_to(point(comp.x, comp.y + px(22.)), true);
    let d = v.read_with(&s.cx, |v, _| v.insert_drag()).expect("a drag");
    assert_eq!((d.strip, d.slot, d.from, d.over), (5, 51, 0, Some(1)));
    assert!(
        s.calls_like("move_insert").is_empty(),
        "nothing until the drop"
    );
    // Far below: still the last place.
    s.move_to(point(comp.x, comp.y + px(200.)), true);
    let d = v.read_with(&s.cx, |v, _| v.insert_drag()).unwrap();
    assert_eq!(d.over, Some(1));
    s.up(point(comp.x, comp.y + px(200.)), 1);
    assert_eq!(s.calls_like("move_insert"), ["move_insert(51,5,1)"]);
    assert!(v.read_with(&s.cx, |v, _| v.insert_drag()).is_none());
    let comp = s.element("button", "Comp").bounds.origin.y;
    assert!(s.element("button", "Verb").bounds.origin.y < comp, "moved");
}

#[gpui::test]
fn an_insert_dropped_where_it_was_or_lifted_elsewhere_moves_nothing(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let comp = s.element("button", "Comp").bounds.center();
    s.down(comp, 1);
    s.move_to(point(comp.x, comp.y + px(22.)), true);
    s.move_to(point(comp.x, comp.y + px(3.)), true);
    s.up(point(comp.x, comp.y + px(3.)), 1);
    assert!(s.calls_like("move_insert").is_empty(), "back in its place");
    // The mouse-up went elsewhere (a move without the button): the drag
    // ends and nothing moves.
    s.down(comp, 1);
    s.move_to(point(comp.x, comp.y + px(22.)), true);
    s.move_to(point(comp.x, comp.y + px(22.)), false);
    let v = s.view();
    assert!(v.read_with(&s.cx, |v, _| v.insert_drag()).is_none());
    s.up(point(comp.x, comp.y + px(22.)), 1);
    assert!(s.calls_like("move_insert").is_empty());
}

#[gpui::test]
fn send_rows_show_the_swift_labels(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let six = StripEntry::level_text(-6.0);
    assert_eq!(
        s.value("slider", "Send knob 1 of Audio 1"),
        Some(six.clone())
    );
    assert_eq!(s.value("textfield", "Send 1 of Audio 1"), Some(six));
    assert!(s.find("text", "PRE").is_some(), "the pre-fader chip");
    assert_eq!(
        s.value("menu", "Add a send to Audio 1").as_deref(),
        Some("Send")
    );
    // The plus is there only while a strip has no sends.
    assert!(s.find("button", "New bus fed by Audio 1").is_none());
    assert!(s.find("button", "New bus fed by Audio 2").is_some());
    // Master sends nothing.
    assert!(s.find("menu", "Add a send to Master").is_none());
    assert!(s.find("button", "New bus fed by Master").is_none());
    faders_line_up(&mut s);
}

#[gpui::test]
fn a_send_knob_drag_is_one_gesture_on_the_first_send(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let at = s
        .element("slider", "Send knob 1 of Audio 1")
        .bounds
        .center();
    s.down(at, 1);
    assert!(s.fake.gestures().is_empty(), "a bare press sends nothing");
    s.move_to(point(at.x, at.y - px(2.)), true);
    s.move_to(point(at.x, at.y - px(30.)), true);
    s.up(point(at.x, at.y - px(30.)), 1);
    let target = T::SendLevel { strip: 5, send: 0 };
    let phases: Vec<GesturePhaseRow> = s
        .fake
        .gestures()
        .into_iter()
        .filter(|g| g.0 == target)
        .map(|g| g.1)
        .collect();
    assert_eq!(
        phases.first(),
        Some(&GesturePhaseRow::Touch { value: None })
    );
    assert_eq!(phases.last(), Some(&GesturePhaseRow::Release));
    assert_eq!(
        phases
            .iter()
            .filter(|p| **p == GesturePhaseRow::Release)
            .count(),
        1
    );
    assert!(s.fake.gestures().iter().all(|g| g.0 == target));
}

#[gpui::test]
fn a_held_send_knob_lets_go_when_undo_moves_its_send(cx: &mut TestAppContext) {
    // Audio 1 also sends to Bus 2 (3) at −12 dB, as its second send.
    let f = fake();
    f.edit_mixer(|m| {
        let at = m.strips.iter().position(|s| s.id == 2).unwrap() + 1;
        m.strips
            .insert(at, strip_row(3, "Bus 2", StripKindRow::Bus));
        m.buses.push(vizdaw_ffi::mixer::BusRow {
            id: 3,
            name: "Bus 2".into(),
        });
        m.strips[0].sends.push(SendRow {
            target: 3,
            level_db: -12.0,
            pre_fader: false,
            enabled: true,
        });
    });
    let mut s = setup(cx, f);
    let at = s
        .element("slider", "Send knob 1 of Audio 1")
        .bounds
        .center();
    s.down(at, 1);
    s.move_to(point(at.x, at.y - px(2.)), true);
    s.move_to(point(at.x, at.y - px(10.)), true);
    let before = s.fake.gestures().len();
    assert!(before >= 2, "touched and moved");
    // ⌘Z while the mouse is down takes Bus 1's send away: Bus 2's is send
    // 1 now.
    s.fake.edit_mixer(|m| {
        m.strips[0].sends.remove(0);
    });
    let doc = s.window.read_with(&s.cx, |v, _| v.doc()).unwrap();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.document_mut(doc).unwrap().undo();
        cx.notify();
    });
    s.cx.run_until_parked();
    s.move_to(point(at.x, at.y - px(40.)), true);
    s.move_to(point(at.x, at.y - px(60.)), true);
    s.up(point(at.x, at.y - px(60.)), 1);
    let after: Vec<GesturePhaseRow> = s.fake.gestures()[before..].iter().map(|g| g.1).collect();
    assert_eq!(
        after,
        [GesturePhaseRow::Release],
        "released once; no value reaches Bus 2's send"
    );
    let pressed = s.app.read_with(&s.cx, |a, _| {
        a.store
            .document(doc)
            .and_then(|d| d.mixer())
            .and_then(|m| m.pressed(MixerPlacement::View))
    });
    assert_eq!(pressed, None, "the drag ended");
}

#[gpui::test]
fn a_typed_send_level_is_one_click_and_bad_text_is_refused(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("textfield", "Send 1 of Audio 1");
    assert!(s.element("textfield", "Send 1 of Audio 1").focused);
    s.cx.simulate_input("-3");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    let target = T::SendLevel { strip: 5, send: 0 };
    let phases: Vec<GesturePhaseRow> = s.fake.gestures().into_iter().map(|g| g.1).collect();
    assert_eq!(phases.len(), 3, "{phases:?}");
    assert!(s.fake.gestures().iter().all(|g| g.0 == target));
    s.fake.clear_gestures();
    s.click("textfield", "Send 1 of Audio 1");
    s.cx.simulate_input("inf");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert!(s.fake.gestures().is_empty());
    assert_eq!(s.status().as_deref(), Some(StripEntry::LEVEL_HINT));
}

#[gpui::test]
fn the_send_menu_adds_a_send_or_a_new_bus(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("menu", "Add a send to Audio 2");
    assert_eq!(s.titles(), ["Bus 1", "New Bus"]);
    s.click("menuitem", "Bus 1");
    assert_eq!(s.calls_like("add_send"), ["add_send(6,2)"]);
    assert!(s.find("slider", "Send knob 1 of Audio 2").is_some());
    assert!(s.find("button", "New bus fed by Audio 2").is_none());
    // The menu's New Bus, and the plus.
    s.click("menu", "Add a send to Audio 2");
    s.click("menuitem", "New Bus");
    assert_eq!(s.calls_like("new_bus_from"), ["new_bus_from(6,Bus 2)"]);
    s.click("button", "New bus fed by Synth");
    assert_eq!(
        s.calls_like("new_bus_from"),
        ["new_bus_from(6,Bus 2)", "new_bus_from(1,Bus 3)"]
    );
    assert!(s.find("slider", "Send knob 2 of Audio 2").is_some());
}

#[gpui::test]
fn a_sends_menu_switches_pre_fader_and_on_and_removes_it(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.right_click("slider", "Send knob 1 of Audio 1");
    assert_eq!(
        s.items(),
        [
            ("Pre-Fader".into(), true, true),
            ("On".into(), true, true),
            ("MIDI Learn".into(), true, false),
            ("Remove Send".into(), true, false),
        ]
    );
    s.click("menuitem", "Pre-Fader");
    assert_eq!(
        s.calls_like("edit_send"),
        ["edit_send(5,0,2,-6.0,false,true)"]
    );
    assert!(s.find("text", "PRE").is_none());
    s.right_click("slider", "Send knob 1 of Audio 1");
    s.click("menuitem", "On");
    assert_eq!(
        s.calls_like("edit_send").last().map(String::as_str),
        Some("edit_send(5,0,2,-6.0,false,false)")
    );
    s.right_click("slider", "Send knob 1 of Audio 1");
    assert_eq!(s.items()[1], ("On".into(), true, false));
    s.click("menuitem", "Remove Send");
    assert_eq!(s.calls_like("remove_send"), ["remove_send(5,0)"]);
    assert!(s.find("slider", "Send knob 1 of Audio 1").is_none());
    assert!(s.find("button", "New bus fed by Audio 1").is_some());
}

#[gpui::test]
fn at_the_default_window_size_the_strips_fit_their_area(cx: &mut TestAppContext) {
    // The demo's strips plus two buses and a send.
    let f = mixer_fake();
    f.edit_mixer(|m| {
        let mut b2 = strip_row(3, "Bus 2", StripKindRow::Bus);
        b2.sends = vec![SendRow {
            target: 2,
            level_db: 0.0,
            pre_fader: false,
            enabled: true,
        }];
        m.strips.insert(2, b2);
        m.buses.push(vizdaw_ffi::mixer::BusRow {
            id: 3,
            name: "Bus 2".into(),
        });
    });
    let mut s = setup(cx, f);
    let size = s.cx.update(|w, _| w.viewport_size());
    assert_eq!(
        (f32::from(size.width), f32::from(size.height)),
        (1440., 900.)
    );
    // A status line shows too (it takes its height from the regions).
    let doc = s.window.read_with(&s.cx, |v, _| v.doc()).unwrap();
    s.app.update(&mut s.cx, |a, cx| {
        a.store
            .with_document(doc, |d, _| d.set_status(Some("Nothing is playing.".into())));
        cx.notify();
    });
    s.cx.run_until_parked();
    let area = s.element("group", "Mixer").bounds;
    let all = s.elements();
    modes_inside(&all, area);
    let room = s.view().read_with(&s.cx, |v, _| v.room());
    assert_eq!(room.1, 0.0, "nothing to scroll down");
}

/// Every automation mode menu inside the mixer's area `area`.
fn modes_inside(all: &[Element], area: gpui::Bounds<Pixels>) {
    let modes: Vec<&Element> = all
        .iter()
        .filter(|e| e.role == "menu" && e.label.starts_with("Automation of "))
        .collect();
    assert!(!modes.is_empty());
    for e in modes {
        assert!(
            e.bounds.bottom() <= area.bottom() && e.bounds.top() >= area.top(),
            "{} at {:?} outside the mixer {:?}",
            e.label,
            e.bounds,
            area
        );
    }
}

#[gpui::test]
fn the_real_demo_fits_the_default_window_when_the_status_line_comes(cx: &mut TestAppContext) {
    use vizdaw_app::DocSession;
    use vizdaw_ffi::Session;
    // The demo, two buses and a send, on the real core.
    let session = Session::new_demo().unwrap();
    session.add_bus("Bus 1".into()).unwrap();
    session.add_bus("Bus 2".into()).unwrap();
    let m = session.mixer().unwrap();
    session.add_send(m.strips[0].id, m.buses[0].id).unwrap();
    let shared: Arc<dyn DocSession> = session;
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(mixer_fake()))),
    );
    let window = cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
        vizdaw_gpui::new_demo(cx).expect("a demo window")
    });
    let mut vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    let elements = |vcx: &mut VisualTestContext| {
        window
            .read_with(vcx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    };
    let mixer = |all: &[Element]| {
        all.iter()
            .find(|e| e.role == "group" && e.label == "Mixer")
            .expect("the mixer")
            .bounds
    };
    vcx.dispatch_action(RunCommand::new("ui.view_mix"));
    vcx.run_until_parked();
    let all = elements(&mut vcx);
    let before = mixer(&all);
    modes_inside(&all, before);
    // Stop: "Nothing is playing." takes the status line's height.
    vcx.dispatch_action(RunCommand::new("transport.stop"));
    vcx.run_until_parked();
    let all = elements(&mut vcx);
    let area = mixer(&all);
    assert!(
        area.size.height < before.size.height,
        "the status line shows"
    );
    modes_inside(&all, area);
    let view = window
        .read_with(&vcx, |v, _| v.mixer_view(MixerPlacement::View))
        .unwrap()
        .unwrap();
    assert_eq!(view.read_with(&vcx, |v, _| v.room()).1, 0.0);
}

#[gpui::test]
fn a_press_and_release_in_one_frame_leave_no_drag_open(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    // An insert: down and up with no frame between.
    let comp = s.element("button", "Comp").bounds.center();
    let ev = |button| MouseDownEvent {
        button,
        position: comp,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    };
    s.cx.simulate_event(ev(MouseButton::Left));
    s.cx.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position: comp,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    s.cx.run_until_parked();
    let v = s.view();
    assert!(v.read_with(&s.cx, |v, _| v.insert_drag()).is_none());
    assert!(s.fake.calls().is_empty(), "{:?}", s.fake.calls());
    // A send knob likewise.
    let knob = s
        .element("slider", "Send knob 1 of Audio 1")
        .bounds
        .center();
    s.cx.simulate_event(MouseDownEvent {
        position: knob,
        ..ev(MouseButton::Left)
    });
    s.cx.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position: knob,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    s.cx.run_until_parked();
    let doc = s.window.read_with(&s.cx, |v, _| v.doc()).unwrap();
    let pressed = s.app.read_with(&s.cx, |a, _| {
        a.store
            .document(doc)
            .and_then(|d| d.mixer())
            .and_then(|m| m.pressed(MixerPlacement::View))
    });
    assert_eq!(pressed, None, "no press left open");
    assert!(s.fake.gestures().is_empty(), "{:?}", s.fake.gestures());
    assert!(s.fake.calls().is_empty(), "{:?}", s.fake.calls());
}

#[gpui::test]
fn a_drag_whose_mouse_up_was_lost_never_drops_on_a_later_click(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let comp = s.element("button", "Comp").bounds.center();
    s.down(comp, 1);
    s.move_to(point(comp.x, comp.y + px(22.)), true);
    let v = s.view();
    assert_eq!(
        v.read_with(&s.cx, |v, _| v.insert_drag().and_then(|d| d.over)),
        Some(1)
    );
    // The up never came (something swallowed it); the next click, on a
    // strip's mute, must not drop the insert at its stale place.
    s.click("button", "Mute Audio 2");
    assert!(v.read_with(&s.cx, |v, _| v.insert_drag()).is_none());
    assert!(
        s.calls_like("move_insert").is_empty(),
        "{:?}",
        s.fake.calls()
    );
}

#[gpui::test]
fn open_in_the_eq_and_instrument_menus_opens_their_windows(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.right_click("group", "EQ of Audio 1");
    s.click("menuitem", "Open");
    assert!(s.is_open(Subject::Plugin(50)));
    s.right_click("button", "Pad, instrument");
    s.click("menuitem", "Open");
    assert!(s.is_open(Subject::Plugin(53)));
    assert_eq!(s.open_windows(), 2);
}

#[gpui::test]
fn a_double_click_on_an_insert_opens_its_window_and_arms_no_drag(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let comp = s.element("button", "Comp").bounds.center();
    s.down(comp, 1);
    s.up(comp, 1);
    assert!(!s.is_open(Subject::Plugin(51)), "a click opens nothing");
    s.down(comp, 2);
    let v = s.view();
    assert!(
        v.read_with(&s.cx, |v, _| v.insert_drag()).is_none(),
        "the double-click arms no drag"
    );
    // Moving on with the button down drags nothing.
    s.move_to(point(comp.x, comp.y + px(22.)), true);
    assert!(v.read_with(&s.cx, |v, _| v.insert_drag()).is_none());
    s.up(point(comp.x, comp.y + px(22.)), 2);
    assert!(s.calls_like("move_insert").is_empty());
    assert!(s.is_open(Subject::Plugin(51)));
    assert_eq!(s.open_windows(), 1);
}

#[gpui::test]
fn a_double_click_on_an_instrument_slot_opens_its_window(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("button", "Pad, instrument");
    assert!(!s.is_open(Subject::Plugin(53)));
    s.double_click("button", "Pad, instrument");
    assert!(s.is_open(Subject::Plugin(53)));
    assert_eq!(s.open_windows(), 1);
}

#[gpui::test]
fn the_eq_slot_opens_on_a_single_click_and_its_bypass_does_not(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    s.click("button", "Bypass EQ of Audio 1");
    assert!(!s.is_open(Subject::Plugin(50)), "the bypass only bypasses");
    let eq = s.element("group", "EQ of Audio 1").bounds;
    // On the name, left of the bypass button.
    s.click_at(point(eq.origin.x + px(8.), eq.center().y));
    assert!(s.is_open(Subject::Plugin(50)));
    // The empty EQ's "EQ ＋" waits for the catalogue.
    s.click("button", "EQ of Audio 2");
    assert_eq!(s.open_windows(), 1);
}

#[gpui::test]
fn a_double_click_on_the_builtin_synth_slot_opens_its_sounds(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    // Its single click is the instrument menu: not yet.
    s.click("button", "Built-in Synth, instrument");
    assert_eq!(s.open_windows(), 0);
    s.double_click("button", "Built-in Synth, instrument");
    // Nothing focused: the first Built-in Synth.
    assert!(s.is_open(Subject::BuiltinSynth(8)));
    assert_eq!(s.open_windows(), 1);
    // Again: the same window.
    s.double_click("button", "Built-in Synth, instrument");
    assert_eq!(s.open_windows(), 1);
}
