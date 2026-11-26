//! The file panels seam (slice 7b Task 1): the macOS panel and Linux
//! portal requests each spec makes (built, not shown: no panel can be
//! seen here), one panel at a time, and File › Open through the seam with
//! a package's `project.json` opening the package.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;
use gpui::{AnyWindowHandle, App, TestAppContext, VisualTestContext};
use vizdaw_app::DocumentStore;
use vizdaw_app::panels::{self, PanelSpec};
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_gpui::file_panels::{
    Chosen, MacPanel, MacType, PanelBackend, PanelPlace, PortalFilter, PortalRequest, blocks_key,
    choose, is_up, mac_panel, portal_request, set_backend,
};
use vizdaw_gpui::menus::{self, ItemSpec, MenuModel};

fn place(dir: Option<&str>, name: &str, ext: &str) -> PanelPlace {
    PanelPlace {
        directory: dir.map(PathBuf::from),
        name: name.into(),
        ext: ext.into(),
    }
}

fn id(s: &str) -> MacType {
    MacType::Identifier(s.into())
}

fn ext(s: &str) -> MacType {
    MacType::Extension(s.into())
}

#[test]
fn the_mac_open_panel_is_swifts_choose_project() {
    assert_eq!(
        mac_panel(&panels::OPEN_PROJECT, &PanelPlace::default()),
        MacPanel {
            save: false,
            choose_files: true,
            choose_folders: true,
            multiple: false,
            can_create_folders: false,
            packages_as_folders: false,
            prompt: "Open".into(),
            title: None,
            message: Some(
                "Choose a VizDAW project, a MIDI file, a DAWproject file or an AAF file".into()
            ),
            types: vec![
                id("org.vizdaw.project"),
                id("public.folder"),
                ext("vizdaw"),
                id("public.midi-audio"),
                ext("mid"),
                ext("midi"),
                ext("smf"),
                ext("midi2"),
                id("org.vizdaw.dawproject"),
                ext("dawproject"),
                id("org.vizdaw.aaf"),
                ext("aaf"),
            ],
            directory: None,
            name: None,
        }
    );
}

#[test]
fn mac_panels_carry_swifts_settings() {
    let copy = mac_panel(
        &panels::OPEN_COPY,
        &place(Some("/m/Song.vizdaw/Co-Producer/Copies"), "", ""),
    );
    assert_eq!(
        copy.directory,
        Some(PathBuf::from("/m/Song.vizdaw/Co-Producer/Copies"))
    );
    assert_eq!((copy.choose_files, copy.choose_folders), (true, true));
    let audio = mac_panel(&panels::IMPORT_AUDIO, &PanelPlace::default());
    assert!(audio.multiple && !audio.choose_folders);
    assert_eq!(audio.types.first(), Some(&id("public.audio")));
    let relink = mac_panel(&panels::RELINK, &place(None, "Kick", ""));
    assert_eq!(
        relink.message.as_deref(),
        Some("Choose the audio file for \u{201c}Kick\u{201d}.")
    );
    let search = mac_panel(&panels::SEARCH_FOLDER, &PanelPlace::default());
    assert_eq!(
        (
            search.choose_files,
            search.choose_folders,
            search.can_create_folders
        ),
        (false, true, false)
    );
    assert!(search.types.is_empty());
    let stems = mac_panel(&panels::BOUNCE_FOLDER, &PanelPlace::default());
    assert!(stems.can_create_folders);
}

#[test]
fn mac_save_panels_name_the_file_and_its_type() {
    let bounce = mac_panel(&panels::BOUNCE_FILE, &place(None, "Song", "aif"));
    assert!(bounce.save && bounce.can_create_folders);
    assert_eq!(bounce.prompt, "Bounce");
    assert_eq!(bounce.message, None);
    assert_eq!(bounce.types, vec![ext("aif")]);
    assert_eq!(bounce.name.as_deref(), Some("Bounce.aif"));
    let save = mac_panel(&panels::SAVE_PROJECT, &place(Some("/m"), "Song", ""));
    assert!(save.types.is_empty(), "Swift's save panel has no types");
    assert_eq!(save.name.as_deref(), Some("Song.vizdaw"));
    assert_eq!(save.directory, Some(PathBuf::from("/m")));
    let report = mac_panel(&panels::REPORT_TEXT, &PanelPlace::default());
    assert_eq!(report.types, vec![id("public.plain-text"), ext("txt")]);
    assert_eq!(report.name.as_deref(), Some("MIDI Report.txt"));
}

