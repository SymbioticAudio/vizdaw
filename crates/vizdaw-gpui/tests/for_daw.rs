//! File › Export › For Another DAW… and DAWproject… (slice 7c Task 4):
//! the sheets with Swift's strings and the core's check (run off the UI
//! thread, again after each change); For Another DAW shares Bounce's
//! format rows and asks for a folder (panel #9c), DAWproject asks for the
//! core's file name (panel #13) and replaces only what was there when the
//! panel answered; an export waits for a running job ("Waiting for the
//! running … to finish…") instead of failing; progress and Cancel show
//! while it runs, the sheet closes when it ends and the report follows;
//! a failed export brings the sheet back with its message.

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
use vizdaw_app::testing::{FakeSession, JobRequest, JobScript, mixer_fake};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{
    BitDepthRow, ContainerRow, DawprojectExportRequestRow, ForDawCheckRow, ForDawRequestRow,
    JobKindRow, ProjectFormatRow, ProjectInfoRow, RegionGainRow, ReportItemRow, ReportKindRow,
    ReportRow,
};
use vizdaw_gpui::control::CallSource;
use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
use vizdaw_gpui::{AppState, DocumentWindow, FileCommand};

const FOR_DAW: &str = "Export for Another DAW";
const DAWPROJECT: &str = "Export DAWproject";
const EXPORT: &str = "Export, then choose where";

/// A panel shown: its row, its suggested name, its answer.
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
        let name = spec.default_name_for(&place.name, &place.ext);
        self.shown.borrow_mut().push((spec.row, name, done));
    }
}

impl FakePanels {
    fn rows(&self) -> Vec<&'static str> {
        self.shown.borrow().iter().map(|(r, _, _)| *r).collect()
    }

    fn names(&self) -> Vec<Option<String>> {
        self.shown
            .borrow()
            .iter()
            .map(|(_, n, _)| n.clone())
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
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    panels: Rc<FakePanels>,
    cx: VisualTestContext,
}

/// The mixer demo at 44.1 kHz 16-bit, its check scripted: stems [2],
/// one second, a left-out group.
fn fake() -> FakeSession {
    let f = mixer_fake();
    f.set_project_info(ProjectInfoRow {
        format: ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int16,
        },
        mismatched: 0,
        rate_mismatch: false,
        device_rate: 0.0,
    });
    let report = ReportRow {
        summary: "Exported 2 tracks".into(),
        items: vec![ReportItemRow {
            kind: ReportKindRow::LeftOut,
            what: "Strip colours".into(),
            count: 2,
            details: vec![],
        }],
    };
    f.set_for_daw_check(ForDawCheckRow {
        report: report.clone(),
        folder: "Song for another DAW".into(),
        files: vec![],
        strips: vec![2],
        start: "Every file starts at bar 1.".into(),
        frames: 44_100,
        warning: Some("One track plays external MIDI.".into()),
    });
    f.set_check_report("export_dawproject_check", report);
    f
}

