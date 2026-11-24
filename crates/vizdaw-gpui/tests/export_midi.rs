//! File › Export › MIDI File… and MIDI Clip File… (slice 7c Task 2): the
//! sheet with Swift's strings and the core's dry run (run off the UI
//! thread, again after each change); Export… asks for `<song>.mid` or
//! `<track>.midi2` (panel #10), or a folder for several clip tracks
//! (panel #9b), where names already taken are asked about before anything
//! is written; a failed export brings the sheet back with its message.

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
use vizdaw_app::testing::{FakeSession, JobRequest, JobScript, demo_fake, lane_row};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{
    ClipExportRequestRow, ClipNamesRow, ReportItemRow, ReportKindRow, ReportRow,
    SmfExportRequestRow,
};
use vizdaw_gpui::control::CallSource;
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

/// The demo with two tracks in Song, Groove (30) and Chords (31).
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

    fn title(&mut self) -> String {
        let doc = self.doc();
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().snapshot().title.clone()
        })
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

    fn smf_exports(&self) -> Vec<SmfExportRequestRow> {
        self.fake
            .jobs_started()
            .into_iter()
            .filter_map(|j| match j.request {
                JobRequest::ExportMidi(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    fn clip_exports(&self) -> Vec<ClipExportRequestRow> {
        self.fake
            .jobs_started()
            .into_iter()
            .filter_map(|j| match j.request {
                JobRequest::ExportMidiClip(r) => Some(r),
                _ => None,
            })
            .collect()
    }
}

#[gpui::test]
fn the_midi_file_sheet_has_swifts_strings_and_the_dry_run(cx: &mut TestAppContext) {
    let f = busy();
    f.set_check_report(
        "export_midi_check",
        ReportRow {
            summary: String::new(),
            items: vec![ReportItemRow {
                kind: ReportKindRow::LeftOut,
                what: "Audio regions".into(),
                count: 3,
                details: vec![],
            }],
        },
    );
    let mut s = setup(cx, f, false);
    s.menu("file.export_midi");
    assert_eq!(s.sheet().as_deref(), Some("Export MIDI File"));
    s.expect("sheet", "Export MIDI File");
    s.expect("text", "Export MIDI File");
    s.expect(
        "text",
        "Notes, controllers, tempo and markers as a Standard MIDI File",
    );
    assert_eq!(
        s.expect("menu", "MIDI file type")["value"],
        "Type 1 (one MIDI track per track)"
    );
    assert_eq!(
        s.expect("menu", "Resolution in ticks per quarter note")["value"],
        "960 PPQ"
    );
    s.expect("text", "Tracks");
    assert!(s.on("checkbox", "Export track Groove"));
    assert!(s.on("checkbox", "Export track Chords"));
    assert!(!s.on("checkbox", "Write per-note expression as MPE"));
    assert!(s.node("checkbox", "Time selection only").is_none());
    s.expect("text", "What won't transfer");
    // The dry run ran (on the background executor) and landed.
    assert_eq!(s.fake.smf_checks().len(), 1);
    s.expect("text", "Left out");
    s.expect("text", "Audio regions (3)");
    assert!(s.node("progress", "Checking what won't transfer").is_none());
    s.expect("button", "Cancel");
    assert_eq!(s.expect("button", "Export\u{2026}")["enabled"], true);
    // Esc cancels: nothing exported.
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(s.smf_exports().is_empty());
    let doc = s.doc();
    let shown = s.app.read_with(&s.cx, |a, _| {
        a.store.document(doc).unwrap().midi_export().shown()
    });
    assert!(!shown);
}

#[gpui::test]
fn each_change_asks_the_dry_run_again(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.menu("file.export_midi");
    s.expect("text", "Everything transfers.");
    s.click("menu", "MIDI file type");
    s.click("menuitem", "Type 0 (everything on one MIDI track)");
    s.click("menu", "Resolution in ticks per quarter note");
    s.click("menuitem", "15 360 PPQ");
    s.click("checkbox", "Write per-note expression as MPE");
    s.click("checkbox", "Export track Groove");
    let checks = s.fake.smf_checks();
    assert_eq!(checks.len(), 5);
    let last = checks.last().unwrap();
    assert!(last.type0 && last.ppq == 15_360 && last.mpe && last.lanes == [31]);
    assert_eq!(
        s.expect("menu", "MIDI file type")["value"],
        "Type 0 (everything on one MIDI track)"
    );
    // No track: Export… is off.
    s.click("checkbox", "Export track Chords");
    assert_eq!(s.expect("button", "Export\u{2026}")["enabled"], false);
}

#[gpui::test]
fn export_asks_for_the_songs_mid_and_exports(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    let title = s.title();
    s.menu("file.export_midi");
    s.click("button", "Export\u{2026}");
    assert_eq!(s.panels.rows(), ["10"]);
    assert_eq!(s.panels.names(), [Some(format!("{title}.mid"))]);
    assert_eq!(
        s.sheet().as_deref(),
        Some("Export MIDI File"),
        "up meanwhile"
    );
    // Typed without the extension: it is added (no such file: no question).
    s.panels.answer("/nowhere-vizdaw-test/Song");
    s.cx.run_until_parked();
    let r = s.smf_exports().pop().expect("an export");
    assert_eq!(r.path, "/nowhere-vizdaw-test/Song.mid");
    assert!(!r.replace, "nothing was there: nothing to replace");
    assert_eq!(r.lanes, [30, 31]);
    assert_eq!(s.sheet(), None);
    assert!(!s.cx.has_pending_prompt());
}

#[gpui::test]
fn a_failed_export_brings_the_sheet_back_with_its_message(cx: &mut TestAppContext) {
    let f = busy();
    f.script_job(JobScript::failed("disk full").steps(&[0.5]));
    let mut s = setup(cx, f, false);
    s.menu("file.export_midi");
    s.click("menu", "Resolution in ticks per quarter note");
    s.click("menuitem", "480 PPQ");
    s.click("button", "Export\u{2026}");
    s.panels.answer("/nowhere-vizdaw-test/x.mid");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None, "the job runs");
    s.tick();
    assert_eq!(s.sheet().as_deref(), Some("Export MIDI File"));
    s.expect("text", "disk full");
    assert!(!s.cx.has_pending_prompt(), "not an alert");
    // The choices are as they were.
    assert_eq!(
        s.expect("menu", "Resolution in ticks per quarter note")["value"],
        "480 PPQ"
    );
}

#[gpui::test]
fn one_clip_track_asks_for_its_midi2(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.menu("file.export_midi_clip");
    assert_eq!(s.sheet().as_deref(), Some("Export MIDI Clip File"));
    s.expect("text", "One track, lossless, as a MIDI 2.0 Clip File");
    assert!(s.node("menu", "MIDI file type").is_none());
    assert_eq!(
        s.expect("checkbox", "Time selection only")["enabled"],
        false,
        "no time selection"
    );
    s.expect(
        "text",
        "Each track becomes one file in the folder you choose.",
    );
    assert_eq!(s.fake.clip_checks().len(), 1);
    s.click("checkbox", "Export track Groove");
    assert!(
        s.node(
            "text",
            "Each track becomes one file in the folder you choose."
        )
        .is_none()
    );
    s.click("button", "Export\u{2026}");
    assert_eq!(s.panels.rows(), ["10"]);
    assert_eq!(s.panels.names(), [Some("Chords.midi2".to_owned())]);
    s.panels.answer("/nowhere-vizdaw-test/Chords.midi2");
    s.cx.run_until_parked();
    let r = s.clip_exports().pop().expect("an export");
    assert!(r.path == "/nowhere-vizdaw-test/Chords.midi2" && !r.replace && r.lanes == [31]);
    assert_eq!(s.sheet(), None);
}

/// Several clip tracks go into a folder: the core names the files first,
/// and when any is there the replace question is asked before anything
/// is written.
#[gpui::test]
fn a_folder_export_asks_before_replacing(cx: &mut TestAppContext) {
    let f = busy();
    let names = |taken: &[&str]| ClipNamesRow {
        names: vec!["01 Groove.midi2".into(), "02 Chords.midi2".into()],
        taken: taken.iter().map(|t| (*t).to_owned()).collect(),
    };
    f.set_clip_names(names(&["02 Chords.midi2"]));
    let mut s = setup(cx, f, false);
    s.menu("file.export_midi_clip");
    s.click("button", "Export\u{2026}");
    assert_eq!(s.panels.rows(), ["9b"]);
    s.panels.answer("/nowhere-vizdaw-test/out");
    s.cx.run_until_parked();
    let asked = s.fake.clip_name_requests();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].path == "/nowhere-vizdaw-test/out" && !asked[0].replace);
    assert!(s.clip_exports().is_empty(), "nothing written yet");
    assert_eq!(
        s.cx.pending_prompt(),
        Some((
            "1 file already exists in out. Replace it?".into(),
            String::new()
        ))
    );
    s.cx.simulate_prompt_answer("Cancel");
    s.cx.run_until_parked();
    assert!(s.clip_exports().is_empty());
    assert_eq!(s.sheet().as_deref(), Some("Export MIDI Clip File"));
    // Again, both taken: Replace replaces them.
    s.fake
        .set_clip_names(names(&["01 Groove.midi2", "02 Chords.midi2"]));
    s.click("button", "Export\u{2026}");
    s.panels.answer("/nowhere-vizdaw-test/out");
    s.cx.run_until_parked();
    assert_eq!(
        s.cx.pending_prompt().map(|p| p.0),
        Some("2 files already exist in out. Replace them?".into())
    );
    s.cx.simulate_prompt_answer("Replace");
    s.cx.run_until_parked();
    let r = s.clip_exports().pop().expect("an export");
    assert!(r.path == "/nowhere-vizdaw-test/out" && r.replace && r.lanes == [30, 31]);
    assert_eq!(s.sheet(), None);
}

