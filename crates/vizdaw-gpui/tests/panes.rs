//! The pane host (Swift `DocumentShell`, `PaneFrame`, `PaneDivider`,
//! `PaneGeometry`): the left, centre, bottom and right regions from the
//! layout of the current main view, the Mix view's centre, the frame-only
//! hiding of a small window, the dividers' drags, the pane pickers, and the
//! transport bar's Main View and pane toggles, and the right pane's
//! Markers view, driven through GPUI's test platform with the scripted
//! session.

use std::sync::Arc;

use core_control::registry::ArgValue;

use gpui::{
    Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, TestAppContext, VisualTestContext, WindowHandle, point, px, size,
};
use vizdaw_app::editor::{DetailEditorKind, MainView, PaneSide, PaneView};
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_app::transport_bar::TransportLayout;
use vizdaw_app::{DocSession, DocumentStore};
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_gpui::panes;
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow, RunCommand};

const LEFT: &str = "Left Pane";
const BOTTOM: &str = "Bottom Pane";
const RIGHT: &str = "Right Pane";
const STRIP_NOTE: &str = "Select a track to see its channel strip.";
const CONTROLLERS_NOTE: &str = "Select a track to see its controllers.";
const BAR: i64 = 61_440;
const CHORDS: &str = "MIDI track, Chords, no destination";

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext) -> Setup {
    let fake = Arc::new(demo_fake());
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

fn is_toggle(e: &Element) -> bool {
    matches!(e.value.as_deref(), Some("on" | "off"))
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

    fn element(&mut self, role: &str, label: &str) -> Element {
        let all = self.elements();
        self.find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {all:#?}"))
    }

    fn group(&mut self, label: &str) -> Option<Bounds<Pixels>> {
        self.find("group", label).map(|e| e.bounds)
    }

    /// The pane's picker (its button, not the transport bar's toggle).
    fn picker(&mut self, side: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == "button" && e.label == side && !is_toggle(e))
    }

    /// The transport bar's toggle of the pane.
    fn toggle(&mut self, side: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == "button" && e.label == side && is_toggle(e))
    }

    fn menu_items(&mut self) -> Vec<String> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "menuitem")
            .map(|e| e.label)
            .collect()
    }

    fn click_at(&mut self, at: Point<Pixels>) {
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.click_at(at);
    }

    fn click_picker(&mut self, side: &str) {
        let at = self.picker(side).expect("a picker").bounds.center();
        self.click_at(at);
    }

    fn run_menu(&mut self, id: &str) {
        self.cx.dispatch_action(RunCommand::new(id.to_owned()));
        self.cx.run_until_parked();
    }

    fn resize(&mut self, width: f32, height: f32) {
        self.cx.simulate_resize(size(px(width), px(height)));
        self.cx.run_until_parked();
    }

    fn pane(&mut self, side: PaneSide) -> vizdaw_app::editor::PaneState {
        self.model(|d| *d.layout().views.current().pane(side))
    }

    fn press(&mut self, at: Point<Pixels>) {
        self.cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        self.cx.run_until_parked();
    }

    fn move_to(&mut self, at: Point<Pixels>) {
        self.cx.simulate_event(MouseMoveEvent {
            position: at,
            modifiers: Modifiers::none(),
            pressed_button: Some(MouseButton::Left),
        });
        self.cx.run_until_parked();
    }

    fn release(&mut self, at: Point<Pixels>) {
        self.cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        });
        self.cx.run_until_parked();
    }

    /// Drags the divider of `side` by (`dx`, `dy`): the size is read in
    /// the middle of the drag, before the release.
    fn drag_divider(&mut self, side: &str, dx: f32, dy: f32) -> f64 {
        let at = self
            .element("slider", &format!("{side} Size"))
            .bounds
            .center();
        self.press(at);
        self.move_to(point(at.x + px(dx / 2.), at.y + px(dy / 2.)));
        let to = point(at.x + px(dx), at.y + px(dy));
        self.move_to(to);
        let side = match side {
            LEFT => PaneSide::Left,
            BOTTOM => PaneSide::Bottom,
            _ => PaneSide::Right,
        };
        let during = self.pane(side).size;
        self.release(to);
        during
    }

    fn renames(&self) -> Vec<String> {
        self.fake
            .edits()
            .into_iter()
            .filter_map(|e| match e {
                EditCommand::Lane {
                    edit: LaneEdit::Rename { name, .. },
                } => Some(name),
                _ => None,
            })
            .collect()
    }
}

