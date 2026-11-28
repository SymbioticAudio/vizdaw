//! The Strip Chart in the bottom pane and in the right pane's Controllers
//! view (Swift `S/StripChartPane.swift`, `S/PaneViews.swift:223-235`):
//! Editors › Strip Chart (⌘3), its bar (Mode, Operation, Shape, Curve,
//! Wave, Period Kind, Period, Phase, Step, Track and Rows), the settings
//! saved with the project and a saved last editor, driven through GPUI's
//! test platform with the scripted session.

use std::sync::Arc;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, TestAppContext,
    VisualTestContext, WindowHandle,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_app::strip_chart::{ChartSlot, MAX_ROWS};
use vizdaw_app::testing::{FakeSession, demo_fake, roll_fake};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow, RunCommand};

const CHART: &str = "Strip Chart";
const PANE: &str = "Bottom Pane";
const RIGHT: &str = "Right Pane";
const CONTROLLERS_NOTE: &str = "Select a track to see its controllers.";

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
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
    let mut s = Setup {
        fake,
        app,
        window,
        cx: vcx,
    };
    // A 360-point bottom pane, as the roll's tests have.
    s.update_model(|d| d.update_layout(|l| l.views.current_mut().bottom.size = 360.));
    s
}

/// The chart on `target`.
fn chart_on(cx: &mut TestAppContext, target: EditorTarget) -> Setup {
    let mut s = setup(cx, roll_fake());
    s.update_model(|d| d.open_detail(target, Some(DetailEditorKind::StripChart)));
    s.cx.run_until_parked();
    s
}

fn chords_chart(cx: &mut TestAppContext) -> Setup {
    chart_on(cx, EditorTarget::Lane { seq: 3, lane: 31 })
}

impl Setup {
    fn doc(&self) -> vizdaw_app::DocId {
        self.window.read_with(&self.cx, |v, _| v.doc()).unwrap()
    }

    fn model<R>(&mut self, f: impl FnOnce(&vizdaw_app::DocumentModel) -> R) -> R {
        let doc = self.doc();
        self.app
            .read_with(&self.cx, |a, _| f(a.store.document(doc).unwrap()))
    }

    fn update_model(&mut self, f: impl FnOnce(&mut vizdaw_app::DocumentModel)) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| f(d));
            cx.notify();
        });
        self.cx.run_until_parked();
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

    fn inside(&mut self, outer: &Element) -> Vec<Element> {
        self.elements()
            .into_iter()
            .filter(|e| e.path.len() > outer.path.len() && e.path.starts_with(&outer.path))
            .collect()
    }

    /// The `role` labelled `label` in the pane group `pane`.
    fn in_pane(&mut self, pane: &str, role: &str, label: &str) -> Option<Element> {
        let p = self.find("group", pane)?;
        self.inside(&p)
            .into_iter()
            .find(|e| e.role == role && e.label == label)
    }

    /// The `role` labelled `label` in the bottom pane.
    fn control(&mut self, role: &str, label: &str) -> Element {
        self.in_pane(PANE, role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in the bottom pane"))
    }

    fn has(&mut self, role: &str, label: &str) -> bool {
        self.in_pane(PANE, role, label).is_some()
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }

    fn click_at(&mut self, position: gpui::Point<Pixels>) {
        self.cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let e = self.control(role, label);
        self.click_at(e.bounds.center());
    }

    /// Opens picker `menu` and picks `item`.
    fn pick(&mut self, menu: &str, item: &str) {
        self.click("menu", menu);
        let e = self
            .find("menuitem", item)
            .unwrap_or_else(|| panic!("no menuitem {item:?} in {menu}"));
        self.click_at(e.bounds.center());
    }

    fn value(&mut self, role: &str, label: &str) -> Option<String> {
        self.control(role, label).value
    }

    fn run_menu(&mut self, id: &str) {
        self.cx.dispatch_action(RunCommand::new(id.to_owned()));
        self.cx.run_until_parked();
    }

    fn saved_layout(&mut self, name: &str) -> serde_json::Value {
        let path = std::env::temp_dir().join(name);
        let doc = self.doc();
        self.app
            .update(&mut self.cx, |a, _| a.save_as(doc, &path))
            .expect("saved");
        serde_json::from_str(&self.fake.layout().expect("a stored layout")).unwrap()
    }
}

#[gpui::test]
fn editors_strip_chart_opens_the_chart(cx: &mut TestAppContext) {
    let mut s = setup(cx, roll_fake());
    assert!(s.find("group", CHART).is_none());
    s.keys("secondary-3");
    assert_eq!(s.model(|d| d.detail_kind_name()), "strip_chart");
    assert!(s.in_pane(PANE, "group", CHART).is_some(), "the chart shows");
    assert!(s.has("text", "Rows: 1"));
    assert_eq!(s.value("menu", "Shape").as_deref(), Some("Straight"));
    // Swift's default step, a 32nd (`M/StripChart.swift:92,240`).
    assert_eq!(s.value("menu", "Step").as_deref(), Some("1/32"));
    assert!(s.control("button", "Draw").selected);
    assert!(!s.control("button", "Modify").selected);
}

