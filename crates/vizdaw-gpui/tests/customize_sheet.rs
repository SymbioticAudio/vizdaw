//! View › Customize Transport Bar… (slice 9c Task 8): the sheet shows
//! the draft bar, an inspector and the palette with Swift's strings;
//! items add by a double click, move along the bar and leave it by
//! drags; the inspector sets a readout's size and a command button's
//! command and label; Cancel writes nothing; Done writes the
//! preference and every window's bar redraws; a failed write is shown.

use std::sync::Arc;

use gpui::{
    Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext,
    WindowHandle, point, px,
};
use serde_json::Value;
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_app::transport_bar::TransportLayout;
use vizdaw_app::{DocumentStore, Prefs};
use vizdaw_gpui::{AppState, DocumentWindow, RunCommand};

struct Setup {
    app: gpui::Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

/// A demo window whose bar is `bar` (`ui.set_transport_bar`'s text), over
/// in-memory preferences.
fn setup(cx: &mut TestAppContext, bar: &str) -> Setup {
    let fake = Arc::new(demo_fake());
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        app.update(cx, |s, _| s.set_prefs(Prefs::in_memory()));
        let window = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, window)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    let mut s = Setup {
        app,
        window,
        cx: vcx,
    };
    s.set_bar(bar);
    s
}

/// A node of a window's tree.
struct Node {
    role: String,
    label: String,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    enabled: bool,
    selected: bool,
    value: Option<String>,
}

impl Node {
    fn centre(&self) -> gpui::Point<gpui::Pixels> {
        point(px(self.x + self.w / 2.), px(self.y + self.h / 2.))
    }
}

fn nodes_of(window: WindowHandle<DocumentWindow>, cx: &VisualTestContext) -> Vec<Node> {
    let tree = window
        .read_with(cx, |v, _| v.registry().borrow().tree())
        .unwrap();
    tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n: &Value| Node {
            role: n["role"].as_str().unwrap().into(),
            label: n["label"].as_str().unwrap().into(),
            x: n["frame"]["x"].as_f64().unwrap() as f32,
            y: n["frame"]["y"].as_f64().unwrap() as f32,
            w: n["frame"]["width"].as_f64().unwrap() as f32,
            h: n["frame"]["height"].as_f64().unwrap() as f32,
            enabled: n["enabled"] == true,
            selected: n["selected"] == true,
            value: n["value"].as_str().map(Into::into),
        })
        .collect()
}

