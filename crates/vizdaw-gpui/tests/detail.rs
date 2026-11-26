//! The bottom detail pane: its header (the kind picker, the title, the
//! close button), the empty state, the List slot, the editors commands
//! that open and close it, and its height in the window (Swift
//! `PaneGeometry`), driven through GPUI's test platform with the scripted
//! session.

use std::sync::Arc;

use gpui::{
    Bounds, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, TestAppContext,
    VisualTestContext, WindowHandle, point, px, size,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_ffi::blocks::{BlockKind, BlockRow};
use vizdaw_gpui::menus::{self, ItemSpec};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, DocumentWindow};

/// One bar of 4/4, in ticks.
const BAR: i64 = 61_440;
const RULER: f32 = 22.;
/// The arrangement's tool bar, above the canvas.
const TOOLBAR: f32 = 32.;

const PANE: &str = "Bottom Pane";
const CLOSE: &str = "Close Detail Editor";
const LIST_SLOT: &str = "Event List";
const EMPTY: &str = "No Editor Open";
const EMPTY_HELP: &str = "Double-click a block, or press \u{2318}1 to \u{2318}4.";

struct Setup {
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
        app,
        window,
        cx: vcx,
    }
}

/// The demo with a two-event block on Chords at bar 1.
fn with_block() -> FakeSession {
    let f = demo_fake();
    f.set_blocks(
        3,
        31,
        vec![BlockRow {
            start: 0,
            end: BAR,
            kind: BlockKind::Events,
            events: vec![1, 2],
            label: None,
            ref_seq: None,
            muted: false,
        }],
    );
    f
}

