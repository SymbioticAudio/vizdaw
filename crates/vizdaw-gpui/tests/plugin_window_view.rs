//! What the plug-in and Built-in Synth windows draw, through GPUI's test
//! platform with the scripted session: the header (Bypass, the disabled
//! Learn and its help, the placeholder's reason), the parameter list
//! (Swift `ParameterList`), the Built-in Synth window's sounds, and ⌘W.

mod mixer_support;

use gpui::{AnyWindowHandle, Modifiers, TestAppContext, VisualTestContext, point, px};
use mixer_support::{Setup, setup};
use vizdaw_app::plugin_window::{LEARN_HELP, NO_PARAMETERS};
use vizdaw_app::testing::{FakeSession, dest_row, mixer_fake, slot_row};
use vizdaw_ffi::automation::GesturePhaseRow;
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::snapshot::{ParamRow, SlotStatus};
use vizdaw_ffi::{BuiltinSound, EditorSize};
use vizdaw_gpui::plugin_windows::{self, PluginWindowView, Subject};
use vizdaw_gpui::registry::Element;

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

/// Synth (1) has insert Gain (10), with no editor: Gain (0, 0…1 at 0.5),
/// Steps (1, stepped 0…4 at 2), Meter (2, read-only) and a hidden one.
/// Destination 7 is a Built-in Synth playing Piano.
fn fake() -> FakeSession {
    let f = mixer_fake();
    f.edit_mixer(|m| m.strips[0].inserts = vec![slot_row(10, "Gain")]);
    let mut steps = param(1, "Steps");
    steps.stepped = true;
    steps.max = 4.;
    steps.value = 2.;
    let mut meter = param(2, "Meter");
    meter.read_only = true;
    let mut hidden = param(3, "Hidden");
    hidden.hidden = true;
    f.set_params(10, vec![param(0, "Gain"), steps, meter, hidden]);
    f.edit_destinations(|d| {
        d.push(DestRow {
            channel: Some(0),
            builtin_sound: Some(BuiltinSound::Piano),
            ..dest_row(7, "Built-in Synth")
        })
    });
    f
}

struct Win {
    handle: AnyWindowHandle,
    cx: VisualTestContext,
}

fn open(s: &mut Setup, subject: Subject) -> Win {
    let doc = s.doc();
    let handle =
        s.cx.update(|_, cx| match subject {
            Subject::Plugin(slot) => plugin_windows::show_plugin_window(doc, slot, cx),
            Subject::BuiltinSynth(dest) => plugin_windows::show_builtin_synth_window(doc, dest, cx),
        })
        .expect("a window");
    s.cx.run_until_parked();
    let cx = VisualTestContext::from_window(handle, &s.cx.cx);
    Win { handle, cx }
}