#[gpui::test]
fn a_folder_with_none_taken_exports_without_replacing(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.menu("file.export_midi_clip");
    s.click("button", "Export\u{2026}");
    s.panels.answer("/nowhere-vizdaw-test/out");
    s.cx.run_until_parked();
    assert!(!s.cx.has_pending_prompt());
    let r = s.clip_exports().pop().expect("an export");
    assert!(r.path == "/nowhere-vizdaw-test/out" && !r.replace);
    assert_eq!(s.sheet(), None);
}

#[gpui::test]
fn under_the_channel_no_sheet_shows(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), true);
    s.menu("file.export_midi");
    s.menu("file.export_midi_clip");
    assert_eq!(s.sheet(), None);
    assert!(s.panels.rows().is_empty());
    assert!(s.fake.smf_checks().is_empty());
}

// ---------------------------------------------------------------------
// Review fixes (round 1)

/// Every replace question has Cancel as its default (Return) and its Esc
/// button; Replace is the destructive one, and must be clicked.
#[test]
fn the_replace_question_defaults_to_cancel() {
    use vizdaw_gpui::sheet_host::{Ask, ButtonRole, cancel_index, default_index};
    let ask = Ask::replace("q", None);
    let labels: Vec<&str> = ask.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(labels, ["Cancel", "Replace"]);
    assert_eq!(default_index(&ask.buttons), Some(0));
    assert_eq!(cancel_index(&ask.buttons), Some(0));
    assert_eq!(ask.role_of(1), Some(ButtonRole::Destructive));
    assert!(!ask.replaces(0) && ask.replaces(1) && !ask.replaces(2));
    // Drawn by GPUI's prompt (the sheet card), the same keys.
    let card = Ask::from_prompt(
        gpui::PromptLevel::Warning,
        "q",
        None,
        &ask.prompt_buttons(false),
    );
    assert_eq!(default_index(&card.buttons), Some(0));
    assert_eq!(cancel_index(&card.buttons), Some(0));
    // Its answers are read against the replace question's own buttons.
    let card_labels: Vec<&str> = card.buttons.iter().map(|b| b.label.as_str()).collect();
    assert_eq!(card_labels, labels);
}