/// The right pane's Markers view (slice 8c Task 6; Swift `MarkersPane`):
/// the empty note; a marker added and a locator set show as rows in time
/// order, each a button with its name and position; a click moves the
/// transport there.
#[gpui::test]
fn the_markers_view_lists_and_moves(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.run_menu("ui.toggle_right");
    s.click_picker(RIGHT);
    s.click("menuitem", "Markers");
    assert!(s.find("text", "No markers or locators.").is_some());
    let seq = s.model(|d| d.selected_seq().unwrap()) as i64;
    s.update_model(|d| {
        let add = [
            ("seq".to_owned(), ArgValue::Int(seq)),
            ("tick".to_owned(), ArgValue::Int(2 * BAR)),
            ("name".to_owned(), ArgValue::Text("Verse".into())),
        ];
        vizdaw_app::commands::run("marker.add", &add, d).unwrap();
    });
    assert!(s.find("text", "No markers or locators.").is_none());
    assert_eq!(s.element("button", "Verse").value.as_deref(), Some("3.1.0"));
    s.fake.locate(BAR).unwrap();
    s.update_model(|d| {
        vizdaw_app::commands::run("locator.set_2", &[], d).unwrap();
    });
    let right = s.group(RIGHT).unwrap();
    let locator = s.element("button", "Locator 2");
    let verse = s.element("button", "Verse");
    assert!(right.contains(&verse.bounds.center()));
    assert!(
        locator.bounds.center().y < verse.bounds.center().y,
        "in time order"
    );
    assert_eq!(locator.value.as_deref(), Some("2.1.0"));
    s.click("button", "Verse");
    assert_eq!(s.fake.transport().unwrap().tick, 2 * BAR);
    assert_eq!(s.model(|d| d.status().map(String::from)), None);
}

#[gpui::test]
fn the_mix_view_hides_the_arrangement(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    assert!(s.group("Arrangement").is_some());
    assert!(s.group("Track Headers").is_some());
    assert!(s.group("Mixer").is_none());
    let headers = s.group("Track Headers").unwrap();

    s.run_menu("ui.view_mix");
    assert_eq!(s.model(|d| d.layout().views.main), MainView::Mix);
    // The arrangement, its headers and its tool bar are out of the tree;
    // the Mix view's panes are all hidden by default, so the mixer has the
    // whole area.
    assert!(s.group("Arrangement").is_none());
    assert!(s.group("Track Headers").is_none());
    assert!(s.find("button", "Snap").is_none());
    for side in [LEFT, BOTTOM, RIGHT] {
        assert!(s.group(side).is_none(), "{side} hidden in the Mix view");
    }
    let mixer = s.group("Mixer").expect("the Mix view's mixer");
    assert_eq!(mixer.origin.x, px(0.));
    // The centre starts where the arrangement's tool bar did.
    assert!(mixer.origin.y < headers.origin.y);
    // The Arrange view's layout is kept.
    assert!(s.model(|d| d.layout().views.arrange.left.shown));

    // The Mix view's own panes.
    s.run_menu("ui.toggle_bottom");
    assert!(s.model(|d| d.layout().views.mix.bottom.shown));
    let bottom = s.group(BOTTOM).expect("the Mix view's bottom pane");
    let mixer = s.group("Mixer").unwrap();
    assert_eq!(bottom.origin.y, mixer.origin.y + mixer.size.height + px(1.));

    s.run_menu("ui.view_arrange");
    assert!(s.group("Arrangement").is_some());
    assert!(s.group("Mixer").is_none());
    assert!(s.group(LEFT).is_some());
}