fn setup(cx: &mut TestAppContext, fake: FakeSession, channel: bool) -> Setup {
    let fake = Arc::new(fake);
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(mixer_fake()))),
    );
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

    /// A bounce the UI started, running for `polls` more polls.
    fn bounce_running(&mut self, polls: usize) {
        self.fake
            .script_job(JobScript::done().steps(&vec![0.4; polls + 1]));
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| {
                let req = d.bounce_request(Some("/tmp/b.wav"), false).unwrap();
                d.try_start_job(JobKindRow::Bounce, |s| s.bounce(req))
                    .unwrap();
            });
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    fn for_daw_exports(&self) -> Vec<ForDawRequestRow> {
        self.fake
            .jobs_started()
            .into_iter()
            .filter_map(|j| match j.request {
                JobRequest::ForDaw(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    fn dawproject_exports(&self) -> Vec<DawprojectExportRequestRow> {
        self.fake
            .jobs_started()
            .into_iter()
            .filter_map(|j| match j.request {
                JobRequest::ExportDawproject(r) => Some(r),
                _ => None,
            })
            .collect()
    }
}

// ---------------------------------------------------------------------
// Export for Another DAW

#[gpui::test]
fn the_for_daw_sheet_has_swifts_strings_and_the_check(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    s.menu("file.export_for_daw");
    assert_eq!(s.sheet().as_deref(), Some(FOR_DAW));
    s.expect("sheet", FOR_DAW);
    s.expect("text", FOR_DAW);
    s.expect("text", "Audio stems and MIDI files for any DAW");
    s.expect("text", "Stems");
    // The core's defaults are the stems; the master is not offered.
    assert!(!s.on("checkbox", "Stem for Synth"));
    assert!(s.on("checkbox", "Stem for Bus 1"));
    assert!(s.node("checkbox", "Stem for Master").is_none());
    // The format rows shared with Bounce, at the project's format.
    assert_eq!(s.expect("menu", "Format")["value"], "WAV");
    assert_eq!(s.expect("menu", "Bits")["value"], "16-bit");
    assert_eq!(s.expect("menu", "Rate")["value"], "44100 Hz");
    assert!(s.on("checkbox", "Dither"));
    assert!(!s.on("checkbox", "Add the stems to this project"));
    // The check ran on the background executor and landed.
    let checks = s.fake.for_daw_checks();
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0].strips, None, "the core's defaults first");
    s.expect("text", "Every file starts at bar 1.");
    s.expect("text", "Length 0:01.0 + tail");
    s.expect("text", "One track plays external MIDI.");
    s.expect("text", "What won't transfer");
    s.expect("text", "Left out");
    s.expect("text", "Strip colours (2)");
    s.expect("button", "Cancel");
    assert!(s.enabled("button", EXPORT));
    // Esc cancels: nothing exported.
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(s.for_daw_exports().is_empty());
    assert!(s.panels.rows().is_empty());
}

#[gpui::test]
fn each_change_asks_the_check_again(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    s.menu("file.export_for_daw");
    s.click("menu", "Format");
    s.click("menuitem", "AIFF");
    s.click("menu", "Bits");
    s.click("menuitem", "32-bit float");
    assert!(!s.enabled("checkbox", "Dither"), "off at 32-bit float");
    s.click("menu", "Rate");
    s.click("menuitem", "96000 Hz");
    s.click("checkbox", "Stem for Synth");
    s.click("checkbox", "Add the stems to this project");
    let checks = s.fake.for_daw_checks();
    assert_eq!(checks.len(), 6);
    let last = checks.last().unwrap();
    assert_eq!(last.container, ContainerRow::Aiff);
    assert!(last.depth == BitDepthRow::Float32 && !last.dither);
    assert!(last.rate == 96_000 && last.add_to_project);
    assert_eq!(last.strips.as_deref(), Some(&[1, 2][..]));
    // Other… opens the rate field.
    s.click("menu", "Rate");
    s.click("menuitem", "Other\u{2026}");
    s.expect("textfield", "Other rate in Hz, 8000 to 384000");
}

#[gpui::test]
fn export_asks_for_a_folder_and_shows_progress_until_the_job_ends(cx: &mut TestAppContext) {
    let f = fake();
    f.script_job(
        JobScript::done()
            .steps(&[0.5])
            .report(ReportRow {
                summary: "Exported 2 tracks".into(),
                items: vec![],
            })
            .written(&["/nowhere-vizdaw-test/Song for another DAW"]),
    );
    let mut s = setup(cx, f, false);
    s.menu("file.export_for_daw");
    s.click("button", EXPORT);
    assert_eq!(s.panels.rows(), ["9c"]);
    assert_eq!(s.sheet().as_deref(), Some(FOR_DAW), "up meanwhile");
    s.panels.answer("/nowhere-vizdaw-test");
    s.cx.run_until_parked();
    let r = s.for_daw_exports().pop().expect("an export");
    assert_eq!(r.path, "/nowhere-vizdaw-test");
    assert_eq!(r.strips.as_deref(), Some(&[2][..]));
    assert!(r.rate == 44_100 && r.depth == BitDepthRow::Int16 && r.dither);
    assert!(!s.cx.has_pending_prompt(), "a new folder: nothing to ask");
    // Running: progress, and Cancel only; the choices are off.
    assert_eq!(s.sheet().as_deref(), Some(FOR_DAW));
    assert_eq!(s.expect("progress", "Export progress")["value"], "0.50");
    s.expect("button", "Cancel the export");
    assert!(s.node("button", EXPORT).is_none());
    assert!(!s.enabled("menu", "Format"));
    assert!(!s.enabled("checkbox", "Stem for Bus 1"));
    // It ends: the sheet goes and the report shows what it wrote.
    s.tick();
    assert_eq!(s.sheet().as_deref(), Some("Export Report"));
    s.expect("button", vizdaw_app::report_text::SHOW_FILES_LABEL);
}

#[gpui::test]
fn cancel_while_running_cancels_the_job(cx: &mut TestAppContext) {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.5, 0.5]));
    let mut s = setup(cx, f, false);
    s.menu("file.export_for_daw");
    s.click("button", EXPORT);
    s.panels.answer("/nowhere-vizdaw-test");
    s.cx.run_until_parked();
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.fake.cancelled_jobs().len(), 1);
    assert_eq!(s.sheet().as_deref(), Some(FOR_DAW), "until the job ends");
    s.tick();
    assert_eq!(s.sheet(), None);
    assert!(!s.cx.has_pending_prompt(), "a cancel is quiet");
}

