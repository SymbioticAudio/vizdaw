//! File › Export › AAF… and File › Import Project › AAF… (slice 7c Task
//! 5): the export sheet with Swift's strings and the core's check (run
//! off the UI thread, again after each change), the folder panel naming
//! the core's folder (#9d; nothing is replaced, nothing asked), waiting
//! for a running job, progress and Cancel, the report after, a failure
//! back in the sheet; the import sheet after the file is read off the UI
//! thread, a new project by default, the MIDI field (keys off while it
//! edits) and its panel (#15), the missing-audio folder (#16) and Clear,
//! and Import into this project or a new one.

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
use vizdaw_app::testing::{FakeCore, FakeSession, JobRequest, JobScript, demo_fake, seq};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{
    AafExportRequestRow, AafFpsRow, AafImportRequestRow, AafInfoRow, JobKindRow, ReportItemRow,
    ReportKindRow, ReportRow,
};
use vizdaw_gpui::control::CallSource;
use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
use vizdaw_gpui::{AppFileCommand, AppState, DocumentWindow, FileCommand};

const EXPORT_AAF: &str = "Export AAF";
const IMPORT_AAF: &str = "Import AAF";
const EXPORT: &str = "Export, then choose where";

/// A panel shown: its row, its message, its answer.
type Shown = (&'static str, Option<String>, oneshot::Sender<Chosen>);

/// Panels a test answers itself.
#[derive(Default)]
struct FakePanels {
    shown: RefCell<Vec<Shown>>,
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
        let message = spec.message_for(&place.name);
        self.shown.borrow_mut().push((spec.row, message, done));
    }
}

impl FakePanels {
    fn rows(&self) -> Vec<&'static str> {
        self.shown.borrow().iter().map(|(r, _, _)| *r).collect()
    }

    fn messages(&self) -> Vec<Option<String>> {
        self.shown
            .borrow()
            .iter()
            .map(|(_, m, _)| m.clone())
            .collect()
    }

    fn answer(&self, path: &str) {
        let (_, _, done) = self.shown.borrow_mut().remove(0);
        done.send(Ok(Some(vec![PathBuf::from(path)]))).unwrap();
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
    core: Arc<FakeCore>,
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    panels: Rc<FakePanels>,
    cx: VisualTestContext,
}

/// The demo, its AAF check scripted with a left-out group.
fn fake() -> FakeSession {
    let f = demo_fake();
    f.set_check_report(
        "export_aaf_check",
        ReportRow {
            summary: "Exported 2 tracks".into(),
            items: vec![ReportItemRow {
                kind: ReportKindRow::LeftOut,
                what: "Plug-ins".into(),
                count: 2,
                details: vec![],
            }],
        },
    );
    f
}

fn info(midi: Option<&str>) -> AafInfoRow {
    AafInfoRow {
        sample_rate: 44_100,
        tracks: 3,
        audio_files: 2,
        offline: 1,
        from_vizdaw: false,
        midi_beside: midi.map(str::to_owned),
    }
}

fn setup(cx: &mut TestAppContext, fake: FakeSession, channel: bool) -> Setup {
    let core = Arc::new(FakeCore::new());
    let fake = Arc::new(fake);
    core.hand_out("new_demo", fake.clone());
    core.set_aaf_info("/m/Song.aaf", info(Some("/m/Song MIDI.mid")));
    core.set_aaf_info("/m/Plain.aaf", info(None));
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
        core,
        fake,
        app,
        window,
        panels,
        cx: vcx,
    }
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
            .rfind(|n| n["role"] == role && n["label"] == label)
    }

    fn expect(&mut self, role: &str, label: &str) -> Value {
        self.node(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {:?}", self.nodes()))
    }

    fn on(&mut self, role: &str, label: &str) -> bool {
        self.expect(role, label)["selected"] == true
    }

    fn enabled(&mut self, role: &str, label: &str) -> bool {
        self.expect(role, label)["enabled"] == true
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

    /// File › Import Project › AAF…, answered with `path`.
    fn import(&mut self, path: &str) {
        self.cx.dispatch_action(AppFileCommand {
            id: "file.import_aaf".into(),
        });
        self.cx.run_until_parked();
        assert_eq!(self.panels.rows(), ["14"]);
        self.panels.answer(path);
        self.cx.run_until_parked();
    }

    /// The document's jobs are polled, then the window draws.
    fn tick(&mut self) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| d.poll_jobs());
            cx.notify();
        });
        self.cx.executor().advance_clock(Duration::from_millis(60));
        self.cx.run_until_parked();
    }

    fn keys_on(&mut self) -> bool {
        let doc = self.doc();
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().transport_keys_enabled()
        })
    }

    fn exports(&self) -> Vec<AafExportRequestRow> {
        self.fake
            .jobs_started()
            .into_iter()
            .filter_map(|j| match j.request {
                JobRequest::ExportAaf(r) => Some(r),
                _ => None,
            })
            .collect()
    }
}