impl Setup {
    fn set_bar(&mut self, items: &str) {
        let bar = TransportLayout::parse(items).unwrap();
        self.app.update(&mut self.cx, |a, cx| {
            a.prefs.set_transport_bar(&bar).unwrap();
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    /// The stored bar's text.
    fn stored(&mut self) -> String {
        self.app
            .read_with(&self.cx, |a, _| a.prefs.transport_bar().tokens())
    }

    fn nodes(&mut self) -> Vec<Node> {
        nodes_of(self.window, &self.cx)
    }

    fn find(&mut self, role: &str, label: &str) -> Option<Node> {
        self.nodes()
            .into_iter()
            .find(|n| n.role == role && n.label == label)
    }

    fn expect(&mut self, role: &str, label: &str) -> Node {
        self.find(role, label).unwrap_or_else(|| {
            let all: Vec<_> = self
                .nodes()
                .into_iter()
                .map(|n| format!("{} {:?}", n.role, n.label))
                .collect();
            panic!("no {role} {label:?} in {all:?}")
        })
    }

    /// The bar's chips, left to right (the palette's carry "not on the
    /// bar").
    fn bar(&mut self) -> Vec<String> {
        self.nodes()
            .into_iter()
            .filter(|n| n.role == "chip" && !n.label.ends_with(", not on the bar"))
            .map(|n| n.label)
            .collect()
    }

    fn palette(&mut self) -> Vec<String> {
        self.nodes()
            .into_iter()
            .filter(|n| n.role == "chip")
            .filter_map(|n| n.label.strip_suffix(", not on the bar").map(String::from))
            .collect()
    }

    fn open(&mut self) {
        self.cx
            .dispatch_action(RunCommand::new("ui.customize_transport"));
        self.cx.run_until_parked();
        assert_eq!(self.sheet().as_deref(), Some("Customize Transport Bar"));
    }

    fn sheet(&mut self) -> Option<String> {
        self.window
            .read_with(&self.cx, |v, _| v.sheet_label())
            .unwrap()
    }

    fn click(&mut self, role: &str, label: &str) {
        let at = self.expect(role, label).centre();
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn double_click(&mut self, role: &str, label: &str) {
        let at = self.expect(role, label).centre();
        for n in [1, 2] {
            self.cx.simulate_event(MouseDownEvent {
                position: at,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count: n,
                first_mouse: false,
            });
            self.cx.simulate_event(MouseUpEvent {
                position: at,
                modifiers: Modifiers::none(),
                button: MouseButton::Left,
                click_count: n,
            });
        }
        self.cx.run_until_parked();
    }

    /// Presses on `from`, moves in steps, releases at `to`.
    fn drag(&mut self, from: gpui::Point<gpui::Pixels>, to: gpui::Point<gpui::Pixels>) {
        self.cx
            .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        for i in 1..=6 {
            let t = i as f32 / 6.;
            let p = point(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t);
            self.cx
                .simulate_mouse_move(p, MouseButton::Left, Modifiers::none());
        }
        self.cx
            .simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn drag_chip(&mut self, from: &str, onto: &str) {
        let a = self.expect("chip", from).centre();
        let b = self.expect("chip", onto);
        // The left half of the target: a drop there goes before it.
        self.drag(a, point(px(b.x + 6.), px(b.y + b.h / 2.)));
    }

    fn keys(&mut self, keys: &str) {
        self.cx.simulate_keystrokes(keys);
        self.cx.run_until_parked();
    }
}

fn texts(s: &mut Setup) -> Vec<String> {
    s.nodes()
        .into_iter()
        .filter(|n| n.role == "text")
        .map(|n| n.label)
        .collect()
}

#[gpui::test]
fn sheet_shows_bar_inspector_and_palette(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop,bar:large");
    s.open();
    let sheet = s.expect("sheet", "Customize Transport Bar");
    // The node is the card inside its 1 point border.
    assert!((sheet.w - 760.).abs() <= 2., "Swift's width: {}", sheet.w);
    assert!(sheet.h >= 478., "Swift's minimum height: {}", sheet.h);
    let t = texts(&mut s);
    for want in [
        "Customize Transport Bar",
        "Drag items onto the bar, along it, or back to the palette to remove them.",
        "Bar",
        "Palette",
        "Select an item on the bar to set its size or its command.",
    ] {
        assert!(t.iter().any(|x| x == want), "no {want:?} in {t:?}");
    }
    s.expect("group", "Transport Bar Items");
    s.expect("group", "Palette");
    assert_eq!(s.bar(), ["Transport Controls", "Loop", "Position in Bars"]);
    let palette = s.palette();
    assert_eq!(palette.first().map(String::as_str), Some("Click"));
    assert!(!palette.contains(&"Loop".to_owned()));
    assert_eq!(
        &palette[palette.len() - 3..],
        ["Flexible Space", "Separator", "Command Button"]
    );
    for n in s
        .nodes()
        .iter()
        .filter(|n| n.label.ends_with(", not on the bar"))
    {
        assert!(n.w >= 140., "{} is {} wide", n.label, n.w);
    }
    for b in ["Reset to Default", "Cancel", "Done"] {
        assert!(s.expect("button", b).enabled);
    }
}

#[gpui::test]
fn one_sheet_at_a_time_and_in_a_mixer_window(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop");
    s.open();
    // A second request while it is up does nothing.
    s.cx.dispatch_action(RunCommand::new("ui.customize_transport"));
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some("Customize Transport Bar"));
    s.click("button", "Cancel");
    assert_eq!(s.sheet(), None);
    // A mixer window takes it too.
    let doc = s.window.read_with(&s.cx, |v, _| v.doc()).unwrap();
    let mixer =
        s.cx.update(|_, cx| DocumentWindow::open_mixer(&s.app, doc, cx))
            .unwrap();
    s.cx.run_until_parked();
    let mut m = VisualTestContext::from_window(mixer.into(), &s.cx);
    m.dispatch_action(RunCommand::new("ui.customize_transport"));
    m.run_until_parked();
    assert_eq!(
        mixer
            .read_with(&m, |v, _| v.sheet_label())
            .unwrap()
            .as_deref(),
        Some("Customize Transport Bar")
    );
}

#[gpui::test]
fn double_click_in_the_palette_adds_to_the_bar(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop");
    s.open();
    s.double_click("chip", "Click, not on the bar");
    assert_eq!(s.bar(), ["Transport Controls", "Loop", "Click"]);
    assert!(!s.palette().contains(&"Click".to_owned()));
    // The accessible action does the same.
    let at = s
        .nodes()
        .into_iter()
        .find(|n| n.role == "button" && n.label == "Add to Bar")
        .expect("an Add to Bar action")
        .centre();
    s.cx.simulate_click(at, Modifiers::none());
    s.cx.run_until_parked();
    assert_eq!(s.bar().len(), 4, "{:?}", s.bar());
    // Nothing is written until Done.
    assert_eq!(s.stored(), "cluster,loop");
}

#[gpui::test]
fn drag_along_the_bar_reorders(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop,bar:large");
    s.open();
    s.drag_chip("Position in Bars", "Loop");
    assert_eq!(s.bar(), ["Transport Controls", "Position in Bars", "Loop"]);
    // A palette chip dragged onto a bar chip goes in before it.
    s.drag_chip("Click, not on the bar", "Loop");
    assert_eq!(
        s.bar(),
        ["Transport Controls", "Position in Bars", "Click", "Loop"]
    );
    // Dropped on the end of the bar: it ends last.
    let a = s.expect("chip", "Transport Controls").centre();
    let end = s.expect("chip", "Loop");
    s.drag(a, point(px(end.x + end.w + 20.), px(end.y + end.h / 2.)));
    assert_eq!(
        s.bar(),
        ["Position in Bars", "Click", "Loop", "Transport Controls"]
    );
    assert_eq!(s.stored(), "cluster,loop,bar:large", "not written yet");
}

#[gpui::test]
fn drag_to_the_palette_removes(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop,bar:large");
    s.open();
    let from = s.expect("chip", "Loop").centre();
    let to = s.expect("group", "Palette").centre();
    s.drag(from, to);
    assert_eq!(s.bar(), ["Transport Controls", "Position in Bars"]);
    assert!(s.palette().contains(&"Loop".to_owned()));
    // The cluster cannot be dragged off.
    let from = s.expect("chip", "Transport Controls").centre();
    s.drag(from, to);
    assert_eq!(s.bar(), ["Transport Controls", "Position in Bars"]);
}

#[gpui::test]
fn chip_actions_remove_and_move(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop,bar:large,click");
    s.open();
    s.click("chip", "Loop");
    assert!(s.expect("chip", "Loop").selected);
    s.click("button", "Move Right");
    assert_eq!(
        s.bar(),
        ["Transport Controls", "Position in Bars", "Loop", "Click"]
    );
    assert!(s.expect("chip", "Loop").selected, "the selection follows");
    s.click("button", "Move Left");
    assert_eq!(
        s.bar(),
        ["Transport Controls", "Loop", "Position in Bars", "Click"]
    );
    s.click("button", "Remove");
    assert_eq!(s.bar(), ["Transport Controls", "Position in Bars", "Click"]);
    // The cluster has no Remove.
    s.click("chip", "Transport Controls");
    assert!(!s.expect("button", "Remove").enabled);
    assert!(!s.expect("button", "Move Left").enabled);
}

#[gpui::test]
fn inspector_sets_size_and_command_and_label(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,bar:large");
    s.open();
    // A sized item: Small and Large.
    s.click("chip", "Position in Bars");
    let t = texts(&mut s);
    assert!(t.contains(&"Position in Bars Size".to_owned()), "{t:?}");
    assert!(s.expect("button", "Large").selected);
    s.click("button", "Small");
    assert_eq!(s.bar(), ["Transport Controls", "Position in Bars (Small)"]);
    // An item with no settings says so.
    s.click("chip", "Transport Controls");
    assert!(texts(&mut s).contains(&"Transport Controls has no settings.".to_owned()));
    // A command button: pick its command, search, label it.
    s.double_click("chip", "Command Button, not on the bar");
    s.click("chip", "Command Button: Mixer");
    assert!(s.find("textfield", "Search Commands").is_some());
    s.click("textfield", "Search Commands");
    s.cx.simulate_input("save as");
    s.cx.run_until_parked();
    let spec = core_control::registry::find("file.save_as").unwrap();
    let row = format!("{}, {}", spec.title, spec.menu);
    s.click("button", &row);
    assert_eq!(
        s.bar().last().map(String::as_str),
        Some(format!("Command Button: {}", spec.title).as_str())
    );
    assert!(s.expect("button", &row).selected, "the check shows");
    s.click("textfield", "Button Label");
    s.cx.simulate_input("Save Copy");
    s.click("button", "Set Label");
    assert_eq!(
        s.bar().last().map(String::as_str),
        Some("Command Button: Save Copy")
    );
    s.click("button", "Done");
    assert_eq!(s.sheet(), None);
    assert_eq!(
        s.stored(),
        "cluster,bar:small,command:file.save_as:Save Copy"
    );
}

#[gpui::test]
fn a_long_label_is_refused_and_the_sheet_stays(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster");
    s.open();
    s.double_click("chip", "Command Button, not on the bar");
    s.click("chip", "Command Button: Mixer");
    s.click("textfield", "Button Label");
    s.cx.simulate_input(&"x".repeat(33));
    s.cx.run_until_parked();
    assert!(texts(&mut s).contains(&"A label is at most 32 characters.".to_owned()));
    assert!(!s.expect("button", "Set Label").enabled);
    // Done does not take it: the sheet stays, the bar is unwritten.
    s.click("button", "Done");
    assert_eq!(s.sheet().as_deref(), Some("Customize Transport Bar"));
    assert_eq!(s.stored(), "cluster");
    assert!(texts(&mut s).contains(&"A label is at most 32 characters.".to_owned()));
}

#[gpui::test]
fn cancel_writes_nothing(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop");
    s.open();
    s.double_click("chip", "Click, not on the bar");
    s.click("button", "Cancel");
    assert_eq!(s.sheet(), None);
    assert_eq!(s.stored(), "cluster,loop");
    // Esc is Cancel.
    s.open();
    s.click("chip", "Loop");
    s.click("button", "Remove");
    s.keys("escape");
    assert_eq!(s.sheet(), None);
    assert_eq!(s.stored(), "cluster,loop");
    assert!(
        s.app
            .read_with(&s.cx, |a, _| a.prefs.raw("transportBar").is_some())
    );
    // The sheet opens again on the stored bar.
    s.open();
    assert_eq!(s.bar(), ["Transport Controls", "Loop"]);
}

#[gpui::test]
fn done_writes_and_every_bar_redraws(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop,bar:large");
    let doc = s.window.read_with(&s.cx, |v, _| v.doc()).unwrap();
    let mixer =
        s.cx.update(|_, cx| DocumentWindow::open_mixer(&s.app, doc, cx))
            .unwrap();
    s.cx.run_until_parked();
    let other = VisualTestContext::from_window(mixer.into(), &s.cx);
    let has_loop = |c: &VisualTestContext| {
        nodes_of(mixer, c)
            .iter()
            .any(|n| n.role == "button" && n.label == "Loop")
    };
    assert!(has_loop(&other), "the mixer window's bar shows Loop");
    s.open();
    s.click("chip", "Loop");
    s.click("button", "Remove");
    assert_eq!(s.stored(), "cluster,loop,bar:large", "not until Done");
    s.keys("enter");
    assert_eq!(s.sheet(), None);
    assert_eq!(s.stored(), "cluster,bar:large");
    other.run_until_parked();
    assert!(!has_loop(&other), "the other window's bar redrew");
    assert!(
        !nodes_of(s.window, &s.cx)
            .iter()
            .any(|n| n.role == "button" && n.label == "Loop"),
        "and this window's"
    );
}

#[gpui::test]
fn a_failed_write_is_shown_and_the_sheet_stays(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop");
    let dir = std::env::temp_dir().join(format!("vizdaw-customize-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    s.open();
    s.app.update(&mut s.cx, |a, _| {
        a.prefs = Prefs::at(blocker.join("prefs.json"));
    });
    s.click("chip", "Loop");
    s.click("button", "Remove");
    s.click("button", "Done");
    assert_eq!(s.sheet().as_deref(), Some("Customize Transport Bar"));
    let problem = s
        .nodes()
        .into_iter()
        .find(|n| n.role == "text" && n.value.as_deref() == Some("problem"))
        .expect("the problem line");
    assert!(!problem.label.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn reset_to_default(cx: &mut TestAppContext) {
    let mut s = setup(cx, "cluster,loop");
    s.open();
    s.click("button", "Reset to Default");
    let standard = TransportLayout::standard().items.len();
    assert_eq!(s.bar().len(), standard);
    assert!(s.bar().contains(&"Faults".to_owned()));
    // Cancel after a reset keeps the stored bar.
    s.click("button", "Cancel");
    assert_eq!(s.stored(), "cluster,loop");
    // Done after a reset stores the standard bar.
    s.open();
    s.click("button", "Reset to Default");
    s.click("button", "Done");
    assert_eq!(s.stored(), TransportLayout::standard().tokens());
    assert_eq!(
        s.app
            .read_with(&s.cx, |a, _| a.prefs.raw("transportBar").cloned()),
        Some(Value::String("standard".into()))
    );
}
