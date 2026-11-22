//! The document window: transport bar, shortcuts, title and the element
//! registry, driven through GPUI's test platform with the scripted session.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Context, Entity, Focusable, IntoElement, Modifiers, ParentElement, Pixels, Point, Render,
    Styled, TestAppContext, VisualTestContext, Window, WindowHandle, div, px,
};
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_app::{AlertItem, DocumentStore};
use vizdaw_gpui::registry::{Registry, RegistryScope};
use vizdaw_gpui::{AppState, DocumentWindow};

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

/// The app with one demo document window over `fake`.
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

/// The ID `ui.tree` gives the element with `role` and `label`.
fn element_id(s: &mut Setup, role: &str, label: &str) -> String {
    let tree = s
        .window
        .read_with(&s.cx, |v, _| v.registry().borrow().tree())
        .unwrap();
    tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["role"] == role && n["label"] == label)
        .unwrap_or_else(|| panic!("no {role} {label:?} in {tree}"))["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn center(s: &mut Setup, id: &str) -> Point<Pixels> {
    s.window
        .read_with(&s.cx, |v, _| {
            v.registry().borrow().resolve(id).unwrap().bounds.center()
        })
        .unwrap()
}

fn click(s: &mut Setup, role: &str, label: &str) {
    let id = element_id(s, role, label);
    let at = center(s, &id);
    s.cx.simulate_click(at, Modifiers::none());
    s.cx.run_until_parked();
}

#[gpui::test]
fn play_button_toggles_transport(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    click(&mut s, "button", "Play");
    assert_eq!(s.fake.count("play(3)"), 1);
    let playing = s.app.read_with(&s.cx, |a, _| {
        a.transport(s.window_doc()).unwrap().is_playing()
    });
    assert!(playing);
    // The button now reads Stop; pressing it stops.
    click(&mut s, "button", "Stop");
    assert_eq!(s.fake.count("stop"), 1);
}

#[gpui::test]
fn space_plays_when_no_text_field_is_focused(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.cx.simulate_keystrokes("space");
    assert_eq!(s.fake.count("play(3)"), 1);
    // S stops and returns to where playing started.
    s.cx.simulate_keystrokes("s");
    assert_eq!(s.fake.count("stop"), 1);
    assert_eq!(s.fake.count("locate(0)"), 1);
}

#[gpui::test]
fn space_types_into_a_focused_text_field(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let field = s
        .window
        .update(&mut s.cx, |v, window, cx| {
            let field = v.add_field("Name", cx);
            window.focus(&field.focus_handle(cx), cx);
            field
        })
        .unwrap();
    s.cx.run_until_parked();
    s.cx.simulate_keystrokes("s space");
    let text = field.read_with(&s.cx, |f, _| f.text().to_owned());
    assert_eq!(text, "s ");
    assert_eq!(s.fake.count("play(3)"), 0);
    assert_eq!(s.fake.count("stop"), 0);
}

#[gpui::test]
fn title_shows_the_dirty_mark(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    assert_eq!(s.cx.window_title().as_deref(), Some("Untitled"));
    let mut edited = s.fake.snap();
    edited.dirty = true;
    s.fake.bump(edited);
    let doc = s.window_doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.document_mut(doc).unwrap().undo();
        cx.notify();
    });
    s.cx.run_until_parked();
    assert_eq!(
        s.cx.window_title().as_deref(),
        Some("Untitled \u{2014} Edited")
    );
}

#[gpui::test]
fn registry_paths_follow_the_view_tree(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let before = element_id(&mut s, "button", "Play");
    // An alert banner is drawn above the transport bar: the bar moves down
    // one place in the view tree, so Play's path changes.
    s.app.update(&mut s.cx, |a, cx| {
        a.store.post_alert(AlertItem::new("Open Failed", "nope"));
        cx.notify();
    });
    s.cx.run_until_parked();
    let after = element_id(&mut s, "button", "Play");
    assert_ne!(before, after);
    let stale = s
        .window
        .read_with(&s.cx, |v, _| {
            v.registry()
                .borrow()
                .resolve(&before)
                .map(|_| ())
                .unwrap_err()
        })
        .unwrap();
    assert_eq!(stale.code.as_str(), "no_element");
    // The new ID clicks Play.
    let at = center(&mut s, &after);
    s.cx.simulate_click(at, Modifiers::none());
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("play(3)"), 1);
}

