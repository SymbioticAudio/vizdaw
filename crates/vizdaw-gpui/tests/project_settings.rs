//! File › Project Settings… and Import Audio… (slice 7b Task 4): the sheet
//! opens loaded (format, note, song info) with Swift's strings; typing in
//! its fields turns the document's keys off; OK sends only what changed (a
//! song-info change once, a format change as the format job with its
//! banner); a held merge turns OK off with Swift's help; Cancel sends
//! nothing. Import Audio takes several files through panel #7 into the
//! import job at Swift's import target; a failed import is the alert
//! "Import Failed".

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
use vizdaw_app::project_settings::HELD_HELP;
use vizdaw_app::testing::{FakeSession, JobRequest, JobScript, demo_fake};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{
    BitDepthRow, ImportTargetRow, JobKindRow, MediaOriginRow, MediaRow, ProjectFormatRow,
    SongInfoRow,
};
use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
use vizdaw_gpui::{AppState, DocumentWindow, FileCommand};

/// Panels a test answers itself.
#[derive(Default)]
struct FakePanels {
    shown: RefCell<Vec<(&'static str, oneshot::Sender<Chosen>)>>,
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

    fn answer(&self, paths: &[&str]) {
        let (_, done) = self.shown.borrow_mut().remove(0);
        done.send(Ok(Some(paths.iter().map(PathBuf::from).collect())))
            .unwrap();
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

    fn open_settings(&mut self) {
        self.cx.dispatch_action(FileCommand {
            id: "file.project_settings".into(),
        });
        self.cx.run_until_parked();
        assert_eq!(self.sheet().as_deref(), Some("Project Settings"));
    }

    fn keys_on(&mut self) -> bool {
        let doc = self.doc();
        self.app.read_with(&self.cx, |a, _| {
            a.store.document(doc).unwrap().transport_keys_enabled()
        })
    }

    /// The document's idle runs (jobs polled), then the window draws.
    fn tick(&mut self) {
        self.cx.executor().advance_clock(Duration::from_millis(60));
        self.cx.run_until_parked();
    }

    fn formats(&self) -> Vec<ProjectFormatRow> {
        self.fake
            .jobs_started()
            .into_iter()
            .filter_map(|j| match j.request {
                JobRequest::Format(f) => Some(f),
                _ => None,
            })
            .collect()
    }
}

fn song_fake() -> FakeSession {
    let f = demo_fake();
    f.set_song(SongInfoRow {
        title: Some("Song".into()),
        comment: Some("line one\nline two".into()),
        ..SongInfoRow::default()
    });
    f
}

#[gpui::test]
fn the_sheet_opens_loaded_with_swifts_strings(cx: &mut TestAppContext) {
    let f = song_fake();
    f.set_media(vec![MediaRow {
        id: 1,
        name: "a.wav".into(),
        path: "Audio/a.caf".into(),
        channels: 2,
        frames: 48_000 * 5,
        sample_rate: 48_000,
        origin: MediaOriginRow::Imported,
        original_name: None,
        offline: None,
        uses: 1,
        used_percent: 100.0,
        peak: None,
    }]);
    let mut s = setup(cx, f);
    s.open_settings();
    s.expect("sheet", "Project Settings");
    for t in ["Project Settings", "Format", "Song info"] {
        s.expect("text", t);
    }
    s.expect(
        "text",
        "Changing the format converts 1 audio file (5 s of audio) into new files (one undo step).",
    );
    assert_eq!(s.expect("menu", "Sample rate")["value"], "48 kHz");
    assert_eq!(s.expect("menu", "Bit depth")["value"], "24-bit");
    assert_eq!(s.expect("textfield", "Song title")["value"], "Song");
    assert_eq!(s.expect("textfield", "Artist")["value"], "");
    // A loaded comment keeps its line break.
    assert_eq!(
        s.expect("textfield", "Comment")["value"],
        "line one\nline two"
    );
    for l in [
        "Album",
        "Original artist",
        "Composer",
        "Songwriter",
        "Producer",
        "Arranger",
        "Year",
        "Genre",
        "Copyright",
        "Website",
    ] {
        s.expect("textfield", l);
    }
    assert_eq!(s.expect("button", "OK")["enabled"], true);
    s.expect("button", "Cancel");
    // Cancel sends nothing.
    s.click("button", "Cancel");
    assert_eq!(s.sheet(), None);
    assert_eq!(s.fake.count("set_song_info"), 0);
    assert!(s.formats().is_empty());
}

#[gpui::test]
fn typing_turns_the_keys_off_and_ok_sends_the_song_info_once(cx: &mut TestAppContext) {
    let mut s = setup(cx, song_fake());
    s.open_settings();
    assert!(s.keys_on());
    // Tab to Artist (the second field), type.
    s.cx.simulate_keystrokes("tab tab");
    s.cx.run_until_parked();
    assert!(!s.keys_on(), "a field edits: the keys are off");
    s.cx.simulate_input("Band");
    s.cx.simulate_keystrokes("space");
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("play(3)"), 0, "space typed");
    assert_eq!(s.expect("textfield", "Artist")["value"], "Band ");
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(s.keys_on());
    assert_eq!(s.fake.count("set_song_info"), 1);
    let song = s.fake.song();
    assert_eq!(song.artist.as_deref(), Some("Band "));
    assert_eq!(song.title.as_deref(), Some("Song"));
    assert_eq!(song.comment.as_deref(), Some("line one\nline two"));
    assert!(s.formats().is_empty(), "the format did not change");
    // Opened again and OK'd untouched: nothing is sent.
    s.open_settings();
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.fake.count("set_song_info"), 1);
}