#[gpui::test]
fn panes_follow_the_layout(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // Arrange: left (Channel Strip, 220) and bottom (260) shown, right
    // hidden.
    let left = s.group(LEFT).expect("the left pane");
    let bottom = s.group(BOTTOM).expect("the bottom pane");
    assert!(s.group(RIGHT).is_none());
    assert_eq!(left.origin.x, px(0.));
    assert_eq!(left.size.width, px(220.));
    let headers = s.group("Track Headers").unwrap();
    // The centre starts after the left pane and its 1-point divider.
    assert_eq!(headers.origin.x, px(221.));
    assert_eq!(bottom.origin.x, px(221.));
    assert_eq!(bottom.size.height, px(260.));
    // The left pane runs the regions' full height, the bottom pane's too.
    assert_eq!(
        left.origin.y + left.size.height,
        bottom.origin.y + bottom.size.height
    );
    // Its picker shows its view; with nothing selected, the strip's note.
    assert_eq!(
        s.picker(LEFT).and_then(|e| e.value).as_deref(),
        Some("Channel Strip")
    );
    assert!(s.find("text", STRIP_NOTE).is_some());

    // A selected track: its strip (Chords, a MIDI track with no
    // destination: "No Destination").
    s.update_model(|d| d.select_track(31, false));
    assert!(s.find("text", STRIP_NOTE).is_none());
    assert!(s.find("slider", "Fader of No Destination").is_some());

    // The right pane: at the right edge, the bottom pane beside it.
    s.run_menu("ui.toggle_right");
    let right = s.group(RIGHT).expect("the right pane");
    let bottom = s.group(BOTTOM).unwrap();
    assert_eq!(right.size.width, px(260.));
    assert_eq!(right.origin.x + right.size.width, px(1440.));
    assert_eq!(bottom.origin.x + bottom.size.width + px(1.), right.origin.x);
    // It runs beside the bottom pane only (the centre takes the room
    // above it, Swift layout A).
    assert_eq!(right.origin.y, bottom.origin.y);
    assert_eq!(
        s.picker(RIGHT).and_then(|e| e.value).as_deref(),
        Some("Controllers")
    );
    // A track is selected: its Strip Chart.
    assert!(s.find("text", CONTROLLERS_NOTE).is_none());
    assert!(s.find("group", "Strip Chart").is_some());

    // Hidden: the centre takes the room.
    s.run_menu("ui.toggle_left");
    assert!(s.group(LEFT).is_none());
    assert_eq!(s.group("Track Headers").unwrap().origin.x, px(0.));
    assert!(s.find("text", STRIP_NOTE).is_none());
    // A size set through the layout shows, clamped to its range.
    s.run_menu("ui.toggle_left");
    s.update_model(|d| d.update_layout(|l| l.views.arrange.left.size = 300.));
    assert_eq!(s.group(LEFT).unwrap().size.width, px(300.));
    s.update_model(|d| d.update_layout(|l| l.views.arrange.left.size = 5.));
    assert_eq!(s.group(LEFT).unwrap().size.width, px(160.));
}

