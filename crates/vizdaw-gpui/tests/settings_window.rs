//! The Settings window (slice 9a Task 4): one per app, the nine tabs in
//! Swift's order, placeholders for the tabs not built yet, and none of a
//! document's keys.

use std::sync::Arc;

use core_control::bindings::Keymap;
use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use std::time::Duration;
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeCore, FakeSession, demo_fake};
use vizdaw_ffi::host::InputDeviceRow;
use vizdaw_ffi::{InputInfoRow, VizError};
use vizdaw_gpui::menus::{ItemSpec, menu_model};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::settings_window::{SettingsTab, SettingsWindow, open_settings};
use vizdaw_gpui::{AppState, DocumentWindow, OpenSettings};

struct Setup {
    core: Arc<FakeCore>,
    fake: Arc<FakeSession>,
    app: gpui::Entity<AppState>,
    doc: Option<WindowHandle<DocumentWindow>>,
    cx: VisualTestContext,
}

/// An app with one demo document (its window front), or none.
fn setup(cx: &mut TestAppContext, with_doc: bool) -> Setup {
    let core = Arc::new(FakeCore::new());
    let fake = Arc::new(demo_fake());
    core.hand_out("new_demo", fake.clone());
    let store = DocumentStore::with_core(core.clone());
    let (app, doc) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let doc = with_doc.then(|| vizdaw_gpui::new_demo(cx).expect("a demo window"));
        (app, doc)
    });
    let vcx = match doc {
        Some(d) => VisualTestContext::from_window(d.into(), cx),
        None => cx.add_empty_window().clone(),
    };
    vcx.run_until_parked();
    Setup {
        core,
        fake,
        app,
        doc,
        cx: vcx,
    }
}

impl Setup {
    fn open(&mut self, tab: Option<SettingsTab>) -> WindowHandle<SettingsWindow> {
        self.cx.update(|_, cx| open_settings(tab, cx));
        self.cx.run_until_parked();
        self.handle().expect("the Settings window is open")
    }

    fn handle(&mut self) -> Option<WindowHandle<SettingsWindow>> {
        self.app.read_with(&self.cx, |a, _| a.settings_window)
    }

    fn elements(&mut self) -> Vec<Element> {
        let h = self.handle().unwrap();
        let mut vcx = VisualTestContext::from_window(h.into(), &self.cx);
        vcx.run_until_parked();
        vcx.update(|w, _| w.refresh());
        vcx.run_until_parked();
        h.read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    }

    fn click(&mut self, role: &str, label: &str) {
        let el = self
            .elements()
            .into_iter()
            .find(|e| e.role == role && e.label == label)
            .unwrap_or_else(|| panic!("no {role} {label}"));
        let mut vcx = VisualTestContext::from_window(self.handle().unwrap().into(), &self.cx);
        vcx.simulate_click(el.bounds.center(), Modifiers::none());
        vcx.run_until_parked();
    }
}

fn settings_cx(s: &mut Setup) -> VisualTestContext {
    let h = s.handle().unwrap();
    VisualTestContext::from_window(h.into(), &s.cx)
}

#[gpui::test]
fn settings_opens_once_and_comes_to_the_front(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let first = s.open(Some(SettingsTab::Keys));
    let again = s.open(None);
    assert_eq!(first.window_id(), again.window_id());
    // It stays on the tab it was on.
    let tab = first.read_with(&s.cx, |v, _| v.tab()).unwrap();
    assert_eq!(tab, SettingsTab::Keys);
    // A named tab selects it.
    s.open(Some(SettingsTab::Plugins));
    let tab = first.read_with(&s.cx, |v, _| v.tab()).unwrap();
    assert_eq!(tab, SettingsTab::Plugins);
    // Only one Settings window; `windows` does not list it.
    assert!(s.app.read_with(&s.cx, |a, _| a.windows().len()) == 1);
}

#[gpui::test]
fn settings_opens_with_no_document(cx: &mut TestAppContext) {
    let mut s = setup(cx, false);
    s.open(None);
    s.cx.dispatch_action(OpenSettings);
    s.cx.run_until_parked();
    assert!(s.handle().is_some());
}