#[gpui::test]
fn a_format_change_runs_the_format_job_with_its_banner(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.script_job(JobScript::done().steps(&[0.25; 40]));
    let mut s = setup(cx, f);
    s.open_settings();
    s.click("menu", "Sample rate");
    s.click("menuitem", "96 kHz");
    s.click("menu", "Bit depth");
    s.click("menuitem", "32-bit float");
    assert_eq!(s.expect("menu", "Sample rate")["value"], "96 kHz");
    s.click("button", "OK");
    assert_eq!(s.sheet(), None);
    assert_eq!(
        s.formats(),
        [ProjectFormatRow {
            sample_rate: 96_000,
            depth: BitDepthRow::Float32
        }]
    );
    assert_eq!(s.fake.jobs_started()[0].kind, JobKindRow::Format);
    assert_eq!(s.fake.count("set_song_info"), 0);
    s.tick();
    s.expect("progress", "Converting media\u{2026}");
}

#[gpui::test]
fn a_held_merge_turns_ok_off_for_a_format_change(cx: &mut TestAppContext) {
    let mut s = setup(cx, demo_fake());
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.document_mut(doc).unwrap().set_merge_held(true);
        cx.notify();
    });
    s.open_settings();
    let ok = s.expect("button", "OK");
    assert_eq!(ok["enabled"], true, "nothing changed yet");
    assert!(ok.get("value").is_none_or(Value::is_null));
    s.click("menu", "Sample rate");
    s.click("menuitem", "96 kHz");
    let ok = s.expect("button", "OK");
    assert_eq!(ok["enabled"], false);
    assert_eq!(ok["value"], HELD_HELP);
    // Return waits too.
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.sheet().as_deref(), Some("Project Settings"));
    // Back to the project's rate: OK again.
    s.click("menu", "Sample rate");
    s.click("menuitem", "48 kHz");
    assert_eq!(s.expect("button", "OK")["enabled"], true);
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(s.formats().is_empty());
}

#[gpui::test]
fn import_audio_puts_one_file_on_the_selected_track_at_the_playhead(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.script_job(JobScript::done().steps(&[0.25; 40]));
    let mut s = setup(cx, f);
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.with_document(doc, |d, _| d.select_track(31, false));
        cx.notify();
    });
    s.cx.run_until_parked();
    s.cx.simulate_keystrokes("secondary-shift-i");
    s.cx.run_until_parked();
    assert_eq!(s.panels.rows(), ["7"]);
    s.panels.answer(&["/m/a.wav"]);
    s.cx.run_until_parked();
    let started = s.fake.jobs_started();
    assert_eq!(
        started[0].request,
        JobRequest::Import {
            paths: vec!["/m/a.wav".into()],
            target: Some(ImportTargetRow {
                seq: 3,
                lane: Some(31),
                tick: 0
            }),
        }
    );
    s.tick();
    s.expect("progress", "Importing audio\u{2026}");
    // Several files: new tracks.
    s.cx.simulate_keystrokes("secondary-shift-i");
    s.cx.run_until_parked();
    s.panels.answer(&["/m/a.wav", "/m/b.wav"]);
    s.cx.run_until_parked();
    let JobRequest::Import { target, .. } = &s.fake.jobs_started()[1].request else {
        panic!("an import")
    };
    assert_eq!(target.as_ref().unwrap().lane, None);
}

#[gpui::test]
fn a_failed_import_is_the_import_failed_alert(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.script_job(JobScript::failed("b.wav is not audio"));
    let mut s = setup(cx, f);
    s.cx.simulate_keystrokes("secondary-shift-i");
    s.cx.run_until_parked();
    s.panels.answer(&["/m/b.wav"]);
    s.cx.run_until_parked();
    s.tick();
    s.tick();
    let prompt = s.cx.pending_prompt();
    let sheet = s.sheet();
    assert!(
        prompt
            .as_ref()
            .is_some_and(|(t, m)| t == "Import Failed" && m == "b.wav is not audio")
            || sheet.as_deref() == Some("Import Failed"),
        "prompt {prompt:?}, sheet {sheet:?}"
    );
}