impl Win {
    fn elements(&mut self) -> Vec<Element> {
        let h = self.handle.downcast::<PluginWindowView>().unwrap();
        self.cx.run_until_parked();
        h.read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
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

    fn click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn is_gone(&mut self) -> bool {
        let h = self.handle;
        self.cx.cx.update(|cx| h.update(cx, |_, _, _| ())).is_err()
    }
}

fn gain_target() -> AutomationTargetRow {
    AutomationTargetRow::PluginParam { slot: 10, param: 0 }
}

#[gpui::test]
fn the_header_has_bypass_and_a_disabled_learn(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let mut w = open(&mut s, Subject::Plugin(10));
    let bypass = w.element("checkbox", "Bypass");
    assert!(bypass.enabled && !bypass.selected);
    let learn = w.element("button", "Learn");
    assert!(!learn.enabled, "MIDI Learn comes with the surfaces");
    assert_eq!(learn.value.as_deref(), Some(LEARN_HELP));
    // The header is 37 points, the list under it.
    assert!(bypass.bounds.origin.y < px(37.));
    assert!(w.element("slider", "Gain").bounds.origin.y > px(37.));
}

#[gpui::test]
fn bypass_toggles_the_slot(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let mut w = open(&mut s, Subject::Plugin(10));
    w.click("checkbox", "Bypass");
    assert!(s.fake.calls().contains(&"set_bypass(10,true)".to_owned()));
    assert!(w.element("checkbox", "Bypass").selected);
    w.click("checkbox", "Bypass");
    assert!(s.fake.calls().contains(&"set_bypass(10,false)".to_owned()));
    assert!(!w.element("checkbox", "Bypass").selected);
}

#[gpui::test]
fn the_parameter_list_shows_names_sliders_and_values(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let mut w = open(&mut s, Subject::Plugin(10));
    let gain = w.element("slider", "Gain");
    assert!(gain.enabled);
    assert_eq!(gain.value.as_deref(), Some("0.50"));
    assert_eq!(w.element("slider", "Steps").value.as_deref(), Some("2"));
    let meter = w.element("slider", "Meter");
    assert!(!meter.enabled, "read-only");
    assert_eq!(meter.value.as_deref(), Some("0.50, read-only"));
    assert!(w.find("slider", "Hidden").is_none(), "hidden is left out");
    // The name (110 wide) left of the slider, the value (48 wide) right.
    let name = w.element("text", "Gain");
    assert_eq!(name.bounds.size.width, px(110.));
    assert!(name.bounds.origin.x < gain.bounds.origin.x);
    let values: Vec<Element> = w
        .elements()
        .into_iter()
        .filter(|e| e.role == "text" && e.label == "0.50")
        .collect();
    assert_eq!(values.len(), 2, "Gain's and Meter's");
    assert_eq!(values[0].bounds.size.width, px(48.));
    assert!(values[0].bounds.origin.x > gain.bounds.origin.x);
    assert!(w.find("text", NO_PARAMETERS).is_none());
}

#[gpui::test]
fn an_empty_list_says_why(cx: &mut TestAppContext) {
    let f = fake();
    f.set_params(10, vec![]);
    let mut s = setup(cx, f);
    let mut w = open(&mut s, Subject::Plugin(10));
    // No editor: the error's message.
    assert!(
        w.find("text", "editor unavailable: the plugin has no editor")
            .is_some(),
        "{:#?}",
        w.elements()
    );
}

#[gpui::test]
fn a_placeholder_shows_its_reason(cx: &mut TestAppContext) {
    let f = fake();
    f.set_params(10, vec![]);
    f.edit_mixer(|m| {
        m.strips[0].inserts[0].status = SlotStatus::Placeholder {
            reason: "Missing".into(),
        }
    });
    let mut s = setup(cx, f);
    let mut w = open(&mut s, Subject::Plugin(10));
    // The reason in the header and as the list's text.
    let texts: Vec<Element> = w
        .elements()
        .into_iter()
        .filter(|e| e.role == "text" && e.label == "Missing")
        .collect();
    assert_eq!(texts.len(), 2, "{texts:#?}");
    assert!(texts[0].bounds.origin.y < px(37.));
}

#[gpui::test]
fn an_open_editor_shows_no_list(cx: &mut TestAppContext) {
    let f = fake();
    f.set_editor(
        10,
        Some(EditorSize {
            width: 200,
            height: 100,
            resizable: false,
        }),
    );
    let mut s = setup(cx, f);
    let mut w = open(&mut s, Subject::Plugin(10));
    assert!(w.find("slider", "Gain").is_none());
    assert!(w.find("checkbox", "Bypass").is_some());
}

#[gpui::test]
fn a_parameter_drag_is_one_gesture(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let mut w = open(&mut s, Subject::Plugin(10));
    s.fake.clear_gestures();
    let b = w.element("slider", "Gain").bounds;
    let y = b.center().y;
    let x = |f: f32| b.origin.x + b.size.width * f;
    w.cx.simulate_mouse_down(point(x(0.5), y), gpui::MouseButton::Left, Modifiers::none());
    w.cx.simulate_mouse_move(
        point(x(0.75), y),
        gpui::MouseButton::Left,
        Modifiers::none(),
    );
    w.cx.simulate_mouse_move(point(x(1.5), y), gpui::MouseButton::Left, Modifiers::none());
    w.cx.simulate_mouse_up(point(x(1.5), y), gpui::MouseButton::Left, Modifiers::none());
    w.cx.run_until_parked();
    let phases: Vec<GesturePhaseRow> = s
        .fake
        .gestures()
        .into_iter()
        .filter(|g| g.0 == gain_target())
        .map(|g| g.1)
        .collect();
    assert_eq!(
        phases.first(),
        Some(&GesturePhaseRow::Touch { value: None }),
        "{phases:?}"
    );
    assert_eq!(phases.last(), Some(&GesturePhaseRow::Release), "{phases:?}");
    let touches = phases
        .iter()
        .filter(|p| matches!(p, GesturePhaseRow::Touch { .. }))
        .count();
    let releases = phases
        .iter()
        .filter(|p| matches!(p, GesturePhaseRow::Release))
        .count();
    assert_eq!((touches, releases), (1, 1), "one undo step: {phases:?}");
    // Past the end: the top of the range.
    assert!(
        phases.contains(&GesturePhaseRow::Value { value: 1.0 }),
        "{phases:?}"
    );
    // Learn now names it.
    assert_eq!(
        w.element("button", "Learn").value.as_deref(),
        Some("MIDI Learn: Gain")
    );
    // A read-only parameter takes no gesture.
    s.fake.clear_gestures();
    w.click("slider", "Meter");
    assert!(s.fake.gestures().is_empty(), "{:?}", s.fake.gestures());
}

#[gpui::test]
fn the_builtin_synth_window_picks_a_sound(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let mut w = open(&mut s, Subject::BuiltinSynth(7));
    let group = w.element("radiogroup", "Sound");
    let radios: Vec<Element> = w
        .elements()
        .into_iter()
        .filter(|e| e.role == "radio")
        .collect();
    let titles: Vec<&str> = radios.iter().map(|r| r.label.as_str()).collect();
    assert_eq!(titles, ["Piano", "Bass", "Pad", "Lead", "Drums"]);
    assert!(
        radios
            .iter()
            .all(|r| group.path.len() < r.path.len() && r.path.starts_with(&group.path))
    );
    assert!(radios[0].selected && !radios[1].selected);
    assert!(
        w.find(
            "text",
            "Changes every track that plays this Built-in Synth."
        )
        .is_some()
    );
    w.click("radio", "Bass");
    assert_eq!(s.fake.builtin_sounds(), [(7, BuiltinSound::Bass)]);
    assert!(w.element("radio", "Bass").selected);
    // It follows the document (an undo, say).
    s.fake.edit_destinations(|d| {
        d.iter_mut().find(|d| d.id == 7).unwrap().builtin_sound = Some(BuiltinSound::Lead)
    });
    let doc = s.doc();
    s.cx.update(|_, cx| {
        vizdaw_gpui::AppState::global(cx).update(cx, |a, cx| {
            let d = a.store.document_mut(doc).unwrap();
            let mut snap = d.snapshot().clone();
            snap.version += 1;
            d.apply(snap);
            cx.notify();
        })
    });
    for _ in 0..4 {
        s.cx.executor()
            .advance_clock(std::time::Duration::from_millis(300));
        s.cx.run_until_parked();
    }
    assert!(w.element("radio", "Lead").selected);
}

#[gpui::test]
fn secondary_w_closes_a_plugin_window(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake());
    let mut w = open(&mut s, Subject::Plugin(10));
    w.cx.simulate_keystrokes("secondary-w");
    w.cx.run_until_parked();
    assert!(w.is_gone());
    assert!(s.fake.windows_open().is_empty());
    // The Built-in Synth window too.
    let mut w = open(&mut s, Subject::BuiltinSynth(7));
    w.cx.simulate_keystrokes("secondary-w");
    w.cx.run_until_parked();
    assert!(w.is_gone());
    // The document window stays.
    let doc_window = s.window;
    assert!(
        s.cx.cx
            .update(|cx| doc_window.update(cx, |_, _, _| ()))
            .is_ok()
    );
}
