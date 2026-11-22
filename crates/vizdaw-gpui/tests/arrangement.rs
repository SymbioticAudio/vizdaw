//! The arrangement canvas: its tree node and block children, the ruler's
//! click and drag, the playhead, the wheel shared with the track headers,
//! and the zoom buttons, driven through GPUI's test platform with the
//! scripted session.

use std::sync::Arc;
use std::time::Duration;

use core_control::protocol::{RawRequest, RequestId};
use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollDelta,
    ScrollEvent, TestAppContext, VisualTestContext, WindowHandle, point, px,
};
use serde_json::{Map, json};
use vizdaw_app::DocumentStore;
use vizdaw_app::editor::{DetailEditorKind, EditorTarget, Tool};
use vizdaw_app::testing::{FakeSession, demo_fake, lane_row};
use vizdaw_ffi::blocks::{BlockKind, BlockRow};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::{EventKindRow, EventRow};
use vizdaw_ffi::selection::{SelectionRow, TickRange};
use vizdaw_gpui::control;
use vizdaw_gpui::menus::{self, ItemSpec};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow};

/// One bar of 4/4, in ticks: 160 points at the default zoom.
const BAR: i64 = 61_440;
const RULER: f32 = 22.;

const GROOVE: &str = "MIDI track, Groove, no destination";
const CHORDS: &str = "MIDI track, Chords, no destination";

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
    Setup {
        fake,
        app,
        window,
        cx: vcx,
    }
}

fn events_block(start: i64, end: i64, ids: &[u64]) -> BlockRow {
    BlockRow {
        start,
        end,
        kind: BlockKind::Events,
        events: ids.to_vec(),
        label: None,
        ref_seq: None,
        muted: false,
    }
}

fn reference_block(start: i64, end: i64, name: &str) -> BlockRow {
    BlockRow {
        start,
        end,
        kind: BlockKind::Reference,
        events: vec![9],
        label: Some(name.into()),
        ref_seq: Some(1),
        muted: false,
    }
}

/// The demo with a reference to Beat on Groove (bar 2), a two-event block
/// on Chords (bar 1) and one far off to the right.
fn with_blocks() -> FakeSession {
    let f = demo_fake();
    f.set_blocks(3, 30, vec![reference_block(BAR, 2 * BAR, "Beat")]);
    f.set_blocks(
        3,
        31,
        vec![
            events_block(0, BAR, &[1, 2]),
            events_block(500 * BAR, 501 * BAR, &[3]),
        ],
    );
    f
}

impl Setup {
    fn doc(&self) -> vizdaw_app::DocId {
        self.window.read_with(&self.cx, |v, _| v.doc()).unwrap()
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

    fn children(&mut self, role: &str, label: &str) -> Vec<Element> {
        let group = self.element(role, label);
        self.elements()
            .into_iter()
            .filter(|e| e.path.len() == group.path.len() + 1 && e.path.starts_with(&group.path))
            .collect()
    }

    fn canvas(&mut self) -> Element {
        self.element("group", "Arrangement")
    }

    /// A point `x` points into the canvas, `y` points below its top.
    fn at(&mut self, x: f32, y: f32) -> Point<Pixels> {
        let c = self.canvas().bounds.origin;
        point(c.x + px(x), c.y + px(y))
    }

    fn click_at(&mut self, at: Point<Pixels>) {
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.click_at(at);
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }

    fn drag(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        self.cx
            .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        self.cx.run_until_parked();
        self.cx
            .simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
        self.cx.run_until_parked();
    }

    fn release(&mut self, at: Point<Pixels>) {
        self.cx
            .simulate_mouse_up(at, MouseButton::Left, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn locates(&self) -> Vec<String> {
        self.fake
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("locate("))
            .collect()
    }

    fn time_range(&mut self) -> Option<TickRange> {
        let doc = self.doc();
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().selection().time_range
        })
    }

    fn playhead(&mut self) -> Option<f32> {
        self.window
            .read_with(&self.cx, |v, _| v.arrangement().playhead_x())
            .unwrap()
    }

    fn ticks_per_point(&mut self) -> f64 {
        self.window
            .read_with(&self.cx, |v, _| v.arrangement().axis().ticks_per_point())
            .unwrap()
    }

    fn canvas_focused(&mut self) -> bool {
        self.window
            .update(&mut self.cx, |v, window, cx| {
                v.arrangement().focus_handle().is_focused(window) && window.focused(cx).is_some()
            })
            .unwrap()
    }

    fn scroll(&mut self, at: Point<Pixels>, delta: ScrollDelta, modifiers: Modifiers) {
        self.cx.simulate_event(ScrollEvent {
            position: at,
            delta,
            modifiers,
            ..Default::default()
        });
        self.cx.run_until_parked();
    }

    fn value(&mut self, role: &str, label: &str) -> Option<String> {
        self.element(role, label).value
    }
}

fn labels(elements: &[Element]) -> Vec<(String, String)> {
    elements
        .iter()
        .map(|e| (e.role.clone(), e.label.clone()))
        .collect()
}