fn imports(f: &FakeSession) -> Vec<AafImportRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::ImportAaf(r) => Some(r),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------
// Export AAF

#[gpui::test]
fn the_export_sheet_has_swifts_strings_and_the_check(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    s.menu("file.export_aaf");
    assert_eq!(s.sheet().as_deref(), Some(EXPORT_AAF));
    s.expect("sheet", EXPORT_AAF);
    s.expect("text", EXPORT_AAF);
    s.expect(
        "text",
        "Audio, fades and volume; MIDI as MIDI files beside it",
    );
    assert!(s.on("checkbox", "Embed audio in the AAF file"));
    assert_eq!(s.expect("menu", "Bit depth")["value"], "24-bit");
    assert_eq!(
        s.expect("menu", "Stereo tracks")["value"],
        "As left and right mono tracks"
    );
    assert_eq!(s.expect("menu", "Start")["value"], "00:00:00:00");
    assert_eq!(s.expect("menu", "Frame rate")["value"], "25");
    assert!(s.on("checkbox", "Clip gain"));
    assert!(s.on("checkbox", "Track volume and pan"));
    s.expect("text", "Writes the folder \u{201c}Song AAF\u{201d}.");
    // The check ran on the background executor and landed.
    assert_eq!(s.fake.aaf_checks().len(), 1);
    s.expect("text", "What won't transfer");
    s.expect("text", "Plug-ins (2)");
    s.expect("button", "Cancel");
    assert!(s.enabled("button", EXPORT));
    // Each change asks again.
    s.click("menu", "Frame rate");
    s.click("menuitem", "29.97 drop frame");
    s.click("menu", "Start");
    s.click("menuitem", "01:00:00:00");
    s.click("checkbox", "Embed audio in the AAF file");
    let checks = s.fake.aaf_checks();
    assert_eq!(checks.len(), 4);
    let last = checks.last().unwrap();
    assert!(last.fps == AafFpsRow::Fps2997Drop && last.hour_start && !last.embed);
    assert_eq!(s.expect("menu", "Frame rate")["value"], "29.97 drop frame");
    // Esc cancels: nothing exported.
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(s.exports().is_empty() && s.panels.rows().is_empty());
}

#[gpui::test]
fn export_asks_for_the_folder_naming_the_cores_and_shows_progress(cx: &mut TestAppContext) {
    let f = fake();
    f.script_job(
        JobScript::done()
            .steps(&[0.5])
            .report(ReportRow {
                summary: "Exported 2 tracks".into(),
                items: vec![],
            })
            .written(&["/nowhere-vizdaw-test/Song AAF/Song.aaf"]),
    );
    let mut s = setup(cx, f, false);
    s.menu("file.export_aaf");
    s.click("menu", "Bit depth");
    s.click("menuitem", "16-bit");
    s.click("button", EXPORT);
    assert_eq!(s.panels.rows(), ["9d"]);
    assert_eq!(
        s.panels.messages(),
        [Some(
            "Choose where to put the folder \u{201c}Song AAF\u{201d}.".to_owned()
        )]
    );
    assert_eq!(s.sheet().as_deref(), Some(EXPORT_AAF), "up meanwhile");
    s.panels.answer("/nowhere-vizdaw-test");
    s.cx.run_until_parked();
    let r = s.exports().pop().expect("an export");
    assert_eq!(r.path, "/nowhere-vizdaw-test");
    assert_eq!(r.depth, vizdaw_ffi::AafDepthRow::Int16);
    assert!(!s.cx.has_pending_prompt(), "a new folder: nothing to ask");
    // Running: progress, and Cancel only; the choices are off.
    assert_eq!(s.expect("progress", "Export progress")["value"], "0.50");
    s.expect("button", "Cancel the export");
    assert!(s.node("button", EXPORT).is_none());
    assert!(!s.enabled("menu", "Frame rate"));
    assert!(!s.enabled("checkbox", "Clip gain"));
    // It ends: the sheet goes and the report shows what it wrote.
    s.tick();
    assert_eq!(s.sheet().as_deref(), Some("Export Report"));
}

