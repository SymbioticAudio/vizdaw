//! The arrangement canvas: its tree node and block children, the ruler's
//! click and drag, the playhead, the wheel shared with the track headers,
//! and the zoom buttons, driven through GPUI's test platform with the
//! scripted session.

use std::sync::Arc;
use std::time::Duration;

use core_control::protocol::{RawRequest, RequestId};
use gpui::{
    Entity, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollEvent, TestAppContext,
    VisualTestContext, WindowHandle, point, px,
};
use serde_json::{Map, json};
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeSession, demo_fake, lane_row};
use vizdaw_ffi::blocks::{BlockKind, BlockRow};
use vizdaw_ffi::selection::TickRange;
use vizdaw_gpui::control;
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