#[gpui::test]
fn the_arrangement_is_in_the_tree_with_its_blocks(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    let blocks = s.children("group", "Arrangement");
    // Only the visible blocks, in row order.
    assert_eq!(
        labels(&blocks),
        vec![
            (
                "button".to_string(),
                "Reference to Beat on Groove, bar 2".to_string()
            ),
            (
                "button".to_string(),
                "Block on Chords, bar 1, 2 events".to_string()
            ),
        ]
    );
    // The Chords block: bar 1 of row 1, under the ruler, inset 2.
    let canvas = s.canvas().bounds;
    let chords = &blocks[1].bounds;
    assert_eq!(chords.origin.x, canvas.origin.x);
    assert_eq!(chords.origin.y, canvas.origin.y + px(RULER + 48. + 2.));
    assert_eq!(chords.size.width, px(160.));
    assert_eq!(chords.size.height, px(44.));
    // The canvas's centre is in its body, below the ruler.
    assert!(canvas.center().y > canvas.origin.y + px(RULER));
    // A channel click on it is answered.
    let id = core_control::element::element_id(&s.canvas().path, "group", "Arrangement");
    let mut params = Map::new();
    params.insert("element".into(), json!(id));
    let raw = RawRequest {
        id: RequestId::Num(1),
        cmd: "ui.click".into(),
        params,
    };
    let command = core_control::validate(&raw).expect("a valid request");
    let handled = s.cx.cx.update(|cx| control::handle(&command, cx));
    s.cx.run_until_parked();
    assert!(
        matches!(handled, control::Handled::Reply(Ok(_))),
        "{handled:?}"
    );
    assert!(s.canvas_focused());
    // Scrolled right past bar 501, the far block shows instead.
    let at = s.at(300., 100.);
    s.scroll(
        at,
        ScrollDelta::Pixels(point(px(-160. * 500.), px(0.))),
        Modifiers::none(),
    );
    assert_eq!(
        labels(&s.children("group", "Arrangement")),
        vec![(
            "button".to_string(),
            "Block on Chords, bar 501, 1 event".to_string()
        )]
    );
}

#[gpui::test]
fn the_zoom_buttons_step_by_one_and_a_half(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    assert_eq!(s.ticks_per_point(), 384.);
    s.click("button", "Zoom In");
    assert_eq!(s.ticks_per_point(), 256.);
    s.click("button", "Zoom Out");
    s.click("button", "Zoom Out");
    assert_eq!(s.ticks_per_point(), 576.);
    // The block follows the zoom.
    let chords = s.element("button", "Block on Chords, bar 1, 2 events");
    assert!((f32::from(chords.bounds.size.width) - BAR as f32 / 576.).abs() < 0.01);
}

#[gpui::test]
fn a_ruler_click_locates(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    let at = s.at(160., 10.);
    s.click_at(at);
    assert_eq!(s.locates(), vec![format!("locate({BAR})")]);
    assert!(s.canvas_focused(), "a click takes the keyboard");
    // A click below the ruler moves nothing.
    let below = s.at(400., 200.);
    s.click_at(below);
    assert_eq!(s.locates().len(), 1);
}

#[gpui::test]
fn a_click_on_the_arrangement_ends_a_field_edit(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.click("textfield", "Position");
    s.keys("7");
    let body = s.at(400., 200.);
    s.click_at(body);
    assert!(
        s.locates().is_empty(),
        "the typed position is not committed"
    );
    assert_eq!(s.value("textfield", "Position").as_deref(), Some("1.1.0"));
    assert!(s.canvas_focused());
    let keys = {
        let doc = s.doc();
        s.app.read_with(&s.cx, |a, _| {
            a.store.document(doc).unwrap().transport_keys_enabled()
        })
    };
    assert!(keys);
    // The keyboard is the window's again: Space plays.
    s.keys("space");
    assert_eq!(s.fake.count("play(3)"), 1);
}

#[gpui::test]
fn a_ruler_drag_selects_a_range(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    let (from, to) = (s.at(40., 10.), s.at(200., 10.));
    s.drag(from, to);
    // Released below the ruler: the range still ends where the pointer is.
    let end = s.at(200., 150.);
    s.release(end);
    assert_eq!(
        s.time_range(),
        Some(TickRange {
            start: BAR / 4,
            end: 5 * BAR / 4
        })
    );
    let lanes: Vec<u64> = {
        let doc = s.doc();
        s.app.read_with(&s.cx, |a, _| {
            a.store
                .document(doc)
                .unwrap()
                .selection()
                .lanes
                .iter()
                .map(|l| l.lane)
                .collect()
        })
    };
    assert_eq!(lanes, vec![30, 31]);
    assert!(s.locates().is_empty(), "a drag is not a click");
    // Edit In and Out show the range.
    assert_eq!(s.value("textfield", "Edit In").as_deref(), Some("1.2.0"));
    assert_eq!(s.value("textfield", "Edit Out").as_deref(), Some("2.2.0"));

    // Escape cancels a drag: the release then does nothing.
    let sets = s.fake.count("set_selection");
    let (from, to) = (s.at(300., 10.), s.at(500., 10.));
    s.drag(from, to);
    s.keys("escape");
    s.release(to);
    assert_eq!(s.fake.count("set_selection"), sets);
    assert!(s.locates().is_empty());

    // A move inside the dead zone is a click.
    let (from, to) = (s.at(160., 10.), s.at(162., 10.));
    s.drag(from, to);
    s.release(to);
    assert_eq!(s.locates(), vec![format!("locate({BAR})")]);
}

/// Swift `ArrangementViewTests.locatingWhileStoppedMovesThePlayhead`.
#[gpui::test]
fn locating_while_stopped_moves_the_playhead(cx: &mut TestAppContext) {
    let fake = with_blocks();
    fake.set_transport(|t| t.seq = Some(3));
    let mut s = setup(cx, fake);
    assert_eq!(s.playhead(), Some(0.));
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.document_mut(doc).unwrap().locate(BAR);
        cx.notify();
    });
    // The next poll reads it.
    s.cx.executor().advance_clock(Duration::from_millis(300));
    s.cx.run_until_parked();
    assert_eq!(s.playhead(), Some(160.));
}