#[gpui::test]
fn a_small_window_hides_panes_for_the_frame_only(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.run_menu("ui.toggle_right");
    // Narrower: the right pane shrinks first, then the left.
    s.resize(600., 640.);
    let left = s.group(LEFT).expect("still shown");
    let right = s.group(RIGHT).expect("still shown");
    assert_eq!(right.size.width, px(180.));
    assert_eq!(left.size.width, px(178.));
    // Narrower still: the right pane goes for this frame, the left gets
    // its size back.
    s.resize(500., 640.);
    assert!(s.group(RIGHT).is_none());
    assert_eq!(s.group(LEFT).unwrap().size.width, px(220.));
    assert!(s.pane(PaneSide::Right).shown, "the layout keeps it");
    // Too short for the bottom pane: the left goes first, then the bottom.
    s.resize(1000., 300.);
    assert!(s.group(LEFT).is_none());
    assert!(s.group(BOTTOM).is_none());
    assert!(s.group("Arrangement").is_some());
    assert!(s.pane(PaneSide::Left).shown && s.pane(PaneSide::Bottom).shown);
    // Room again: they come back at their sizes.
    s.resize(1000., 640.);
    assert_eq!(s.group(LEFT).unwrap().size.width, px(220.));
    assert_eq!(s.group(RIGHT).unwrap().size.width, px(260.));
    assert_eq!(s.group(BOTTOM).unwrap().size.height, px(260.));
}

#[test]
fn frames_port_the_swift_geometry() {
    let setup = vizdaw_app::editor::ViewLayouts::default().arrange;
    let f = panes::frames(1000., 560., &setup);
    assert_eq!(f.left, Some(panes::Rect::new(0., 0., 220., 560.)));
    assert_eq!(f.centre, panes::Rect::new(221., 0., 779., 299.));
    assert_eq!(f.bottom, Some(panes::Rect::new(221., 300., 779., 260.)));
    assert_eq!(f.right, None);
    assert!(f.hidden.is_empty());
    // Too small for anything but the centre: the right, the left, then
    // the bottom go, in that order.
    let mut all = setup;
    all.right.shown = true;
    let f = panes::frames(300., 200., &all);
    assert_eq!(
        f.hidden,
        vec![PaneSide::Right, PaneSide::Left, PaneSide::Bottom]
    );
    assert_eq!(f.centre, panes::Rect::new(0., 0., 300., 200.));
    // A drag is clamped to the range and to what leaves the centre its
    // minimum; a non-finite delta keeps the size.
    assert_eq!(
        panes::drag(&setup, PaneSide::Left, 1000., 1000., 560.),
        480.
    );
    assert_eq!(
        panes::drag(&setup, PaneSide::Left, -1000., 1000., 560.),
        160.
    );
    assert_eq!(
        panes::drag(&setup, PaneSide::Right, -40., 1000., 560.),
        300.
    );
    assert_eq!(
        panes::drag(&setup, PaneSide::Bottom, 500., 1000., 560.),
        399.
    );
    assert_eq!(
        panes::drag(&setup, PaneSide::Bottom, f64::NAN, 1000., 560.),
        260.
    );
}

#[gpui::test]
fn a_divider_drag_clamps_and_persists(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let height = s.group(BOTTOM).unwrap().size.height;
    assert_eq!(
        s.element("slider", "Bottom Pane Size").value.as_deref(),
        Some("260 points")
    );
    // Up 100: the size is set once, at the drag's end.
    let during = s.drag_divider(BOTTOM, 0., -100.);
    assert_eq!(during, 260., "nothing changes while dragging");
    assert_eq!(s.pane(PaneSide::Bottom).size, 360.);
    assert_eq!(s.group(BOTTOM).unwrap().size.height, height + px(100.));
    // Far up: the centre keeps its 160 points.
    s.drag_divider(BOTTOM, 0., -2000.);
    let bottom = s.group(BOTTOM).unwrap();
    let canvas = s.group("Arrangement").unwrap();
    assert_eq!(
        canvas.origin.y + canvas.size.height + px(1.),
        bottom.origin.y
    );
    let size = s.pane(PaneSide::Bottom).size;
    assert!(size < 900. && size > 360., "{size}");
    // The left pane's divider, far left: its least, 160.
    s.drag_divider(LEFT, -500., 0.);
    assert_eq!(s.pane(PaneSide::Left).size, 160.);
    assert_eq!(s.group(LEFT).unwrap().size.width, px(160.));
    // A press and release without a move changes nothing.
    let at = s.element("slider", "Left Pane Size").bounds.center();
    s.press(at);
    s.release(at);
    assert_eq!(s.pane(PaneSide::Left).size, 160.);
    // The right pane's, dragged left, grows it.
    s.run_menu("ui.toggle_right");
    s.drag_divider(RIGHT, -40., 0.);
    assert_eq!(s.pane(PaneSide::Right).size, 300.);
    // The sizes are the layout's: saved with it.
    let saved = s.model(|d| serde_json::to_value(&d.layout().views).unwrap());
    assert_eq!(saved["arrange"]["left"]["size"], 160.);
    assert_eq!(saved["arrange"]["right"]["size"], 300.);
}

