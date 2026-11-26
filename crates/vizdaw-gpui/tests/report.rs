//! The import and export report sheet (slice 7c Task 3; Swift
//! `ReportSheet`): it shows by itself after an import or export the
//! window's UI ran, and from File › Last Import or Export Report…; its
//! title follows the report; groups open onto their lines and places;
//! Copy, Save as Text… (panel #17, `MIDI Report.txt`), Show in Finder
//! (Linux: Show in Folder) for what an export wrote, Done (Esc). Never
//! under the control channel.

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
use vizdaw_app::interchange::ExportKind;
use vizdaw_app::panels::PanelSpec;
use vizdaw_app::report_text;
use vizdaw_app::testing::{FakeSession, JobScript, demo_fake, lane_row};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{ReportItemRow, ReportKindRow, ReportRow};
use vizdaw_gpui::control::CallSource;
use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
use vizdaw_gpui::{AppState, DocumentWindow, FileCommand};

type Shown = (&'static str, Option<String>, oneshot::Sender<Chosen>);

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
    fn shown(&self) -> Vec<(&'static str, Option<String>)> {
        self.shown
            .borrow()
            .iter()
            .map(|(r, n, _)| (*r, n.clone()))
            .collect()
    }

    fn answer(&self, path: &std::path::Path) {
        let (_, _, done) = self.shown.borrow_mut().remove(0);
        done.send(Ok(Some(vec![path.to_path_buf()]))).unwrap();
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

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
    panels: Rc<FakePanels>,
    revealed: Rc<RefCell<Vec<PathBuf>>>,
    cx: VisualTestContext,
}

fn busy() -> FakeSession {
    let f = demo_fake();
    let mut a = lane_row(30, "Groove", false);
    a.midi_event_count = 12;
    f.set_lanes(3, vec![a, lane_row(31, "Chords", false)]);
    f
}

fn setup(cx: &mut TestAppContext, fake: FakeSession, channel: bool) -> Setup {
    let fake = Arc::new(fake);
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let panels = Rc::new(FakePanels::default());
    let backend: Rc<dyn PanelBackend> = panels.clone();
    let revealed = Rc::new(RefCell::new(vec![]));
    let seen = revealed.clone();
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        set_backend(backend, cx);
        vizdaw_gpui::sheets::report::set_revealer(
            Rc::new(move |p, _| seen.borrow_mut().push(p.to_path_buf())),
            cx,
        );
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
        revealed,
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

    fn model<T>(&mut self, f: impl FnOnce(&mut vizdaw_app::DocumentModel) -> T) -> T {
        let doc = self.doc();
        self.app.update(&mut self.cx, |a, cx| {
            cx.notify();
            a.store.with_document(doc, |d, _| f(d)).unwrap()
        })
    }

    /// The document's jobs are polled, then the window draws.
    fn tick(&mut self) {
        self.model(|d| d.poll_jobs());
        self.cx.executor().advance_clock(Duration::from_millis(60));
        self.cx.run_until_parked();
    }

    /// An SMF export through the sheet's model, ending with `report`
    /// having written `written`.
    fn export(&mut self, report: ReportRow, written: &[&str]) {
        self.fake.script_job(
            JobScript::done()
                .steps(&[0.5])
                .report(report)
                .written(written),
        );
        self.model(|d| {
            d.show_midi_export(ExportKind::Smf);
            d.start_midi_export("/out/Song.mid", true);
        });
        self.cx.run_until_parked();
        self.tick();
    }

    fn shown(&mut self) -> bool {
        self.model(|d| d.report_shown())
    }
}

fn item(kind: ReportKindRow, what: &str, count: u64, details: &[&str]) -> ReportItemRow {
    ReportItemRow {
        kind,
        what: what.into(),
        count,
        details: details.iter().map(|d| d.to_string()).collect(),
    }
}

/// An export's report: Left out with places, and a note.
fn exported() -> ReportRow {
    ReportRow {
        summary: "Exported 2 tracks".into(),
        items: vec![
            item(ReportKindRow::Note, "Tempo kept", 1, &[]),
            item(
                ReportKindRow::LeftOut,
                "Muted notes",
                5,
                &["bar 1", "bar 2"],
            ),
        ],
    }
}

#[gpui::test]
fn an_exports_report_shows_after_the_job_with_swifts_strings(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.export(exported(), &["/out/Song.mid"]);
    assert_eq!(s.sheet().as_deref(), Some("Export Report"));
    s.expect("sheet", "Export Report");
    s.expect("text", "Export Report");
    s.expect("text", "Exported 2 tracks");
    // Groups start closed: their headings only.
    assert_ne!(s.expect("disclosure", "Left out, 5")["selected"], true);
    s.expect("disclosure", "Notes, 1");
    assert!(s.node("text", "Tempo kept").is_none());
    s.expect("button", "Copy the report");
    s.expect("button", "Save the report as text");
    s.expect("button", report_text::SHOW_FILES_LABEL);
    s.expect("button", "Done");
    // Esc is Done.
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(!s.shown());
}

#[gpui::test]
fn groups_open_onto_their_lines_and_places(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.export(exported(), &["/out/Song.mid"]);
    s.click("disclosure", "Left out, 5");
    assert_eq!(s.expect("disclosure", "Left out, 5")["selected"], true);
    // A line with places is a disclosure of its own.
    assert_ne!(s.expect("disclosure", "Muted notes (5)")["selected"], true);
    assert!(s.node("text", "bar 1").is_none());
    s.click("disclosure", "Muted notes (5)");
    s.expect("text", "bar 1");
    s.expect("text", "bar 2");
    s.expect("text", "and 3 more");
    s.click("disclosure", "Notes, 1");
    s.expect("text", "Tempo kept");
    // Closed again.
    s.click("disclosure", "Left out, 5");
    assert!(s.node("text", "bar 1").is_none());
}

#[gpui::test]
fn a_merges_conflicts_come_first(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    let r = ReportRow {
        summary: "Merged: 1 applied, 0 kept, 0 added, 0 removed, 1 in conflict".into(),
        items: vec![
            item(ReportKindRow::Changed, "Tempo", 1, &[]),
            item(
                ReportKindRow::Conflict,
                "Drums: level changed in both",
                1,
                &[],
            ),
        ],
    };
    s.model(|d| d.show_report(r));
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some("Merge Report"));
    let labels: Vec<String> = s
        .nodes()
        .into_iter()
        .filter(|n| n["role"] == "disclosure")
        .map(|n| n["label"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(labels, ["Conflicts, 1", "Changed, 1"]);
    // Nothing written: no Show in Finder.
    assert!(s.node("button", report_text::SHOW_FILES_LABEL).is_none());
}

#[gpui::test]
fn an_imports_report_without_groups_says_everything_transferred(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.model(|d| {
        d.show_report(ReportRow {
            summary: "Imported 1 track".into(),
            items: vec![],
        })
    });
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some("Import Report"));
    s.expect("text", "Everything transferred.");
    assert!(s.node("button", report_text::SHOW_FILES_LABEL).is_none());
    // Return does nothing (Done is Esc's only).
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some("Import Report"));
}