#[gpui::test]
fn the_playhead_shows_only_for_the_shown_sequence(cx: &mut TestAppContext) {
    let fake = with_blocks();
    fake.set_transport(|t| t.seq = Some(1));
    let mut s = setup(cx, fake);
    assert_eq!(s.playhead(), None);
}

#[gpui::test]
fn page_follow_moves_the_view_while_playing(cx: &mut TestAppContext) {
    let fake = with_blocks();
    fake.set_transport(|t| {
        t.seq = Some(3);
        t.playing = true;
        t.tick = 400 * BAR;
        t.ticks_per_second = 0.;
    });
    let mut s = setup(cx, fake);
    s.cx.executor().advance_clock(Duration::from_millis(40));
    s.cx.run_until_parked();
    let origin = s
        .window
        .read_with(&s.cx, |v, _| v.arrangement().axis().origin())
        .unwrap();
    // The playhead sits 16 points in.
    assert_eq!(origin, 400 * BAR - 16 * 384);
    assert_eq!(s.playhead(), Some(16.));
}

#[gpui::test]
fn wheel_scroll_moves_headers_and_canvas_together(cx: &mut TestAppContext) {
    let fake = with_blocks();
    let mut many = vec![lane_row(30, "Groove", false), lane_row(31, "Chords", false)];
    many.extend((0..20).map(|i| lane_row(100 + i, &format!("Track {i}"), false)));
    fake.set_lanes(3, many);
    let mut s = setup(cx, fake);
    let header = s.element("button", CHORDS).bounds;
    let block = s
        .element("button", "Block on Chords, bar 1, 2 events")
        .bounds;
    assert_eq!(header.origin.y, block.origin.y - px(2.), "rows line up");

    // Over the canvas, a trackpad scroll down by 30 points.
    let at = s.at(300., 100.);
    s.scroll(
        at,
        ScrollDelta::Pixels(point(px(0.), px(-30.))),
        Modifiers::none(),
    );
    let header2 = s.element("button", CHORDS).bounds;
    let block2 = s
        .element("button", "Block on Chords, bar 1, 2 events")
        .bounds;
    assert_eq!(header2.origin.y, header.origin.y - px(30.));
    assert_eq!(block2.origin.y, block.origin.y - px(30.));

    // Over the headers, one wheel notch back up (12 points a line).
    let over = s.element("group", "Track Headers").bounds.center();
    s.scroll(over, ScrollDelta::Lines(point(0., 1.)), Modifiers::none());
    let header3 = s.element("button", CHORDS).bounds;
    let block3 = s
        .element("button", "Block on Chords, bar 1, 2 events")
        .bounds;
    assert_eq!(header3.origin.y, header2.origin.y + px(12.));
    assert_eq!(block3.origin.y, block2.origin.y + px(12.));

    // Never above the first row.
    s.scroll(
        at,
        ScrollDelta::Pixels(point(px(0.), px(500.))),
        Modifiers::none(),
    );
    assert_eq!(s.element("button", CHORDS).bounds, header);

    // Sideways moves the canvas only.
    s.scroll(
        at,
        ScrollDelta::Pixels(point(px(-40.), px(0.))),
        Modifiers::none(),
    );
    let moved = s
        .element("button", "Block on Chords, bar 1, 2 events")
        .bounds;
    assert_eq!(moved.origin.x, block.origin.x - px(40.));
    assert_eq!(s.element("button", CHORDS).bounds, header);

    // With the secondary modifier the wheel zooms about the pointer.
    let pointer = s.at(200., 100.);
    let tick_before = s
        .window
        .read_with(&s.cx, |v, _| v.arrangement().axis().tick_at(200.))
        .unwrap();
    let secondary = s.cx.update(|_, cx| Modifiers::secondary_key(cx));
    s.scroll(pointer, ScrollDelta::Lines(point(0., 1.)), secondary);
    assert!((s.ticks_per_point() - 384. / 1.1).abs() < 1e-6);
    let tick_after = s
        .window
        .read_with(&s.cx, |v, _| v.arrangement().axis().tick_at(200.))
        .unwrap();
    assert!(
        (tick_after - tick_before).abs() <= 1,
        "{tick_before} {tick_after}"
    );
}

#[gpui::test]
fn the_headers_and_add_row_show_beside_the_canvas(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    let headers = s.element("group", "Track Headers").bounds;
    let canvas = s.canvas().bounds;
    // The column, its 1 px line, then the canvas, all from the same top.
    assert_eq!(headers.size.width, px(200.));
    assert_eq!(canvas.origin.x, headers.origin.x + px(201.));
    assert_eq!(canvas.origin.y, headers.origin.y);
    assert_eq!(canvas.size.height, headers.size.height);
    // The first row starts under the ruler, as the canvas's first row.
    let groove = s.element("button", GROOVE).bounds;
    assert_eq!(groove.origin.y, canvas.origin.y + px(RULER));
    let children = labels(&s.children("group", "Track Headers"));
    assert_eq!(
        children.last(),
        Some(&("button".to_string(), "Add Track".to_string()))
    );
    // The transport bar is above, the zoom buttons too.
    let play = s.element("button", "Play").bounds;
    assert!(headers.origin.y >= play.bottom());
    assert!(s.element("button", "Zoom In").bounds.bottom() <= canvas.origin.y);
}

