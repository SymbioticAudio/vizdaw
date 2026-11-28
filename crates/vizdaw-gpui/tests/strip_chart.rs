//! The Strip Chart in the bottom pane and in the right pane's Controllers
//! view (Swift `S/StripChartPane.swift`, `S/PaneViews.swift:223-235`):
//! Editors › Strip Chart (⌘3), its bar (Mode, Operation, Shape, Curve,
//! Wave, Period Kind, Period, Phase, Step, Track and Rows), the settings
//! saved with the project and a saved last editor, driven through GPUI's
//! test platform with the scripted session.

use std::sync::Arc;

use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, ScrollDelta, ScrollEvent,
    TestAppContext, TouchPhase, VisualTestContext, WindowHandle, point, px,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::arrangement::RULER_HEIGHT;
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, PaneView};
use vizdaw_app::strip_chart::{
    ChartSlot, MAX_ROWS, StripLane, StripMode, StripOp, StripScale, erase_command, line_command,
};
use vizdaw_app::testing::{FakeSession, demo_fake, mixer_fake, roll_fake};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::EditCommand;
use vizdaw_ffi::events::AutomationTargetRow;
use vizdaw_ffi::modify::{ModifyRow, SelectModeRow};
use vizdaw_ffi::strip::{LineOpRow, LineRow, LineShapeRow};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow, RunCommand};

const CHART: &str = "Strip Chart";
const PANE: &str = "Bottom Pane";
const RIGHT: &str = "Right Pane";
const CONTROLLERS_NOTE: &str = "Select a track to see its controllers.";
/// A quarter.
const Q: i64 = 15_360;
const PANE_SLOT: ChartSlot = ChartSlot::Pane;
/// Swift presses at x 0; here 4 points in (tick 384, which the quarter
/// snap takes to 0): the canvas's first 3 points are under the left
/// pane's divider handle.
const START_X: f32 = 4.;

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

    /// Window point of chart point `(x, y)` (below the ruler) in the
    /// chart of pane group `pane`.
    fn at_in(&mut self, pane: &str, x: f32, y: f32) -> gpui::Point<Pixels> {
        let o = self
            .in_pane(pane, "group", CHART)
            .unwrap_or_else(|| panic!("no chart in {pane}"))
            .bounds
            .origin;
        point(o.x + px(x), o.y + px(RULER_HEIGHT + y))
    }

    /// The bottom pane's chart point `(x, y)`.
    fn at(&mut self, x: f32, y: f32) -> gpui::Point<Pixels> {
        self.at_in(PANE, x, y)
    }

    fn down(&mut self, at: gpui::Point<Pixels>, m: Modifiers) {
        self.cx.simulate_mouse_down(at, MouseButton::Left, m);
        self.cx.run_until_parked();
    }

    fn move_to(&mut self, at: gpui::Point<Pixels>, m: Modifiers) {
        self.cx.simulate_mouse_move(at, Some(MouseButton::Left), m);
        self.cx.run_until_parked();
    }

    fn up(&mut self, at: gpui::Point<Pixels>, m: Modifiers) {
        self.cx.simulate_mouse_up(at, MouseButton::Left, m);
        self.cx.run_until_parked();
    }

    fn drag(&mut self, from: gpui::Point<Pixels>, to: gpui::Point<Pixels>, m: Modifiers) {
        self.down(from, m);
        self.move_to(to, m);
        self.up(to, m);
    }

    /// The wheel over the bottom pane's chart, `dx` points across.
    fn wheel(&mut self, dx: f32) {
        let at = self.at(400., 40.);
        self.cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(dx), px(0.))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
            source: Default::default(),
        });
        self.cx.run_until_parked();
    }

    /// The labels of the values the bottom pane's chart lists.
    fn value_labels(&mut self) -> Vec<String> {
        let chart = self.control("group", CHART);
        self.inside(&chart)
            .into_iter()
            .filter(|e| e.role == "button")
            .map(|e| e.label)
            .collect()
    }

    /// Opens row `row`'s menu (its title, `menu` "Row ‹n›").
    fn open_row_menu(&mut self, row: usize) {
        let e = self.control("menu", &format!("Row {}", row + 1));
        self.click_at(e.bounds.center());
    }

    fn menuitem(&mut self, label: &str) -> Element {
        self.find("menuitem", label)
            .unwrap_or_else(|| panic!("no menuitem {label:?}"))
    }

    fn last_call(&self) -> Option<String> {
        self.fake.calls().last().cloned()
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

// MARK: Strokes, the row menu and the tree (Swift `T/StripChartViewTests.swift`)

/// Row 0's scale: one velocity row, 96 points tall.
fn velocity_scale() -> StripScale {
    StripScale::new(StripLane::Velocity.range(), 0., 96.)
}

#[gpui::test]
fn a_drag_in_draw_mode_sets_velocities_along_the_line(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    s.fake.clear_calls();
    let (from, to) = (s.at(START_X, 30.), s.at(160., 48.));
    s.drag(from, to, Modifiers::none());
    let sc = velocity_scale();
    let line = LineRow {
        from: 0,
        to: Q,
        start: sc.value_at(30.),
        end: sc.value_at(48.),
        step: Q / 8,
        shape: LineShapeRow::Straight,
    };
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&line_command(
            StripLane::Velocity,
            LineOpRow::Set,
            line,
            false
        ))
    );
    let calls = s.fake.calls();
    assert_eq!(
        calls.first().map(String::as_str),
        Some("begin_edit(Change Velocity)")
    );
    assert_eq!(calls.last().map(String::as_str), Some("end_edit"));
    assert!(s.model(|d| d.strip_stroke(PANE_SLOT).is_none()));
}