/// Return on the folder export's question (drawn as the sheet card) is
/// Cancel: nothing is written; Esc likewise.
#[gpui::test]
fn return_on_the_folder_question_writes_nothing(cx: &mut TestAppContext) {
    let f = busy();
    f.set_clip_names(ClipNamesRow {
        names: vec!["01 Groove.midi2".into(), "02 Chords.midi2".into()],
        taken: vec!["01 Groove.midi2".into()],
    });
    let mut s = setup(cx, f, false);
    s.cx.update(|_, cx| vizdaw_gpui::sheet_host::use_sheet_prompts(cx));
    s.menu("file.export_midi_clip");
    for key in ["enter", "escape"] {
        s.click("button", "Export\u{2026}");
        s.panels.answer("/nowhere-vizdaw-test/out");
        s.cx.run_until_parked();
        let up = s
            .window
            .update(&mut s.cx, |_, window, _| window.has_active_prompt())
            .unwrap();
        assert!(up, "the question");
        s.cx.simulate_keystrokes(key);
        s.cx.run_until_parked();
        let up = s
            .window
            .update(&mut s.cx, |_, window, _| window.has_active_prompt())
            .unwrap();
        assert!(!up, "{key} answered it");
        assert!(s.clip_exports().is_empty(), "{key} wrote nothing");
        assert_eq!(s.sheet().as_deref(), Some("Export MIDI Clip File"));
    }
}

