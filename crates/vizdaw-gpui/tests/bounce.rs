//! File › Bounce… (slice 7b Task 6): the sheet opens at the project's
//! format with Swift's strings and the core's check (run off the UI
//! thread); a mix asks for `Bounce.<ext>` (panel #8), stems for a folder
//! (panel #9a), Add to project alone for nothing; the job's progress and
//! Cancel show in the sheet, which closes when the job ends; a finished
//! bounce names its files in the status line, a failed one is the alert
//! "Bounce Failed". The `Rate (Hz)` field turns the document's keys off
//! while typing.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use futures::channel::oneshot;
use gpui::{
    AnyWindowHandle, App, Entity, Modifiers, TestAppContext, VisualTestContext, WindowHandle,
};
use serde_json::Value;
use vizdaw_app::panels::PanelSpec;
use vizdaw_app::testing::{FakeSession, JobRequest, JobScript, demo_fake, mixer_fake};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{
    BounceCheckRow, BounceRequestRow, BounceResultRow, BounceSourceRow, BouncedFileRow,
    ContainerRow,
};
use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
use vizdaw_gpui::{AppState, DocumentWindow, FileCommand};

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

    fn cancel(&self) {
        let (_, _, done) = self.shown.borrow_mut().remove(0);
        done.send(Ok(None)).unwrap();
    }
}

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    panels: Rc<FakePanels>,
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
    let panels = Rc::new(FakePanels::default());
    let backend: Rc<dyn PanelBackend> = panels.clone();
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        set_backend(backend, cx);
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

    /// The last node with `role` and `label` (the sheet is drawn last,
    /// over the window's own controls of the same name).
    fn node(&mut self, role: &str, label: &str) -> Option<Value> {
        self.nodes()
            .into_iter()
            .rfind(|n| n["role"] == role && n["label"] == label)
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

    /// Clicks the sheet's Bounce button (the source picker's label and
    /// the sheet's are Bounce too, but neither is a button).
    fn bounce(&mut self) {
        self.click("button", "Bounce");
    }

    fn sheet(&mut self) -> Option<String> {
        self.window
            .read_with(&self.cx, |v, _| v.sheet_label())
            .unwrap()
    }

    fn open(&mut self) {
        self.cx.dispatch_action(FileCommand {
            id: "file.bounce".into(),
        });
        self.cx.run_until_parked();
        assert_eq!(self.sheet().as_deref(), Some("Bounce"));
    }

    fn keys_on(&mut self) -> bool {
        let doc = self.doc();
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().transport_keys_enabled()
        })
    }

    fn status(&mut self) -> Option<String> {
        let doc = self.doc();
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().status().map(str::to_owned)
        })
    }

    /// The document's jobs are polled (idle's poll, which runs at most
    /// every 50 ms of real time), then the window draws.
    fn tick(&mut self) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store.with_document(doc, |d, _| d.poll_jobs());
            cx.notify();
        });
        self.cx.executor().advance_clock(Duration::from_millis(60));
        self.cx.run_until_parked();
    }

    /// Whether the node is selected (a segment, a toggle that is on).
    fn on(&mut self, role: &str, label: &str) -> bool {
        self.expect(role, label)["selected"] == true
    }

    fn bounces(&self) -> Vec<BounceRequestRow> {
        self.fake
            .jobs_started()
            .into_iter()
            .filter_map(|j| match j.request {
                JobRequest::Bounce(r) => Some(r),
                _ => None,
            })
            .collect()
    }
}