#[gpui::test]
fn pane_pickers_choose_views(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click_picker(LEFT);
    assert_eq!(s.menu_items(), ["Channel Strip", "Sequences", "Inspector"]);
    assert!(s.element("menuitem", "Channel Strip").selected);
    s.click("menuitem", "Sequences");
    assert!(s.menu_items().is_empty(), "the menu closes");
    assert_eq!(s.pane(PaneSide::Left).view, PaneView::Sequences);
    assert_eq!(
        s.picker(LEFT).and_then(|e| e.value).as_deref(),
        Some("Sequences")
    );
    assert!(
        s.find("text", "Sequences comes in a later version.")
            .is_some()
    );

    s.run_menu("ui.toggle_right");
    s.click_picker(RIGHT);
    assert_eq!(
        s.menu_items(),
        ["Controllers", "Media Pool", "Plug-ins", "Markers"]
    );
    s.click("menuitem", "Markers");
    assert_eq!(s.pane(PaneSide::Right).view, PaneView::Markers);
    assert!(s.find("text", "No markers or locators.").is_some());

    // The bottom pane lists the editors, then its other views.
    s.click_picker(BOTTOM);
    assert_eq!(
        s.menu_items(),
        ["Piano Roll", "List", "Strip Chart", "Audio Editor", "Mixer"]
    );
    assert!(s.element("menuitem", "Piano Roll").selected);
    s.click("menuitem", "Mixer");
    assert_eq!(s.pane(PaneSide::Bottom).view, PaneView::Mixer);
    assert_eq!(
        s.picker(BOTTOM).and_then(|e| e.value).as_deref(),
        Some("Mixer")
    );
    let mixer = s.group("Mixer").expect("the bottom pane's mixer");
    let bottom = s.group(BOTTOM).unwrap();
    assert!(bottom.contains(&mixer.center()));
    // The editor stays mounted, out of the tree.
    assert!(s.find("text", "No Editor Open").is_none());
    // An editor kind shows the editor again, in that kind, on the shown
    // sequence.
    s.click_picker(BOTTOM);
    s.click("menuitem", "List");
    assert_eq!(s.pane(PaneSide::Bottom).view, PaneView::Editor);
    assert_eq!(
        s.model(|d| d.layout().detail.as_ref().map(|d| d.kind)),
        Some(DetailEditorKind::List)
    );
    assert!(s.group("Mixer").is_none());
    assert!(s.find("table", "Event List").is_some());
    // Under another view the open editor is out of the tree.
    s.click_picker(BOTTOM);
    s.click("menuitem", "Mixer");
    assert!(s.find("table", "Event List").is_none());
    assert!(s.find("button", "Close Detail Editor").is_none());
    s.click_picker(BOTTOM);
    s.click("menuitem", "List");
    assert!(s.find("table", "Event List").is_some());

    // The Mix view's bottom pane has no Mixer.
    s.run_menu("ui.view_mix");
    s.run_menu("ui.toggle_bottom");
    s.click_picker(BOTTOM);
    assert_eq!(
        s.menu_items(),
        ["Piano Roll", "List", "Strip Chart", "Audio Editor"]
    );
    s.click("menuitem", "Audio Editor");
    assert_eq!(s.pane(PaneSide::Bottom).view, PaneView::AudioEditor);
    assert!(
        s.find("text", "Audio Editor comes in a later version.")
            .is_some()
    );
    // Escape closes an open picker.
    s.click_picker(BOTTOM);
    assert!(!s.menu_items().is_empty());
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert!(s.menu_items().is_empty());
}