/// Return on the save panel's own replace question (the typed name
/// lacked the extension, and that file exists) is Cancel: nothing is
/// exported, no `replace: true` is sent.
#[gpui::test]
fn return_on_the_save_panels_question_writes_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Song.mid"), b"x").unwrap();
    let typed = dir.path().join("Song");
    let mut s = setup(cx, busy(), false);
    s.cx.update(|_, cx| vizdaw_gpui::sheet_host::use_sheet_prompts(cx));
    s.menu("file.export_midi");
    s.click("button", "Export\u{2026}");
    s.panels.answer(&typed.to_string_lossy());
    s.cx.run_until_parked();
    let up = s
        .window
        .update(&mut s.cx, |_, window, _| window.has_active_prompt())
        .unwrap();
    assert!(up, "the app's replace question");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert!(s.smf_exports().is_empty());
    assert_eq!(s.sheet().as_deref(), Some("Export MIDI File"));
}

/// The same question answered on the platform prompt: Replace exports,
/// replacing; Cancel nothing.
#[gpui::test]
fn the_save_panels_question_replaces_only_on_replace(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Song.mid"), b"x").unwrap();
    let typed = dir.path().join("Song");
    let mut s = setup(cx, busy(), false);
    s.menu("file.export_midi");
    s.click("button", "Export\u{2026}");
    s.panels.answer(&typed.to_string_lossy());
    s.cx.run_until_parked();
    s.cx.simulate_prompt_answer("Cancel");
    s.cx.run_until_parked();
    assert!(s.smf_exports().is_empty());
    s.click("button", "Export\u{2026}");
    s.panels.answer(&typed.to_string_lossy());
    s.cx.run_until_parked();
    s.cx.simulate_prompt_answer("Replace");
    s.cx.run_until_parked();
    let r = s.smf_exports().pop().expect("an export");
    assert!(r.replace && r.path == dir.path().join("Song.mid").to_string_lossy());
}

/// A file the panel confirmed replacing is sent with `replace: true`.
#[gpui::test]
fn a_confirmed_existing_file_is_sent_as_a_replace(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("Song.mid");
    std::fs::write(&file, b"x").unwrap();
    let mut s = setup(cx, busy(), false);
    s.menu("file.export_midi");
    s.click("button", "Export\u{2026}");
    s.panels.answer(&file.to_string_lossy());
    s.cx.run_until_parked();
    let r = s.smf_exports().pop().expect("an export");
    assert!(r.replace && r.path == file.to_string_lossy());
}

/// Review fix: cancelled as the folder panel answers, nothing is written
/// and nothing asked (the names race itself is pinned in the model's
/// `a_names_answer_after_cancel_writes_nothing`).
#[gpui::test]
fn cancelled_as_the_folder_panel_answers_writes_nothing(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy(), false);
    s.menu("file.export_midi_clip");
    s.click("button", "Export\u{2026}");
    // The panel answers, and the sheet is cancelled before anything runs.
    s.panels.answer("/nowhere-vizdaw-test/out");
    let doc = s.doc();
    s.cx.update(|_, cx| {
        AppState::global(cx).update(cx, |a, cx| {
            a.store.with_document(doc, |d, _| d.hide_midi_export());
            cx.notify();
        })
    });
    s.cx.run_until_parked();
    assert!(s.clip_exports().is_empty());
    assert!(!s.cx.has_pending_prompt());
    assert_eq!(s.sheet(), None);
}