#[gpui::test]
fn the_bar_shows_only_the_pickers_its_mode_uses(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    assert!(!s.has("menu", "Operation"), "Draw has no operation");
    s.click("button", "Erase");
    assert!(!s.has("menu", "Shape"), "Erase draws no shape");
    assert!(s.has("menu", "Step"));
    s.click("button", "Modify");
    assert!(s.has("menu", "Operation"));
    assert!(s.has("menu", "Shape"));
    s.pick("Operation", "Compress");
    assert_eq!(s.value("menu", "Operation").as_deref(), Some("Compress"));
    assert!(!s.has("menu", "Shape"), "Compress draws no shape");
    s.click("button", "Draw");
    assert!(!s.has("menu", "Curve"));
    s.pick("Shape", "Curved");
    assert_eq!(s.value("menu", "Curve").as_deref(), Some("Ease Out"));
    s.pick("Shape", "Oscillator");
    assert!(!s.has("menu", "Curve"));
    for picker in ["Wave", "Period Kind", "Period", "Phase"] {
        assert!(s.has("menu", picker), "{picker}");
    }
    assert_eq!(s.value("menu", "Period").as_deref(), Some("1/4"));
    s.pick("Period Kind", "Clock");
    assert_eq!(s.value("menu", "Period").as_deref(), Some("500 ms"));
    s.pick("Phase", "90°");
    assert_eq!(s.model(|d| d.strip_settings().phase), 250);
}

#[gpui::test]
fn bar_changes_are_saved_with_the_project(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    s.click("button", "Modify");
    let saved = s.saved_layout("vizdaw-strip-chart-bar.vizdaw");
    assert_eq!(saved["stripChart"]["mode"], "modify");
}

#[gpui::test]
fn a_sequence_target_shows_the_track_menu(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    assert!(!s.has("menu", "Track"), "a track target has no menu");
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Sequence { seq: 3 },
            Some(DetailEditorKind::StripChart),
        )
    });
    assert_eq!(s.value("menu", "Track").as_deref(), Some("Groove"));
    s.pick("Track", "Bass");
    assert_eq!(s.model(|d| d.strip_lane(ChartSlot::Pane)), Some((3, 32)));
    assert_eq!(s.value("menu", "Track").as_deref(), Some("Bass"));
}

#[gpui::test]
fn the_rows_stepper_adds_and_removes_rows(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    assert!(!s.control("button", "Decrease Rows").enabled);
    assert!(s.control("button", "Increase Rows").enabled);
    s.click("button", "Increase Rows");
    assert!(s.has("text", "Rows: 2"));
    assert_eq!(s.model(|d| d.strip_rows(ChartSlot::Pane).len()), 2);
    for _ in 2..MAX_ROWS {
        s.click("button", "Increase Rows");
    }
    assert!(s.has("text", &format!("Rows: {MAX_ROWS}")));
    assert!(!s.control("button", "Increase Rows").enabled);
    s.click("button", "Increase Rows");
    assert_eq!(s.model(|d| d.strip_rows(ChartSlot::Pane).len()), MAX_ROWS);
    s.click("button", "Decrease Rows");
    assert_eq!(
        s.model(|d| d.strip_rows(ChartSlot::Pane).len()),
        MAX_ROWS - 1
    );
    assert!(s.control("button", "Increase Rows").enabled);
}

#[gpui::test]
fn the_controllers_pane_shows_the_focused_tracks_chart(cx: &mut TestAppContext) {
    let mut s = setup(cx, roll_fake());
    s.run_menu("ui.toggle_right");
    assert!(s.in_pane(RIGHT, "text", CONTROLLERS_NOTE).is_some());
    assert!(s.in_pane(RIGHT, "group", CHART).is_none());
    s.update_model(|d| d.select_track(31, false));
    assert!(s.in_pane(RIGHT, "text", CONTROLLERS_NOTE).is_none());
    assert!(s.in_pane(RIGHT, "group", CHART).is_some(), "the chart");
    assert!(
        s.in_pane(RIGHT, "text", "Rows: 1").is_some(),
        "with its bar"
    );
    // The bottom pane's chart is not open.
    assert!(s.in_pane(PANE, "group", CHART).is_none());
}

/// A song saved with the Strip Chart as its last editor: a block's
/// double-click opens it.
#[gpui::test]
fn a_saved_strip_chart_last_editor_opens_the_chart(cx: &mut TestAppContext) {
    let fake = roll_fake();
    fake.set_layout(Some(r#"{"lastKind":"stripChart"}"#));
    let mut s = setup(cx, fake);
    let block = s
        .find("button", "Block on Chords, bar 1, 1 event")
        .expect("the block")
        .bounds;
    let at = block.center();
    for count in [1, 2] {
        s.cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: count,
            first_mouse: false,
        });
        s.cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: count,
        });
    }
    s.cx.run_until_parked();
    assert_eq!(s.model(|d| d.detail_kind_name()), "strip_chart");
    assert!(s.in_pane(PANE, "group", CHART).is_some());
}