#[gpui::test]
fn an_export_waits_for_a_running_bounce(cx: &mut TestAppContext) {
    let f = fake();
    let mut s = setup(cx, f, false);
    s.bounce_running(1);
    s.menu("file.export_for_daw");
    s.click("button", EXPORT);
    s.panels.answer("/nowhere-vizdaw-test");
    s.cx.run_until_parked();
    assert!(s.for_daw_exports().is_empty());
    assert!(!s.cx.has_pending_prompt(), "not a Busy alert");
    s.expect(
        "text",
        "Waiting for the running import, export or bounce to finish\u{2026}",
    );
    s.expect("progress", "Waiting");
    s.expect("button", "Cancel the waiting export");
    assert!(!s.enabled("menu", "Rate"));
    // Esc drops the waiting export; the sheet stays.
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some(FOR_DAW));
    assert!(s.enabled("button", EXPORT));
    // Again: once the bounce ends the export starts.
    s.click("button", EXPORT);
    s.panels.answer("/nowhere-vizdaw-test");
    s.cx.run_until_parked();
    s.tick();
    assert!(s.for_daw_exports().is_empty(), "still bouncing");
    s.tick();
    assert_eq!(s.for_daw_exports().len(), 1);
}

#[gpui::test]
fn a_failed_export_brings_the_sheet_back_with_its_message(cx: &mut TestAppContext) {
    let f = fake();
    f.script_job(JobScript::failed("The disk is full.").steps(&[0.5]));
    let mut s = setup(cx, f, false);
    s.menu("file.export_for_daw");
    s.click("menu", "Rate");
    s.click("menuitem", "48000 Hz");
    s.click("button", EXPORT);
    s.panels.answer("/nowhere-vizdaw-test");
    s.cx.run_until_parked();
    // Asked away meanwhile (another window's menu, say).
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.with_document(doc, |d, _| d.hide_for_daw());
        cx.notify();
    });
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    s.tick();
    assert_eq!(s.sheet().as_deref(), Some(FOR_DAW));
    s.expect("text", "The disk is full.");
    assert!(!s.cx.has_pending_prompt(), "not an alert");
    assert_eq!(
        s.expect("menu", "Rate")["value"],
        "48000 Hz",
        "as they were"
    );
}

#[gpui::test]
fn a_folder_answered_after_the_sheet_went_starts_nothing(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    s.menu("file.export_for_daw");
    s.click("button", EXPORT);
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.with_document(doc, |d, _| d.hide_for_daw());
        cx.notify();
    });
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    s.panels.answer("/nowhere-vizdaw-test");
    s.cx.run_until_parked();
    assert!(s.for_daw_exports().is_empty());
}

// ---------------------------------------------------------------------
// Export DAWproject