#[test]
fn the_linux_open_is_one_file_chooser_that_takes_a_projects_project_json() {
    assert_eq!(
        portal_request(&panels::OPEN_PROJECT, &PanelPlace::default()),
        PortalRequest {
            save: false,
            title: "Choose a VizDAW project, a MIDI file, a DAWproject file or an AAF file".into(),
            accept_label: "Open".into(),
            multiple: false,
            directory: false,
            filter: Some(PortalFilter {
                label: "VizDAW project, MIDI file, DAWproject file or AAF file".into(),
                globs: [
                    "project.json",
                    "*.mid",
                    "*.midi",
                    "*.smf",
                    "*.midi2",
                    "*.dawproject",
                    "*.aaf"
                ]
                .map(String::from)
                .to_vec(),
                mime_types: vec!["audio/midi".into(), "audio/x-midi".into()],
            }),
            current_folder: None,
            current_name: None,
        }
    );
}

#[test]
fn linux_requests_carry_the_prompt_folder_and_name() {
    let copy = portal_request(
        &panels::OPEN_COPY,
        &place(Some("/m/Song.vizdaw/Co-Producer/Copies"), "", ""),
    );
    assert!(!copy.directory);
    assert_eq!(
        copy.filter.map(|f| f.globs),
        Some(vec!["project.json".to_owned()])
    );
    assert_eq!(
        copy.current_folder,
        Some(PathBuf::from("/m/Song.vizdaw/Co-Producer/Copies"))
    );
    let folders: Vec<&PanelSpec> = panels::ALL
        .iter()
        .filter(|s| s.kind == panels::PanelKind::Folder)
        .collect();
    assert_eq!(folders.len(), 6);
    for spec in folders {
        let r = portal_request(spec, &place(None, "Song AAF", ""));
        assert!(r.directory && r.filter.is_none(), "row {}", spec.row);
        assert_eq!(r.accept_label, spec.prompt);
    }
    let aaf = portal_request(&panels::AAF_EXPORT_FOLDER, &place(None, "Song AAF", ""));
    assert_eq!(
        aaf.title,
        "Choose where to put the folder \u{201c}Song AAF\u{201d}."
    );
    let audio = portal_request(&panels::IMPORT_AUDIO, &PanelPlace::default());
    assert!(audio.multiple);
    assert_eq!(audio.filter.unwrap().mime_types, vec!["audio/*".to_owned()]);
    let save = portal_request(&panels::SAVE_PROJECT, &place(Some("/m"), "Song", ""));
    assert_eq!(
        (
            save.save,
            save.title.as_str(),
            save.accept_label.as_str(),
            save.current_name.as_deref()
        ),
        (true, "Save", "Save", Some("Song.vizdaw"))
    );
    assert_eq!(save.filter, None);
    assert_eq!(save.current_folder, Some(PathBuf::from("/m")));
    let midi = portal_request(&panels::MIDI_EXPORT_FILE, &place(None, "Bass", "midi2"));
    assert_eq!(
        (midi.accept_label.as_str(), midi.current_name.as_deref()),
        ("Export", Some("Bass.midi2"))
    );
    assert_eq!(midi.filter.unwrap().globs, vec!["*.midi2".to_owned()]);
}

struct Setup {
    opened: Arc<Mutex<Vec<String>>>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext) -> Setup {
    let opened = Arc::new(Mutex::new(Vec::new()));
    let log = opened.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(demo_fake()))),
        Box::new(move |p| {
            log.lock().unwrap().push(p.to_owned());
            Ok(Arc::new(demo_fake()))
        }),
    );
    let window = cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
        vizdaw_gpui::new_demo(cx).expect("a demo window")
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    Setup { opened, cx: vcx }
}

#[gpui::test]
fn file_open_asks_with_swifts_prompt_and_opens_a_chosen_project_json_s_package(
    cx: &mut TestAppContext,
) {
    let mut s = setup(cx);
    s.cx.dispatch_action(vizdaw_gpui::OpenDocument);
    s.cx.run_until_parked();
    assert!(s.cx.did_prompt_for_paths());
    assert!(s.cx.update(|_, cx| is_up(cx)));
    s.cx.simulate_path_prompt_response(|o| {
        assert!(o.files && o.directories && !o.multiple);
        assert_eq!(o.prompt.as_ref().map(|p| p.as_ref()), Some("Open"));
        Some(vec![PathBuf::from("/m/Song.vizdaw/project.json")])
    });
    s.cx.run_until_parked();
    assert!(!s.cx.update(|_, cx| is_up(cx)));
    assert_eq!(*s.opened.lock().unwrap(), vec!["/m/Song.vizdaw".to_owned()]);
}