#[gpui::test]
fn an_export_waits_for_a_running_bounce_and_a_failure_comes_back(cx: &mut TestAppContext) {
    let f = fake();
    let mut s = setup(cx, f, false);
    s.fake.script_job(JobScript::done().steps(&[0.4, 0.4]));
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.with_document(doc, |d, _| {
            let req = d.bounce_request(Some("/tmp/b.wav"), false).unwrap();
            d.try_start_job(JobKindRow::Bounce, |s| s.bounce(req))
                .unwrap();
        });
        cx.notify();
    });
    s.cx.run_until_parked();
    s.menu("file.export_aaf");
    s.click("button", EXPORT);
    s.panels.answer("/nowhere-vizdaw-test");
    s.cx.run_until_parked();
    assert!(s.exports().is_empty());
    assert!(!s.cx.has_pending_prompt(), "not a Busy alert");
    s.expect("text", "Waiting for the running job to finish\u{2026}");
    s.expect("button", "Cancel the waiting export");
    assert!(!s.enabled("menu", "Bit depth"));
    // The bounce ends: the export starts, and fails.
    s.fake
        .script_job(JobScript::failed("The disk is full.").steps(&[0.5]));
    s.tick();
    assert!(s.exports().is_empty(), "still bouncing");
    s.tick();
    assert_eq!(s.exports().len(), 1);
    s.tick();
    assert_eq!(s.sheet().as_deref(), Some(EXPORT_AAF));
    s.expect("text", "The disk is full.");
    assert!(!s.cx.has_pending_prompt(), "not an alert");
    assert!(s.enabled("button", EXPORT));
}

#[gpui::test]
fn a_folder_answered_after_the_sheet_went_starts_nothing(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    s.menu("file.export_aaf");
    s.click("button", EXPORT);
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.with_document(doc, |d, _| d.hide_aaf_export());
        cx.notify();
    });
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    s.panels.answer("/nowhere-vizdaw-test");
    s.cx.run_until_parked();
    assert!(s.exports().is_empty());
}

// ---------------------------------------------------------------------
// Import AAF

#[gpui::test]
fn the_import_sheet_shows_once_the_file_is_read_with_swifts_defaults(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    s.import("/m/Song.aaf");
    assert!(s.core.calls().contains(&"aaf_info(/m/Song.aaf)".to_owned()));
    assert_eq!(s.sheet().as_deref(), Some(IMPORT_AAF));
    s.expect("sheet", IMPORT_AAF);
    s.expect("text", IMPORT_AAF);
    s.expect(
        "text",
        "Song.aaf: 44100 Hz, 3 tracks, 2 audio files, 1 missing",
    );
    s.expect("radiogroup", "Import into");
    assert!(
        s.on("radio", "Open as a new project"),
        "new_project defaults true"
    );
    assert!(!s.on("radio", "Add to this project"));
    assert_eq!(
        s.expect("textfield", "MIDI file beside it")["value"],
        "/m/Song MIDI.mid"
    );
    s.expect("button", "Choose the MIDI file");
    s.expect("text", "Look for missing audio in\u{2026}");
    s.expect("text", "Nowhere else");
    s.expect("button", "Choose a folder to look for missing audio in");
    assert!(
        s.node("button", "Clear the folder to look for missing audio in")
            .is_none()
    );
    assert!(s.on("checkbox", "Join left and right tracks into stereo"));
    s.expect("button", "Cancel");
    s.expect("button", "Import");
    // Esc: nothing imported.
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(imports(&s.fake).is_empty());
}

