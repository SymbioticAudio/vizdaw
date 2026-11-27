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
    // Surfaces has come (slice 9d): no placeholder.
    s.open(Some(SettingsTab::Surfaces));
    assert!(
        !s.elements()
            .iter()
            .any(|e| e.role == "text" && e.label.ends_with("later version."))
    );
    s.click("tab", "Co-Producer");
    let all = s.elements();
    assert!(
        all.iter()
            .any(|e| e.role == "text" && e.label == "Co-Producer comes in a later version."),
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

// ---------------------------------------------------------------------
// Task 6: the Recording and Automation tabs.

impl Setup {
    fn enabled(&mut self, role: &str, label: &str) -> bool {
        self.find(role, label).unwrap().enabled
    }

    /// The front document window's element `role` `label`'s value.
    fn doc_value(&mut self, role: &str, label: &str) -> Option<String> {
        let h = self.doc.unwrap();
        h.read_with(&self.cx, |v, _| {
            v.registry()
                .borrow()
                .elements()
                .iter()
                .find(|e| e.role == role && e.label == label)
                .and_then(|e| e.value.clone())
        })
        .unwrap()
    }

    fn items(&mut self) -> Vec<String> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "menuitem")
            .map(|e| e.label)
            .collect()
    }
}

#[gpui::test]
fn the_recording_tab_shows_swifts_rows(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Recording));
    for t in [
        "New Projects",
        "New Documents",
        "Disk",
        "Count-in: off",
        "Pre-roll: off",
        "Loop recording: each MIDI pass becomes a new take. Off: passes layer into the playing take. Audio passes always become takes.",
        "Open documents keep their own options: change those in the transport bar's record popover.",
        "Recording warns below 1 GB of free disk space and stops below 100 MB.",
    ] {
        assert!(s.has_text(t), "text {t}");
    }
    assert_eq!(s.value("menu", "Sample rate").unwrap(), "48 kHz");
    assert_eq!(s.value("menu", "Bit depth").unwrap(), "24-bit");
    assert_eq!(
        s.value("menu", "Monitoring for new tracks").unwrap(),
        "Auto"
    );
    assert_eq!(s.value("checkbox", "MIDI passes as takes").unwrap(), "off");
    s.click("menu", "Sample rate");
    let rates: Vec<String> = vizdaw_ffi::project_rates()
        .into_iter()
        .map(vizdaw_app::formats::rate_title)
        .collect();
    assert_eq!(s.items(), rates);
    s.click("menuitem", "48 kHz");
    s.click("menu", "Bit depth");
    assert_eq!(s.items(), ["16-bit", "24-bit", "32-bit float"]);
    s.click("menuitem", "24-bit");
    s.click("menu", "Monitoring for new tracks");
    assert_eq!(s.items(), ["Auto", "Input", "Off"]);
    s.click("menuitem", "Off");
    assert_eq!(s.value("menu", "Monitoring for new tracks").unwrap(), "Off");
    assert!(
        s.core
            .calls()
            .iter()
            .any(|c| c.starts_with("set_default_monitor"))
    );
    // The steppers stop at Swift's ends: count-in 0-4, pre-roll 0-8.
    assert!(!s.enabled("button", "Decrease Count-in"));
    assert!(!s.enabled("button", "Decrease Pre-roll"));
    for _ in 0..4 {
        s.click("button", "Increase Count-in");
    }
    assert!(s.has_text("Count-in: 4 bars"));
    assert!(!s.enabled("button", "Increase Count-in"));
    for _ in 0..8 {
        s.click("button", "Increase Pre-roll");
    }
    assert!(s.has_text("Pre-roll: 8 bars"));
    assert!(!s.enabled("button", "Increase Pre-roll"));
}

