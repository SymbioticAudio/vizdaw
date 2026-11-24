//! Relink Media… and Open Co-Producer Copy… (slice 7b Task 5): a project
//! that opens with missing or changed media shows the Relink sheet with
//! Swift's strings; Locate… (panel #5) and Search Folder… (panel #6) start
//! `Relink` jobs; Leave Offline (Esc) closes it and the same items are not
//! offered again; Relink Media… shows it again; a recheck that finds new
//! items offers it; it goes once nothing is left to relink; a held merge
//! turns Locate… and Search Folder… off with Swift's help; under the
//! control channel it never shows. Open Co-Producer Copy… shows panel #2
//! in the project's copies folder and opens the copy (on Linux its
//! `project.json` names it).

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use core_control::ControlError;
use core_control::server::Call;
use futures::channel::oneshot;
use gpui::{
    AnyWindowHandle, App, Entity, Modifiers, TestAppContext, VisualTestContext, WindowHandle,
};
use serde_json::Value;
use vizdaw_app::panels::PanelSpec;
use vizdaw_app::relink::{HELD_HELP, MESSAGE, TITLE};
use vizdaw_app::testing::{FakeCore, FakeSession, JobRequest, JobScript, demo_fake};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::session::CopiesSizeRow;
use vizdaw_ffi::{MediaOriginRow, MediaRow, OfflineReasonRow};
use vizdaw_gpui::control::CallSource;
use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
use vizdaw_gpui::{AppState, DocumentWindow, FileCommand};

/// Panels a test answers itself, with what they were shown with.
#[derive(Default)]
struct FakePanels {
    shown: RefCell<Vec<(&'static str, PanelPlace, oneshot::Sender<Chosen>)>>,
}

impl PanelBackend for FakePanels {
    fn show(
        &self,
        spec: &PanelSpec,
        place: &PanelPlace,
        _: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        _: &mut App,
    ) {
        self.shown
            .borrow_mut()
            .push((spec.row, place.clone(), done));
    }
}

impl FakePanels {
    fn rows(&self) -> Vec<&'static str> {
        self.shown.borrow().iter().map(|(r, _, _)| *r).collect()
    }

    fn place(&self, i: usize) -> PanelPlace {
        self.shown.borrow()[i].1.clone()
    }

    fn answer(&self, paths: &[&str]) {
        let (_, _, done) = self.shown.borrow_mut().remove(0);
        done.send(Ok(Some(paths.iter().map(PathBuf::from).collect())))
            .unwrap();
    }
}

/// A channel with no calls: pumping it turns the channel on.
struct NoCalls;

impl CallSource for NoCalls {
    fn next_call(&self) -> Option<Call> {
        None
    }
    fn reply(&self, _: u64, _: Result<Value, ControlError>) -> bool {
        true
    }
    fn extend(&self, _: u64, _: Duration) -> bool {
        true
    }
    fn is_waiting(&self, _: u64) -> bool {
        true
    }
}

struct Setup {
    fake: Arc<FakeSession>,
    core: Arc<FakeCore>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    panels: Rc<FakePanels>,
    cx: VisualTestContext,
}

fn media(id: u64, name: &str, offline: Option<OfflineReasonRow>) -> MediaRow {
    MediaRow {
        id,
        name: name.into(),
        path: format!("Media/{name}.caf"),
        channels: 2,
        frames: 48_000,
        sample_rate: 48_000,
        origin: MediaOriginRow::Imported,
        original_name: None,
        offline,
        uses: 1,
        used_percent: 100.0,
        peak: None,
    }
}

/// The demo, over `fake`, open in a window (`channel`: the control
/// channel was on first).
fn setup_with(cx: &mut TestAppContext, fake: FakeSession, channel: bool) -> Setup {
    let fake = Arc::new(fake);
    let core = Arc::new(FakeCore::new());
    core.hand_out("new_demo", fake.clone());
    let store = DocumentStore::with_core(core.clone());
    let panels = Rc::new(FakePanels::default());
    let backend: Rc<dyn PanelBackend> = panels.clone();
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        set_backend(backend, cx);
        if channel {
            vizdaw_gpui::control::pump(&NoCalls, cx);
        }
        let window = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, window)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    Setup {
        fake,
        core,
        app,
        window,
        panels,
        cx: vcx,
    }
}

