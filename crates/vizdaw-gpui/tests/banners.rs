//! The project banners above a document window's editors (Swift
//! `ProjectBanners`): a running job's progress and Cancel, media in
//! another format and Convert, a device at another rate, disk underruns
//! and a finished job's problems, each with its button, in the registry.

use std::sync::Arc;
use std::time::Duration;

use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use serde_json::Value;
use vizdaw_app::testing::{FakeSession, JobScript, demo_fake};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::{BitDepthRow, ProjectFormatRow, ProjectInfoRow};
use vizdaw_gpui::{AppState, DocumentWindow};

struct Setup {
    fake: Arc<FakeSession>,
    app: Entity<AppState>,
    window: WindowHandle<DocumentWindow>,
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
    let (app, window) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let window = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, window)
    });
    let vcx = VisualTestContext::from_window(window.into(), cx);
    vcx.run_until_parked();
    Setup {
        fake,
        app,
        window,
        cx: vcx,
    }
}

impl Setup {
    fn doc(&self) -> DocId {
        self.window.read_with(&self.cx, |v, _| v.doc()).unwrap()
    }

    /// The document's idle runs (jobs polled), then the window draws.
    fn tick(&mut self) {
        self.cx.executor().advance_clock(Duration::from_millis(60));
        self.cx.run_until_parked();
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
}

fn y(n: &Value) -> f64 {
    n["frame"]["y"].as_f64().unwrap()
}

fn bottom(n: &Value) -> f64 {
    y(n) + n["frame"]["height"].as_f64().unwrap()
}

#[gpui::test]
fn a_running_job_shows_its_progress_and_cancel_above_the_editors(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.script_job(JobScript::done().steps(&[0.25; 40]));
    let mut s = setup(cx, fake);
    assert!(s.node("button", "Cancel").is_none(), "no job, no bar");
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store
            .with_document(doc, |d, _| d.import_audio(vec!["/tmp/tone.wav".into()]));
        cx.notify();
    });
    s.tick();
    let progress = s.expect("progress", "Importing audio\u{2026}");
    assert_eq!(progress["value"], "0.25", "{progress}");
    assert_eq!(progress["frame"]["width"], 120.0, "{progress}");
    s.expect("text", "Importing audio\u{2026}");
    let cancel = s.expect("button", "Cancel");
    // Below the transport bar, above the editors' tool bar.
    let position = s.expect("textfield", "Position");
    let snap = s
        .nodes()
        .into_iter()
        .find(|n| n["label"] == "Snap")
        .expect("the tool bar's Snap");
    assert!(y(&progress) >= bottom(&position), "{progress} {position}");
    assert!(bottom(&cancel) <= y(&snap) + 0.5, "{cancel} {snap}");

    s.click("button", "Cancel");
    let started = s.fake.jobs_started();
    assert_eq!(s.fake.cancelled_jobs(), vec![started[0].id]);
}

#[gpui::test]
fn each_bar_shows_its_text_and_button_top_first(cx: &mut TestAppContext) {
    let fake = demo_fake();
    let format = ProjectFormatRow {
        sample_rate: 48_000,
        depth: BitDepthRow::Int24,
    };
    fake.set_project_info(ProjectInfoRow {
        format,
        mismatched: 2,
        rate_mismatch: true,
        device_rate: 44_100.0,
    });
    fake.set_transport(|t| {
        t.faults = vec!["stream-underrun".into()];
        t.rate_mismatch = true;
    });
    // The convert job runs a while.
    fake.script_job(JobScript::done().steps(&[0.5; 40]));
    let mut s = setup(cx, fake);
    s.tick();
    let rate = s.expect(
        "text",
        "The audio device runs at 44.1 kHz; this project is 48 kHz. Audio regions are muted.",
    );
    let rate_button = s.expect("button", "Change Project Rate to 44.1 kHz");
    let label = vizdaw_ffi::format_label(format);
    let mismatched = s.expect(
        "text",
        &format!("2 media files are not in the project format ({label}) and stay silent."),
    );
    s.expect("button", "Convert 2 Files");
    let disk = s.expect("text", "Disk too slow: some audio played as silence.");
    assert!(y(&rate) < y(&mismatched) && y(&mismatched) < y(&disk));
    assert!(y(&rate_button) < y(&mismatched));

    // Dismiss hides the disk warning.
    s.click("button", "Dismiss");
    assert!(
        s.node("text", "Disk too slow: some audio played as silence.")
            .is_none()
    );

    // Convert starts the convert job: its bar replaces the offer.
    s.click("button", "Convert 2 Files");
    assert!(s.fake.calls().iter().any(|c| c == "convert_media"));
    s.tick();
    s.expect("progress", "Converting media\u{2026}");
    assert!(s.node("button", "Convert 2 Files").is_none());

    // The rate button changes the project to the device's rate.
    s.click("button", "Change Project Rate to 44.1 kHz");
    assert!(
        s.fake
            .calls()
            .iter()
            .any(|c| c.starts_with("set_project_format(44100")),
        "{:?}",
        s.fake.calls()
    );
}

#[gpui::test]
fn a_finished_jobs_problems_show_until_dismissed(cx: &mut TestAppContext) {
    let fake = demo_fake();
    fake.script_job(JobScript::done().created(&[1]).problems(&[
        "a.wav: unreadable",
        "b.wav: unreadable",
        "c.wav: unreadable",
        "d.wav: unreadable",
    ]));
    let mut s = setup(cx, fake);
    let doc = s.doc();
    s.app.update(&mut s.cx, |a, cx| {
        a.store
            .with_document(doc, |d, _| d.import_audio(vec!["/tmp/a.wav".into()]));
        cx.notify();
    });
    s.tick();
    let text = "a.wav: unreadable\nb.wav: unreadable\nc.wav: unreadable\nd.wav: unreadable";
    let problems = s.expect("text", text);
    // At most three lines show.
    let one_line = s.expect("button", "Dismiss")["frame"]["height"]
        .as_f64()
        .unwrap();
    assert!(
        problems["frame"]["height"].as_f64().unwrap() < one_line * 3.0 - 4.0,
        "{problems}"
    );
    s.click("button", "Dismiss");
    assert!(s.node("text", text).is_none());
}
