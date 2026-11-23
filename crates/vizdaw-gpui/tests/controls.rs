//! The mixer's controls (Swift `Fader`, `Knob`, `NumberBox`,
//! `StripToggle`, `StripMenuFace`), each in a small host view against the
//! scripted session, driven through GPUI's test platform: one gesture per
//! drag, nothing for a bare press, ⌥ fine and alone, the double-click
//! reset, the drag released on deactivation and on unmount, typed values,
//! and the tree's records.

use std::sync::Arc;
use std::time::Instant;

use gpui::{
    AppContext, Bounds, Context, Entity, FocusHandle, InputEvent, InteractiveElement, IntoElement,
    Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Render, Styled, Subscription, TestAppContext, VisualTestContext, Window, WindowBounds,
    WindowHandle, WindowOptions, div, point, px, size,
};
use vizdaw_app::mixer::{MixerMath, MixerPlacement, StripEntry, reset_value};
use vizdaw_app::testing::{FakeSession, mixer_fake};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::automation::GesturePhaseRow;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_gpui::AppState;
use vizdaw_gpui::controls::{
    self, ControlBinding, KnobSize, MixerRef, NumberBinding, NumberBox, Toggle,
    toggle::toggle_mute, toggle::toggle_solo,
};
use vizdaw_gpui::registry::{Element, RegistryScope, SharedRegistry};
use vizdaw_gpui::strip_palette::{KnobFunction, tokens};

const FADER: T = T::StripFader { strip: 1 };
const PAN: T = T::StripPan { strip: 1 };
const THROW: f64 = 160.;
const PLACE: MixerPlacement = MixerPlacement::View;

/// A view laying out strip 1's controls in a column.
struct Host {
    mixer: MixerRef,
    registry: SharedRegistry,
    /// `None`: the level box is gone (dropped).
    level: Option<Entity<NumberBox>>,
    /// The fader's drag state, as drawn last.
    fader_state: Option<Entity<controls::DragState>>,
    pan_box: Entity<NumberBox>,
    show_fader: bool,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Host {
    fn new(mixer: MixerRef, window: &mut Window, cx: &mut Context<Self>) -> Self {
        mixer.with(cx, |m, _| m.appeared(PLACE));
        let subscriptions = vec![
            cx.observe(&mixer.app, |_, _, cx| cx.notify()),
            controls::watch_activation(mixer.clone(), window, cx),
        ];
        Host {
            level: Some(cx.new(|cx| NumberBox::new(window, cx))),
            fader_state: None,
            pan_box: cx.new(|cx| NumberBox::new(window, cx)),
            mixer,
            registry: SharedRegistry::default(),
            show_fader: true,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mixer = self.mixer.clone();
        let parts = mixer.read(cx, |m, _| {
            let s = m.strip(1).unwrap();
            (
                ControlBinding::fader(&mixer, m, 1).unwrap(),
                ControlBinding::pan(&mixer, m, 1).unwrap(),
                NumberBinding::level(&mixer, m, 1).unwrap(),
                NumberBinding::pan(&mixer, m, 1).unwrap(),
                Toggle::mute(m, 1).unwrap(),
                Toggle::solo(m, 1).unwrap(),
                vizdaw_app::mixer::AutomationModes::title(s.automation),
                tokens().automation(s.automation),
                m.meters().level_at(1, Instant::now()),
            )
        });
        let mut root = RegistryScope::root(&self.registry);
        let Some((fader, pan, level, pan_text, mute, solo, mode, dot, meter)) = parts else {
            return div().into_any_element();
        };
        let cap = tokens().strip_instrument;
        let mut col = div()
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(16.));
        if !self.show_fader {
            // Keep no handle on a control that is gone.
            self.fader_state = None;
        } else {
            self.fader_state = Some(controls::drag_state(&fader, window, cx));
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .child(controls::fader(
                        &fader,
                        THROW,
                        gpui::rgb(cap),
                        &mut root,
                        window,
                        cx,
                    ))
                    .child(controls::meter(meter, THROW + 4., window)),
            );
        }
        if let Some(nb) = &self.level {
            col = col.child(controls::number_box(nb, level, &mut root, cx));
        }
        col.child(controls::knob(
            &pan,
            KnobSize::Medium,
            tokens().knob(KnobFunction::Pan),
            0.5,
            &mut root,
            window,
            cx,
        ))
        .child(controls::number_box(&self.pan_box, pan_text, &mut root, cx))
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(4.))
                .child(controls::toggle(mute, &mut root, toggle_mute(&mixer, 1)))
                .child(controls::toggle(solo, &mut root, toggle_solo(&mixer, 1))),
        )
        .child(div().w(px(108.)).child(controls::menu_face(
            "Automation of Synth",
            mode,
            Some(dot),
            true,
            &mut root,
            |_, _| {},
        )))
        .into_any_element()
    }
}

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    doc: DocId,
    window: WindowHandle<Host>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext, fake: FakeSession) -> Setup {
    let fake = Arc::new(fake);
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(mixer_fake()))),
    );
    let (app, doc, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let doc = app
            .update(cx, |s, _| s.store.new_demo())
            .expect("a document");
        let mixer = MixerRef::new(app.clone(), doc, PLACE);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(600.), px(700.)),
                    ))),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| Host::new(mixer, window, cx)),
            )
            .expect("a window");
        (app, doc, window)
    });
    window
        .update(cx, |_, window, _| window.activate_window())
        .unwrap();
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    fake.clear_gestures();
    Setup {
        fake,
        app,
        doc,
        window,
        cx: vcx,
    }
}