/// Two containers, each holding one button, drawn in either order.
struct Swappable {
    swapped: bool,
    registry: Rc<RefCell<Registry>>,
}

impl Render for Swappable {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let mut root = RegistryScope::root(&self.registry);
        let names = if self.swapped { ["B", "A"] } else { ["A", "B"] };
        let mut column = div().size_full();
        for name in names {
            let mut scope = root.container();
            column = column.child(
                div().size(px(40.)).child(
                    div()
                        .relative()
                        .size(px(20.))
                        .child(scope.control("button", name, true)),
                ),
            );
        }
        column
    }
}

#[gpui::test]
fn swapping_containers_swaps_their_paths(cx: &mut TestAppContext) {
    let registry: Rc<RefCell<Registry>> = Rc::default();
    let reg = registry.clone();
    let w = cx.add_window(move |_, _| Swappable {
        swapped: false,
        registry: reg,
    });
    cx.run_until_parked();
    let id_of = |label: &str| {
        let tree = registry.borrow().tree();
        tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["label"] == label)
            .unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let a = id_of("A");
    assert!(a.starts_with("0.0:"), "{a}");
    w.update(cx, |v, _, cx| {
        v.swapped = true;
        cx.notify();
    })
    .unwrap();
    cx.run_until_parked();
    assert!(id_of("A").starts_with("1.0:"));
    assert_eq!(
        registry.borrow().resolve(&a).unwrap_err().code.as_str(),
        "no_element"
    );
}

#[gpui::test]
fn save_on_an_untitled_document_asks_for_a_path(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.cx.dispatch_action(vizdaw_gpui::Save);
    s.cx.run_until_parked();
    assert!(s.cx.did_prompt_for_new_path());
    assert_eq!(s.fake.count("save"), 0);
    s.cx.simulate_new_path_selection(|dir| Some(dir.join("Song")));
    s.cx.run_until_parked();
    let calls = s.fake.calls();
    assert!(
        calls
            .iter()
            .any(|c| c.starts_with("save_as(") && c.ends_with("Song.vizdaw)")),
        "{calls:?}"
    );
    assert_eq!(s.cx.window_title().as_deref(), Some("Song"));
}

#[gpui::test]
fn an_alert_shows_until_dismissed(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    // A folder without the extension is refused with an alert.
    let opened =
        s.cx.update(|_, cx| vizdaw_gpui::open_path(Path::new("/tmp/folder"), cx));
    assert!(opened.is_none());
    s.cx.run_until_parked();
    click(&mut s, "button", "Dismiss");
    let left = s.app.read_with(&s.cx, |a, _| a.store.alert().cloned());
    assert_eq!(left, None);
}

impl Setup {
    fn window_doc(&self) -> vizdaw_app::DocId {
        self.window.read_with(&self.cx, |v, _| v.doc()).unwrap()
    }
}

#[gpui::test]
fn readouts_follow_the_poll_at_the_transport_interval(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let doc = s.window_doc();
    let bar = |s: &mut Setup| {
        s.app
            .read_with(&s.cx, |a, _| a.readout(doc).unwrap().texts.bar.clone())
    };
    s.cx.simulate_keystrokes("space");
    s.fake.set_transport(|t| t.position_bbu = "3.1.0".into());
    // Playing: polled every 33 ms.
    s.cx.executor().advance_clock(Duration::from_millis(40));
    s.cx.run_until_parked();
    assert_eq!(bar(&mut s), "3.1.0");
    s.fake.set_playing(false);
    s.cx.executor().advance_clock(Duration::from_millis(40));
    s.cx.run_until_parked();
    // Stopped: the interval is read again, now 250 ms.
    s.fake.set_transport(|t| t.position_bbu = "5.1.0".into());
    s.cx.executor().advance_clock(Duration::from_millis(100));
    s.cx.run_until_parked();
    assert_eq!(bar(&mut s), "3.1.0");
    s.cx.executor().advance_clock(Duration::from_millis(200));
    s.cx.run_until_parked();
    assert_eq!(bar(&mut s), "5.1.0");
}