#[gpui::test]
fn the_panels_fill_the_choices_and_import_adds_to_this_project(cx: &mut TestAppContext) {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.5]));
    let mut s = setup(cx, f, false);
    s.import("/m/Song.aaf");
    // Typed into, then chosen: the panel's answer wins.
    s.cx.simulate_keystrokes("tab");
    s.cx.simulate_input("x");
    s.cx.run_until_parked();
    s.click("button", "Choose the MIDI file");
    assert_eq!(s.panels.rows(), ["15"]);
    s.panels.answer("/m/Other.mid");
    s.cx.run_until_parked();
    assert_eq!(
        s.expect("textfield", "MIDI file beside it")["value"],
        "/m/Other.mid"
    );
    s.click("button", "Choose a folder to look for missing audio in");
    assert_eq!(s.panels.rows(), ["16"]);
    s.panels.answer("/m/Media");
    s.cx.run_until_parked();
    s.expect("text", "Media");
    // Clear, then choose again.
    s.click("button", "Clear the folder to look for missing audio in");
    s.expect("text", "Nowhere else");
    s.click("button", "Choose a folder to look for missing audio in");
    s.panels.answer("/m/Media");
    s.cx.run_until_parked();
    s.click("radio", "Add to this project");
    s.click("checkbox", "Join left and right tracks into stereo");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert_eq!(
        imports(&s.fake),
        [AafImportRequestRow {
            path: "/m/Song.aaf".into(),
            new_project: false,
            midi_path: Some("/m/Other.mid".into()),
            media_folder: Some("/m/Media".into()),
            join_pairs: false,
        }]
    );
    let windows = s.app.read_with(&s.cx, |a, _| a.document_windows().count());
    assert_eq!(windows, 1, "no project is made");
}

#[gpui::test]
fn typing_the_midi_path_turns_the_keys_off_and_is_sent(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    let made = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    s.core.hand_out("new_project", made.clone());
    s.import("/m/Plain.aaf");
    assert_eq!(s.expect("textfield", "MIDI file beside it")["value"], "");
    assert!(s.keys_on());
    s.cx.simulate_keystrokes("tab");
    s.cx.run_until_parked();
    assert!(!s.keys_on(), "the field edits: the keys are off");
    s.cx.simulate_input("/m/typed.mid");
    // Return at once (no frame drew since the typing) imports into a new
    // project at the file's rate, with what was typed.
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert!(s.keys_on());
    assert!(imports(&s.fake).is_empty());
    assert_eq!(
        imports(&made),
        [AafImportRequestRow {
            path: "/m/Plain.aaf".into(),
            new_project: true,
            midi_path: Some("/m/typed.mid".into()),
            media_folder: None,
            join_pairs: true,
        }]
    );
    assert!(
        s.core
            .calls()
            .iter()
            .any(|c| c.starts_with("new_project(44100,")),
        "{:?}",
        s.core.calls()
    );
    let windows = s.app.read_with(&s.cx, |a, _| a.document_windows().count());
    assert_eq!(windows, 2, "the new project's window opens");
}

#[gpui::test]
fn a_file_that_cannot_be_read_is_the_alert_and_no_sheet(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    s.import("/m/Broken.aaf");
    assert_eq!(s.sheet(), None);
    assert!(s.cx.has_pending_prompt(), "Import Failed");
}

// ---------------------------------------------------------------------
// Both

#[gpui::test]
fn under_the_channel_no_sheet_shows(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), true);
    s.menu("file.export_aaf");
    assert_eq!(s.sheet(), None);
    assert!(s.fake.aaf_checks().is_empty());
    // A read landing under the channel shows nothing either.
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.begin_aaf_import(doc, "/m/Song.aaf");
        cx.notify();
    });
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
}

#[test]
fn every_later_item_is_wired() {
    assert!(vizdaw_gpui::file_menu::LATER.is_empty());
}