fn plain(cx: &mut TestAppContext) -> Setup {
    setup(cx, mixer_fake())
}

fn touch(v: Option<f64>) -> GesturePhaseRow {
    GesturePhaseRow::Touch { value: v }
}

fn value(v: f64) -> GesturePhaseRow {
    GesturePhaseRow::Value { value: v }
}

const RELEASE: GesturePhaseRow = GesturePhaseRow::Release;

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Modifiers::none()
    }
}

impl Setup {
    fn elements(&mut self) -> Vec<Element> {
        self.window
            .read_with(&self.cx, |v, _| v.registry.borrow().elements().to_vec())
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

    fn value_of(&mut self, role: &str, label: &str) -> Option<String> {
        self.element(role, label).value
    }

    /// The middle of the fader's cap column.
    fn fader_at(&mut self) -> Point<Pixels> {
        let b = self.element("slider", "Fader of Synth").bounds;
        point(
            b.origin.x + px((controls::fader::SCALE_WIDTH + controls::fader::COLUMN / 2.) as f32),
            b.center().y,
        )
    }

    fn knob_at(&mut self) -> Point<Pixels> {
        self.element("slider", "Pan knob of Synth").bounds.center()
    }

    fn down(&mut self, at: Point<Pixels>, modifiers: Modifiers, click_count: usize) {
        self.cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers,
            click_count,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    fn move_to(&mut self, at: Point<Pixels>, modifiers: Modifiers) {
        self.cx.simulate_event(MouseMoveEvent {
            position: at,
            pressed_button: Some(MouseButton::Left),
            modifiers,
        });
        self.cx.run_until_parked();
    }

    fn up(&mut self, at: Point<Pixels>, modifiers: Modifiers, click_count: usize) {
        self.cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers,
            click_count,
        });
        self.cx.run_until_parked();
    }

    fn double_click(&mut self, at: Point<Pixels>) {
        self.down(at, Modifiers::none(), 1);
        self.up(at, Modifiers::none(), 1);
        self.down(at, Modifiers::none(), 2);
        self.up(at, Modifiers::none(), 2);
    }

    /// Presses at `at`, then moves up 2 points (the drag begins) and then
    /// `dy` points more (up is negative).
    fn press_and_drag(&mut self, at: Point<Pixels>, dy: f32, m: Modifiers) {
        self.down(at, m, 1);
        self.move_to(point(at.x, at.y - px(2.)), m);
        self.move_to(point(at.x, at.y - px(2.) + px(dy)), m);
    }

    fn click_at(&mut self, at: Point<Pixels>) {
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.click_at(at);
    }

    fn gestures(&self) -> Vec<(T, GesturePhaseRow, bool)> {
        self.fake.gestures()
    }

    fn phases(&self) -> Vec<GesturePhaseRow> {
        self.gestures().iter().map(|g| g.1).collect()
    }

    fn gain(&mut self) -> f64 {
        let doc = self.doc;
        self.app.read_with(&self.cx, |a, _| {
            let m = a.store.document(doc).unwrap().mixer().unwrap();
            m.gain_db(m.strip(1).unwrap())
        })
    }

    fn pressed(&mut self) -> Option<T> {
        let doc = self.doc;
        self.app.read_with(&self.cx, |a, _| {
            a.store
                .document(doc)
                .unwrap()
                .mixer()
                .unwrap()
                .pressed(PLACE)
        })
    }

