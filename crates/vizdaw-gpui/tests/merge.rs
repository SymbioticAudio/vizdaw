//! Merge DAWproject (slice 7c Task 6): the return prompt's Merge With
//! Existing Project… on the open match (its window gets the sheet) or
//! through panel #12 (a Linux `project.json` is its package); the sheet
//! with Swift's strings while the Hold job reads the file, the snapshot
//! list when the file lost its export ID (read off the UI thread), the
//! preview and its conflicts; Merge (Return) then the report, Cancel
//! (Esc) once; a queued merge after the report; a held merge disables
//! Convert and the rate change with the help; nothing under the channel,
//! and taking the sheet down there cancels nothing.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
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
use vizdaw_app::testing::{FakeCore, FakeSession, JobRequest, JobScript, demo_fake};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{
    BitDepthRow, DawprojectInfoRow, DawprojectMergeRequestRow, MergePreviewRow, ProjectFormatRow,
    ProjectInfoRow, ReportRow, SnapshotRow,
};
use vizdaw_gpui::control::CallSource;
use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
use vizdaw_gpui::{AppState, DocumentWindow};

const PID: &str = "0f8b6f8e-3c1a-4d2e-9b7a-5e6f7a8b9c0d";
const EID: &str = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
const OTHER: &str = "2b3c4d5e-6f7a-4b8c-9d0e-1f2a3b4c5d6e";
const FILE: &str = "/tmp/Back.dawproject";
const SECOND: &str = "/tmp/Second.dawproject";
const PROMPT: &str = "This DAWproject came from a VizDAW project.";
const MERGE_ANSWER: &str = "Merge With Existing Project\u{2026}";
const TITLE: &str = "Merge DAWproject";

type Shown = (&'static str, oneshot::Sender<Chosen>);

#[derive(Default)]
struct FakePanels {
    shown: RefCell<Vec<Shown>>,
}

impl PanelBackend for FakePanels {
    fn show(
        &self,
        spec: &PanelSpec,
        _: &PanelPlace,
        _: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        _: &mut App,
    ) {
        self.shown.borrow_mut().push((spec.row, done));
    }
}

impl FakePanels {
    fn rows(&self) -> Vec<&'static str> {
        self.shown.borrow().iter().map(|(r, _)| *r).collect()
    }

    fn answer(&self, path: &str) {
        let (_, done) = self.shown.borrow_mut().remove(0);
        done.send(Ok(Some(vec![PathBuf::from(path)]))).unwrap();
    }
}

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

fn info(export: Option<&str>) -> DawprojectInfoRow {
    DawprojectInfoRow {
        sample_rate: 48_000,
        tracks: 2,
        audio_files: 0,
        from_vizdaw: true,
        export_id: export.map(str::to_owned),
        project_id: Some(PID.into()),
    }
}