#[gpui::test]
fn the_sheet_opens_at_the_projects_format_with_the_cores_check(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_bounce_check(BounceCheckRow {
        external_lanes: 1,
        frames: 48_000 * 92 + 19_200,
        files: vec![],
        warning: Some("1 lane plays external MIDI and will be silent.".into()),
    });
    let mut s = setup(cx, f);
    s.open();
    s.expect("sheet", "Bounce");
    s.expect("text", "Bounce");
    assert!(s.on("button", "Mix"));
    assert!(!s.on("button", "Stems"));
    s.expect(
        "text",
        "Files ignore mute and solo. Add to project bounces as heard: muted and solo-silenced strips, and strips routed into a muted bus, are silent.",
    );
    assert!(s.node("text", vizdaw_app::bounce::STEMS_CAPTION).is_none());
    assert_eq!(
        s.expect("checkbox", "Time selection only")["enabled"],
        false,
        "no time selection"
    );
    assert_eq!(s.expect("menu", "Format")["value"], "WAV");
    assert_eq!(s.expect("menu", "Bits")["value"], "24-bit");
    assert_eq!(s.expect("menu", "Rate")["value"], "48000 Hz");
    assert!(!s.on("checkbox", "Dither"));
    assert!(!s.on("checkbox", "Add to project"));
    assert!(s.node("checkbox", "Also write the files").is_none());
    // The check ran (on the background executor) and landed.
    assert_eq!(s.fake.bounce_checks().len(), 1);
    s.expect("text", "1 lane plays external MIDI and will be silent.");
    s.expect("text", "Length 1:32.4 + tail");
    assert!(s.node("progress", "Bounce progress").is_none());
    s.expect("button", "Cancel");
    // Cancel bounces nothing.
    s.click("button", "Cancel");
    assert_eq!(s.sheet(), None);
    assert!(s.bounces().is_empty());
}

#[gpui::test]
fn a_mix_asks_for_bounce_dot_ext_and_replaces_as_confirmed(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.open();
    s.click("menu", "Format");
    s.click("menuitem", "AIFF");
    assert_eq!(s.expect("menu", "Format")["value"], "AIFF");
    let checks = s.fake.bounce_checks();
    assert_eq!(checks.len(), 2, "checked again after the change");
    assert_eq!(checks[1].container, ContainerRow::Aiff);
    s.bounce();
    assert_eq!(s.panels.rows(), ["8"]);
    assert_eq!(s.panels.names(), [Some("Bounce.aif".to_owned())]);
    // A cancelled panel leaves the sheet up and bounces nothing.
    s.panels.cancel();
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some("Bounce"));
    assert!(s.bounces().is_empty());
    s.bounce();
    // Typed without the extension: it is added (no such file: no question).
    s.panels.answer("/nowhere-vizdaw-test/Mix");
    s.cx.run_until_parked();
    let r = s.bounces().pop().expect("a bounce");
    assert_eq!(r.path.as_deref(), Some("/nowhere-vizdaw-test/Mix.aif"));
    assert!(r.replace, "the panel confirmed");
    assert_eq!(r.source, BounceSourceRow::Mix);
    assert_eq!(r.container, ContainerRow::Aiff);
}

#[gpui::test]
fn stems_ask_for_a_folder_and_need_a_strip(cx: &mut TestAppContext) {
    let mut s = setup(cx, mixer_fake());
    s.open();
    s.click("button", "Stems");
    assert!(s.on("button", "Stems"));
    s.expect(
        "text",
        "Files already in the folder are kept; new stems take the next free number.",
    );
    assert!(s.on("checkbox", "Synth"));
    assert!(s.on("checkbox", "Bus 1"));
    assert!(s.node("checkbox", "Master").is_none());
    s.click("checkbox", "Synth");
    s.click("checkbox", "Bus 1");
    assert_eq!(s.expect("button", "Bounce")["enabled"], false);
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert!(s.panels.rows().is_empty(), "Return waits too");
    s.click("checkbox", "Bus 1");
    assert_eq!(s.expect("button", "Bounce")["enabled"], true);
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.panels.rows(), ["9a"]);
    s.panels.answer("/o/stems");
    s.cx.run_until_parked();
    let r = s.bounces().pop().expect("a bounce");
    assert_eq!(r.source, BounceSourceRow::Stems { strips: vec![2] });
    assert_eq!(r.path.as_deref(), Some("/o/stems"));
    assert!(!r.replace, "stems never replace");
}

#[gpui::test]
fn add_to_project_alone_asks_for_nothing(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.open();
    s.click("checkbox", "Add to project");
    assert!(s.on("checkbox", "Also write the files"));
    s.click("checkbox", "Also write the files");
    s.bounce();
    assert!(s.panels.rows().is_empty());
    let r = s.bounces().pop().expect("a bounce");
    assert!(r.add_to_project && r.path.is_none());
    // Done at once: the sheet closed.
    assert_eq!(s.sheet(), None);
}