/// The demo with `a` missing and `b` online.
fn setup(cx: &mut TestAppContext) -> Setup {
    let f = demo_fake();
    f.set_media(vec![
        media(1, "a", Some(OfflineReasonRow::Missing)),
        media(2, "b", None),
    ]);
    setup_with(cx, f, false)
}

impl Setup {
    fn doc(&self) -> DocId {
        self.window.read_with(&self.cx, |v, _| v.doc()).unwrap()
    }

    fn nodes(&mut self) -> Vec<Value> {
        let tree = self
            .window
            .read_with(&self.cx, |v, _| v.registry().borrow().tree())
            .unwrap();
        tree["nodes"].as_array().unwrap().clone()
    }

    fn node(&mut self, role: &str, label: &str) -> Option<Value> {
        self.nodes()
            .into_iter()
            .find(|n| n["role"] == role && n["label"] == label)
    }

    fn expect(&mut self, role: &str, label: &str) -> Value {
        self.node(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {:?}", self.nodes()))
    }

    fn click(&mut self, role: &str, label: &str) {
        let n = self.expect(role, label);
        let f = &n["frame"];
        let at = gpui::point(
            gpui::px((f["x"].as_f64().unwrap() + f["width"].as_f64().unwrap() / 2.) as f32),
            gpui::px((f["y"].as_f64().unwrap() + f["height"].as_f64().unwrap() / 2.) as f32),
        );
        self.cx.simulate_click(at, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn sheet(&mut self) -> Option<String> {
        self.window
            .read_with(&self.cx, |v, _| v.sheet_label())
            .unwrap()
    }

    fn menu(&mut self, id: &str) {
        self.cx.dispatch_action(FileCommand { id: id.into() });
        self.cx.run_until_parked();
    }

    /// The app state changed (what the idle timer's poll does).
    fn notify(&mut self) {
        self.app.update(&mut self.cx, |_, cx| cx.notify());
        self.cx.run_until_parked();
    }

    /// The window's poll runs (idle, jobs polled), then the window draws.
    fn tick(&mut self) {
        self.cx.executor().advance_clock(Duration::from_millis(300));
        self.cx.run_until_parked();
    }

    fn started(&self) -> Vec<JobRequest> {
        self.fake
            .jobs_started()
            .into_iter()
            .map(|j| j.request)
            .collect()
    }
}

#[gpui::test]
fn a_project_with_missing_media_opens_with_the_sheet(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
    s.expect("sheet", TITLE);
    s.expect("text", TITLE);
    s.expect("text", MESSAGE);
    // Only the missing item is listed.
    s.expect("text", "a");
    s.expect("text", "Missing \u{b7} Media/a.caf");
    assert!(s.node("text", "b").is_none());
    let locate = s.expect("button", "Locate a");
    assert_eq!(locate["enabled"], true);
    s.expect("button", "Search Folder\u{2026}");
    s.expect("button", "Leave Offline");
    assert!(s.node("button", "Locate b").is_none());
}

#[gpui::test]
fn media_in_another_format_does_not_offer_the_sheet(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Format))]);
    let mut s = setup_with(cx, f, false);
    assert_eq!(s.sheet(), None, "the Convert banner handles it");
}

#[gpui::test]
fn leave_offline_closes_it_and_the_menu_shows_it_again(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    s.notify();
    s.tick();
    assert_eq!(s.sheet(), None, "the same items are not offered again");
    s.menu("media.relink");
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
    s.click("button", "Leave Offline");
    assert_eq!(s.sheet(), None);
}

#[gpui::test]
fn locate_relinks_the_item_through_panel_5(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.fake
        .script_job(JobScript::done().steps(&[0.5]).created(&[1]));
    s.click("button", "Locate a");
    assert_eq!(s.panels.rows(), ["5"]);
    assert_eq!(s.panels.place(0).name, "a", "the message names the item");
    s.panels.answer(&["/x/a.caf"]);
    s.cx.run_until_parked();
    assert_eq!(
        s.started(),
        [JobRequest::Relink {
            media: 1,
            path: "/x/a.caf".into()
        }]
    );
    // The job's banner shows on the window; the sheet stays up meanwhile.
    s.expect("progress", "Relinking media\u{2026}");
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
    // The job ends with the item back online: nothing is left, the sheet
    // goes.
    s.fake
        .set_media(vec![media(1, "a", None), media(2, "b", None)]);
    s.tick();
    s.tick();
    assert_eq!(s.sheet(), None);
}

