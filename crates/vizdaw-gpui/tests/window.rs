//! The document window: transport bar, shortcuts, title and the element
//! registry, driven through GPUI's test platform with the scripted session.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Context, Entity, IntoElement, Modifiers, ParentElement, Pixels, Point, Render, Styled,
    TestAppContext, VisualTestContext, Window, WindowHandle, div, px,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeSession, demo_fake};
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
    // The Position field, its text all selected by the click.
    let at = s
        .window
        .read_with(&s.cx, |v, _| {
            let r = v.registry().borrow();
            let e = r.elements().iter().find(|e| e.label == "Position").cloned();
            e.map(|e| e.bounds.center())
        })
        .unwrap()
        .expect("the Position field");
    s.cx.simulate_click(at, Modifiers::none());
    s.cx.run_until_parked();
    s.cx.simulate_keystrokes("s space");
    s.cx.run_until_parked();
    let text = s
        .window
        .read_with(&s.cx, |v, _| {
            let r = v.registry().borrow();
            let e = r.elements().iter().find(|e| e.label == "Position").cloned();
            e.and_then(|e| e.value)
        })
        .unwrap();
    assert_eq!(text.as_deref(), Some("s "));
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
    // A disk banner is drawn before the transport bar: the bar moves one
    // place in the view tree, so Play's path changes.
    s.fake
        .set_transport(|t| t.faults = vec!["stream-underrun".into()]);
    for _ in 0..5 {
        s.cx.executor().advance_clock(Duration::from_millis(100));
        s.cx.run_until_parked();
    }
    s.cx.update(|window, _| window.refresh());
    s.cx.run_until_parked();
    assert!(
        s.window
            .read_with(&s.cx, |v, _| v
                .registry()
                .borrow()
                .elements()
                .iter()
                .any(|e| e.label == "Dismiss"))
            .unwrap(),
        "the disk banner shows"
    );
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
fn a_refused_open_is_a_prompt_with_ok_on_the_window(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    // A folder without the extension is refused with an alert.
    let opened =
        s.cx.update(|_, cx| vizdaw_gpui::open_path(Path::new("/tmp/folder"), cx));
    assert!(opened.is_none());
    s.cx.run_until_parked();
    assert_eq!(
        s.cx.pending_prompt(),
        Some((
            "Open Failed".into(),
            "folder is not a .vizdaw project.".into()
        ))
    );
    // No banner: the alert left the store for the prompt.
    let left = s.app.read_with(&s.cx, |a, _| a.store.alert().cloned());
    assert_eq!(left, None);
    s.cx.simulate_prompt_answer("OK");
    s.cx.run_until_parked();
    assert!(!s.cx.has_pending_prompt());
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

/// Swift `ClosePrompts.sheetText(savePending:)`: a Save made while
/// recording is owed, so the close question offers no Don't Save (that
/// would drop a save the user asked for) and says the take and the
/// changes will be saved; Cancel keeps the window and the owed save.
#[gpui::test]
fn closing_while_a_save_is_owed_offers_no_dont_save(cx: &mut TestAppContext) {
    use vizdaw_app::close_prompts::SAVE_PENDING_INFORMATIVE;
    let fake = demo_fake();
    let mut lane = vizdaw_app::testing::lane_row(30, "Vocal", false);
    lane.arm = true;
    lane.input = Some(vizdaw_ffi::edit::LaneInputRow::Audio {
        first: 0,
        stereo: true,
        return_of: None,
    });
    fake.set_lanes(3, vec![lane]);
    fake.set_path(Some("/tmp/Owed.vizdaw"));
    let mut s = setup(cx, fake);
    let doc = s.window_doc();
    s.app.update(&mut s.cx, |a, _| {
        a.store.with_document(doc, |d, _| {
            d.select(Some(3));
            d.toggle_record().unwrap();
            assert_eq!(d.save(), vizdaw_app::SaveAction::Saved);
        });
    });
    s.fake.set_dirty(true);
    s.cx.dispatch_action(vizdaw_gpui::CloseWindow);
    s.cx.run_until_parked();
    let (_, detail) = s.cx.pending_prompt().expect("the close question");
    assert_eq!(detail, SAVE_PENDING_INFORMATIVE);
    // The second button is Cancel now, not Don't Save.
    s.cx.simulate_prompt_answer("Cancel");
    s.cx.run_until_parked();
    s.cx.cx.update(|_| {});
    assert_eq!(s.app.read_with(&s.cx, |a, _| a.windows().len()), 1);
    assert_eq!(s.fake.count("cancel_save_after_pass"), 0);
    assert_eq!(s.fake.count("close"), 0);
}

#[gpui::test]
fn quit_asks_when_only_the_session_knows_it_is_unsaved(cx: &mut TestAppContext) {
    let fake = demo_fake();
    // The snapshot says clean; the session says unsaved.
    fake.set_unsaved(Some(Ok(true)));
    let mut s = setup(cx, fake);
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

/// The demo window with the in-window menu bar shown (it is off on macOS).
fn setup_with_menu_bar(cx: &mut TestAppContext) -> Setup {
    let mut s = setup(cx, demo_fake());
    s.window
        .update(&mut s.cx, |v, _, cx| v.set_menu_bar_shown(true, cx))
        .unwrap();
    s.cx.run_until_parked();
    s
}

fn has_element(s: &mut Setup, role: &str, label: &str) -> bool {
    s.window
        .read_with(&s.cx, |v, _| {
            v.registry()
                .borrow()
                .elements()
                .iter()
                .any(|e| e.role == role && e.label == label)
        })
        .unwrap()
}

#[gpui::test]
fn the_in_window_menu_bar_runs_an_item(cx: &mut TestAppContext) {
    let mut s = setup_with_menu_bar(cx);
    assert!(!has_element(&mut s, "menuitem", "New Audio Track"));
    click(&mut s, "menu", "Track");
    assert!(has_element(&mut s, "menuitem", "New Audio Track"));
    click(&mut s, "menuitem", "New Audio Track");
    let made: Vec<String> = s
        .fake
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("new_track("))
        .collect();
    assert_eq!(made, vec!["new_track(3,Audio,None)".to_string()]);
    // The list closes; the Edit menu now offers Undo.
    assert!(!has_element(&mut s, "menuitem", "New Audio Track"));
    assert!(has_element(&mut s, "menu", "Edit"));
}

#[gpui::test]
fn escape_closes_the_open_menu(cx: &mut TestAppContext) {
    let mut s = setup_with_menu_bar(cx);
    click(&mut s, "menu", "Track");
    assert!(has_element(&mut s, "menuitem", "New Audio Track"));
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert!(!has_element(&mut s, "menuitem", "New Audio Track"));
    // A click outside closes it too.
    click(&mut s, "menu", "Track");
    assert!(has_element(&mut s, "menuitem", "New Audio Track"));
    s.cx.simulate_click(gpui::point(px(900.), px(600.)), Modifiers::none());
    s.cx.run_until_parked();
    assert!(!has_element(&mut s, "menuitem", "New Audio Track"));
    assert_eq!(s.fake.count("play(3)"), 0);
}

#[gpui::test]
fn a_disabled_item_does_nothing(cx: &mut TestAppContext) {
    let mut s = setup_with_menu_bar(cx);
    click(&mut s, "menu", "Track");
    let disabled = s
        .window
        .read_with(&s.cx, |v, _| {
            v.registry()
                .borrow()
                .elements()
                .iter()
                .find(|e| e.role == "menuitem" && e.label == "Delete Track")
                .map(|e| e.enabled)
        })
        .unwrap();
    assert_eq!(disabled, Some(false));
    click(&mut s, "menuitem", "Delete Track");
    assert!(s.fake.edits().is_empty());
    // The list stays open, and no refusal was raised.
    assert!(has_element(&mut s, "menuitem", "Delete Track"));
    let doc = s.window_doc();
    let status = s.app.read_with(&s.cx, |a, _| {
        a.store.document(doc).unwrap().status().map(String::from)
    });
    assert_eq!(status, None);
}

#[gpui::test]
fn the_track_keys_run_their_commands(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let key = if cfg!(target_os = "macos") {
        "cmd-alt-a"
    } else {
        "ctrl-alt-a"
    };
    s.cx.simulate_keystrokes(key);
    s.cx.run_until_parked();
    assert_eq!(
        s.fake
            .calls()
            .iter()
            .filter(|c| c.starts_with("new_track("))
            .count(),
        1
    );
}

#[gpui::test]
fn the_app_menus_follow_the_front_document(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let undo_title = |s: &mut Setup| {
        let menus = s.cx.update(|_, cx| cx.get_menus()).expect("menus");
        let edit = menus.iter().find(|m| m.name == "Edit").unwrap();
        edit.items
            .iter()
            .find_map(|i| match i {
                gpui::OwnedMenuItem::Action { name, disabled, .. } if name.starts_with("Undo") => {
                    Some((name.clone(), *disabled))
                }
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(undo_title(&mut s), ("Undo".to_string(), true));
    let mut next = s.fake.snap();
    next.undo_label = Some("New Track".into());
    s.fake.bump(next);
    s.cx.dispatch_action(vizdaw_gpui::menus::RunCommand::new("track.new_audio"));
    s.cx.run_until_parked();
    assert_eq!(undo_title(&mut s), ("Undo New Track".to_string(), false));
}

/// The Mixer window opens at Swift's 760×560, kept inside a small
/// screen.
#[test]
fn the_mixer_window_opens_at_760_by_560_inside_the_screen() {
    use gpui::{Bounds, point, size};
    use vizdaw_gpui::shell::mixer_window_size;
    let screen = |w: f32, h: f32| Some(Bounds::new(point(px(0.), px(0.)), size(px(w), px(h))));
    assert_eq!(
        mixer_window_size(screen(2560., 1415.)),
        size(px(760.), px(560.))
    );
    assert_eq!(
        mixer_window_size(screen(640., 480.)),
        size(px(640.), px(480.))
    );
    assert_eq!(mixer_window_size(None), size(px(760.), px(560.)));
}

/// An edited document's Mixer window closes without asking, and the
/// document stays; the document window still asks.
#[gpui::test]
fn the_mixer_window_closes_without_asking(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.set_dirty(true);
    let mut s = setup(cx, fake);
    s.cx.dispatch_action(vizdaw_gpui::RunCommand::new("window.mixer".to_owned()));
    s.cx.run_until_parked();
    let doc = s.window_doc();
    let mixer = s
        .app
        .read_with(&s.cx, |a, _| a.mixer_window_of(doc).map(|w| w.handle))
        .expect("a Mixer window");
    let mut mcx = VisualTestContext::from_window(mixer.into(), &s.cx);
    mcx.dispatch_action(vizdaw_gpui::CloseWindow);
    mcx.run_until_parked();
    s.cx.cx.update(|_| {});
    assert!(!s.cx.has_pending_prompt());
    let left = s
        .app
        .read_with(&s.cx, |a, _| (a.windows().len(), a.store.count()));
    assert_eq!(left, (1, 1));
    assert_eq!(s.fake.count("close"), 0);
    // Quit asks once, for the one document.
    s.cx.dispatch_action(vizdaw_gpui::RunCommand::new("window.mixer".to_owned()));
    s.cx.run_until_parked();
    s.cx.dispatch_action(vizdaw_gpui::Quit);
    s.cx.run_until_parked();
    assert!(s.cx.has_pending_prompt());
    s.cx.simulate_prompt_answer("Discard Changes and Quit");
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("close"), 1);
}

#[gpui::test]
fn save_as_writes_a_name_typed_with_the_extension_in_capitals_as_typed(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let dir = tempfile::tempdir().unwrap();
    let pick = dir.path().join("Song.VIZDAW");
    s.cx.dispatch_action(vizdaw_gpui::SaveAs);
    s.cx.run_until_parked();
    s.cx.simulate_new_path_selection(move |_| Some(pick.clone()));
    s.cx.run_until_parked();
    assert!(!s.cx.has_pending_prompt());
    assert_eq!(
        save_as_calls(&s),
        vec![format!(
            "save_as({})",
            dir.path().join("Song.VIZDAW").display()
        )]
    );
}