#[gpui::test]
fn a_long_song_builds_only_the_rows_in_view(cx: &mut TestAppContext) {
    let fake = with_blocks();
    let tracks: Vec<_> = (0..200)
        .map(|i| lane_row(100 + i, &format!("Track {i}"), false))
        .collect();
    fake.set_lanes(3, tracks);
    fake.set_blocks(3, 299, vec![events_block(0, BAR, &[7])]);
    let mut s = setup(cx, fake);
    // At most a window's height of rows (the build never under-counts a
    // canvas that just grew), far fewer than 200.
    let window =
        s.cx.update(|window, _| f32::from(window.viewport_size().height));
    let most = (window / 48.).ceil() as usize + 1;
    let built = |s: &mut Setup| {
        s.window
            .read_with(&s.cx, |v, _| v.arrangement().rows_built())
            .unwrap()
    };
    let n = built(&mut s);
    assert!(n > 0 && n <= most, "{n} rows built, {most} at most in view");
    // Scrolled to the end, the last track's block shows; still only a
    // screenful is built.
    let at = s.at(300., 100.);
    s.scroll(
        at,
        ScrollDelta::Pixels(point(px(0.), px(-100_000.))),
        Modifiers::none(),
    );
    assert!(
        s.find("button", "Block on Track 199, bar 1, 1 event")
            .is_some()
    );
    let n = built(&mut s);
    assert!(n > 0 && n <= most, "{n} rows built after the scroll");
}

/// Rows a taller canvas exposes are built in the same frame: a window
/// enlarged after a smaller draw shows the blocks of its new last rows.
#[gpui::test]
fn rows_exposed_by_a_taller_canvas_are_built(cx: &mut TestAppContext) {
    let fake = with_blocks();
    let tracks: Vec<_> = (0..60)
        .map(|i| lane_row(100 + i, &format!("Track {i}"), false))
        .collect();
    fake.set_lanes(3, tracks);
    for i in 0..60 {
        fake.set_blocks(3, 100 + i, vec![events_block(0, BAR, &[i])]);
    }
    let mut s = setup(cx, fake);
    s.cx.simulate_resize(gpui::size(px(1000.), px(300.)));
    s.cx.run_until_parked();
    s.cx.simulate_resize(gpui::size(px(1000.), px(900.)));
    s.cx.run_until_parked();
    let body = f32::from(s.canvas().bounds.size.height) - RULER;
    let last = (body / 48.).ceil() as usize - 1;
    let label = format!("Block on Track {last}, bar 1, 1 event");
    assert!(s.find("button", &label).is_some(), "no {label:?}");
}

// MARK: Block editing (slice 4)

/// The Chords block (bar 1 of row 1) and the Groove reference (bar 2 of
/// row 0), in canvas points; empty space on the Chords row; and below
/// every row.
const ON_CHORDS: (f32, f32) = (50., RULER + 48. + 24.);
const ON_GROOVE: (f32, f32) = (200., RULER + 24.);
const EMPTY_CHORDS: (f32, f32) = (400., RULER + 48. + 24.);
const BELOW_ROWS: (f32, f32) = (400., 200.);
const CHORDS_BLOCK: &str = "Block on Chords, bar 1, 2 events";
const GROOVE_BLOCK: &str = "Reference to Beat on Groove, bar 2";

fn alt() -> Modifiers {
    Modifiers {
        alt: true,
        ..Modifiers::none()
    }
}

fn note(id: u64, tick: i64) -> EventRow {
    EventRow {
        pitch: 60,
        velocity: 0x8000,
        duration: BAR / 4,
        ..EventRow::blank(id, tick, EventKindRow::Note)
    }
}

impl Setup {
    fn pt(&mut self, (x, y): (f32, f32)) -> Point<Pixels> {
        self.at(x, y)
    }

    fn click_with(&mut self, at: (f32, f32), modifiers: Modifiers) {
        let at = self.pt(at);
        self.cx.simulate_click(at, modifiers);
        self.cx.run_until_parked();
    }

    fn double_click(&mut self, at: (f32, f32)) {
        let position = self.pt(at);
        for (count, _) in [(1, ()), (2, ())] {
            self.cx.simulate_event(MouseDownEvent {
                position,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count: count,
                first_mouse: false,
            });
            self.cx.simulate_event(MouseUpEvent {
                position,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count: count,
            });
        }
        self.cx.run_until_parked();
    }

    fn right_click(&mut self, at: (f32, f32)) {
        let position = self.pt(at);
        self.cx
            .simulate_mouse_down(position, MouseButton::Right, Modifiers::none());
        self.cx
            .simulate_mouse_up(position, MouseButton::Right, Modifiers::none());
        self.cx.run_until_parked();
    }

    /// Presses at `from` and moves to `to`, the button held.
    fn press_and_move(&mut self, from: (f32, f32), to: (f32, f32), modifiers: Modifiers) {
        let (from, to) = (self.pt(from), self.pt(to));
        self.cx
            .simulate_mouse_down(from, MouseButton::Left, modifiers);
        self.cx.run_until_parked();
        self.cx
            .simulate_mouse_move(to, Some(MouseButton::Left), modifiers);
        self.cx.run_until_parked();
    }

    fn release_with(&mut self, at: (f32, f32), modifiers: Modifiers) {
        let at = self.pt(at);
        self.cx.simulate_mouse_up(at, MouseButton::Left, modifiers);
        self.cx.run_until_parked();
    }

