//! File › Import MIDI File… (slice 7c Task 1): the panel's file is read
//! off the UI thread, then the sheet shows with Swift's strings and
//! defaults; Import sends the choices as an `ImportMidi` job; Esc cancels;
//! "A new project" turns the destinations off, locks the tempo map on and
//! imports into a new 48 kHz project whose window opens; a file that
//! cannot be read is the alert "Import Failed".

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
use vizdaw_app::testing::{FakeCore, FakeSession, JobRequest, demo_fake, dest_row, lane_row, seq};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{
    ChannelMapRow, ChannelUseRow, MidiFileInfoRow, MidiFileKindRow, MidiImportRequestRow,
};
use vizdaw_gpui::control::CallSource;
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

    fn answer(&self, path: &str) {
        let (_, done) = self.shown.borrow_mut().remove(0);
        done.send(Ok(Some(vec![PathBuf::from(path)]))).unwrap();
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

/// The demo with events in Song's tracks (so the tempo map is a choice)
/// and the destinations Piano (5) and Bass Synth (6).
fn busy() -> FakeSession {
    let f = demo_fake();
    let mut a = lane_row(30, "Groove", false);
    a.midi_event_count = 12;
    f.set_lanes(3, vec![a, lane_row(31, "Chords", false)]);
    f.set_destinations(vec![dest_row(5, "Piano"), dest_row(6, "Bass Synth")]);
    f
}

fn info() -> MidiFileInfoRow {
    let ch = |group, channel, notes, suggested| ChannelUseRow {
        group,
        channel,
        notes,
        suggested,
    };
    MidiFileInfoRow {
        kind: MidiFileKindRow::Smf1,
        tracks: 2,
        channels: vec![ch(0, 0, 10, Some(5)), ch(1, 9, 1, None)],
        has_tempo: true,
        mpe_found: false,
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

fn setup(cx: &mut TestAppContext, fake: FakeSession) -> Setup {
    setup_with(cx, fake, false)
}

/// The demo over `fake` in a window (`channel`: the control channel was
/// on first).
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

    /// File › Import MIDI File…, the panel answered with `path`.
    fn import(&mut self, path: &str) {
        self.cx.dispatch_action(FileCommand {
            id: "file.import_midi".into(),
        });
        self.cx.run_until_parked();
        assert_eq!(self.panels.rows(), ["3"]);
        self.panels.answer(path);
        self.cx.run_until_parked();
    }

    fn imports(f: &FakeSession) -> Vec<MidiImportRequestRow> {
        f.jobs_started()
            .into_iter()
            .filter_map(|j| match j.request {
                JobRequest::ImportMidi(r) => Some(r),
                _ => None,
            })
            .collect()
    }

    fn on(&mut self, role: &str, label: &str) -> bool {
        self.expect(role, label)["selected"] == true
    }

    fn enabled(&mut self, role: &str, label: &str) -> bool {
        self.expect(role, label)["enabled"] == true
    }
}

#[gpui::test]
fn the_sheet_shows_the_file_with_swifts_defaults(cx: &mut TestAppContext) {
    let f = busy();
    f.set_midi_file_info("/tmp/Song.mid", info());
    let mut s = setup(cx, f);
    s.import("/tmp/Song.mid");
    assert_eq!(s.sheet().as_deref(), Some("Import MIDI File"));
    s.expect("sheet", "Import MIDI File");
    s.expect("text", "Import MIDI File");
    s.expect("text", "Song.mid: Standard MIDI File, type 1, 2 tracks");
    s.expect("text", "Import into");
    assert!(s.on("radio", "This project, at the playhead"));
    assert!(!s.on("radio", "A new project"));
    s.expect("text", "Where each channel plays");
    s.expect("text", "Channel 1");
    s.expect("text", "10 notes");
    s.expect("text", "Group 2, channel 10");
    s.expect("text", "1 note");
    assert_eq!(
        s.expect("menu", "Destination for channel 1")["value"],
        "Piano"
    );
    assert_eq!(
        s.expect("menu", "Destination for group 2, channel 10")["value"],
        "New built-in synth"
    );
    assert!(!s.on("checkbox", "Split tracks by channel"));
    let mpe = s.expect("checkbox", "Treat as MPE");
    assert!(mpe["selected"] != true);
    assert_eq!(
        mpe["value"],
        "Member-channel notes, bend, CC 74 and pressure become per-note expression"
    );
    let tempo = s.expect("checkbox", "Use the file's tempo map");
    assert!(tempo["selected"] != true && tempo["enabled"] == true);
    assert_eq!(
        tempo["value"],
        "Replace this sequence's tempo and meter from the playhead with the file's"
    );
    s.expect("button", "Cancel");
    s.expect("button", "Import");
    // Esc cancels: nothing is imported.
    s.cx.simulate_keystrokes("escape");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    assert!(Setup::imports(&s.fake).is_empty());
}

#[gpui::test]
fn import_sends_the_choices(cx: &mut TestAppContext) {
    let f = busy();
    f.set_midi_file_info("/tmp/Song.mid", info());
    let mut s = setup(cx, f);
    s.import("/tmp/Song.mid");
    s.click("menu", "Destination for group 2, channel 10");
    s.click("menuitem", "Bass Synth");
    assert_eq!(
        s.expect("menu", "Destination for group 2, channel 10")["value"],
        "Bass Synth"
    );
    s.click("checkbox", "Split tracks by channel");
    s.click("checkbox", "Treat as MPE");
    s.click("checkbox", "Use the file's tempo map");
    assert!(s.on("checkbox", "Split tracks by channel"));
    assert!(s.on("checkbox", "Use the file's tempo map"));
    // Return presses Import.
    s.cx.simulate_keystrokes("enter");
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
    let map = |group, channel, dest| ChannelMapRow {
        group,
        channel,
        dest,
    };
    assert_eq!(
        Setup::imports(&s.fake),
        [MidiImportRequestRow {
            path: "/tmp/Song.mid".into(),
            new_project: false,
            seq: 3,
            lane: None,
            tick: 0,
            channel_map: vec![map(0, 0, Some(5)), map(1, 9, Some(6))],
            split_channels: Some(true),
            mpe: true,
            use_tempo: true,
        }]
    );
}

#[gpui::test]
fn an_empty_sequence_locks_the_tempo_map_on(cx: &mut TestAppContext) {
    let f = demo_fake();
    f.set_midi_file_info("/tmp/Song.mid", info());
    let mut s = setup(cx, f);
    s.import("/tmp/Song.mid");
    let tempo = s.expect("checkbox", "Use the file's tempo map");
    assert!(tempo["selected"] == true && tempo["enabled"] == false);
    assert_eq!(
        tempo["value"],
        "A sequence without events always takes the file's tempo and meter"
    );
}

#[gpui::test]
fn a_new_project_imports_with_the_choices_in_its_own_window(cx: &mut TestAppContext) {
    let f = busy();
    f.set_midi_file_info("/tmp/Song.mid", info());
    let mut s = setup(cx, f);
    s.import("/tmp/Song.mid");
    s.click("radio", "A new project");
    assert!(s.on("radio", "A new project"));
    s.expect(
        "text",
        "In a new project each channel plays a new built-in synth.",
    );
    assert!(s.node("text", "Where each channel plays").is_none());
    assert!(!s.enabled("menu", "Destination for channel 1"));
    let tempo = s.expect("checkbox", "Use the file's tempo map");
    assert!(tempo["selected"] == true && tempo["enabled"] == false);
    s.click("checkbox", "Treat as MPE");
    let made = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    s.core.hand_out("new_project", made.clone());
    let before = s.cx.update(|_, cx| cx.windows().len());
    s.click("button", "Import");
    assert_eq!(s.sheet(), None);
    assert!(Setup::imports(&s.fake).is_empty(), "not into this project");
    let r = Setup::imports(&made)
        .pop()
        .expect("imported into the new project");
    assert!(r.new_project && r.seq == 9 && r.use_tempo && r.mpe);
    assert_eq!(r.split_channels, Some(false));
    assert!(r.channel_map.len() == 2 && r.channel_map.iter().all(|c| c.dest.is_none()));
    assert!(
        s.core
            .calls()
            .iter()
            .any(|c| c.starts_with("new_project(48000,")),
        "{:?}",
        s.core.calls()
    );
    assert_eq!(
        s.cx.update(|_, cx| cx.windows().len()),
        before + 1,
        "its window opened"
    );
    let docs = s.app.read_with(&s.cx, |a, _| a.store.count());
    assert_eq!(docs, 2);
}

#[gpui::test]
fn a_file_that_cannot_be_read_is_an_alert(cx: &mut TestAppContext) {
    let mut s = setup(cx, busy());
    s.import("/tmp/Missing.mid");
    let prompt = s.cx.pending_prompt();
    let sheet = s.sheet();
    assert!(
        prompt
            .as_ref()
            .is_some_and(|(t, m)| t == "Import Failed" && m == "/tmp/Missing.mid could not be read")
            || sheet.as_deref() == Some("Import Failed"),
        "prompt {prompt:?}, sheet {sheet:?}"
    );
    let doc = s.doc();
    let read = s.app.read_with(&s.cx, |a, _| {
        a.store.document(doc).unwrap().midi_import().is_some()
    });
    assert!(!read);
}

#[gpui::test]
fn under_the_control_channel_the_sheet_never_shows(cx: &mut TestAppContext) {
    let f = busy();
    f.set_midi_file_info("/tmp/Song.mid", info());
    let mut s = setup_with(cx, f, true);
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.begin_midi_import(doc, "/tmp/Song.mid");
        cx.notify();
    });
    s.cx.run_until_parked();
    let landed = s.app.read_with(&s.cx, |a, _| {
        a.store.document(doc).unwrap().midi_read().is_some()
    });
    assert!(landed, "the read landed");
    assert_eq!(s.sheet(), None);
}

#[gpui::test]
fn the_sheet_goes_when_the_document_hides_it(cx: &mut TestAppContext) {
    let f = busy();
    f.set_midi_file_info("/tmp/Song.mid", info());
    let mut s = setup(cx, f);
    s.import("/tmp/Song.mid");
    assert_eq!(s.sheet().as_deref(), Some("Import MIDI File"));
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store.with_document(doc, |d, _| d.hide_midi_import());
        cx.notify();
    });
    s.cx.run_until_parked();
    assert_eq!(s.sheet(), None);
}