#[gpui::test]
fn changing_the_format_sets_the_next_new_project(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Recording));
    s.choose("Sample rate", "44.1 kHz");
    s.choose("Bit depth", "16-bit");
    assert_eq!(s.value("menu", "Sample rate").unwrap(), "44.1 kHz");
    let (store, saved) = s.app.read_with(&s.cx, |a, _| {
        (a.store.default_format(), a.prefs.default_format())
    });
    assert_eq!(store, saved);
    assert_eq!(saved.sample_rate, 44_100);
    assert_eq!(depth_name(saved.depth), "16-bit");
    // File › New starts at it.
    s.cx.dispatch_action(vizdaw_gpui::NewDocument);
    s.cx.run_until_parked();
    assert_eq!(s.doc_value("menu", "Sample rate").unwrap(), "44.1 kHz");
    assert_eq!(s.doc_value("menu", "Bit depth").unwrap(), "16-bit");
}

fn depth_name(d: vizdaw_ffi::BitDepthRow) -> &'static str {
    vizdaw_app::formats::depth_title(d)
}

#[gpui::test]
fn count_in_applies_to_the_next_new_document_not_open_ones(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.core.hand_out("new_demo", Arc::new(demo_fake()));
    let open_id = s.app.read_with(&s.cx, |a, _| a.store.front().unwrap());
    s.open(Some(SettingsTab::Recording));
    s.click("button", "Increase Count-in");
    s.click("button", "Increase Count-in");
    s.click("button", "Increase Pre-roll");
    s.click("checkbox", "MIDI passes as takes");
    assert!(s.has_text("Count-in: 2 bars"));
    assert!(s.has_text("Pre-roll: 1 bar"));
    assert_eq!(s.value("checkbox", "MIDI passes as takes").unwrap(), "on");
    let saved = s
        .app
        .read_with(&s.cx, |a, _| a.audio.settings().count_in_bars);
    assert_eq!(saved, 2);
    // The open document keeps its own; the next one starts from these.
    s.cx.update(|_, cx| vizdaw_gpui::new_demo(cx));
    s.cx.run_until_parked();
    let (open, next) = s.app.read_with(&s.cx, |a, _| {
        let new_id = a.store.front().unwrap();
        assert_ne!(new_id, open_id);
        let r = |id| {
            let d = a.store.document(id).unwrap();
            (
                d.record().count_in_bars(),
                d.record().pre_roll_bars(),
                d.record().midi_passes_as_takes(),
            )
        };
        (r(open_id), r(new_id))
    });
    assert_eq!(open, (0, 0, false));
    assert_eq!(next, (2, 1, true));
}

#[gpui::test]
fn the_automation_tab_shows_swifts_rows(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Automation));
    for t in [
        "Return time: 250 ms",
        "How closely written automation follows the move: 0.1 dB on faders and sends, 0.25 % elsewhere at Medium.",
        "Always on: writing every parameter of every plugin would bury the tracks.",
        "Changes apply from the next Play.",
    ] {
        assert!(s.has_text(t), "text {t}");
    }
    assert_eq!(s.value("menu", "After Write").unwrap(), "Latch");
    assert_eq!(s.value("menu", "Thinning").unwrap(), "Medium");
    let always = s
        .find("checkbox", "Write plugin parameters only when touched")
        .expect("the always-on toggle");
    assert_eq!(always.value.as_deref(), Some("on"));
    assert!(!always.enabled);
    // Return time: 0-2000 in steps of 50.
    s.click("button", "Increase Return time");
    assert!(s.has_text("Return time: 300 ms"));
    s.click("button", "Decrease Return time");
    s.click("button", "Decrease Return time");
    assert!(s.has_text("Return time: 200 ms"));
    let saved = s
        .app
        .read_with(&s.cx, |a, _| a.automation.settings().return_ms);
    assert_eq!(saved, 200);
    s.choose("Thinning", "High");
    assert_eq!(s.value("menu", "Thinning").unwrap(), "High");
    s.click("menu", "Thinning");
    assert_eq!(s.items(), ["Low", "Medium", "High"]);
}