/// ⌘ on macOS, ⌃ elsewhere, as `menus::secondary!`.
fn secondary(key: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("cmd-{key}")
    } else {
        format!("ctrl-{key}")
    }
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

    fn pane(&mut self) -> Option<Bounds<Pixels>> {
        self.find("group", PANE).map(|e| e.bounds)
    }

    fn canvas(&mut self) -> Bounds<Pixels> {
        self.element("group", "Arrangement").bounds
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }

    /// The bottom pane's picker (not the transport bar's toggle, whose
    /// value is on or off).
    fn picker(&mut self) -> Element {
        self.elements()
            .into_iter()
            .find(|e| {
                e.role == "button"
                    && e.label == PANE
                    && !matches!(e.value.as_deref(), Some("on" | "off"))
            })
            .expect("the bottom pane's picker")
    }

    fn click_picker(&mut self) {
        let at = self.picker().bounds.center();
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let at = self.element(role, label).bounds.center();
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn double_click_canvas(&mut self, x: f32, y: f32) {
        let c = self.canvas().origin;
        let position = point(c.x + px(x), c.y + px(y));
        for count in [1, 2] {
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

    fn resize(&mut self, width: f32, height: f32) {
        self.cx.simulate_resize(size(px(width), px(height)));
        self.cx.run_until_parked();
    }

    /// The front document's menu item `title` in `menu`: (enabled, checked).
    fn menu_item(&mut self, menu: &str, title: &str) -> (bool, Option<bool>) {
        let keymap = self.app.read_with(&self.cx, |a, _| a.keymap.clone());
        let model = self.model(|d| menus::menu_model(Some(d), &keymap));
        model
            .menus
            .iter()
            .find(|m| m.title == menu)
            .unwrap_or_else(|| panic!("no menu {menu}"))
            .items
            .iter()
            .find_map(|i| match i {
                ItemSpec::Command {
                    title: t,
                    enabled,
                    checked,
                    ..
                } if t == title => Some((*enabled, *checked)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no item {title} in {menu}"))
    }

    fn run_menu(&mut self, id: &str) {
        let action = menus::RunCommand::new(id.to_owned());
        self.cx.dispatch_action(action);
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn editors_list_opens_the_list_on_the_shown_sequence(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    // Nothing open yet: the empty state, the pane shown.
    assert!(s.model(|d| d.layout().detail.is_none()));
    assert!(s.find("text", EMPTY).is_some());
    assert!(s.find("text", EMPTY_HELP).is_some());
    assert!(s.find("button", CLOSE).is_none());
    assert_eq!(s.menu_item("Editors", "List"), (true, None));
    assert!(!s.menu_item("Editors", CLOSE).0);

    s.keys(&secondary("2"));
    let detail = s.model(|d| d.layout().detail.clone()).expect("a detail");
    assert_eq!(detail.kind, DetailEditorKind::List);
    assert_eq!(detail.target, EditorTarget::Sequence { seq: 3 });
    assert_eq!(s.model(|d| d.detail_title()), "Song");
    assert!(s.find("text", "Song").is_some(), "the title shows");
    assert!(s.find("table", LIST_SLOT).is_some(), "the List slot");
    assert!(s.find("button", CLOSE).is_some());
    assert!(s.find("text", EMPTY).is_none());
    assert_eq!(s.picker().value.as_deref(), Some("List"));
    assert!(s.menu_item("Editors", CLOSE).0);

    // The pane's picker lists the Piano Roll and the List, then the
    // pane's other views (slice 6a); choosing the List keeps it.
    s.click_picker();
    let items: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| e.label)
        .collect();
    assert_eq!(items, ["Piano Roll", "List", "Audio Editor", "Mixer"]);
    assert!(s.element("menuitem", "List").selected);
    s.click("menuitem", "List");
    assert!(s.find("menuitem", "List").is_none(), "the menu closes");
    assert_eq!(
        s.model(|d| d.layout().detail.as_ref().map(|d| d.kind)),
        Some(DetailEditorKind::List)
    );
}

#[gpui::test]
fn a_double_clicked_block_opens_its_range(cx: &mut TestAppContext) {
    let mut s = setup(cx, with_block());
    s.double_click_canvas(50., RULER + 48. + 24.);
    assert_eq!(s.model(|d| d.detail_title()), "Song \u{203a} Chords, bar 1");
    assert!(s.find("text", "Song \u{203a} Chords, bar 1").is_some());
    // In the last-used editor: the Piano Roll at first.
    assert!(s.find("group", "Piano Roll").is_some());
    assert!(s.find("table", LIST_SLOT).is_none());
    // ⌘2 then keeps the block's target, in the List.
    s.keys(&secondary("2"));
    assert!(s.find("table", LIST_SLOT).is_some());
    assert_eq!(
        s.model(|d| d.layout().detail.as_ref().map(|d| d.target)),
        Some(EditorTarget::Block {
            seq: 3,
            lane: 31,
            start: 0,
            end: BAR
        })
    );
}

#[gpui::test]
fn closing_the_detail_shows_the_empty_state(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.run_menu("editors.list");
    assert!(s.find("table", LIST_SLOT).is_some());
    s.click("button", CLOSE);
    assert!(s.model(|d| d.layout().detail.is_none()));
    assert!(s.find("text", EMPTY).is_some());
    assert!(s.find("text", EMPTY_HELP).is_some());
    assert!(s.find("table", LIST_SLOT).is_none());
    assert!(s.find("button", CLOSE).is_none());
    assert!(!s.menu_item("Editors", CLOSE).0);
    // The pane stays shown.
    assert!(s.pane().is_some());
    // The menu item closes it too.
    s.run_menu("editors.list");
    s.run_menu("editors.close_detail");
    assert!(s.model(|d| d.layout().detail.is_none()));
}

#[gpui::test]
fn toggle_bottom_hides_and_shows_the_pane(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.run_menu("editors.list");
    let pane = s.pane().expect("shown by default");
    assert_eq!(pane.size.height, px(260.));
    let canvas = s.canvas();
    // The pane sits under the arrangement, a 1-point divider between.
    assert_eq!(pane.origin.y, canvas.origin.y + canvas.size.height + px(1.));
    assert_eq!(s.menu_item("View", PANE), (true, Some(true)));

    s.keys(if cfg!(target_os = "macos") {
        "ctrl-cmd-down"
    } else {
        "ctrl-down"
    });
    assert!(!s.model(|d| d.layout().views.arrange.bottom.shown));
    assert!(s.pane().is_none(), "hidden: out of the tree");
    assert!(s.find("table", LIST_SLOT).is_none());
    assert_eq!(s.menu_item("View", PANE), (true, Some(false)));
    // The arrangement takes the room.
    assert_eq!(s.canvas().size.height, canvas.size.height + px(261.));
    // The detail stays open.
    assert!(s.model(|d| d.layout().detail.is_some()));

    s.run_menu("ui.toggle_bottom");
    assert!(s.model(|d| d.layout().views.arrange.bottom.shown));
    assert_eq!(s.pane().map(|p| p.size.height), Some(px(260.)));
    assert!(s.find("table", LIST_SLOT).is_some());
}

#[gpui::test]
fn a_short_window_gives_the_arrangement_its_minimum(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.resize(1000., 440.);
    let pane = s.pane().expect("still fits");
    let canvas = s.canvas();
    // The centre (tool bar and canvas) keeps 160 points; the pane shrinks.
    assert_eq!(canvas.size.height + px(TOOLBAR), px(160.));
    assert!(pane.size.height < px(260.) && pane.size.height >= px(120.));
    assert_eq!(pane.origin.y, canvas.origin.y + canvas.size.height + px(1.));

    // Too short for both: the pane goes for this frame only.
    s.resize(1000., 300.);
    assert!(s.pane().is_none());
    assert!(s.model(|d| d.layout().views.arrange.bottom.shown));
    assert!(
        s.find("text", EMPTY).is_none(),
        "its content is out of the tree"
    );

    // A size past the range is clamped (900 at most).
    s.update_model(|d| d.update_layout(|l| l.views.arrange.bottom.size = 2000.));
    s.resize(1000., 1400.);
    assert_eq!(s.pane().map(|p| p.size.height), Some(px(900.)));
    s.update_model(|d| d.update_layout(|l| l.views.arrange.bottom.size = 10.));
    assert_eq!(s.pane().map(|p| p.size.height), Some(px(120.)));
}