#[gpui::test]
fn a_click_in_draw_mode_draws_one_point(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    let at = s.at(165., 48.);
    s.down(at, Modifiers::none());
    s.up(at, Modifiers::none());
    match s.fake.gesture_edits().last() {
        Some(EditCommand::SelectAndModify {
            modify: Some(ModifyRow::Line { line, .. }),
            ..
        }) => assert_eq!((line.from, line.to), (Q, Q), "snapped to the quarter"),
        other => panic!("no line: {other:?}"),
    }
}

#[gpui::test]
fn escape_leaves_no_undo_step(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    let (from, to) = (s.at(START_X, 30.), s.at(160., 48.));
    s.down(from, Modifiers::none());
    s.move_to(to, Modifiers::none());
    assert!(s.model(|d| d.gesture().is_some()));
    s.keys("escape");
    assert_eq!(s.last_call().as_deref(), Some("abort_edit"));
    assert!(s.model(|d| d.gesture().is_none() && d.strip_stroke(PANE_SLOT).is_none()));
    // The release that follows sends nothing.
    let n = s.fake.calls().len();
    s.up(to, Modifiers::none());
    assert_eq!(s.fake.calls().len(), n);
}

#[gpui::test]
fn modify_mode_reads_the_operations_amount(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    s.update_model(|d| {
        d.update_strip_settings(|x| {
            x.mode = StripMode::Modify;
            x.op = StripOp::Scale;
        })
    });
    // Right of the row's title (a press there opens its menu).
    let (from, to) = (s.at(220., 3.), s.at(330., 93.));
    s.drag(from, to, Modifiers::alt());
    match s.fake.gesture_edits().last() {
        Some(EditCommand::SelectAndModify {
            mode,
            modify: Some(ModifyRow::Line { op, line, .. }),
            ..
        }) => {
            assert_eq!((*op, *mode), (LineOpRow::Scale, SelectModeRow::Refine));
            assert_eq!((line.start, line.end), (200, 0), "top 200 %, bottom 0 %");
            assert_eq!((line.from, line.to), (Q, 2 * Q));
        }
        other => panic!("no line: {other:?}"),
    }
}