#[gpui::test]
fn progress_and_cancel_show_in_the_sheet_until_the_job_ends(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.script_job(JobScript::done().steps(&[0.25; 40]));
    let mut s = setup(cx, f);
    s.open();
    s.click("checkbox", "Add to project");
    s.click("checkbox", "Also write the files");
    s.bounce();
    assert_eq!(s.sheet().as_deref(), Some("Bounce"));
    assert_eq!(s.expect("progress", "Bounce progress")["value"], "0.25");
    assert!(
        s.node("button", "Bounce").is_none(),
        "only Cancel while it runs"
    );
    assert_eq!(s.expect("button", "Mix")["enabled"], false);
    // Esc presses Cancel: the job is asked to stop; the sheet waits.
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    let job = s.fake.jobs_started()[0].id;
    assert_eq!(s.fake.cancelled_jobs(), [job]);
    assert_eq!(s.sheet().as_deref(), Some("Bounce"));
    s.tick();
    s.tick();
    assert_eq!(s.sheet(), None, "the cancelled job ended");
    assert!(!s.cx.has_pending_prompt(), "cancelled quietly");
}

#[gpui::test]
fn a_finished_bounce_names_its_files_and_a_failed_one_is_the_alert(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.script_job(JobScript::done().steps(&[0.5]).bounce(BounceResultRow {
        files: vec![BouncedFileRow {
            path: "/o/Bounce.wav".into(),
            frames: 96_000,
            rate: 48_000,
            bits: "24-bit".into(),
            channels: 2,
            clipped: 0,
        }],
        media: vec![],
    }));
    f.script_job(JobScript::failed("no space").steps(&[0.5]));
    let mut s = setup(cx, f);
    s.open();
    s.bounce();
    s.panels.answer("/nowhere-vizdaw-test/Bounce.wav");
    s.cx.run_until_parked();
    s.tick();
    s.tick();
    assert_eq!(s.sheet(), None);
    assert!(
        s.status()
            .is_some_and(|t| t.contains("wrote /o/Bounce.wav (96000 frames, 48000 Hz, 24-bit)")),
        "{:?}",
        s.status()
    );
    s.open();
    s.bounce();
    s.panels.answer("/nowhere-vizdaw-test/Bounce.wav");
    s.cx.run_until_parked();
    s.tick();
    s.tick();
    s.tick();
    let prompt = s.cx.pending_prompt();
    let sheet = s.sheet();
    assert!(
        prompt
            .as_ref()
            .is_some_and(|(t, m)| t == "Bounce Failed" && m.starts_with("no space"))
            || sheet.as_deref() == Some("Bounce Failed"),
        "prompt {prompt:?}, sheet {sheet:?}"
    );
}

#[gpui::test]
fn the_other_rate_field_turns_the_keys_off_and_sets_the_rate(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    s.open();
    assert!(
        s.node("textfield", "Other rate in Hz, 8000 to 384000")
            .is_none()
    );
    s.click("menu", "Rate");
    s.click("menuitem", "Other\u{2026}");
    assert_eq!(s.expect("menu", "Rate")["value"], "Other\u{2026}");
    assert_eq!(
        s.expect("textfield", "Other rate in Hz, 8000 to 384000")["value"],
        "48000"
    );
    assert!(s.keys_on());
    s.cx.simulate_keystrokes("tab");
    s.cx.run_until_parked();
    assert!(!s.keys_on(), "the field edits: the keys are off");
    s.cx.simulate_input("22050");
    s.cx.run_until_parked();
    assert_eq!(s.fake.bounce_checks().last().unwrap().rate, 22_050);
    // Below the range: clamped.
    s.cx.simulate_keystrokes("backspace backspace backspace backspace");
    s.cx.run_until_parked();
    assert_eq!(s.fake.bounce_checks().last().unwrap().rate, 8_000);
    s.cx.simulate_input("1050");
    s.cx.run_until_parked();
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.panels.rows(), ["8"]);
    s.panels.answer("/nowhere-vizdaw-test/Bounce.wav");
    s.cx.run_until_parked();
    assert_eq!(s.bounces().pop().unwrap().rate, 21_050);
}