#[gpui::test]
fn after_write_never_offers_write(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Automation));
    s.click("menu", "After Write");
    assert_eq!(s.items(), ["Off", "Read", "Touch", "Latch", "Trim"]);
    s.click("menuitem", "Trim");
    assert_eq!(s.value("menu", "After Write").unwrap(), "Trim");
}

#[gpui::test]
fn a_refused_automation_change_shows_the_problem(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Automation));
    s.core.refuse_next(
        "set_automation_settings",
        VizError::Busy {
            message: "Stop recording to change automation.".into(),
        },
    );
    s.choose("After Write", "Touch");
    assert_eq!(s.value("menu", "After Write").unwrap(), "Latch");
    let p = s
        .find("text", "Stop recording to change automation.")
        .expect("the red problem line");
    assert_eq!(p.value.as_deref(), Some("problem"));
    let saved = s
        .app
        .read_with(&s.cx, |a, _| a.automation.settings().after_write.clone());
    assert_eq!(saved, "latch");
    s.choose("After Write", "Touch");
    assert!(
        s.find("text", "Stop recording to change automation.")
            .is_none()
    );
}

#[gpui::test]
fn reselecting_audio_while_a_read_hangs_starts_no_second_read(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.fake.set_input(info("Scarlett"));
    let hold = s.fake.hold_input_info();
    s.open(Some(SettingsTab::Audio));
    s.wait_for("the read starts", |_| hold.started());
    let one = s.fake.reads("input_info");
    s.click("tab", "Keys");
    s.click("tab", "Audio");
    s.cx.executor().advance_clock(Duration::from_millis(2500));
    s.cx.run_until_parked();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(s.fake.reads("input_info"), one, "one read is out");
    hold.release();
    s.wait_for("the round trip", |s| {
        s.has_text("Round trip 6.8 ms (input 38 + output 34 + 2 × 128 frames)")
    });
}

// ---- Appearance (slice 9b Task 5) ----

const THEME_NAMES: [&str; 6] = [
    "Classic Light",
    "Studio Dark",
    "Flat",
    "Console",
    "Symbiotic",
    "EG13",
];

fn caption_text() -> &'static str {
    if cfg!(target_os = "macos") {
        "The theme applies to every window. It is a preference of this Mac, not saved in projects."
    } else {
        "The theme applies to every window. It is a preference of this computer, not saved in projects."
    }
}

#[gpui::test]
fn the_appearance_tab_lists_the_six_with_swatches(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.open(Some(SettingsTab::Appearance));
    let radios: Vec<_> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "radio")
        .collect();
    let names: Vec<&str> = radios.iter().map(|e| e.label.as_str()).collect();
    assert_eq!(names, THEME_NAMES);
    assert_eq!(
        radios.iter().filter(|e| e.selected).count(),
        1,
        "{radios:#?}"
    );
    assert!(radios[0].selected, "Classic Light is the default");
    assert!(s.find("radiogroup", "Theme").is_some());
    assert!(s.has_text(caption_text()));
    assert!(s.find("checkbox", "Show channel colour bars").is_some());
    assert_eq!(
        s.value("checkbox", "Show channel colour bars").as_deref(),
        Some("on")
    );
    assert!(!s.has_text("Your Themes"));
    // Eight swatches a theme: window, panel, accent, then the five strip
    // kinds.
    let t = vizdaw_app::theme::theme("studio-dark").unwrap();
    let sw = vizdaw_gpui::settings_window::theme_swatches(t);
    assert_eq!(
        sw,
        [
            t.surface.window.rgb,
            t.surface.panel.rgb,
            t.accent.accent.rgb,
            t.strip.channel.rgb,
            t.strip.instrument.rgb,
            t.strip.aux_return.rgb,
            t.strip.bus.rgb,
            t.strip.master.rgb,
        ]
    );
}