#[gpui::test]
fn a_second_panel_while_one_is_up_is_refused_quietly(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.cx.dispatch_action(vizdaw_gpui::OpenDocument);
    s.cx.run_until_parked();
    s.cx.dispatch_action(vizdaw_gpui::SaveAs);
    s.cx.dispatch_action(vizdaw_gpui::OpenDocument);
    s.cx.run_until_parked();
    assert!(!s.cx.did_prompt_for_new_path());
    s.cx.simulate_path_prompt_response(|_| None);
    s.cx.run_until_parked();
    // The refused ones left nothing queued, and the next one shows.
    assert!(!s.cx.did_prompt_for_paths());
    assert!(!s.cx.update(|_, cx| is_up(cx)));
    assert!(!s.cx.has_pending_prompt());
    s.cx.dispatch_action(vizdaw_gpui::SaveAs);
    s.cx.run_until_parked();
    assert!(s.cx.did_prompt_for_new_path());
    s.cx.simulate_new_path_selection(|_| None);
    s.cx.run_until_parked();
    assert!(s.opened.lock().unwrap().is_empty());
}

/// Panels a test answers itself, through their completions.
#[derive(Default)]
struct FakePanels {
    shown: RefCell<Vec<(&'static str, oneshot::Sender<Chosen>)>>,
    /// The parent window each panel was shown for.
    parents: RefCell<Vec<Option<AnyWindowHandle>>>,
    /// The windows whose panel the backend was told to end.
    ended: RefCell<Vec<AnyWindowHandle>>,
}

impl PanelBackend for FakePanels {
    fn show(
        &self,
        spec: &PanelSpec,
        _place: &PanelPlace,
        window: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        _cx: &mut App,
    ) {
        self.parents.borrow_mut().push(window);
        self.shown.borrow_mut().push((spec.row, done));
    }

    fn end(&self, window: AnyWindowHandle, _cx: &mut App) {
        self.ended.borrow_mut().push(window);
    }
}

impl FakePanels {
    fn rows(&self) -> Vec<&'static str> {
        self.shown.borrow().iter().map(|(r, _)| *r).collect()
    }

    fn answer(&self, i: usize, chosen: Chosen) {
        let (_, done) = self.shown.borrow_mut().remove(i);
        done.send(chosen).unwrap();
    }
}

fn fake_panels(s: &mut Setup) -> Rc<FakePanels> {
    let fake = Rc::new(FakePanels::default());
    let backend: Rc<dyn PanelBackend> = fake.clone();
    s.cx.update(|_, cx| set_backend(backend, cx));
    fake
}

fn any_enabled(model: &MenuModel) -> bool {
    fn on(i: &ItemSpec) -> bool {
        match i {
            ItemSpec::Command { enabled, .. } | ItemSpec::Mixer { enabled, .. } => *enabled,
            ItemSpec::Submenu { enabled, items, .. } => *enabled || items.iter().any(on),
            _ => false,
        }
    }
    model.menus.iter().any(|m| m.items.iter().any(on))
}

#[gpui::test]
fn the_answer_arrives_through_the_panels_completion_and_menus_are_off_meanwhile(
    cx: &mut TestAppContext,
) {
    let mut s = setup(cx);
    let fake = fake_panels(&mut s);
    assert!(s.cx.update(|_, cx| any_enabled(&menus::shown(cx).unwrap())));
    let got: Rc<RefCell<Option<Chosen>>> = Rc::default();
    let slot = got.clone();
    s.cx.update(|_, cx| {
        let task = choose(&panels::IMPORT_MIDI, PanelPlace::default(), None, cx);
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach();
    });
    s.cx.run_until_parked();
    assert_eq!(fake.rows(), vec!["3"]);
    assert!(got.borrow().is_none());
    assert!(s.cx.update(|_, cx| is_up(cx)));
    assert!(!s.cx.update(|_, cx| any_enabled(&menus::shown(cx).unwrap())));
    // Keys are off, apart from the panel's own cancel keys.
    assert!(blocks(&mut s, "space"));
    assert!(blocks(&mut s, "backspace"));
    assert!(!blocks(&mut s, "escape"));
    assert!(!blocks(&mut s, "cmd-."));
    fake.answer(0, Ok(Some(vec![PathBuf::from("/m/a.mid")])));
    s.cx.run_until_parked();
    assert_eq!(
        got.borrow_mut().take(),
        Some(Ok(Some(vec![PathBuf::from("/m/a.mid")])))
    );
    assert!(!s.cx.update(|_, cx| is_up(cx)));
    assert!(s.cx.update(|_, cx| any_enabled(&menus::shown(cx).unwrap())));
    assert!(!blocks(&mut s, "space"));
}