    fn model<R>(&mut self, f: impl FnOnce(&vizdaw_app::DocumentModel) -> R) -> R {
        let doc = self.doc();
        self.app
            .read_with(&self.cx, |a, _| f(a.store.document(doc).unwrap()))
    }

    fn selection(&mut self) -> SelectionRow {
        self.model(|d| d.selection().clone())
    }

    fn selected_lanes(&mut self) -> Vec<u64> {
        self.selection().lanes.iter().map(|l| l.lane).collect()
    }

    /// The fake's calls of a drag: begins, gesture edits, ends and aborts.
    fn drag_calls(&self) -> Vec<String> {
        self.fake
            .calls()
            .into_iter()
            .filter(|c| {
                c.starts_with("begin_edit") || c.starts_with("edit_gesture") || c.ends_with("_edit")
            })
            .collect()
    }

    fn undo_label(&mut self) -> Option<String> {
        self.model(|d| d.snapshot().undo_label.clone())
    }

    fn status(&mut self) -> Option<String> {
        self.model(|d| d.status().map(String::from))
    }

    fn channel(&mut self, cmd: &str, params: serde_json::Value) -> control::Handled {
        let serde_json::Value::Object(params) = params else {
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

    fn channel_ok(&mut self, cmd: &str, params: serde_json::Value) -> serde_json::Value {
        match self.channel(cmd, params) {
            control::Handled::Reply(Ok(v)) => v,
            other => panic!("{cmd} failed: {other:?}"),
        }
    }
}

fn mv(events: &[u64], delta: i64, to_lane: Option<LaneRef>) -> EditCommand {
    EditCommand::Event {
        edit: EventEdit::Move {
            events: events.to_vec(),
            delta,
            to_lane,
        },
    }
}

const GROOVE_LANE: LaneRef = LaneRef { seq: 3, lane: 30 };

#[gpui::test]
fn a_click_selects_a_block_and_shift_adds(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.click_with(ON_CHORDS, Modifiers::none());
    let sel = s.selection();
    assert_eq!(sel.events, vec![1, 2]);
    assert_eq!(s.selected_lanes(), vec![31]);
    assert_eq!(sel.time_range, Some(TickRange { start: 0, end: BAR }));
    // ⇧ adds the Groove reference: its event, its track, the range united.
    s.click_with(ON_GROOVE, Modifiers::shift());
    let sel = s.selection();
    assert_eq!(sel.events, vec![1, 2, 9]);
    assert_eq!(s.selected_lanes(), vec![31, 30]);
    assert_eq!(
        sel.time_range,
        Some(TickRange {
            start: 0,
            end: 2 * BAR
        })
    );
    // A plain click selects only the clicked block.
    s.click_with(ON_GROOVE, Modifiers::none());
    assert_eq!(s.selection().events, vec![9]);
    assert_eq!(s.selected_lanes(), vec![30]);
    assert!(s.drag_calls().is_empty(), "a click is not a drag");
}

#[gpui::test]
fn a_click_on_empty_space_clears_unless_shift(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.click_with(ON_CHORDS, Modifiers::none());
    s.click_with(EMPTY_CHORDS, Modifiers::shift());
    assert_eq!(s.selection().events, vec![1, 2], "⇧ keeps the selection");
    s.click_with(EMPTY_CHORDS, Modifiers::none());
    let sel = s.selection();
    assert!(sel.events.is_empty() && sel.lanes.is_empty() && sel.time_range.is_none());
    // Below every row too.
    s.click_with(ON_CHORDS, Modifiers::none());
    s.click_with(BELOW_ROWS, Modifiers::none());
    assert!(s.selection().events.is_empty());
}

#[gpui::test]
fn a_drag_moves_a_block_as_one_undo_step(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.fake.clear_calls();
    s.press_and_move(ON_CHORDS, (300., ON_CHORDS.1), Modifiers::none());
    // The live preview: the gesture is open and its target applied.
    assert!(s.model(|d| d.gesture_open()));
    assert_eq!(s.drag_calls(), ["begin_edit(Move Block)", "edit_gesture"]);
    let to = s.pt((310., ON_CHORDS.1));
    s.cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    s.cx.run_until_parked();
    s.release_with((310., ON_CHORDS.1), Modifiers::none());
    let calls = s.drag_calls();
    assert_eq!(calls.first().unwrap(), "begin_edit(Move Block)");
    assert_eq!(calls.last().unwrap(), "end_edit");
    assert_eq!(calls.iter().filter(|c| c.starts_with("begin")).count(), 1);
    assert!(!s.model(|d| d.gesture_open()));
    assert_eq!(s.undo_label().as_deref(), Some("Move Block"));
    let Some(EditCommand::Event {
        edit:
            EventEdit::Move {
                events,
                delta,
                to_lane,
            },
    }) = s.fake.gesture_edits().last().cloned()
    else {
        panic!("expected a move: {:?}", s.fake.gesture_edits());
    };
    assert_eq!(events, vec![1, 2]);
    assert!(delta > 0);
    assert_eq!(to_lane, None);
    // The selection is not changed by a drag.
    assert!(s.selection().events.is_empty());
}

#[gpui::test]
fn an_option_drag_copies(cx: &mut TestAppContext) {
    let fake = with_blocks();
    fake.set_events(3, 31, vec![note(1, 0), note(2, BAR / 4)]);
    let mut s = setup(cx, fake);
    s.fake.clear_calls();
    s.press_and_move(ON_CHORDS, (300., ON_GROOVE.1), alt());
    s.release_with((300., ON_GROOVE.1), alt());
    assert_eq!(
        s.drag_calls(),
        ["begin_edit(Copy Block)", "edit_gesture", "end_edit"]
    );
    assert_eq!(s.undo_label().as_deref(), Some("Copy Block"));
    let Some(EditCommand::Event {
        edit: EventEdit::Insert { seq, lane, events },
    }) = s.fake.gesture_edits().last().cloned()
    else {
        panic!("expected an insert: {:?}", s.fake.gesture_edits());
    };
    assert_eq!((seq, lane), (3, 30));
    assert_eq!(events.len(), 2);
    assert!(events[0].tick > 0 && events[1].tick - events[0].tick == BAR / 4);
}

#[gpui::test]
fn a_drag_onto_another_track_merges(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.fake.clear_calls();
    s.press_and_move(ON_CHORDS, (ON_CHORDS.0, ON_GROOVE.1), Modifiers::none());
    s.release_with((ON_CHORDS.0, ON_GROOVE.1), Modifiers::none());
    assert_eq!(
        s.drag_calls(),
        ["begin_edit(Move Block)", "edit_gesture", "end_edit"]
    );
    assert_eq!(s.fake.gesture_edits(), [mv(&[1, 2], 0, Some(GROOVE_LANE))]);
}

#[gpui::test]
fn escape_mid_drag_leaves_the_song_unchanged(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.fake.clear_calls();
    s.press_and_move(ON_CHORDS, (300., ON_CHORDS.1), Modifiers::none());
    s.keys("escape");
    assert!(!s.model(|d| d.gesture_open()));
    // The matching mouse-up (and moves before it) are swallowed.
    let to = s.pt((350., ON_CHORDS.1));
    s.cx.simulate_mouse_move(to, Some(MouseButton::Left), Modifiers::none());
    s.release_with((350., ON_CHORDS.1), Modifiers::none());
    assert_eq!(
        s.drag_calls(),
        ["begin_edit(Move Block)", "edit_gesture", "abort_edit"]
    );
    assert_eq!(s.undo_label(), None);
    assert!(s.selection().events.is_empty(), "nor is it a click");
}

#[gpui::test]
fn the_eraser_deletes_the_clicked_block(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.click("button", "Eraser");
    assert_eq!(s.model(|d| d.tool()), Tool::Eraser);
    let mut next = s.fake.snap();
    next.version += 1;
    next.undo_label = Some("Delete Events".into());
    s.fake.bump(next);
    s.click_with(ON_CHORDS, Modifiers::none());
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Delete { events: vec![1, 2] }
        })
    );
    assert_eq!(s.undo_label().as_deref(), Some("Delete Events"));
    // The eraser does not drag.
    s.fake.clear_calls();
    s.press_and_move(ON_GROOVE, (400., ON_GROOVE.1), Modifiers::none());
    s.release_with((400., ON_GROOVE.1), Modifiers::none());
    assert!(s.drag_calls().is_empty());
    // A refusal is the model's status.
    s.fake.set_edit_error(Some(vizdaw_ffi::VizError::Model {
        message: "the events are locked".into(),
    }));
    s.click_with(ON_CHORDS, Modifiers::none());
    assert_eq!(s.status().as_deref(), Some("the events are locked"));
}

