//! The strips' slot rows (Swift `SlotView`, `BuiltinSynthSlotView`,
//! `EqSlotView`, `StripInserts`) and sends (`SendKnobRow`, `SendKnob`) in
//! the Mix view, through GPUI's test platform with the scripted session:
//! the Swift labels, the slot and send menus, the insert drag, the send
//! knob and box, and the parts that wait for the plugin windows and the
//! catalogue (shown disabled).

use std::sync::Arc;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, WindowHandle, point, px,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::mixer::{MixerPlacement, StripEntry};
use vizdaw_app::testing::{FakeSession, mixer_fake, slot_row, strip_row};
use vizdaw_ffi::automation::GesturePhaseRow;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_ffi::mixer::{SendRow, StripKindRow};
use vizdaw_ffi::snapshot::SlotStatus;
use vizdaw_gpui::mixer_view::MixerView;
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow, RunCommand};

/// Audio 1 (5: EQ "Q" 50, inserts Comp 51 and Verb 52 bypassed, a
/// pre-fader send to Bus 1 at −6 dB), Audio 2 (6: no slots, no sends),
/// Pad (7: a plugin instrument, slot 53), Synth (1, the Built-in Synth),
/// Bus 1 (2), then Master (0).
fn fake() -> FakeSession {
    let f = mixer_fake();
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
    assert!(!synth.enabled, "its menu and sound window come later");
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
            ("Open".into(), false, false),
            ("Bypass".into(), true, false),
            ("Sidechain".into(), true, false),
            ("Remove".into(), true, false),
        ]
    );
    // Open waits for the plugin windows: nothing happens.
    s.click("menuitem", "Open");
    assert!(s.fake.calls().is_empty(), "{:?}", s.fake.calls());
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
            ("Open".into(), false, false),
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
            ("Open".into(), false, false),
            ("Load".into(), true, false),
            ("Bypass".into(), true, false),
        ]
    );
    s.click("menuitem", "Load");
    assert_eq!(s.fake.loaded(), [vec![53]]);
    // A click on the slot or the Built-in Synth's (their menus and
    // windows come later) does nothing.
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
