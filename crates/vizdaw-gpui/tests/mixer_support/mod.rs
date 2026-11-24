//! The Mix view through GPUI's test platform with a scripted session:
//! what the group, VCA and MIDI strip tests share.

#![allow(dead_code)]

use std::sync::Arc;

use gpui::{
    Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, TestAppContext, VisualTestContext, WindowHandle, point, px,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::mixer::MixerPlacement;
use vizdaw_app::testing::{FakeSession, mixer_fake};
use vizdaw_ffi::automation::GesturePhaseRow;
use vizdaw_ffi::events::AutomationTargetRow as T;
use vizdaw_gpui::controls;
use vizdaw_gpui::mixer_view::{MixerView, POLL_EVERY};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow, RunCommand};

pub struct Setup {
    pub fake: Arc<FakeSession>,
    pub app: Entity<AppState>,
    pub window: WindowHandle<DocumentWindow>,
    pub cx: VisualTestContext,
}

/// The demo window on `f`, in the Mix view, with no calls logged yet.
pub fn setup(cx: &mut TestAppContext, f: FakeSession) -> Setup {
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
    pub fn doc(&self) -> vizdaw_app::DocId {
        self.window.read_with(&self.cx, |v, _| v.doc()).unwrap()
    }

    pub fn model<R>(&mut self, f: impl FnOnce(&vizdaw_app::DocumentModel) -> R) -> R {
        let doc = self.doc();
        self.app
            .read_with(&self.cx, |a, _| f(a.store.document(doc).unwrap()))
    }

    pub fn view(&mut self) -> Entity<MixerView> {
        self.window
            .read_with(&self.cx, |v, _| v.mixer_view(MixerPlacement::View))
            .unwrap()
            .expect("the Mix view's mixer")
    }

    pub fn elements(&mut self) -> Vec<Element> {
        self.window
            .read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    }

    pub fn find(&mut self, role: &str, label: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == role && e.label == label)
    }

    pub fn all(&mut self, role: &str, label: &str) -> Vec<Element> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == role && e.label == label)
            .collect()
    }

    pub fn element(&mut self, role: &str, label: &str) -> Element {
        let all = self.elements();
        self.find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {all:#?}"))
    }

    pub fn value(&mut self, role: &str, label: &str) -> Option<String> {
        self.element(role, label).value
    }

    /// The faders' labels, left to right.
    pub fn fader_labels(&mut self) -> Vec<String> {
        let mut f: Vec<Element> = self
            .elements()
            .into_iter()
            .filter(|e| e.role == "slider" && e.label.starts_with("Fader of "))
            .collect();
        f.sort_by(|a, b| a.bounds.origin.x.partial_cmp(&b.bounds.origin.x).unwrap());
        f.into_iter().map(|e| e.label).collect()
    }

    /// Every fader's top is the same.
    pub fn faders_line_up(&mut self) {
        let ys: Vec<f32> = self
            .elements()
            .into_iter()
            .filter(|e| e.role == "slider" && e.label.starts_with("Fader of "))
            .map(|e| f32::from(e.bounds.origin.y))
            .collect();
        assert!(ys.len() > 1);
        assert!(ys.iter().all(|y| *y == ys[0]), "{ys:?}");
    }

    /// The open menus' items: (title, enabled, checked).
    pub fn items(&mut self) -> Vec<(String, bool, bool)> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "menuitem")
            .map(|e| (e.label, e.enabled, e.selected))
            .collect()
    }

    pub fn titles(&mut self) -> Vec<String> {
        self.items().into_iter().map(|i| i.0).collect()
    }

    pub fn click_at(&mut self, at: Point<Pixels>) {
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    pub fn click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.click_at(at);
    }

    pub fn double_click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        for count in [1, 2] {
            self.down_with(at, count, Modifiers::none());
            self.up(at, count);
        }
    }

    pub fn right_click(&mut self, role: &str, label: &str) {
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

    /// The middle of the cap column of the fader labelled `label`.
    pub fn fader_at(&mut self, label: &str) -> Point<Pixels> {
        let b = self.element("slider", label).bounds;
        point(
            b.origin.x + px((controls::fader::SCALE_WIDTH + controls::fader::COLUMN / 2.) as f32),
            b.center().y,
        )
    }

    pub fn down_with(&mut self, at: Point<Pixels>, count: usize, modifiers: Modifiers) {
        self.cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers,
            click_count: count,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    pub fn down(&mut self, at: Point<Pixels>) {
        self.down_with(at, 1, Modifiers::none());
    }

    pub fn move_to(&mut self, at: Point<Pixels>, pressed: bool) {
        self.cx.simulate_event(MouseMoveEvent {
            position: at,
            pressed_button: pressed.then_some(MouseButton::Left),
            modifiers: Modifiers::none(),
        });
        self.cx.run_until_parked();
    }

    pub fn up(&mut self, at: Point<Pixels>, count: usize) {
        self.cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::none(),
            click_count: count,
        });
        self.cx.run_until_parked();
    }

    /// Down at `at`, 30 points up in two moves, then up.
    pub fn drag_up(&mut self, at: Point<Pixels>) {
        self.down(at);
        self.move_to(point(at.x, at.y - px(10.)), true);
        self.move_to(point(at.x, at.y - px(30.)), true);
        self.up(point(at.x, at.y - px(30.)), 1);
    }

    /// Down and up at `at` with no frame between them.
    pub fn press_and_release_in_one_frame(&mut self, at: Point<Pixels>) {
        self.cx.simulate_event(MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        self.cx.simulate_event(MouseUpEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::none(),
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    /// The phases the session took for `t`.
    pub fn of(&self, t: T) -> Vec<GesturePhaseRow> {
        self.fake
            .gestures()
            .into_iter()
            .filter(|g| g.0 == t)
            .map(|g| g.1)
            .collect()
    }

    /// Every `run_mixer`: its id and its arguments as `name=value`.
    pub fn mixer_calls(&self) -> Vec<(String, Vec<String>)> {
        self.fake
            .run_mixers()
            .into_iter()
            .map(|(id, args)| {
                let args = args
                    .into_iter()
                    .map(|a| {
                        let v = match a.value {
                            vizdaw_ffi::control::ControlValue::Int { value } => value.to_string(),
                            vizdaw_ffi::control::ControlValue::Text { value } => value,
                            vizdaw_ffi::control::ControlValue::Flag { value } => value.to_string(),
                        };
                        format!("{}={v}", a.name)
                    })
                    .collect();
                (id, args)
            })
            .collect()
    }

    pub fn status(&mut self) -> Option<String> {
        self.model(|d| d.status().map(String::from))
    }

    /// One poll period passes.
    pub fn period(&mut self) {
        self.cx.executor().advance_clock(POLL_EVERY);
        self.cx.run_until_parked();
    }

    /// Whether a press is open in the Mix view (a strip's, a VCA's or a
    /// MIDI strip's).
    pub fn pressed(&mut self) -> bool {
        self.model(|d| {
            d.mixer().is_some_and(|m| {
                m.pressed(MixerPlacement::View).is_some()
                    || m.midi_pressed(MixerPlacement::View).is_some()
            })
        })
    }

    /// The mixer's strips area.
    pub fn area(&mut self) -> Bounds<Pixels> {
        self.element("group", "Mixer").bounds
    }

    /// Sets the status line, so the mixer's area takes its height.
    pub fn show_status(&mut self, text: &str) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store
                .with_document(doc, |d, _| d.set_status(Some(text.into())));
            cx.notify();
        });
        self.cx.run_until_parked();
    }
}

impl Setup {
    /// Clears the status line.
    pub fn show_status_cleared(&mut self) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| d.set_status(None));
            cx.notify();
        });
        self.cx.run_until_parked();
    }
}