#[gpui::test]
fn the_dawproject_sheet_has_swifts_strings_and_the_check(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    s.menu("file.export_dawproject");
    assert_eq!(s.sheet().as_deref(), Some(DAWPROJECT));
    s.expect("text", DAWPROJECT);
    s.expect("text", "Most complete: tracks, mix, plug-ins, automation");
    assert_eq!(
        s.expect("menu", "Region gain")["value"],
        "Apply to the audio"
    );
    s.expect("text", "What won't transfer");
    s.expect("text", "Strip colours (2)");
    assert_eq!(s.fake.dawproject_checks().len(), 1);
    // A change asks again.
    s.click("menu", "Region gain");
    s.click("menuitem", "Leave out");
    let checks = s.fake.dawproject_checks();
    assert_eq!(checks.len(), 2);
    assert_eq!(checks[1].region_gain, RegionGainRow::LeaveOut);
    assert!(s.enabled("button", EXPORT));
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(s.dawproject_exports().is_empty());
}

#[gpui::test]
fn export_asks_for_the_cores_name_and_replaces_only_what_is_there(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let there = dir.path().join("Song.dawproject");
    std::fs::write(&there, b"x").unwrap();
    let mut s = setup(cx, fake(), false);
    s.menu("file.export_dawproject");
    s.click("button", EXPORT);
    assert_eq!(s.panels.rows(), ["13"]);
    assert_eq!(s.panels.names(), [Some("Song.dawproject".to_owned())]);
    // No such file: no replace.
    let new = dir.path().join("New.dawproject");
    s.panels.answer(&new.to_string_lossy());
    s.cx.run_until_parked();
    let r = s.dawproject_exports().pop().expect("an export");
    assert!(r.path == new.to_string_lossy() && !r.replace);
    assert_eq!(s.sheet(), None, "done at once: the sheet goes");
    // The panel confirmed replacing the file that is there.
    s.menu("file.export_dawproject");
    s.click("button", EXPORT);
    s.panels.answer(&there.to_string_lossy());
    s.cx.run_until_parked();
    let r = s.dawproject_exports().pop().expect("an export");
    assert!(r.path == there.to_string_lossy() && r.replace);
}

#[gpui::test]
fn a_dawproject_export_waits_for_the_running_job(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), false);
    s.bounce_running(0);
    s.menu("file.export_dawproject");
    s.click("button", EXPORT);
    s.panels.answer("/nowhere-vizdaw-test/Song.dawproject");
    s.cx.run_until_parked();
    assert!(s.dawproject_exports().is_empty());
    assert!(!s.cx.has_pending_prompt());
    s.expect("text", "Waiting for the running job to finish\u{2026}");
    s.expect("button", "Cancel the waiting export");
    assert!(!s.enabled("menu", "Region gain"));
    s.tick();
    let r = s.dawproject_exports().pop().expect("the bounce ended");
    assert!(!r.replace);
}

#[gpui::test]
fn a_failed_dawproject_export_brings_the_sheet_back(cx: &mut TestAppContext) {
    let f = fake();
    f.script_job(JobScript::failed("disk full").steps(&[0.5]));
    let mut s = setup(cx, f, false);
    s.menu("file.export_dawproject");
    s.click("button", EXPORT);
    s.panels.answer("/nowhere-vizdaw-test/Song.dawproject");
    s.cx.run_until_parked();
    s.expect("progress", "Export progress");
    s.expect("button", "Cancel the export");
    s.tick();
    assert_eq!(s.sheet().as_deref(), Some(DAWPROJECT));
    s.expect("text", "disk full");
    assert!(!s.cx.has_pending_prompt(), "not an alert");
    assert!(s.enabled("button", EXPORT));
}

// ---------------------------------------------------------------------
// Both

#[gpui::test]
fn under_the_channel_no_sheet_shows(cx: &mut TestAppContext) {
    let mut s = setup(cx, fake(), true);
    s.menu("file.export_for_daw");
    s.menu("file.export_dawproject");
    assert_eq!(s.sheet(), None);
    assert!(s.fake.for_daw_checks().is_empty());
    assert!(s.fake.dawproject_checks().is_empty());
}

#[test]
fn both_items_are_wired() {
    let later = vizdaw_gpui::file_menu::LATER;
    assert!(!later.contains(&"file.export_for_daw"));
    assert!(!later.contains(&"file.export_dawproject"));
}