#[gpui::test]
fn the_transport_bar_switches_views_and_panes(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.app.update(&mut s.cx, |a, cx| {
        let bar = TransportLayout::parse("cluster,main_view,panes").unwrap();
        a.prefs.set_transport_bar(&bar).unwrap();
        cx.notify();
    });
    s.cx.run_until_parked();
    assert!(s.group("Main View").is_some());
    assert!(s.element("button", "Arrange").selected);
    assert!(!s.element("button", "Mix").selected);
    let value = |s: &mut Setup, side| s.toggle(side).and_then(|e| e.value);
    assert_eq!(value(&mut s, LEFT).as_deref(), Some("on"));
    assert_eq!(value(&mut s, BOTTOM).as_deref(), Some("on"));
    assert_eq!(value(&mut s, RIGHT).as_deref(), Some("off"));

    let at = s.toggle(RIGHT).unwrap().bounds.center();
    s.click_at(at);
    assert!(s.pane(PaneSide::Right).shown);
    assert_eq!(value(&mut s, RIGHT).as_deref(), Some("on"));
    assert!(s.group(RIGHT).is_some());

    s.click("button", "Mix");
    assert_eq!(s.model(|d| d.layout().views.main), MainView::Mix);
    assert!(s.element("button", "Mix").selected);
    assert!(s.group("Arrangement").is_none());
    // The toggles show the Mix view's panes.
    for side in [LEFT, BOTTOM, RIGHT] {
        assert_eq!(value(&mut s, side).as_deref(), Some("off"), "{side}");
    }
    let at = s.toggle(LEFT).unwrap().bounds.center();
    s.click_at(at);
    assert!(s.model(|d| d.layout().views.mix.left.shown));
    assert!(s.group(LEFT).is_some());

    s.click("button", "Arrange");
    assert_eq!(s.model(|d| d.layout().views.main), MainView::Arrange);
    assert!(s.group("Arrangement").is_some());

    // Folded into » (the standard bar in a narrow window): the pane
    // toggles are its items.
    s.app.update(&mut s.cx, |a, cx| {
        a.prefs.reset_transport_bar().unwrap();
        cx.notify();
    });
    s.resize(700., 640.);
    assert!(s.toggle(LEFT).is_none());
    s.click("button", "More Transport Items");
    let items = s.menu_items();
    for title in ["Main View", LEFT, BOTTOM, RIGHT] {
        assert!(items.iter().any(|i| i == title), "{title} in {items:?}");
    }
    assert!(s.element("menuitem", LEFT).selected);
    s.click("menuitem", LEFT);
    assert!(!s.pane(PaneSide::Left).shown);
}

#[gpui::test]
fn a_view_change_commits_the_track_rename(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let lane = 31;
    s.window
        .update(&mut s.cx, |v, window, cx| v.begin_rename(lane, window, cx))
        .unwrap();
    s.cx.run_until_parked();
    assert!(s.find("button", CHORDS).is_some());
    s.cx.simulate_input("Keys");
    s.cx.run_until_parked();
    s.run_menu("ui.view_mix");
    assert_eq!(s.renames(), vec!["Keys".to_string()]);
    assert_eq!(
        s.window.read_with(&s.cx, |v, _| v.renaming()).unwrap(),
        None
    );
    // A pane change does too.
    s.run_menu("ui.view_arrange");
    s.window
        .update(&mut s.cx, |v, window, cx| v.begin_rename(30, window, cx))
        .unwrap();
    s.cx.run_until_parked();
    s.cx.simulate_input("Beat");
    s.cx.run_until_parked();
    s.run_menu("ui.toggle_left");
    assert_eq!(s.renames(), vec!["Keys".to_string(), "Beat".to_string()]);
}