#[gpui::test]
fn search_folder_searches_through_panel_6(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.fake
        .script_job(JobScript::done().steps(&[0.5]).created(&[1]));
    s.click("button", "Search Folder\u{2026}");
    assert_eq!(s.panels.rows(), ["6"]);
    s.panels.answer(&["/x"]);
    s.cx.run_until_parked();
    assert_eq!(s.started(), [JobRequest::Search("/x".into())]);
    assert_eq!(s.sheet().as_deref(), Some(TITLE), "the sheet stays up");
}

#[gpui::test]
fn a_held_merge_turns_relinking_off(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.document_mut(doc).unwrap().set_merge_held(true);
        cx.notify();
    });
    s.cx.run_until_parked();
    let locate = s.expect("button", "Locate a");
    assert_eq!(locate["enabled"], false);
    assert_eq!(locate["value"], HELD_HELP);
    let search = s.expect("button", "Search Folder\u{2026}");
    assert_eq!(search["enabled"], false);
    assert_eq!(search["value"], HELD_HELP);
    s.click("button", "Locate a");
    assert!(s.panels.rows().is_empty());
}

#[gpui::test]
fn a_recheck_that_finds_new_missing_media_offers_the_sheet(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_media(vec![media(1, "a", None)]);
    let mut s = setup_with(cx, f, false);
    assert_eq!(s.sheet(), None);
    s.fake
        .set_media(vec![media(1, "a", Some(OfflineReasonRow::Changed))]);
    s.fake.set_recheck_media(true);
    s.cx.update(|_, cx| vizdaw_gpui::sheets::relink::recheck_all(cx));
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
    s.expect("text", "Changed \u{b7} Media/a.caf");
}

#[gpui::test]
fn under_the_control_channel_the_sheet_never_shows(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_media(vec![media(1, "a", Some(OfflineReasonRow::Missing))]);
    let mut s = setup_with(cx, f, true);
    assert_eq!(s.sheet(), None);
    s.menu("media.relink");
    assert_eq!(s.sheet(), None);
    s.fake.set_recheck_media(true);
    s.cx.update(|_, cx| vizdaw_gpui::sheets::relink::recheck_all(cx));
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
}

#[gpui::test]
fn open_copy_shows_panel_2_in_the_copies_folder(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_path(Some("/m/Song.vizdaw"));
    f.set_copies_size(CopiesSizeRow {
        count: 1,
        bytes: 10,
    });
    let mut s = setup_with(cx, f, false);
    s.menu("file.open_copy");
    assert_eq!(s.panels.rows(), ["2"]);
    assert_eq!(
        s.panels.place(0).directory,
        Some(PathBuf::from("/m/Song.vizdaw/Co-Producer/Copies"))
    );
    // On Linux a project is chosen by its `project.json`: its package.
    s.panels
        .answer(&["/m/Song.vizdaw/Co-Producer/Copies/Take.vizdaw/project.json"]);
    s.cx.run_until_parked();
    assert!(
        s.core
            .calls()
            .contains(&"open_copy(/m/Song.vizdaw/Co-Producer/Copies/Take.vizdaw)".to_owned()),
        "{:?}",
        s.core.calls()
    );
    let docs = s.app.read_with(&s.cx, |a, _| a.store.count());
    assert_eq!(docs, 2, "the copy opened as a new project");
    let windows = s.app.read_with(&s.cx, |a, _| a.document_windows().count());
    assert_eq!(windows, 2, "in a window of its own");
}

#[gpui::test]
fn open_copy_refuses_what_is_not_a_copy(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_path(Some("/m/Song.vizdaw"));
    f.set_copies_size(CopiesSizeRow {
        count: 1,
        bytes: 10,
    });
    let mut s = setup_with(cx, f, false);
    s.menu("file.open_copy");
    s.panels.answer(&["/m/Other.vizdaw"]);
    s.cx.run_until_parked();
    assert!(!s.core.calls().iter().any(|c| c.starts_with("open_copy")));
    assert_eq!(
        s.cx.pending_prompt(),
        Some((
            "Open Failed".into(),
            "That is not a Co-Producer copy.".into()
        ))
    );
}