#[gpui::test]
fn the_tabs_are_swifts_nine_in_order(cx: &mut TestAppContext) {
    assert_eq!(
        SettingsTab::ALL.map(SettingsTab::label),
        [
            "Appearance",
            "Editing",
            "Keys",
            "Audio",
            "Recording",
            "Automation",
            "Surfaces",
            "Plug-ins",
            "Co-Producer"
        ]
    );
    let mut s = setup(cx, true);
    s.open(None);
    let tabs: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "tab")
        .map(|e| e.label)
        .collect();
    assert_eq!(tabs, SettingsTab::ALL.map(|t| t.label()));
    // Clicking a tab selects it.
    s.click("tab", "Audio");
    let h = s.handle().unwrap();
    assert_eq!(
        h.read_with(&s.cx, |v, _| v.tab()).unwrap(),
        SettingsTab::Audio
    );
}

#[gpui::test]
fn an_unbuilt_tab_says_it_comes_later(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Keys));
    let line = s
        .elements()
        .into_iter()
        .find(|e| e.role == "text" && e.label.ends_with("later version."))
        .expect("the placeholder");
    assert_eq!(line.label, "Keys comes in a later version.");
    s.click("tab", "Plug-ins");
    let all = s.elements();
    assert!(
        all.iter()
            .any(|e| e.role == "text" && e.label == "Plug-ins comes in a later version."),
        "{all:#?}"
    );
}

#[gpui::test]
fn space_in_settings_does_not_play(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(None);
    let mut vcx = settings_cx(&mut s);
    vcx.simulate_keystrokes("space s");
    vcx.run_until_parked();
    assert_eq!(s.fake.count("play(3)"), 0);
    assert_eq!(s.fake.count("stop()"), 0);
}

#[gpui::test]
fn closing_settings_leaves_the_documents(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(None);
    let mut vcx = settings_cx(&mut s);
    vcx.simulate_keystrokes("secondary-w");
    vcx.run_until_parked();
    assert!(s.handle().is_none(), "the handle is cleared");
    assert_eq!(s.app.read_with(&s.cx, |a, _| a.windows().len()), 1);
    assert!(s.doc.unwrap().read_with(&s.cx, |v, _| v.doc()).is_ok());
    // It opens again.
    s.open(None);
}

#[gpui::test]
fn secondary_comma_opens_settings(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.cx.simulate_keystrokes("secondary-,");
    s.cx.run_until_parked();
    assert!(s.handle().is_some());
}

#[test]
fn the_vizdaw_menu_lists_settings_with_its_key() {
    let keymap = Keymap::default();
    {
        let m = menu_model(None, &keymap);
        let items = &m.menus[0].items;
        assert_eq!(m.menus[0].title, "VizDAW");
        assert!(matches!(
            &items[0],
            ItemSpec::Command { title, keys: Some(k), enabled: true, .. }
                if title == "Settings…" && k.contains(',')
        ));
        assert!(matches!(items[1], ItemSpec::Separator));
        assert!(matches!(
            &items[2],
            ItemSpec::Command { id: "app.quit", .. }
        ));
    }
}

// ---- The Audio tab (slice 9a Task 5) ----

fn info(device: &str) -> InputInfoRow {
    InputInfoRow {
        device: device.into(),
        channels: 2,
        names: vec![],
        input_latency_frames: 38,
        output_latency_frames: 34,
        buffer_frames: 128,
        round_trip_frames: 328,
        opening: false,
        sample_rate: 48_000.0,
        midi_fx_dropped: 0,
        per_note_dropped: 0,
        mpe_stolen: 0,
    }
}

impl Setup {
    fn find(&mut self, role: &str, label: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == role && e.label == label)
    }

    fn value(&mut self, role: &str, label: &str) -> Option<String> {
        self.find(role, label).and_then(|e| e.value)
    }

    fn has_text(&mut self, text: &str) -> bool {
        self.find("text", text).is_some()
    }

    /// Runs the app until `done` holds: background reads run on real
    /// threads, so the loop gives them a moment.
    fn wait_for(&mut self, what: &str, mut done: impl FnMut(&mut Setup) -> bool) {
        for _ in 0..300 {
            self.cx.run_until_parked();
            if done(self) {
                return;
            }
            // The read's thread answers into a slot the window looks in
            // every 20 ms of the app's clock.
            std::thread::sleep(Duration::from_millis(5));
            self.cx.executor().advance_clock(Duration::from_millis(20));
        }
        panic!("never: {what}\n{:#?}", self.elements());
    }

    fn choose(&mut self, menu: &str, item: &str) {
        self.click("menu", menu);
        self.click("menuitem", item);
    }
}