#[gpui::test]
fn copy_puts_the_plain_text_on_the_clipboard(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.export(exported(), &["/out/Song.mid"]);
    s.click("button", "Copy the report");
    let text = s.cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(text, Some(report_text::plain(&exported())));
    assert_eq!(s.sheet().as_deref(), Some("Export Report"), "stays up");
}

#[gpui::test]
fn save_as_text_writes_midi_report_txt(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.export(exported(), &["/out/Song.mid"]);
    s.click("button", "Save the report as text");
    assert_eq!(s.panels.shown(), [("17", Some("MIDI Report.txt".into()))]);
    let dir = tempfile::tempdir().unwrap();
    // Typed without the extension: it is added.
    s.panels.answer(&dir.path().join("Notes"));
    s.cx.run_until_parked();
    let written = std::fs::read_to_string(dir.path().join("Notes.txt")).unwrap();
    assert_eq!(written, report_text::plain(&exported()));
    assert_eq!(s.sheet().as_deref(), Some("Export Report"), "stays up");
}

#[gpui::test]
fn a_save_that_fails_is_an_alert_over_the_sheet(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.export(exported(), &["/out/Song.mid"]);
    s.click("button", "Save the report as text");
    s.panels
        .answer(std::path::Path::new("/nowhere-vizdaw-test/MIDI Report.txt"));
    s.cx.run_until_parked();
    let (title, _) = s.cx.pending_prompt().expect("an alert");
    assert_eq!(
        title,
        "The file \u{201c}MIDI Report.txt\u{201d} couldn\u{2019}t be saved."
    );
    s.cx.simulate_prompt_answer("OK");
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some("Export Report"));
}