#[gpui::test]
fn the_pencil_is_chosen_but_does_nothing_yet(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.click("button", "Pencil");
    assert_eq!(s.model(|d| d.tool()), Tool::Pencil);
    assert!(s.element("button", "Pencil").selected);
    s.fake.clear_calls();
    s.click_with(ON_CHORDS, Modifiers::none());
    s.press_and_move(ON_CHORDS, (300., ON_CHORDS.1), Modifiers::none());
    s.release_with((300., ON_CHORDS.1), Modifiers::none());
    assert!(s.fake.calls().is_empty(), "{:?}", s.fake.calls());
    s.click("button", "Arrow");
    assert_eq!(s.model(|d| d.tool()), Tool::Arrow);
}

#[gpui::test]
fn a_double_click_opens_the_detail_on_the_block(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.double_click(ON_CHORDS);
    let detail = s.model(|d| d.layout().detail.clone()).expect("a detail");
    assert_eq!(detail.kind, DetailEditorKind::List);
    assert_eq!(
        detail.target,
        EditorTarget::Block {
            seq: 3,
            lane: 31,
            start: 0,
            end: BAR
        }
    );
    let e = s.channel_ok("state.get", json!({"target": "editor"}));
    assert_eq!(
        e["detail"],
        json!({"kind": "list", "seq": 3, "lane": 31, "start": 0, "end": BAR})
    );
}

#[gpui::test]
fn the_block_menu_opens_the_list_and_deletes(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    assert!(s.find("menuitem", "Open in List").is_none());
    s.right_click(ON_CHORDS);
    // An unselected block is selected first.
    assert_eq!(s.selection().events, vec![1, 2]);
    let items: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| e.label)
        .collect();
    assert_eq!(items, ["Open in List", "Delete"]);
    s.click("menuitem", "Open in List");
    assert!(s.find("menuitem", "Open in List").is_none(), "it closes");
    let detail = s.model(|d| d.layout().detail.clone()).expect("a detail");
    assert_eq!(detail.kind, DetailEditorKind::List);
    assert_eq!(
        detail.target,
        EditorTarget::Block {
            seq: 3,
            lane: 31,
            start: 0,
            end: BAR
        }
    );
    // A selected block keeps the selection it is part of.
    s.click_with(ON_GROOVE, Modifiers::shift());
    s.right_click(ON_CHORDS);
    assert_eq!(s.selection().events, vec![1, 2, 9]);
    // Escape closes the menu without acting.
    s.keys("escape");
    assert!(s.find("menuitem", "Delete").is_none());
    s.right_click(ON_CHORDS);
    s.click("menuitem", "Delete");
    assert_eq!(
        s.fake.edits().last(),
        Some(&EditCommand::Event {
            edit: EventEdit::Delete { events: vec![1, 2] }
        })
    );
    // On empty space there is no menu.
    s.right_click(EMPTY_CHORDS);
    assert!(s.find("menuitem", "Delete").is_none());
}