#[gpui::test]
fn dropping_the_callers_task_still_clears_the_panel_up_flag(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let fake = fake_panels(&mut s);
    s.cx.update(|_, cx| {
        drop(choose(
            &panels::IMPORT_MIDI,
            PanelPlace::default(),
            None,
            cx,
        ))
    });
    s.cx.run_until_parked();
    assert_eq!(fake.rows(), vec!["3"]);
    fake.answer(0, Ok(None));
    s.cx.run_until_parked();
    assert!(!s.cx.update(|_, cx| is_up(cx)));
    s.cx.update(|_, cx| drop(choose(&panels::IMPORT_AAF, PanelPlace::default(), None, cx)));
    s.cx.run_until_parked();
    assert_eq!(fake.rows(), vec!["14"]);
}

fn blocks(s: &mut Setup, key: &'static str) -> bool {
    let key = gpui::Keystroke::parse(key).unwrap();
    s.cx.update(|_, cx| blocks_key(&key, cx))
}

/// Final review: the backend gets the window that asked, so the macOS
/// panel shows as that window's sheet (a click into the window cannot
/// bring it in front of the panel); no window, no parent.
#[gpui::test]
fn the_backend_gets_the_asking_window_as_the_panels_parent(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let fake = fake_panels(&mut s);
    let window = s.cx.update(|window, _| window.window_handle());
    s.cx.dispatch_action(vizdaw_gpui::OpenDocument);
    s.cx.run_until_parked();
    assert_eq!(fake.rows(), vec!["1"]);
    assert_eq!(*fake.parents.borrow(), vec![Some(window)]);
    fake.answer(0, Ok(None));
    s.cx.run_until_parked();
    s.cx.update(|_, cx| {
        drop(choose(
            &panels::IMPORT_MIDI,
            PanelPlace::default(),
            None,
            cx,
        ))
    });
    s.cx.run_until_parked();
    assert_eq!(fake.parents.borrow()[1], None);
}

/// Re-review: a panel whose window closes while it is up is ended by the
/// backend (macOS: the sheet would never answer) and reads as cancelled,
/// so keys and menus come back even if the panel never answers.
#[gpui::test]
fn a_panel_whose_window_closes_is_ended_and_the_flag_clears(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let fake = fake_panels(&mut s);
    let other =
        s.cx.update(|_, cx| vizdaw_gpui::new_demo(cx).expect("a second window"));
    s.cx.run_until_parked();
    let got: Rc<RefCell<Option<Chosen>>> = Rc::default();
    let slot = got.clone();
    s.cx.update(|_, cx| {
        let task = choose(
            &panels::IMPORT_MIDI,
            PanelPlace::default(),
            Some(other.into()),
            cx,
        );
        cx.spawn(async move |_| *slot.borrow_mut() = Some(task.await))
            .detach();
    });
    s.cx.run_until_parked();
    assert_eq!(fake.rows(), vec!["3"]);
    assert!(s.cx.update(|_, cx| is_up(cx)));
    other
        .update(&mut s.cx, |_, window, _| window.remove_window())
        .unwrap();
    s.cx.run_until_parked();
    let other: AnyWindowHandle = other.into();
    assert_eq!(*fake.ended.borrow(), vec![other]);
    assert!(!s.cx.update(|_, cx| is_up(cx)), "keys and menus are back");
    assert_eq!(got.borrow_mut().take(), Some(Ok(None)), "read as cancelled");
    // The fake never answered: its sender is still held.
    assert_eq!(fake.rows(), vec!["3"]);
    // Another window closing later ends nothing more.
    assert_eq!(fake.ended.borrow().len(), 1);
}

#[test]
fn a_panel_title_reaches_the_mac_panel_and_the_portal() {
    use vizdaw_gpui::settings_window::EDITOR_PANEL;
    let m = mac_panel(&EDITOR_PANEL, &PanelPlace::default());
    assert_eq!(m.title.as_deref(), Some("Choose External Sample Editor"));
    let p = portal_request(&EDITOR_PANEL, &PanelPlace::default());
    assert_eq!(p.title, "Choose External Sample Editor");
    // A panel with no title keeps its message as the portal's title.
    assert_eq!(
        mac_panel(&panels::OPEN_PROJECT, &PanelPlace::default()).title,
        None
    );
}