fn preview(lines: &[&str]) -> MergePreviewRow {
    MergePreviewRow {
        applied: 3,
        kept: 1,
        added: 1,
        removed: 0,
        conflicts: lines.len() as u32,
        conflict_lines: lines.iter().map(|l| l.to_string()).collect(),
        summary: "Merged: 3 applied, 1 kept, 1 added, 0 removed, 2 in conflict".into(),
        export_id: EID.into(),
        exported_at: 1_800_000_000,
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

/// The demo open as "/tmp/Song.vizdaw", its project ID `PID`.
fn setup(cx: &mut TestAppContext) -> Setup {
    let core = Arc::new(FakeCore::new());
    let fake = Arc::new(demo_fake());
    fake.set_path(Some("/tmp/Song.vizdaw"));
    fake.set_project_id(Some(PID));
    core.hand_out("new_demo", fake.clone());
    core.set_dawproject_info(FILE, info(Some(EID)));
    core.set_dawproject_info(SECOND, info(Some(EID)));
    let store = DocumentStore::with_core(core.clone());
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

    fn nodes_of(&mut self, w: WindowHandle<DocumentWindow>) -> Vec<Value> {
        let tree = w
            .read_with(&self.cx, |v, _| v.registry().borrow().tree())
            .unwrap();
        tree["nodes"].as_array().unwrap().clone()
    }

    fn node(&mut self, role: &str, label: &str) -> Option<Value> {
        let w = self.window;
        self.nodes_of(w)
            .into_iter()
            .rfind(|n| n["role"] == role && n["label"] == label)
    }

    fn expect(&mut self, role: &str, label: &str) -> Value {
        let w = self.window;
        self.node(role, label)
            .unwrap_or_else(|| panic!("no {role} {label:?} in {:?}", self.nodes_of(w)))
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

    fn sheet_of(&mut self, w: WindowHandle<DocumentWindow>) -> Option<String> {
        w.read_with(&self.cx, |v, _| v.sheet_label()).unwrap()
    }

    fn sheet(&mut self) -> Option<String> {
        let w = self.window;
        self.sheet_of(w)
    }

    fn keys(&mut self, k: &str) {
        self.cx.simulate_keystrokes(k);
        self.cx.run_until_parked();
    }

    /// Opens `path` as the GUI does: the return prompt shows.
    fn open(&mut self, path: &str) {
        self.cx
            .update(|_, cx| vizdaw_gpui::open_path(Path::new(path), cx));
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

    fn merges(&self) -> Vec<DawprojectMergeRequestRow> {
        self.fake
            .jobs_started()
            .into_iter()
            .filter_map(|j| match j.request {
                JobRequest::MergeDawproject(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    fn calls(&self, name: &str) -> usize {
        self.fake
            .calls()
            .iter()
            .filter(|c| c.starts_with(name))
            .count()
    }

    fn held(&mut self) -> bool {
        let doc = self.doc();
        self.app
            .read_with(&self.cx, |a, _| a.store.document(doc).unwrap().merge_held())
    }

    /// The return prompt (the platform's), answered Merge.
    fn merge_from_prompt(&mut self, path: &str) {
        self.open(path);
        assert_eq!(
            self.cx.pending_prompt().map(|p| p.0).as_deref(),
            Some(PROMPT)
        );
        self.cx.simulate_prompt_answer(MERGE_ANSWER);
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn merge_into_the_open_match_previews_then_merges(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.fake.script_job(
        JobScript::done()
            .merge(preview(&[
                "Drums: level changed in both",
                "Keys: notes changed in both",
            ]))
            .steps(&[0.4]),
    );
    s.merge_from_prompt(FILE);
    assert_eq!(s.panels.rows(), Vec::<&str>::new(), "no panel: it matched");
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
    assert_eq!(s.merges().len(), 1);
    s.expect("text", TITLE);
    s.expect("text", "Back.dawproject");
    s.expect("text", "Reading the file\u{2026}");
    assert_eq!(s.expect("progress", "Merge progress")["value"], "0.40");
    assert!(!s.enabled("button", "Merge into this project"));
    s.expect("button", "Cancel");
    s.tick();
    s.expect(
        "text",
        "Merged: 3 applied, 1 kept, 1 added, 0 removed, 2 in conflict",
    );
    s.expect("text", "Conflicts (VizDAW's version is kept)");
    s.expect("group", "Conflicts, 2");
    s.expect("text", "Drums: level changed in both");
    assert!(s.enabled("button", "Merge into this project"));
    s.expect("button", "Cancel the merge; nothing changes");
    assert!(s.node("button", "Preview the merge again").is_none());
    assert!(s.held());
    // Return is Merge: the report follows.
    s.fake.set_merge_report(ReportRow {
        summary: "Merged: 3 applied, 1 kept, 1 added, 0 removed, 2 in conflict".into(),
        items: vec![],
    });
    s.keys("enter");
    assert_eq!(s.calls("confirm_merge"), 1);
    assert_eq!(s.sheet().as_deref(), Some("Merge Report"));
    assert!(!s.held());
    s.keys("escape");
    assert_eq!(s.sheet(), None);
    assert_eq!(s.calls("confirm_merge") + s.calls("cancel_merge"), 1);
}

#[gpui::test]
fn no_conflicts_and_esc_cancels_once(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.fake
        .script_job(JobScript::done().merge(preview(&[])).steps(&[0.4]));
    s.merge_from_prompt(FILE);
    s.tick();
    s.expect("text", "No conflicts.");
    s.keys("escape");
    assert_eq!(s.sheet(), None);
    assert_eq!(s.calls("cancel_merge"), 1);
    assert!(!s.held());
    s.keys("escape");
    assert_eq!(s.calls("cancel_merge"), 1, "once");
}

#[gpui::test]
fn a_lost_export_id_lists_the_exports_to_choose_from(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.core.set_dawproject_info(FILE, info(None));
    s.fake.set_snapshots(vec![
        SnapshotRow {
            export_id: OTHER.into(),
            exported_at: 1_800_000_500,
            tracks: 3,
        },
        SnapshotRow {
            export_id: EID.into(),
            exported_at: 1_800_000_000,
            tracks: 1,
        },
    ]);
    s.merge_from_prompt(FILE);
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
    assert_eq!(s.fake.reads("dawproject_snapshots"), 1);
    s.expect("text", "Which export did this file come from?");
    s.expect("list", "Exports of this project");
    let rows: Vec<Value> = {
        let w = s.window;
        s.nodes_of(w)
            .into_iter()
            .filter(|n| n["role"] == "row")
            .collect()
    };
    assert_eq!(rows.len(), 2);
    let first = rows[0]["label"].as_str().unwrap().to_owned();
    let second = rows[1]["label"].as_str().unwrap().to_owned();
    assert!(first.starts_with("Export of ") && first.ends_with(", 3 tracks"));
    assert!(second.ends_with(", 1 track"));
    assert_eq!(rows[0]["selected"], true, "the newest is selected");
    assert!(s.enabled("button", "Preview the merge with the chosen export"));
    assert!(s.merges().is_empty(), "nothing before Preview");
    s.click("row", &second);
    assert_eq!(s.expect("row", &second)["selected"], true);
    s.fake.script_job(JobScript::done().steps(&[0.5]));
    s.keys("enter");
    assert_eq!(s.merges().len(), 1);
    assert_eq!(s.merges()[0].snapshot.as_deref(), Some(EID));
    s.expect("text", "Reading the file\u{2026}");
}

#[gpui::test]
fn no_match_asks_for_the_project(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // Another project's ID: no open project matches.
    s.fake.set_project_id(Some(OTHER));
    s.merge_from_prompt(FILE);
    assert_eq!(s.panels.rows(), ["12"]);
    assert_eq!(s.sheet(), None);
    let song = Arc::new(demo_fake());
    song.set_path(Some("/tmp/Theirs.vizdaw"));
    song.script_job(JobScript::done().steps(&[0.5]));
    s.core.hand_out("open", song.clone());
    // Linux: the package's project.json is that package.
    s.panels.answer("/tmp/Theirs.vizdaw/project.json");
    s.cx.run_until_parked();
    assert!(
        s.core
            .calls()
            .iter()
            .any(|c| c == "open(/tmp/Theirs.vizdaw)"),
        "{:?}",
        s.core.calls()
    );
    let w = s.cx.update(|_, cx| {
        AppState::global(cx)
            .read(cx)
            .document_windows()
            .map(|w| w.handle)
            .find(|h| *h != s.window)
    });
    let w = w.expect("its window opened");
    assert_eq!(s.sheet_of(w).as_deref(), Some(TITLE));
    assert_eq!(song.jobs_started().len(), 1);
}

#[gpui::test]
fn a_queued_merge_shows_after_the_report(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.fake
        .script_job(JobScript::done().merge(preview(&[])).steps(&[0.4]));
    s.merge_from_prompt(FILE);
    s.tick();
    // A second file, merged into the same project, waits.
    s.merge_from_prompt_queued(SECOND);
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
    s.expect("text", "Back.dawproject");
    s.fake.set_merge_report(ReportRow {
        summary: "Merged: 1 applied, 0 kept, 0 added, 0 removed, 0 in conflict".into(),
        items: vec![],
    });
    s.fake.script_job(JobScript::done().steps(&[0.5]));
    s.keys("enter");
    assert_eq!(
        s.sheet().as_deref(),
        Some("Merge Report"),
        "the report first"
    );
    assert_eq!(s.merges().len(), 2, "the second began");
    s.keys("escape");
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
    s.expect("text", "Second.dawproject");
    assert_eq!(s.calls("cancel_merge"), 0);
    assert!(s.fake.cancelled_jobs().is_empty());
}

impl Setup {
    /// A second return prompt answered Merge while the sheet is up: the
    /// store queues it on the matched document.
    fn merge_from_prompt_queued(&mut self, path: &str) {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            a.store
                .document_mut(doc)
                .unwrap()
                .begin_merge(path, info(Some(EID)));
            cx.notify();
        });
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn a_held_merge_turns_convert_and_the_rate_change_off(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.fake.set_project_info(ProjectInfoRow {
        format: ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int24,
        },
        mismatched: 1,
        rate_mismatch: true,
        device_rate: 48_000.0,
    });
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.document_mut(doc).unwrap().refresh_project_info();
        cx.notify();
    });
    s.cx.run_until_parked();
    assert!(s.enabled("button", "Convert 1 File"));
    s.fake
        .script_job(JobScript::done().merge(preview(&[])).steps(&[0.4]));
    s.merge_from_prompt(FILE);
    s.tick();
    let help = "Finish or cancel the DAWproject merge waiting to be confirmed first.";
    for label in ["Convert 1 File", "Change Project Rate to 48 kHz"] {
        let b = s.expect("button", label);
        assert_eq!(b["enabled"], false, "{label}");
        assert_eq!(b["value"], help, "{label}");
    }
    s.keys("escape");
    for label in ["Convert 1 File", "Change Project Rate to 48 kHz"] {
        let b = s.expect("button", label);
        assert_eq!(b["enabled"], true, "{label}");
        assert!(b["value"].is_null(), "{label}");
    }
}

#[gpui::test]
fn under_the_channel_the_sheet_goes_and_nothing_is_cancelled(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.fake
        .script_job(JobScript::done().merge(preview(&[])).steps(&[0.4]));
    s.merge_from_prompt(FILE);
    s.tick();
    assert_eq!(s.sheet().as_deref(), Some(TITLE));
    s.cx.update(|_, cx| {
        vizdaw_gpui::control::pump(&NoCalls, cx);
        // Outside this window's update.
        cx.defer(vizdaw_gpui::alerts::present);
    });
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(s.held(), "still held");
    assert_eq!(s.calls("cancel_merge") + s.calls("confirm_merge"), 0);
}