#[gpui::test]
fn choosing_a_theme_switches_every_window_and_saves(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.app.update(&mut s.cx, |a, _| {
        a.set_prefs(vizdaw_app::Prefs::in_memory());
    });
    s.open(Some(SettingsTab::Appearance));
    s.click("radio", "Flat");
    assert_eq!(vizdaw_gpui::appearance::current().id, "flat");
    let (saved, shown) = s.app.read_with(&s.cx, |a, _| {
        (
            a.prefs.raw("appearance").cloned(),
            a.appearance.theme.clone(),
        )
    });
    assert_eq!(shown, "flat");
    assert_eq!(saved.unwrap()["theme"], "flat");
    let sel = s
        .elements()
        .into_iter()
        .find(|e| e.role == "radio" && e.selected)
        .unwrap();
    assert_eq!(sel.label, "Flat");
    // The command and the tab share one path.
    s.app.update(&mut s.cx, |a, cx| {
        let args = [(
            "theme".to_owned(),
            core_control::registry::ArgValue::Text("eg13".into()),
        )];
        a.run_ui_theme(&args, cx).unwrap();
    });
    s.cx.run_until_parked();
    let sel = s
        .elements()
        .into_iter()
        .find(|e| e.role == "radio" && e.selected)
        .unwrap();
    assert_eq!(sel.label, "EG13");
}

#[gpui::test]
fn the_colour_bars_toggle_saves_and_hides_the_bars(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.app.update(&mut s.cx, |a, _| {
        a.set_prefs(vizdaw_app::Prefs::in_memory());
    });
    s.open(Some(SettingsTab::Appearance));
    assert!(vizdaw_gpui::appearance::colour_bars());
    s.click("checkbox", "Show channel colour bars");
    assert!(!vizdaw_gpui::appearance::colour_bars());
    assert_eq!(
        s.value("checkbox", "Show channel colour bars").as_deref(),
        Some("off")
    );
    let saved = s
        .app
        .read_with(&s.cx, |a, _| a.prefs.raw("appearance").cloned())
        .unwrap();
    assert_eq!(saved["colourBars"], false);
    s.click("checkbox", "Show channel colour bars");
    assert!(vizdaw_gpui::appearance::colour_bars());
}

#[gpui::test]
fn a_failed_theme_write_still_switches_and_shows_the_problem(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    let dir = std::env::temp_dir().join(format!("vizdaw-appearance-tab-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    s.app.update(&mut s.cx, |a, _| {
        a.set_prefs(vizdaw_app::Prefs::at(blocker.join("prefs.json")));
    });
    s.open(Some(SettingsTab::Appearance));
    s.click("radio", "Console");
    assert_eq!(vizdaw_gpui::appearance::current().id, "console");
    let p = s
        .elements()
        .into_iter()
        .find(|e| e.role == "text" && e.value.as_deref() == Some("problem"))
        .expect("the red problem line");
    assert!(p.label.contains("could not be written"), "{}", p.label);
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn square_corners_in_flat(cx: &mut TestAppContext) {
    let mut s = setup(cx, true);
    s.app.update(&mut s.cx, |a, _| {
        a.set_prefs(vizdaw_app::Prefs::in_memory());
    });
    s.open(Some(SettingsTab::Appearance));
    assert_eq!(vizdaw_gpui::appearance::corner(5.), 5.);
    assert!(vizdaw_gpui::strip_palette::tokens().radius > 0.);
    for (name, id) in [("Flat", "flat"), ("Symbiotic", "symbiotic")] {
        s.click("radio", name);
        assert_eq!(vizdaw_gpui::appearance::current().id, id);
        assert_eq!(vizdaw_gpui::appearance::corner(5.), 0.);
        assert_eq!(vizdaw_gpui::strip_palette::tokens().radius, 0.);
    }
    s.click("radio", "Console");
    assert_eq!(vizdaw_gpui::appearance::corner(5.), 5.);
}