#[gpui::test]
fn an_erase_drag_deletes_the_rows_series(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    s.update_model(|d| {
        d.set_strip_row(PANE_SLOT, 0, StripLane::Controller(7));
        d.update_strip_settings(|x| x.mode = StripMode::Erase);
    });
    s.fake.clear_calls();
    let (from, to) = (s.at(220., 30.), s.at(320., 40.));
    s.drag(from, to, Modifiers::none());
    assert_eq!(
        s.fake.gesture_edits().last(),
        erase_command(StripLane::Controller(7), Q, 2 * Q).as_ref()
    );
    let calls = s.fake.calls();
    assert_eq!(
        calls.first().map(String::as_str),
        Some("begin_edit(Erase Events)")
    );
    assert_eq!(calls.last().map(String::as_str), Some("end_edit"));
}

/// Review Focus 1.
#[gpui::test]
fn escape_before_the_dead_zone_sends_nothing(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    s.fake.clear_calls();
    let (from, to) = (s.at(220., 30.), s.at(221., 31.));
    s.down(from, Modifiers::none());
    s.move_to(to, Modifiers::none());
    s.keys("escape");
    let edits: Vec<_> = s
        .fake
        .calls()
        .into_iter()
        .filter(|c| c.contains("edit"))
        .collect();
    assert!(edits.is_empty(), "{edits:?}");
    assert!(s.model(|d| d.gesture().is_none() && d.strip_stroke(PANE_SLOT).is_none()));
}

/// A stroke held at `(220, 30)` → `(320, 48)` in the bottom pane's chart,
/// its gesture open.
fn mid_stroke(s: &mut Setup) -> gpui::Point<Pixels> {
    let (from, to) = (s.at(220., 30.), s.at(320., 48.));
    s.down(from, Modifiers::none());
    s.move_to(to, Modifiers::none());
    assert!(s.model(|d| d.gesture().is_some()));
    to
}

/// Review Focus 1: the detail switched to another editor, and the bottom
/// pane switched to another view while its detail stays a Strip Chart.
#[gpui::test]
fn leaving_the_pane_mid_stroke_leaves_no_undo_step(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    let to = mid_stroke(&mut s);
    s.update_model(|d| d.show_detail(DetailEditorKind::List));
    assert_eq!(s.last_call().as_deref(), Some("abort_edit"));
    assert!(s.model(|d| d.gesture().is_none() && d.strip_stroke(PANE_SLOT).is_none()));
    let n = s.fake.calls().len();
    s.up(to, Modifiers::none());
    assert_eq!(s.fake.calls().len(), n, "the release sends nothing");
}

#[gpui::test]
fn another_bottom_view_mid_stroke_leaves_no_undo_step(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    let to = mid_stroke(&mut s);
    s.update_model(|d| d.update_layout(|l| l.views.current_mut().bottom.view = PaneView::Mixer));
    assert_eq!(
        s.model(|d| d.detail_kind_name()),
        "strip_chart",
        "the detail stays"
    );
    assert_eq!(s.last_call().as_deref(), Some("abort_edit"));
    assert!(s.model(|d| d.gesture().is_none() && d.strip_stroke(PANE_SLOT).is_none()));
    let n = s.fake.calls().len();
    s.up(to, Modifiers::none());
    assert_eq!(s.fake.calls().len(), n, "the release sends nothing");
}

/// Review Focus 1: the window losing the keyboard.
#[gpui::test]
fn losing_the_window_mid_stroke_leaves_no_undo_step(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    mid_stroke(&mut s);
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    assert_eq!(s.last_call().as_deref(), Some("abort_edit"));
    assert!(s.model(|d| d.gesture().is_none() && d.strip_stroke(PANE_SLOT).is_none()));
}

/// `roll_fake`'s notes have velocity `0x8000` (64), as Swift's fake.
#[gpui::test]
fn the_tree_reads_each_value(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    assert_eq!(
        s.value_labels(),
        ["Velocity 64, bar 1 beat 1", "Velocity 64, bar 1 beat 2"]
    );
    assert_eq!(s.value("menu", "Row 1").as_deref(), Some("Velocity"));
}

