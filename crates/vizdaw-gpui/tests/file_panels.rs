//! The file panels seam (slice 7b Task 1): the macOS panel and Linux
//! portal requests each spec makes (built, not shown: no panel can be
//! seen here), one panel at a time, and File › Open through the seam with
//! a package's `project.json` opening the package.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use gpui::{TestAppContext, VisualTestContext};
use vizdaw_app::DocumentStore;
use vizdaw_app::panels::{self, PanelSpec};
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_gpui::file_panels::{
    MacPanel, MacType, PanelPlace, PortalFilter, PortalRequest, is_up, mac_panel, portal_request,
};

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
                mime_types: vec![],
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