/// Swift `applicationDidBecomeActive`: the app back from the background
/// rechecks every project's media.
#[gpui::test]
fn coming_back_to_the_app_rechecks_the_media(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_media(vec![media(1, "a", None)]);
    let mut s = setup_with(cx, f, false);
    s.cx.deactivate_window();
    s.cx.executor().advance_clock(Duration::from_secs(5));
    s.cx.run_until_parked();
    s.fake
        .set_media(vec![media(1, "a", Some(OfflineReasonRow::Missing))]);
    s.fake.set_recheck_media(true);
    s.window
        .update(&mut s.cx, |_, window, _| window.activate_window())
        .unwrap();
    s.cx.run_until_parked();
    assert!(s.fake.calls().iter().any(|c| c == "recheck_media"));
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
}

fn rechecks(s: &Setup) -> usize {
    s.fake
        .calls()
        .iter()
        .filter(|c| *c == "recheck_media")
        .count()
}

fn activate(s: &mut Setup) {
    s.window
        .update(&mut s.cx, |_, window, _| window.activate_window())
        .unwrap();
    s.cx.run_until_parked();
}

/// Review fix: a file panel taking the keyboard is not the app leaving.
#[gpui::test]
fn a_file_panel_is_not_the_app_leaving(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_media(vec![media(1, "a", None)]);
    let mut s = setup_with(cx, f, false);
    s.menu("media.import");
    assert_eq!(s.panels.rows(), ["7"]);
    s.cx.deactivate_window();
    s.cx.executor().advance_clock(Duration::from_secs(5));
    s.cx.run_until_parked();
    s.panels.answer(&[]);
    s.cx.run_until_parked();
    activate(&mut s);
    assert_eq!(rechecks(&s), 0, "no recheck after a panel");
}

/// Review fix: an alert of ours up is not the app leaving either.
#[gpui::test]
fn a_prompt_is_not_the_app_leaving(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_media(vec![media(1, "a", None)]);
    let mut s = setup_with(cx, f, false);
    s.app.update(&mut s.cx, |a, cx| {
        a.store
            .post_alert(vizdaw_app::AlertItem::new("Some Alert", "text"));
        cx.notify();
    });
    s.cx.run_until_parked();
    assert!(s.cx.has_pending_prompt());
    s.cx.deactivate_window();
    s.cx.executor().advance_clock(Duration::from_secs(5));
    s.cx.run_until_parked();
    s.cx.simulate_prompt_answer("OK");
    s.cx.run_until_parked();
    activate(&mut s);
    assert_eq!(rechecks(&s), 0, "no recheck after a prompt");
}

/// Review fix: focus churn (back within a moment) does not recheck; a
/// real app switch rechecks once.
#[gpui::test]
fn only_a_real_return_rechecks_once(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_media(vec![media(1, "a", None)]);
    let mut s = setup_with(cx, f, false);
    s.cx.deactivate_window();
    activate(&mut s);
    assert_eq!(rechecks(&s), 0, "back at once: no recheck");
    s.cx.deactivate_window();
    s.cx.executor().advance_clock(Duration::from_secs(5));
    s.cx.run_until_parked();
    activate(&mut s);
    assert_eq!(rechecks(&s), 1);
    activate(&mut s);
    assert_eq!(rechecks(&s), 1, "once");
}

/// Review fix: a document with two windows is rechecked once.
#[gpui::test]
fn a_document_with_two_windows_is_rechecked_once(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_media(vec![media(1, "a", None)]);
    let mut s = setup_with(cx, f, false);
    let doc = s.doc();
    let app = s.app.clone();
    s.cx.update(|_, cx| {
        DocumentWindow::open_mixer(&app, doc, cx).unwrap();
    });
    s.cx.run_until_parked();
    let windows = s.app.read_with(&s.cx, |a, _| a.windows().len());
    assert_eq!(windows, 2);
    s.cx.update(|_, cx| vizdaw_gpui::sheets::relink::recheck_all(cx));
    s.cx.run_until_parked();
    assert_eq!(rechecks(&s), 1);
}

/// Review: each row's Locate… relinks its own item.
#[gpui::test]
fn locate_relinks_the_row_it_is_on(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_media(vec![
        media(1, "a", Some(OfflineReasonRow::Missing)),
        media(5, "c", Some(OfflineReasonRow::Changed)),
    ]);
    let mut s = setup_with(cx, f, false);
    s.click("button", "Locate c");
    assert_eq!(s.panels.place(0).name, "c");
    s.panels.answer(&["/x/c.caf"]);
    s.cx.run_until_parked();
    assert_eq!(
        s.started(),
        [JobRequest::Relink {
            media: 5,
            path: "/x/c.caf".into()
        }]
    );
}