    fn touched(&mut self, t: T) -> bool {
        let doc = self.doc;
        self.app.read_with(&self.cx, |a, _| {
            a.store
                .document(doc)
                .unwrap()
                .mixer()
                .unwrap()
                .is_touched(t)
        })
    }

    fn status(&mut self) -> Option<String> {
        let doc = self.doc;
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().status().map(String::from)
        })
    }

    fn type_into(&mut self, role: &str, label: &str, text: &str, key: &str) {
        self.click(role, label);
        assert!(
            self.element(role, label).focused,
            "{label} edits after a click"
        );
        self.cx.simulate_input(text);
        self.cx.simulate_keystrokes(key);
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn a_fader_drag_is_one_gesture(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let at = s.fader_at();
    s.press_and_drag(at, -16., Modifiers::none());
    s.move_to(point(at.x, at.y - px(30.)), Modifiers::none());
    // Off the control, the drag still follows.
    s.move_to(point(at.x + px(200.), at.y - px(40.)), Modifiers::none());
    s.up(point(at.x + px(200.), at.y - px(40.)), Modifiers::none(), 1);
    let g = s.gestures();
    assert!(g.iter().all(|(t, _, alone)| *t == FADER && !alone), "{g:?}");
    let phases = s.phases();
    assert_eq!(phases.first(), Some(&touch(None)), "{phases:?}");
    assert_eq!(phases.last(), Some(&RELEASE), "{phases:?}");
    assert_eq!(
        phases
            .iter()
            .filter(|p| matches!(p, GesturePhaseRow::Touch { .. }))
            .count(),
        1
    );
    assert_eq!(phases.iter().filter(|p| **p == RELEASE).count(), 1);
    assert_eq!(phases.len(), 2 + 4, "a value per move: {phases:?}");
    // 38 points up of a 160-point throw from 0 dB, the first 2 the drag's
    // start: +0.2375 of the throw, +12 dB at most.
    assert!((s.gain() - MixerMath::MAX_DB).abs() < 0.05, "{}", s.gain());
    assert_eq!(s.pressed(), None);
    assert!(!s.touched(FADER));
}

#[gpui::test]
fn a_bare_press_sends_nothing(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let at = s.fader_at();
    s.down(at, Modifiers::none(), 1);
    assert_eq!(s.pressed(), Some(FADER));
    s.up(at, Modifiers::none(), 1);
    // Under the least drag distance: still a press.
    s.down(at, Modifiers::none(), 1);
    s.move_to(point(at.x + px(1.), at.y - px(1.)), Modifiers::none());
    s.up(point(at.x + px(1.), at.y - px(1.)), Modifiers::none(), 1);
    let k = s.knob_at();
    s.down(k, Modifiers::none(), 1);
    s.up(k, Modifiers::none(), 1);
    assert_eq!(s.gestures(), vec![]);
    assert_eq!(s.pressed(), None);
    assert_eq!(
        s.value_of("slider", "Fader of Synth").as_deref(),
        Some("0.0 dB")
    );
}

#[gpui::test]
fn option_drags_fine(cx: &mut TestAppContext) {
    // Plain: 16 points of a 160-point throw is +0.1, +7.2 dB.
    let mut s = plain(cx);
    let at = s.fader_at();
    s.press_and_drag(at, -16., Modifiers::none());
    assert!((s.gain() - 7.2).abs() < 0.05, "{}", s.gain());
    s.up(at, Modifiers::none(), 1);
    drop(s);

    // ⌥ at the press and while dragging: ten times finer, and the fader
    // moves alone (its group stays).
    let mut s = plain(cx);
    let at = s.fader_at();
    s.press_and_drag(at, -16., alt());
    assert!((s.gain() - 0.7).abs() < 0.05, "{}", s.gain());
    s.up(at, alt(), 1);
    let g = s.gestures();
    assert_eq!(g.first(), Some(&(FADER, touch(None), true)), "{g:?}");
    assert!(g.iter().all(|x| x.2), "alone to the release: {g:?}");

    // ⌥ only after the press: fine, not alone.
    let mut s = plain(cx);
    let at = s.fader_at();
    s.down(at, Modifiers::none(), 1);
    s.move_to(point(at.x, at.y - px(2.)), alt());
    s.move_to(point(at.x, at.y - px(18.)), alt());
    assert!((s.gain() - 0.7).abs() < 0.05, "{}", s.gain());
    s.up(at, alt(), 1);
    assert!(s.gestures().iter().all(|x| !x.2), "{:?}", s.gestures());

    // The pan knob: 200 points for the range, ⌥ ten times finer.
    let mut s = plain(cx);
    let k = s.knob_at();
    s.press_and_drag(k, -20., alt());
    s.up(k, alt(), 1);
    let last = s.gestures().iter().rev().find_map(|g| match g.1 {
        GesturePhaseRow::Value { value } => Some(value),
        _ => None,
    });
    assert!((last.unwrap() - 0.51).abs() < 1e-6, "{last:?}");
    assert!(s.gestures().iter().all(|g| g.0 == PAN && !g.2));
}

#[gpui::test]
fn a_double_click_resets_without_a_touch_left_open(cx: &mut TestAppContext) {
    let fake = mixer_fake();
    fake.edit_mixer(|m| m.strips[0].gain_db = -6.);
    let mut s = setup(cx, fake);
    assert_eq!(
        s.value_of("slider", "Fader of Synth").as_deref(),
        Some("\u{2212}6.0 dB")
    );
    let at = s.fader_at();
    s.double_click(at);
    let r = reset_value(FADER);
    assert_eq!(
        s.gestures(),
        vec![
            (FADER, touch(Some(r)), false),
            (FADER, value(r), false),
            (FADER, RELEASE, false),
        ]
    );
    assert!(!s.touched(FADER));
    assert_eq!(s.pressed(), None);
    assert_eq!(
        s.value_of("slider", "Fader of Synth").as_deref(),
        Some("0.0 dB")
    );
}

#[gpui::test]
fn deactivation_releases_the_drag(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let at = s.fader_at();
    s.press_and_drag(at, -10., Modifiers::none());
    assert!(s.touched(FADER));
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    assert_eq!(s.phases().last(), Some(&RELEASE));
    assert!(!s.touched(FADER));
    assert_eq!(s.pressed(), None);
    // The rest of that drag, and its mouse-up, send nothing.
    let sent = s.gestures().len();
    s.move_to(point(at.x, at.y - px(40.)), Modifiers::none());
    s.up(point(at.x, at.y - px(40.)), Modifiers::none(), 1);
    assert_eq!(s.gestures().len(), sent, "{:?}", s.gestures());
    assert_eq!(s.phases().iter().filter(|p| **p == RELEASE).count(), 1);
}

#[gpui::test]
fn unmounting_the_control_releases_the_drag(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let at = s.fader_at();
    s.press_and_drag(at, -10., Modifiers::none());
    assert!(s.touched(FADER));
    s.window
        .update(&mut s.cx, |h, _, cx| {
            h.show_fader = false;
            cx.notify();
        })
        .unwrap();
    s.cx.run_until_parked();
    s.cx.update(|_, _| {});
    s.cx.run_until_parked();
    assert!(s.find("slider", "Fader of Synth").is_none());
    assert_eq!(s.phases().last(), Some(&RELEASE));
    assert_eq!(s.phases().iter().filter(|p| **p == RELEASE).count(), 1);
    assert!(!s.touched(FADER));
    assert_eq!(s.pressed(), None);
}

#[gpui::test]
fn the_number_box_takes_typed_values_and_refuses_bad_ones(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    s.type_into("textfield", "Level of Synth", "-4.2", "enter");
    let phases = s.phases();
    assert_eq!(phases.len(), 3, "one click: {phases:?}");
    assert!(matches!(
        phases[0],
        GesturePhaseRow::Touch { value: Some(_) }
    ));
    assert_eq!(phases[2], RELEASE);
    assert!(s.gestures().iter().all(|g| g.0 == FADER));
    let level = s.element("textfield", "Level of Synth");
    assert!(!level.focused);
    assert_eq!(level.value.as_deref(), Some("\u{2212}4.2 dB"));
    assert_eq!(
        s.value_of("slider", "Fader of Synth").as_deref(),
        Some("\u{2212}4.2 dB")
    );

    // Refused text: nothing sent, the hint in the status line.
    s.fake.clear_gestures();
    s.type_into("textfield", "Level of Synth", "4e2", "enter");
    assert_eq!(s.gestures(), vec![]);
    assert_eq!(s.status().as_deref(), Some(StripEntry::LEVEL_HINT));
    assert_eq!(
        s.value_of("textfield", "Level of Synth").as_deref(),
        Some("\u{2212}4.2 dB")
    );
    let refused = s
        .window
        .read_with(&s.cx, |h, cx| h.level.as_ref().unwrap().read(cx).refused())
        .unwrap();
    assert!(refused);

    // Escape: nothing sent, the value in use again.
    s.type_into("textfield", "Level of Synth", "-10", "escape");
    assert_eq!(s.gestures(), vec![]);
    let level = s.element("textfield", "Level of Synth");
    assert!(!level.focused);
    assert_eq!(level.value.as_deref(), Some("\u{2212}4.2 dB"));

    // A pan.
    s.type_into("textfield", "Pan of Synth", "L 24", "enter");
    assert!(s.gestures().iter().all(|g| g.0 == PAN));
    assert_eq!(s.phases().len(), 3);
    assert_eq!(
        s.value_of("textfield", "Pan of Synth").as_deref(),
        Some("L 24")
    );
    assert_eq!(
        s.value_of("slider", "Pan knob of Synth").as_deref(),
        Some("L 24")
    );
    s.fake.clear_gestures();
    s.type_into("textfield", "Pan of Synth", "Q", "enter");
    assert_eq!(s.gestures(), vec![]);
    assert_eq!(s.status().as_deref(), Some(StripEntry::PAN_HINT));
}

#[gpui::test]
fn a_box_edits_while_the_transport_keys_wait(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let doc = s.doc;
    s.click("textfield", "Level of Synth");
    let keys = s.app.read_with(&s.cx, |a, _| {
        a.store.document(doc).unwrap().transport_keys_enabled()
    });
    assert!(!keys);
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    let keys = s.app.read_with(&s.cx, |a, _| {
        a.store.document(doc).unwrap().transport_keys_enabled()
    });
    assert!(keys);
}

#[gpui::test]
fn the_knob_centres_on_double_click(cx: &mut TestAppContext) {
    let fake = mixer_fake();
    fake.edit_mixer(|m| m.strips[0].pan = 0.3);
    let mut s = setup(cx, fake);
    assert_eq!(
        s.value_of("slider", "Pan knob of Synth").as_deref(),
        Some("R 30")
    );
    let k = s.knob_at();
    s.double_click(k);
    assert_eq!(
        s.gestures(),
        vec![
            (PAN, touch(Some(0.5)), false),
            (PAN, value(0.5), false),
            (PAN, RELEASE, false),
        ]
    );
    assert!(!s.touched(PAN));
    assert_eq!(
        s.value_of("slider", "Pan knob of Synth").as_deref(),
        Some("C")
    );
}

#[gpui::test]
fn a_focused_fader_steps_with_the_arrow_keys(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let at = s.fader_at();
    s.down(at, Modifiers::none(), 1);
    s.up(at, Modifiers::none(), 1);
    assert!(s.element("slider", "Fader of Synth").focused);
    s.cx.simulate_keystrokes("up");
    s.cx.run_until_parked();
    assert_eq!(
        s.phases().iter().filter(|p| **p == RELEASE).count(),
        1,
        "{:?}",
        s.gestures()
    );
    assert!((s.gain() - 0.7).abs() < 0.05, "{}", s.gain());
    s.cx.simulate_keystrokes("shift-down");
    s.cx.run_until_parked();
    assert!((s.gain() + 6.5).abs() < 0.05, "{}", s.gain());
    assert!(s.gestures().iter().all(|g| g.0 == FADER));
}

#[gpui::test]
fn controls_are_in_the_tree_with_their_values(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let want = [
        ("slider", "Fader of Synth", "0.0 dB"),
        ("slider", "Pan knob of Synth", "C"),
        ("textfield", "Level of Synth", "0.0 dB"),
        ("textfield", "Pan of Synth", "C"),
        ("button", "Mute Synth", "off"),
        ("button", "Solo Synth", "off"),
        ("menu", "Automation of Synth", "Read"),
    ];
    for (role, label, v) in want {
        let e = s.element(role, label);
        assert_eq!(e.value.as_deref(), Some(v), "{label}");
        assert!(e.enabled, "{label}");
        assert!(e.bounds.size.width > px(0.), "{label}");
    }
    // The fader is its scale and cap column, its throw and cap high.
    let f = s.element("slider", "Fader of Synth").bounds;
    assert_eq!(f.size, size(px(60.), px((THROW + 42.) as f32)));
    assert_eq!(
        s.element("slider", "Pan knob of Synth").bounds.size,
        size(px(38.), px(38.))
    );
    assert_eq!(
        s.element("button", "Mute Synth").bounds.size,
        size(px(24.), px(24.))
    );
    // Mute and solo are clicks on the core; the toggles follow.
    s.click("button", "Mute Synth");
    assert_eq!(
        s.gestures(),
        vec![
            (T::StripMute { strip: 1 }, touch(Some(1.)), false),
            (T::StripMute { strip: 1 }, value(1.), false),
            (T::StripMute { strip: 1 }, RELEASE, false),
        ]
    );
    assert_eq!(s.value_of("button", "Mute Synth").as_deref(), Some("on"));
    s.click("button", "Solo Synth");
    assert_eq!(s.value_of("button", "Solo Synth").as_deref(), Some("on"));
    // The meter is not in the tree.
    assert!(s.elements().iter().all(|e| !e.label.contains("eter")));
}

/// Dispatches `events` in one update: no frame is drawn between them.
fn dispatch_unframed(s: &mut Setup, events: Vec<gpui::PlatformInput>) {
    s.cx.update(|window, cx| {
        for e in events {
            window.dispatch_event(e, cx);
        }
    });
    s.cx.run_until_parked();
}

fn left_down(at: Point<Pixels>) -> gpui::PlatformInput {
    MouseDownEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    }
    .to_platform_input()
}