#[gpui::test]
fn a_divider_press_over_the_canvas_only_resizes(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let before = s.model(|d| d.selection().clone());
    let calls = s.fake.calls().len();
    // 2 points above the bottom divider's line: inside its grip and the
    // canvas's last points.
    let line = s.element("slider", "Bottom Pane Size").bounds;
    let canvas = s.group("Arrangement").unwrap();
    let at = point(canvas.center().x, line.center().y - px(2.));
    assert!(canvas.contains(&at), "the grip overlaps the canvas");
    // Up and across the tracks: a press the canvas took would select.
    let to = point(at.x - px(200.), canvas.origin.y + px(30.));
    s.press(at);
    s.move_to(point(at.x - px(100.), at.y - px(100.)));
    s.move_to(to);
    s.release(to);
    assert!(s.pane(PaneSide::Bottom).size > 260.);
    assert_eq!(s.model(|d| d.selection().clone()), before);
    assert_eq!(s.fake.calls().len(), calls, "{:?}", s.fake.calls());
}

#[gpui::test]
fn a_main_view_change_cancels_a_divider_drag(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // Both views show the left pane, so the drag's pane stays drawn.
    s.update_model(|d| d.update_layout(|l| l.views.mix.left.shown = true));
    let at = s.element("slider", "Left Pane Size").bounds.center();
    s.press(at);
    s.move_to(point(at.x + px(60.), at.y));
    s.run_menu("ui.view_mix");
    s.move_to(point(at.x + px(70.), at.y));
    s.release(point(at.x + px(70.), at.y));
    let l = s.model(|d| d.layout().views.clone());
    assert_eq!(l.arrange.left.size, 220., "the drag was dropped");
    assert_eq!(l.mix.left.size, 220., "nothing lands in the Mix view");
}

#[test]
fn the_window_opens_at_the_swift_size() {
    use vizdaw_gpui::shell::default_window_size;
    let big = Bounds::new(point(px(0.), px(0.)), size(px(2560.), px(1415.)));
    assert_eq!(default_window_size(Some(big)), size(px(1440.), px(900.)));
    // A small screen: 90% of it, never under 980×620 where it holds that.
    let small = Bounds::new(point(px(0.), px(25.)), size(px(1280.), px(775.)));
    assert_eq!(default_window_size(Some(small)), size(px(1152.), px(697.5)));
    let tiny = Bounds::new(point(px(0.), px(0.)), size(px(1000.), px(600.)));
    assert_eq!(default_window_size(Some(tiny)), size(px(980.), px(600.)));
    // Never under the window's minimum, 700×450.
    let wee = Bounds::new(point(px(0.), px(0.)), size(px(500.), px(300.)));
    assert_eq!(default_window_size(Some(wee)), size(px(700.), px(450.)));
    // No display known: the Swift size.
    assert_eq!(default_window_size(None), size(px(1440.), px(900.)));
}

#[gpui::test]
fn a_new_window_opens_at_the_swift_size(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // The test display is 1920×1080.
    let shown = s
        .window
        .update(&mut s.cx, |_, window, _| window.viewport_size())
        .unwrap();
    assert_eq!(shown, size(px(1440.), px(900.)));
}

#[gpui::test]
fn a_divider_press_over_a_track_header_selects_nothing(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let line = s.element("slider", "Left Pane Size").bounds;
    let row = s.element("button", CHORDS).bounds;
    // 2 points right of the left divider's line, on a header row.
    let at = point(line.center().x + px(2.), row.center().y);
    assert!(row.contains(&at), "the grip overlaps the header");
    s.press(at);
    s.release(at);
    assert!(s.model(|d| d.selected_lanes()).is_empty());
    // A drag from there only resizes.
    s.press(at);
    s.move_to(point(at.x + px(40.), at.y));
    s.release(point(at.x + px(40.), at.y));
    assert_eq!(s.pane(PaneSide::Left).size, 260.);
    assert!(s.model(|d| d.selected_lanes()).is_empty());
}