#[gpui::test]
fn the_audio_tab_shows_swifts_rows_and_captions(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Audio));
    for section in ["Input", "Latency", "Bounce", "Streaming"] {
        assert!(s.has_text(section), "section {section}");
    }
    for caption in [
        "A new device takes effect at once; while recording or measuring, once that stops.",
        "Off: VizDAW never plays an input back; listen through your interface's direct monitoring. Recording and MIDI thru are unaffected.",
        "About 5.3 ms at 48 kHz, plus the device's own latency once it opens",
        "A bounce renders on after the range until it is quiet, at most this long.",
        "Cache and read-ahead apply from the next playback.",
        "Latency trim",
        "0 frames (0.00 ms)",
        "Bounce tail limit: 10 s",
        "Disk read-ahead: 3 s",
    ] {
        assert!(s.has_text(caption), "caption {caption}");
    }
    assert_eq!(s.value("menu", "Input device").unwrap(), "Same as output");
    assert_eq!(s.value("menu", "I/O Buffer Size").unwrap(), "128 frames");
    assert_eq!(s.value("menu", "RAM cache").unwrap(), "512 MB");
    assert_eq!(s.value("checkbox", "Software Monitoring").unwrap(), "on");
    // The buffer picker lists Swift's six sizes.
    s.click("menu", "I/O Buffer Size");
    let sizes: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| e.label)
        .collect();
    assert_eq!(
        sizes,
        [
            "32 frames",
            "64 frames",
            "128 frames",
            "256 frames",
            "512 frames",
            "1024 frames"
        ]
    );
}

#[gpui::test]
fn choosing_a_buffer_size_goes_to_the_core_then_the_prefs(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Audio));
    s.choose("I/O Buffer Size", "256 frames");
    assert!(
        s.core
            .calls()
            .contains(&"set_io_buffer_frames(256)".to_owned())
    );
    let saved = s
        .app
        .read_with(&s.cx, |a, _| a.audio.settings().buffer_frames);
    assert_eq!(saved, 256);
    assert_eq!(s.value("menu", "I/O Buffer Size").unwrap(), "256 frames");
    // The round-trip caption follows the buffer.
    assert!(s.has_text("About 10.7 ms at 48 kHz, plus the device's own latency once it opens"));
}

#[gpui::test]
fn a_refused_buffer_size_shows_the_problem_and_keeps_the_choice(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Audio));
    s.core.refuse_next(
        "set_io_buffer_frames",
        VizError::Busy {
            message: "Stop recording to change the I/O buffer size.".into(),
        },
    );
    s.choose("I/O Buffer Size", "256 frames");
    assert_eq!(s.value("menu", "I/O Buffer Size").unwrap(), "128 frames");
    let problem = s
        .find("text", "Stop recording to change the I/O buffer size.")
        .expect("the red problem line");
    assert_eq!(problem.value.as_deref(), Some("problem"));
    let saved = s
        .app
        .read_with(&s.cx, |a, _| a.audio.settings().buffer_frames);
    assert_eq!(saved, 128);
    // A later success clears it.
    s.choose("I/O Buffer Size", "64 frames");
    assert!(
        s.find("text", "Stop recording to change the I/O buffer size.")
            .is_none()
    );
}

#[gpui::test]
fn an_absent_device_shows_not_connected(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.core.set_input_devices(vec![InputDeviceRow {
        name: "Scarlett".into(),
        uid: "uid-1".into(),
    }]);
    let core = s.core.clone();
    s.app.update(&mut s.cx, |a, _| {
        let row = InputDeviceRow {
            name: "Gone".into(),
            uid: "uid-gone".into(),
        };
        a.audio
            .set_input_device(Some(row), &mut a.prefs, core.as_ref())
            .unwrap();
    });
    s.open(Some(SettingsTab::Audio));
    // Not connected, with or without the list read yet.
    assert_eq!(
        s.value("menu", "Input device").unwrap(),
        "Gone (not connected)"
    );
    // The picker lists it after the connected one; opening it reads again.
    s.click("menu", "Input device");
    s.wait_for("the device list", |s| {
        s.find("menuitem", "Scarlett").is_some()
    });
    let items: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| e.label)
        .collect();
    assert_eq!(
        items,
        ["Same as output", "Scarlett", "Gone (not connected)"]
    );
}