#[gpui::test]
fn the_tree_reads_the_values_on_screen(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    // Two quarters on (320 points at 96 ticks a point): neither note.
    s.wheel(-320.);
    assert_eq!(
        s.window
            .read_with(&s.cx, |v, _| v.strip_chart(PANE_SLOT).axis().origin())
            .unwrap(),
        2 * Q
    );
    assert!(s.value_labels().is_empty());
    // Back to half a quarter: only the second.
    s.wheel(240.);
    assert_eq!(s.value_labels(), ["Velocity 64, bar 1 beat 2"]);
}

#[gpui::test]
fn the_row_menu_lists_every_kind(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    s.open_row_menu(0);
    for t in [
        "Velocity",
        "Release Velocity",
        "Length",
        "Controller",
        "Pitch Bend",
        "Channel Pressure",
        "Poly Pressure",
        "Program",
        "Tempo",
        "Automation",
        "Add Row",
        "Remove Row",
    ] {
        assert!(s.find("menuitem", t).is_some(), "{t}");
    }
    assert!(
        s.menuitem("Velocity").selected,
        "the current lane is checked"
    );
    assert!(s.menuitem("Add Row").enabled);
    assert!(!s.menuitem("Remove Row").enabled, "one row stays");
    assert!(
        s.find("menuitem", "MIDI Learn").is_none(),
        "not an automation row"
    );
    let tempo = s.menuitem("Tempo").bounds.center();
    s.click_at(tempo);
    assert_eq!(s.model(|d| d.strip_rows(PANE_SLOT)), [StripLane::Tempo]);
    assert!(s.find("menuitem", "Tempo").is_none(), "the menu closed");
}

#[gpui::test]
fn the_row_menus_submenus_open_along_their_path(cx: &mut TestAppContext) {
    let mut s = chords_chart(cx);
    s.open_row_menu(0);
    let c = s.menuitem("Controller").bounds.center();
    s.click_at(c);
    assert!(s.find("menuitem", "Controller 7 (Volume)").is_some());
    let r = s.menuitem("96\u{2013}127").bounds.center();
    s.click_at(r);
    let item = s.menuitem("Controller 100").bounds.center();
    s.click_at(item);
    assert_eq!(
        s.model(|d| d.strip_rows(PANE_SLOT)),
        [StripLane::Controller(100)]
    );
    s.open_row_menu(0);
    let p = s.menuitem("Poly Pressure").bounds.center();
    s.click_at(p);
    assert!(s.find("menuitem", "C4\u{2013}B4").is_some(), "octaves");
}

/// Review Focus 4.
#[gpui::test]
fn an_empty_automation_menu_says_why(cx: &mut TestAppContext) {
    let mut s = setup(cx, roll_fake());
    s.fake.set_call_error(
        "mixer",
        Some(VizError::Busy {
            message: "busy".into(),
        }),
    );
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Lane { seq: 3, lane: 31 },
            Some(DetailEditorKind::StripChart),
        )
    });
    s.open_row_menu(0);
    let auto = s.menuitem("Automation");
    assert!(!auto.enabled);
    assert!(
        auto.value.as_deref().is_some_and(|v| v.contains("busy")),
        "{:?}",
        auto.value
    );
}