#[gpui::test]
fn g_toggles_snap_and_the_menu_shows_it(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    let snap_item = |s: &mut Setup| {
        let doc = s.doc();
        let m = s.app.read_with(&s.cx, |a, _| {
            menus::menu_model(a.store.document(doc), &a.keymap)
        });
        let editors = m
            .menus
            .iter()
            .find(|m| m.title == "Editors")
            .expect("an Editors menu")
            .clone();
        editors
            .items
            .into_iter()
            .find_map(|i| match i {
                ItemSpec::Command {
                    id: "editors.snap",
                    title,
                    keys,
                    enabled,
                    checked,
                } => Some((title, keys, enabled, checked)),
                _ => None,
            })
            .expect("Snap to Grid")
    };
    assert_eq!(
        snap_item(&mut s),
        ("Snap to Grid".into(), Some("G".into()), true, Some(true))
    );
    assert_eq!(s.value("button", "Snap").as_deref(), Some("on"));
    s.keys("g");
    assert!(!s.model(|d| d.layout().snap.enabled));
    assert_eq!(snap_item(&mut s).3, Some(false));
    assert_eq!(s.value("button", "Snap").as_deref(), Some("off"));
    // The menu item runs it too.
    s.window
        .update(&mut s.cx, |_, window, cx| {
            window.dispatch_action(menus::action_for("editors.snap"), cx)
        })
        .unwrap();
    s.cx.run_until_parked();
    assert!(s.model(|d| d.layout().snap.enabled));
    // And the channel, with `on`.
    s.channel_ok(
        "command.run",
        json!({"id": "editors.snap", "params": {"on": false}}),
    );
    assert!(!s.model(|d| d.layout().snap.enabled));
    let e = s.channel_ok("state.get", json!({"target": "editor"}));
    assert_eq!(e["snap"], json!({"enabled": false, "division": "quarter"}));
    // The tool bar's toggle and division menu.
    s.click("button", "Snap");
    assert!(s.model(|d| d.layout().snap.enabled));
    s.click("button", "1/4");
    let items: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| e.label)
        .collect();
    assert_eq!(
        items,
        [
            "Bar", "1/2", "1/4", "1/8", "1/16", "1/32", "Triplet", "Dotted"
        ]
    );
    s.click("menuitem", "1/8");
    assert_eq!(
        s.model(|d| d.layout().snap.division),
        vizdaw_app::editor::Division::Eighth
    );
    s.click("button", "1/8");
    assert_eq!(s.value("menuitem", "Triplet").as_deref(), Some("off"));
    s.click("menuitem", "Triplet");
    assert!(s.model(|d| d.layout().snap.triplet));
}

#[gpui::test]
fn a_snapped_drag_lands_on_the_grid(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    // 250 points at 384 ticks a point: 96 000 ticks, which snaps (1/4) to
    // 92 160 (bar 2, beat 3).
    s.press_and_move(ON_CHORDS, (300., ON_CHORDS.1), Modifiers::none());
    s.release_with((300., ON_CHORDS.1), Modifiers::none());
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&mv(&[1, 2], 92_160, None))
    );
    // Snap off: the raw distance.
    s.keys("g");
    s.press_and_move(ON_CHORDS, (300., ON_CHORDS.1), Modifiers::none());
    s.release_with((300., ON_CHORDS.1), Modifiers::none());
    assert_eq!(
        s.fake.gesture_edits().last(),
        Some(&mv(&[1, 2], 96_000, None))
    );
}

#[gpui::test]
fn selected_blocks_are_selected_in_the_tree(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    assert!(!s.element("button", CHORDS_BLOCK).selected);
    s.click_with(ON_CHORDS, Modifiers::none());
    assert!(s.element("button", CHORDS_BLOCK).selected);
    assert!(!s.element("button", GROOVE_BLOCK).selected);
    assert!(s.element("button", "Arrow").selected);
    assert!(!s.element("button", "Eraser").selected);
    let tree = s
        .window
        .read_with(&s.cx, |v, _| v.registry().borrow().tree())
        .unwrap();
    let node = |label: &str| {
        tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["label"] == label)
            .cloned()
            .unwrap()
    };
    assert_eq!(node(CHORDS_BLOCK)["selected"], json!(true));
    assert_eq!(node(GROOVE_BLOCK).get("selected"), None);
}