#[gpui::test]
fn closing_an_edited_window_asks_first(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.set_dirty(true);
    let mut s = setup(cx, fake);
    s.cx.dispatch_action(vizdaw_gpui::CloseWindow);
    s.cx.run_until_parked();
    assert!(s.cx.has_pending_prompt());
    s.cx.simulate_prompt_answer("Cancel");
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("close"), 0);
    assert_eq!(s.app.read_with(&s.cx, |a, _| a.windows().len()), 1);

    s.cx.dispatch_action(vizdaw_gpui::CloseWindow);
    s.cx.run_until_parked();
    s.cx.simulate_prompt_answer("Don't Save");
    s.cx.run_until_parked();
    // The window's view is released on the next effect flush.
    s.cx.cx.update(|_| {});
    assert_eq!(s.fake.count("close"), 1);
    assert_eq!(s.fake.count("save"), 0);
    let left = s
        .app
        .read_with(&s.cx, |a, _| (a.windows().len(), a.store.count()));
    assert_eq!(left, (0, 0));
}

#[gpui::test]
fn quit_with_unsaved_changes_asks_first(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.set_dirty(true);
    let mut s = setup(cx, fake);
    let doc = s.window_doc();
    // The shortcut, dispatched from the front window.
    let quit_key = if cfg!(target_os = "macos") {
        "cmd-q"
    } else {
        "ctrl-q"
    };
    s.cx.simulate_keystrokes(quit_key);
    s.cx.run_until_parked();
    assert!(s.cx.has_pending_prompt());
    assert_eq!(s.fake.count("close"), 0);
    s.cx.simulate_prompt_answer("Cancel");
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("close"), 0);

    // The menu item, dispatched on the window.
    s.cx.dispatch_action(vizdaw_gpui::Quit);
    s.cx.run_until_parked();
    assert!(s.cx.has_pending_prompt());
    assert_eq!(s.fake.count("close"), 0);
    s.cx.simulate_prompt_answer("Discard Changes and Quit");
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("close"), 1);
    // Nothing is left to poll a closed session.
    let left = s
        .app
        .read_with(&s.cx, |a, _| (a.transport(doc).is_none(), a.store.count()));
    assert_eq!(left, (true, 0));
}

fn save_as_calls(s: &Setup) -> Vec<String> {
    s.fake
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("save_as("))
        .collect()
}

#[gpui::test]
fn save_as_asks_before_replacing_an_existing_project(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("Song.vizdaw")).unwrap();
    let typed = dir.path().join("Song");
    let target = dir.path().join("Song.vizdaw");
    let pick = typed.clone();
    s.cx.dispatch_action(vizdaw_gpui::SaveAs);
    s.cx.run_until_parked();
    assert!(s.cx.did_prompt_for_new_path());
    s.cx.simulate_new_path_selection(move |_| Some(pick.clone()));
    s.cx.run_until_parked();
    assert!(s.cx.has_pending_prompt());
    assert!(save_as_calls(&s).is_empty());
    s.cx.simulate_prompt_answer("Cancel");
    s.cx.run_until_parked();
    assert!(save_as_calls(&s).is_empty());

    let pick = typed.clone();
    s.cx.dispatch_action(vizdaw_gpui::SaveAs);
    s.cx.run_until_parked();
    s.cx.simulate_new_path_selection(move |_| Some(pick.clone()));
    s.cx.run_until_parked();
    assert!(s.cx.has_pending_prompt());
    s.cx.simulate_prompt_answer("Replace");
    s.cx.run_until_parked();
    assert_eq!(
        save_as_calls(&s),
        vec![format!("save_as({})", target.display())]
    );
}

#[gpui::test]
fn save_as_to_a_new_name_does_not_ask(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let dir = tempfile::tempdir().unwrap();
    let pick = dir.path().join("New");
    s.cx.dispatch_action(vizdaw_gpui::SaveAs);
    s.cx.run_until_parked();
    s.cx.simulate_new_path_selection(move |_| Some(pick.clone()));
    s.cx.run_until_parked();
    assert!(!s.cx.has_pending_prompt());
    assert_eq!(
        save_as_calls(&s),
        vec![format!(
            "save_as({})",
            dir.path().join("New.vizdaw").display()
        )]
    );
}