#[gpui::test]
fn a_divider_release_before_the_next_frame_ends_the_drag(cx: &mut TestAppContext) {
    use gpui::InputEvent;
    let mut s = setup(cx);
    let at = s.element("slider", "Left Pane Size").bounds.center();
    // The press and its release in one go: no frame is drawn between.
    s.cx.update(|window, cx| {
        let down = MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        };
        let up = MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count: 1,
        };
        window.dispatch_event(down.to_platform_input(), cx);
        window.dispatch_event(up.to_platform_input(), cx);
    });
    s.cx.run_until_parked();
    // Later moves and a mouse-up elsewhere resize nothing.
    s.move_to(point(at.x + px(60.), at.y));
    s.release(point(at.x + px(60.), at.y));
    assert_eq!(s.pane(PaneSide::Left).size, 220.);
}

#[gpui::test]
fn a_drag_ends_when_its_pane_hides(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let at = s.element("slider", "Left Pane Size").bounds.center();
    s.press(at);
    s.move_to(point(at.x + px(60.), at.y));
    // The pane goes and comes back mid-drag, between two frames (two
    // channel calls in one tick).
    s.update_model(|d| {
        d.update_layout(|l| l.views.arrange.left.shown = false);
        d.update_layout(|l| l.views.arrange.left.shown = true);
    });
    assert!(s.group(LEFT).is_some());
    // The release went unseen; the pointer hovers on, and a click lands
    // elsewhere: no size is set.
    let elsewhere = s.group("Arrangement").unwrap().center();
    s.cx.simulate_event(MouseMoveEvent {
        position: point(at.x + px(90.), at.y),
        modifiers: Modifiers::none(),
        pressed_button: None,
    });
    s.cx.run_until_parked();
    s.click_at(elsewhere);
    assert_eq!(s.pane(PaneSide::Left).size, 220.);
    assert_eq!(s.group(LEFT).unwrap().size.width, px(220.));
}

#[gpui::test]
fn a_drag_ends_when_it_leaves_the_frame(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let at = s.element("slider", "Left Pane Size").bounds.center();
    s.press(at);
    s.move_to(point(at.x + px(60.), at.y));
    // Too short for the left pane for a frame, then room again.
    s.resize(1440., 300.);
    assert!(s.group(LEFT).is_none());
    s.resize(1440., 900.);
    s.move_to(point(at.x + px(90.), at.y));
    s.release(point(at.x + px(90.), at.y));
    assert_eq!(s.pane(PaneSide::Left).size, 220.);
}

#[gpui::test]
fn a_move_without_the_button_ends_the_drag(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let at = s.element("slider", "Left Pane Size").bounds.center();
    s.press(at);
    s.move_to(point(at.x + px(60.), at.y));
    // The release was lost (outside the window): the next move has no
    // button, and the drag ends there, setting nothing.
    s.cx.simulate_event(MouseMoveEvent {
        position: point(at.x + px(80.), at.y),
        modifiers: Modifiers::none(),
        pressed_button: None,
    });
    s.cx.run_until_parked();
    s.move_to(point(at.x + px(100.), at.y));
    s.release(point(at.x + px(100.), at.y));
    assert_eq!(s.pane(PaneSide::Left).size, 220.);
}

#[gpui::test]
fn a_drag_ends_when_its_pane_changes(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let at = s.element("slider", "Left Pane Size").bounds.center();
    s.press(at);
    s.move_to(point(at.x + px(60.), at.y));
    // `ui.pane` sets the size mid-drag: it stands.
    s.update_model(|d| d.update_layout(|l| l.views.arrange.left.size = 300.));
    s.move_to(point(at.x + px(90.), at.y));
    s.release(point(at.x + px(90.), at.y));
    assert_eq!(s.pane(PaneSide::Left).size, 300.);
}