#[gpui::test]
fn undo_is_refused_while_a_drag_is_open(cx: &mut TestAppContext) {
    let fake = with_blocks();
    let mut done = fake.snap();
    done.version += 1;
    done.undo_label = Some("New MIDI Track".into());
    fake.bump(done);
    let mut s = setup(cx, fake);
    s.channel_ok("command.run", json!({"id": "track.new_midi"}));
    assert!(s.model(|d| d.can_undo()));
    s.fake.clear_calls();
    s.press_and_move(ON_CHORDS, (300., ON_CHORDS.1), Modifiers::none());
    s.keys(if cfg!(target_os = "macos") {
        "cmd-z"
    } else {
        "ctrl-z"
    });
    assert_eq!(s.status().as_deref(), Some("Finish the drag first."));
    match s.channel("command.run", json!({"id": "edit.undo"})) {
        control::Handled::Reply(Err(e)) => {
            assert_eq!(e.code, core_control::ErrorCode::Unavailable);
            assert_eq!(e.message, "Finish the drag first.");
        }
        other => panic!("undo ran: {other:?}"),
    }
    assert_eq!(s.fake.count("undo"), 0);
    s.release_with((300., ON_CHORDS.1), Modifiers::none());
    assert_eq!(s.undo_label().as_deref(), Some("Move Block"));
}

#[gpui::test]
fn a_block_drag_near_the_edge_scrolls_and_the_block_follows(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    let width = f32::from(s.canvas().bounds.size.width);
    let edge = (width - 4., ON_CHORDS.1);
    s.press_and_move(ON_CHORDS, edge, Modifiers::none());
    let first = match s.fake.gesture_edits().last().cloned() {
        Some(EditCommand::Event {
            edit: EventEdit::Move { delta, .. },
        }) => delta,
        other => panic!("expected a move, got {other:?}"),
    };
    s.cx.executor().advance_clock(Duration::from_millis(200));
    s.cx.run_until_parked();
    let origin = s
        .window
        .read_with(&s.cx, |v, _| v.arrangement().axis().origin())
        .unwrap();
    assert!(origin > 0, "the view scrolled");
    let later = match s.fake.gesture_edits().last().cloned() {
        Some(EditCommand::Event {
            edit: EventEdit::Move { delta, .. },
        }) => delta,
        other => panic!("expected a move, got {other:?}"),
    };
    assert!(later > first, "the block follows: {first} then {later}");
    s.release_with(edge, Modifiers::none());
    assert!(!s.model(|d| d.gesture_open()));
    // The release ended the scrolling.
    let after = s
        .window
        .read_with(&s.cx, |v, _| v.arrangement().axis().origin())
        .unwrap();
    s.cx.executor().advance_clock(Duration::from_millis(200));
    s.cx.run_until_parked();
    let again = s
        .window
        .read_with(&s.cx, |v, _| v.arrangement().axis().origin())
        .unwrap();
    assert_eq!(after, again);
}

// MARK: Review round 1

fn busy() -> vizdaw_ffi::VizError {
    vizdaw_ffi::VizError::Busy {
        message: "busy".into(),
    }
}

/// Drags the Chords block and releases it with the final `end_edit`
/// meeting Busy: the drag waits for the timer's tick.
fn release_on_busy(s: &mut Setup) {
    s.press_and_move(ON_CHORDS, (300., ON_CHORDS.1), Modifiers::none());
    s.fake.fail_next("end_edit", busy());
    s.release_with((300., ON_CHORDS.1), Modifiers::none());
    assert!(s.model(|d| d.gesture().is_some_and(|g| g.finishing)));
}

#[gpui::test]
fn a_released_drag_pending_on_busy_survives_deactivation(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    release_on_busy(&mut s);
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    assert!(
        !s.fake.calls().iter().any(|c| c == "abort_edit"),
        "{:?}",
        s.fake.calls()
    );
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, _| {
        a.store.document_mut(doc).unwrap().tick();
    });
    assert!(!s.model(|d| d.gesture_open()));
    assert_eq!(s.undo_label().as_deref(), Some("Move Block"));
}

#[gpui::test]
fn deactivation_mid_drag_cancels_it(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.fake.clear_calls();
    s.press_and_move(ON_CHORDS, (300., ON_CHORDS.1), Modifiers::none());
    s.cx.deactivate_window();
    s.cx.run_until_parked();
    assert!(!s.model(|d| d.gesture_open()));
    assert_eq!(s.drag_calls().last().unwrap(), "abort_edit");
}

#[gpui::test]
fn escape_aborts_a_released_drag_pending_on_busy(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_blocks());
    s.fake.clear_calls();
    release_on_busy(&mut s);
    s.keys("escape");
    assert!(!s.model(|d| d.gesture_open()));
    assert_eq!(s.drag_calls().last().unwrap(), "abort_edit");
    assert!(!s.drag_calls().iter().any(|c| c == "end_edit"));
    assert_eq!(s.undo_label(), None);
}

#[gpui::test]
fn a_double_click_keeps_the_saved_last_editor(cx: &mut TestAppContext) {
    let fake = with_blocks();
    fake.set_layout(Some(r#"{"lastKind":"pianoRoll"}"#));
    let mut s = setup(cx, fake);
    s.double_click(ON_CHORDS);
    assert_eq!(
        s.model(|d| d.layout().detail.as_ref().map(|d| d.kind)),
        Some(DetailEditorKind::List)
    );
    assert_eq!(
        s.model(|d| d.layout().last_kind),
        DetailEditorKind::PianoRoll
    );
    let path = std::env::temp_dir().join("vizdaw-last-kind.vizdaw");
    s.channel_ok(
        "command.run",
        json!({"id": "file.save_as", "params": {"path": path.to_string_lossy()}}),
    );
    let saved: serde_json::Value =
        serde_json::from_str(&s.fake.layout().expect("a stored layout")).unwrap();
    assert_eq!(saved["lastKind"], "pianoRoll");
}