#[gpui::test]
fn reset_trim_is_off_at_zero(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Audio));
    assert!(!s.find("button", "Reset Trim").unwrap().enabled);
    assert!(s.find("button", "Decrease Latency trim").unwrap().enabled);
    s.click("button", "Increase Latency trim");
    assert!(s.has_text("+1 frames (0.02 ms)"));
    assert!(s.find("button", "Reset Trim").unwrap().enabled);
    s.click("button", "Reset Trim");
    assert!(s.has_text("0 frames (0.00 ms)"));
    assert!(!s.find("button", "Reset Trim").unwrap().enabled);
    assert_eq!(
        s.core
            .calls()
            .iter()
            .filter(|c| c.starts_with("set_record_latency_trim"))
            .cloned()
            .collect::<Vec<_>>(),
        ["set_record_latency_trim(1)", "set_record_latency_trim(0)"]
    );
}

#[gpui::test]
fn a_stepper_stops_at_its_ends(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Audio));
    // Read-ahead: 2 to 8, at 3.
    s.click("button", "Decrease Disk read-ahead");
    assert!(s.has_text("Disk read-ahead: 2 s"));
    assert!(
        !s.find("button", "Decrease Disk read-ahead")
            .unwrap()
            .enabled
    );
    assert!(
        s.find("button", "Increase Disk read-ahead")
            .unwrap()
            .enabled
    );
    // Bounce tail limit: 0 to 30, at 10.
    for _ in 0..10 {
        s.click("button", "Decrease Bounce tail limit");
    }
    assert!(s.has_text("Bounce tail limit: 0 s"));
    assert!(
        !s.find("button", "Decrease Bounce tail limit")
            .unwrap()
            .enabled
    );
}

#[gpui::test]
fn software_monitoring_refreshes_every_documents_input(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.core.set_software_monitoring(true);
    s.open(Some(SettingsTab::Audio));
    let id = s.app.read_with(&s.cx, |a, _| a.store.front().unwrap());
    let on = |s: &Setup| {
        s.app.read_with(&s.cx, |a, _| {
            a.store.document(id).unwrap().inputs().monitoring_on()
        })
    };
    // The clock has not moved, so no poll has run since.
    s.core.set_software_monitoring(false);
    s.click("checkbox", "Software Monitoring");
    assert!(
        s.core
            .calls()
            .contains(&"set_software_monitoring(false)".to_owned())
    );
    assert_eq!(s.value("checkbox", "Software Monitoring").unwrap(), "off");
    assert!(!on(&s), "the document's meters follow at once");
}

#[gpui::test]
fn the_round_trip_is_read_off_the_ui_thread_only_while_shown(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.fake.set_input(info("Scarlett"));
    let hold = s.fake.hold_input_info();
    s.open(Some(SettingsTab::Audio));
    s.wait_for("the read starts", |_| hold.started());
    // The read is blocked, and rendering never waits for it.
    assert!(s.has_text("About 5.3 ms at 48 kHz, plus the device's own latency once it opens"));
    hold.release();
    s.wait_for("the round trip", |s| {
        s.has_text("Round trip 6.8 ms (input 38 + output 34 + 2 × 128 frames)")
    });
    // It is read again every second while the tab shows.
    let before = s.fake.reads("input_info");
    s.cx.executor().advance_clock(Duration::from_millis(1100));
    s.wait_for("another read", |s| s.fake.reads("input_info") > before);
    // Another tab stops the reads.
    s.click("tab", "Keys");
    s.cx.run_until_parked();
    let after = s.fake.reads("input_info");
    s.cx.executor().advance_clock(Duration::from_secs(3));
    s.cx.run_until_parked();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(s.fake.reads("input_info"), after);
    // Back on Audio they resume; closing the window stops them again.
    s.click("tab", "Audio");
    s.wait_for("reads resume", |s| s.fake.reads("input_info") > after);
    let mut vcx = settings_cx(&mut s);
    vcx.simulate_keystrokes("secondary-w");
    vcx.run_until_parked();
    let closed = s.fake.reads("input_info");
    s.cx.executor().advance_clock(Duration::from_secs(3));
    s.cx.run_until_parked();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(s.fake.reads("input_info"), closed);
}

#[gpui::test]
fn a_stale_window_handle_opens_a_new_window(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let first = s.open(Some(SettingsTab::Keys));
    let mut vcx = settings_cx(&mut s);
    vcx.simulate_keystrokes("secondary-w");
    vcx.run_until_parked();
    assert!(s.handle().is_none());
    // The state still names the closed window.
    s.app
        .update(&mut s.cx, |a, _| a.settings_window = Some(first));
    let again = s.open(Some(SettingsTab::Audio));
    assert_ne!(first.window_id(), again.window_id());
    assert_eq!(
        again.read_with(&s.cx, |v, _| v.tab()).unwrap(),
        SettingsTab::Audio
    );
}