fn left_up(at: Point<Pixels>) -> gpui::PlatformInput {
    MouseUpEvent {
        button: MouseButton::Left,
        position: at,
        modifiers: Modifiers::none(),
        click_count: 1,
    }
    .to_platform_input()
}

#[gpui::test]
fn a_mouse_up_before_the_next_frame_still_lifts(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let at = s.fader_at();
    dispatch_unframed(&mut s, vec![left_down(at), left_up(at)]);
    assert_eq!(s.pressed(), None);
    // A hover afterwards drags nothing.
    s.cx.simulate_event(MouseMoveEvent {
        position: point(at.x, at.y - px(20.)),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    s.cx.run_until_parked();
    assert_eq!(s.gestures(), vec![]);
    assert_eq!(s.pressed(), None);
    assert!(!s.touched(FADER));
}

#[gpui::test]
fn a_move_without_the_button_lifts_the_press(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let at = s.fader_at();
    s.press_and_drag(at, -10., Modifiers::none());
    assert!(s.touched(FADER));
    // The mouse-up went somewhere else (another app, say).
    s.cx.simulate_event(MouseMoveEvent {
        position: point(at.x, at.y - px(40.)),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    s.cx.run_until_parked();
    assert_eq!(s.phases().last(), Some(&RELEASE));
    assert_eq!(s.pressed(), None);
    assert!(!s.touched(FADER));
}

#[gpui::test]
fn a_press_cancelled_elsewhere_forgets_its_origin(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let at = s.fader_at();
    s.down(at, Modifiers::none(), 1);
    let origin = |s: &mut Setup| {
        s.window
            .read_with(&s.cx, |h, cx| {
                h.fader_state.as_ref().unwrap().read(cx).origin()
            })
            .unwrap()
    };
    assert_eq!(origin(&mut s), Some(at));
    let doc = s.doc;
    s.app.update(&mut s.cx, |a, cx| {
        a.store.with_document(doc, |d, _| {
            d.with_mixer(|m, d| m.cancel_drags(d, PLACE, Instant::now()));
        });
        cx.notify();
    });
    s.cx.run_until_parked();
    assert_eq!(s.pressed(), None);
    assert_eq!(origin(&mut s), None);
}

#[gpui::test]
fn a_box_dropped_mid_edit_gives_the_keys_back(cx: &mut TestAppContext) {
    let mut s = plain(cx);
    let doc = s.doc;
    let keys = |s: &mut Setup| {
        s.app.read_with(&s.cx, |a, _| {
            a.store.document(doc).unwrap().transport_keys_enabled()
        })
    };
    s.click("textfield", "Level of Synth");
    assert!(!keys(&mut s));
    s.cx.simulate_input("-3");
    s.window
        .update(&mut s.cx, |h, _, cx| {
            h.level = None;
            cx.notify();
        })
        .unwrap();
    s.cx.run_until_parked();
    s.cx.update(|_, _| {});
    s.cx.run_until_parked();
    assert!(s.find("textfield", "Level of Synth").is_none());
    assert!(keys(&mut s));
    assert_eq!(s.gestures(), vec![], "a dropped edit commits nothing");
}