/// Review Focus 4: a 20-move stroke and its frames read the mixer at most
/// once (an automation row draws with the targets' names every frame).
/// `roll_fake`, as Swift's test: with `mixer_fake` the window's own mixer
/// (the Channel Strip pane) refetches as the selection moves to the
/// stroke's track, which is not the chart's read.
#[gpui::test]
fn a_stroke_loads_the_automation_targets_once(cx: &mut TestAppContext) {
    let mut s = setup(cx, roll_fake());
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Lane { seq: 3, lane: 31 },
            Some(DetailEditorKind::StripChart),
        );
    });
    s.update_model(|d| {
        d.add_strip_row(PANE_SLOT);
        d.set_strip_row(
            PANE_SLOT,
            1,
            StripLane::Automation(AutomationTargetRow::StripFader { strip: 1 }),
        );
    });
    assert_eq!(s.model(|d| d.strip_rows(PANE_SLOT).len()), 2);
    let before = s.fake.mixer_reads();
    let from = s.at(START_X, 30.);
    s.down(from, Modifiers::none());
    for i in 1..=20 {
        let at = s.at(i as f32 * 8., 30. + i as f32);
        s.move_to(at, Modifiers::none());
    }
    let reads = s.fake.mixer_reads() - before;
    let to = s.at(160., 50.);
    s.up(to, Modifiers::none());
    assert!(s.fake.gesture_edits().len() > 1, "the stroke drew");
    // The stroke's frames; the snapshot its release makes is a new
    // version, read once after it.
    assert!(reads <= 1, "{reads} reads");
}

#[gpui::test]
fn midi_learn_on_an_automation_row(cx: &mut TestAppContext) {
    let mut s = setup(cx, mixer_fake());
    s.update_model(|d| {
        d.open_detail(
            EditorTarget::Lane { seq: 3, lane: 31 },
            Some(DetailEditorKind::StripChart),
        );
    });
    let fader = AutomationTargetRow::StripFader { strip: 1 };
    s.update_model(|d| d.set_strip_row(PANE_SLOT, 0, StripLane::Automation(fader)));
    s.open_row_menu(0);
    let learn = s.menuitem("MIDI Learn").bounds.center();
    s.click_at(learn);
    assert_eq!(s.fake.learn_targets(), [fader]);
}

/// The right pane's Controllers chart draws on the focused track, and
/// leaves the bottom pane's detail alone.
#[gpui::test]
fn a_controllers_chart_stroke_draws_on_the_focused_track(cx: &mut TestAppContext) {
    let mut s = setup(cx, roll_fake());
    s.run_menu("ui.toggle_right");
    s.update_model(|d| d.select_track(31, false));
    let detail = s.model(|d| d.layout().detail.clone());
    s.fake.clear_calls();
    let (from, to) = (s.at_in(RIGHT, START_X, 30.), s.at_in(RIGHT, 160., 48.));
    s.down(from, Modifiers::none());
    s.move_to(to, Modifiers::none());
    let lanes: Vec<_> = s.model(|d| d.gesture().unwrap().lanes.iter().copied().collect());
    assert_eq!(lanes, [(3, 31)]);
    s.up(to, Modifiers::none());
    let sc = velocity_scale();
    let line = LineRow {
        from: 0,
        to: Q,
        start: sc.value_at(30.),
        end: sc.value_at(48.),
        step: Q / 8,
        shape: LineShapeRow::Straight,
    };
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&line_command(
            StripLane::Velocity,
            LineOpRow::Set,
            line,
            false
        ))
    );
    assert_eq!(s.last_call().as_deref(), Some("end_edit"));
    assert_eq!(s.model(|d| d.layout().detail.clone()), detail);
}

/// Carried item 5: the right pane stops showing Controllers mid-stroke.
#[gpui::test]
fn a_controllers_stroke_ends_when_the_view_goes(cx: &mut TestAppContext) {
    let mut s = setup(cx, roll_fake());
    s.run_menu("ui.toggle_right");
    s.update_model(|d| d.select_track(31, false));
    let (from, to) = (s.at_in(RIGHT, START_X, 30.), s.at_in(RIGHT, 160., 48.));
    s.down(from, Modifiers::none());
    s.move_to(to, Modifiers::none());
    assert!(s.model(|d| d.gesture().is_some()));
    s.update_model(|d| d.update_layout(|l| l.views.current_mut().right.view = PaneView::Sequences));
    assert_eq!(s.last_call().as_deref(), Some("abort_edit"));
    assert!(s.model(|d| d.gesture().is_none() && d.strip_stroke(ChartSlot::Controllers).is_none()));
}