#[gpui::test]
fn show_in_finder_reveals_what_the_export_wrote(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.export(
        exported(),
        &["/out/01 Groove.midi2", "/out/02 Chords.midi2"],
    );
    let button = s.expect("button", report_text::SHOW_FILES_LABEL);
    assert_eq!(button["enabled"], true);
    if cfg!(target_os = "macos") {
        assert_eq!(report_text::SHOW_FILES, "Show in Finder");
    } else {
        assert_eq!(report_text::SHOW_FILES, "Show in Folder");
    }
    s.click("button", report_text::SHOW_FILES_LABEL);
    assert_eq!(
        *s.revealed.borrow(),
        [PathBuf::from("/out/01 Groove.midi2")]
    );
    assert_eq!(s.sheet().as_deref(), Some("Export Report"), "stays up");
}

#[gpui::test]
fn last_import_or_export_report_shows_the_last_or_says_there_is_none(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    assert!(
        !vizdaw_gpui::file_menu::LATER.contains(&"file.interchange_report"),
        "wired"
    );
    s.menu("file.interchange_report");
    assert_eq!(s.sheet(), None);
    assert_eq!(
        s.model(|d| d.status().map(String::from)).as_deref(),
        Some("There is no import or export report yet.")
    );
    // An export's report, closed, then shown again: its files are kept.
    s.export(exported(), &["/out/Song.mid"]);
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    s.fake.set_interchange_report(Some(exported()));
    s.menu("file.interchange_report");
    assert_eq!(s.sheet().as_deref(), Some("Export Report"));
    s.expect("button", report_text::SHOW_FILES_LABEL);
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    // Another report (the channel's, say) has none, and its own title.
    s.fake.set_interchange_report(Some(ReportRow {
        summary: "Imported 3 tracks".into(),
        items: vec![],
    }));
    s.menu("file.interchange_report");
    assert_eq!(s.sheet().as_deref(), Some("Import Report"));
    s.expect("text", "Imported 3 tracks");
    assert!(s.node("button", report_text::SHOW_FILES_LABEL).is_none());
}

#[gpui::test]
fn under_the_channel_no_report_sheet_shows(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), true);
    s.export(exported(), &["/out/Song.mid"]);
    assert_eq!(s.sheet(), None);
    s.fake.set_interchange_report(Some(exported()));
    s.menu("file.interchange_report");
    assert_eq!(s.sheet(), None);
}

#[gpui::test]
fn the_sheet_goes_when_the_document_hides_the_report(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.export(exported(), &["/out/Song.mid"]);
    assert_eq!(s.sheet().as_deref(), Some("Export Report"));
    s.model(|d| d.hide_report());
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
}

/// A new report while the sheet is up: the sheet shows it, titled anew.
#[gpui::test]
fn a_new_report_while_up_retitles_the_sheet(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.model(|d| {
        d.show_report(ReportRow {
            summary: "Imported 1 track".into(),
            items: vec![],
        })
    });
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some("Import Report"));
    s.model(|d| {
        d.show_report(ReportRow {
            summary: "Merged: 1 applied".into(),
            items: vec![],
        })
    });
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some("Merge Report"));
    s.expect("sheet", "Merge Report");
    s.expect("text", "Merged: 1 applied");
}
