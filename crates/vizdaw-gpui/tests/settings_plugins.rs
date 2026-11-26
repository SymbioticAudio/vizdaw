//! The Plug-ins tab (slice 9c Task 6; Swift `PluginSettingsView`): the
//! new-channel EQ switch and picker, the Install button and its Replace
//! question, on the scripted core.

use std::sync::Arc;
use std::time::Duration;

use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use vizdaw_app::testing::{FakeCore, FakeSession, demo_fake};
use vizdaw_app::{DocumentStore, Prefs};
use vizdaw_ffi::VizError;
use vizdaw_ffi::bundled::DefaultEqRow;
use vizdaw_ffi::snapshot::PluginFormatTag;
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::settings_window::{SettingsTab, SettingsWindow, open_settings};
use vizdaw_gpui::{AppState, DocumentWindow};

const SWITCH: &str = "Put an EQ on new channels";
const PICKER: &str = "EQ for new channels";
const DEST: &str = "/tmp/vizdaw-plugins-tab-test";

struct Setup {
    core: Arc<FakeCore>,
    _fake: Arc<FakeSession>,
    app: gpui::Entity<AppState>,
    _doc: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

fn gain() -> DefaultEqRow {
    DefaultEqRow {
        format: PluginFormatTag::Clap,
        id: "vizdaw.test.gain".into(),
        name: "Test Gain".into(),
    }
}

/// The app over a scripted core; `script` runs before the preferences
/// load (the bundled EQ is read then).
fn setup(cx: &mut TestAppContext, script: impl FnOnce(&FakeCore)) -> Setup {
    let core = Arc::new(FakeCore::new());
    script(&core);
    let fake = Arc::new(demo_fake());
    core.hand_out("new_demo", fake.clone());
    let store = DocumentStore::with_core(core.clone());
    let (app, doc) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        app.update(cx, |s, _| {
            s.set_prefs(Prefs::in_memory());
            s.plugins.set_install_destination(Some(DEST.into()));
        });
        let doc = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, doc)
    });
    let vcx = VisualTestContext::from_window(doc.into(), cx);
    vcx.run_until_parked();
    let mut s = Setup {
        core,
        _fake: fake,
        app,
        _doc: doc,
        cx: vcx,
    };
    s.cx.update(|_, cx| open_settings(Some(SettingsTab::Plugins), cx));
    s.settle();
    s
}

impl Setup {
    /// Lets the tab's off-thread reads land (a thread answers into a slot
    /// the window looks in every 20 ms of the app's clock).
    fn settle(&mut self) {
        for _ in 0..40 {
            self.cx.run_until_parked();
            std::thread::sleep(Duration::from_millis(5));
            self.cx.executor().advance_clock(Duration::from_millis(20));
        }
        self.cx.run_until_parked();
    }

    fn handle(&mut self) -> WindowHandle<SettingsWindow> {
        self.app
            .read_with(&self.cx, |a, _| a.settings_window)
            .expect("the settings window")
    }

    fn elements(&mut self) -> Vec<Element> {
        let h = self.handle();
        let mut vcx = VisualTestContext::from_window(h.into(), &self.cx);
        vcx.run_until_parked();
        vcx.update(|w, _| w.refresh());
        vcx.run_until_parked();
        h.read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    }

    fn find(&mut self, role: &str, label: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == role && e.label == label)
    }

    fn click(&mut self, role: &str, label: &str) {
        let e = self
            .find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label}"));
        let h = self.handle();
        let mut vcx = VisualTestContext::from_window(h.into(), &self.cx);
        vcx.simulate_click(e.bounds.center(), Modifiers::none());
        vcx.run_until_parked();
    }

    fn enabled(&mut self, role: &str, label: &str) -> bool {
        self.find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label}"))
            .enabled
    }

    fn has_text(&mut self, t: &str) -> bool {
        self.find("text", t).is_some()
    }

    fn calls(&self, what: &str) -> Vec<String> {
        self.core
            .calls()
            .into_iter()
            .filter(|c| c.starts_with(what))
            .collect()
    }
}

#[gpui::test]
fn plugins_tab_with_no_bundled_eq_says_so_and_disables(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c| c.set_bundled(None, false));
    assert!(s.has_text("This build of VizDAW has no bundled EQ."));
    assert!(
        !s.enabled("checkbox", SWITCH),
        "no EQ at all: the switch is off-limits"
    );
    assert!(!s.enabled("button", "Install the bundled EQ for other apps"));
    // The caption names the default.
    assert!(s.has_text(
        "New channels, new buses and the master of new projects start with Multi-Q in their EQ slot. Projects you already have are not changed."
    ));
}

#[gpui::test]
fn the_switch_disables_the_picker(cx: &mut TestAppContext) {
    let mut s = setup(cx, |_| {});
    assert!(s.enabled("menu", PICKER));
    assert_eq!(
        s.find("menu", PICKER).unwrap().value.as_deref(),
        Some("Multi-Q (built in)")
    );
    assert_eq!(
        s.find("checkbox", SWITCH).unwrap().value.as_deref(),
        Some("on")
    );
    s.click("checkbox", SWITCH);
    assert_eq!(
        s.find("checkbox", SWITCH).unwrap().value.as_deref(),
        Some("off")
    );
    assert!(!s.enabled("menu", PICKER));
    assert_eq!(
        s.calls("set_new_channel_eq"),
        ["set_new_channel_eq(true)", "set_new_channel_eq(false)"]
    );
    // On again: the picker works.
    s.click("checkbox", SWITCH);
    assert!(s.enabled("menu", PICKER));
}

/// Prefs whose file cannot be written (its folder is a file).
fn unwritable(s: &mut Setup) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vizdaw-plugins-tab-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    s.app.update(&mut s.cx, |a, _| {
        let core = a.store.core();
        let mut p = vizdaw_app::plugin_prefs::PluginPrefs::load(
            &mut Prefs::at(blocker.join("prefs.json")),
            core.as_ref(),
        );
        p.set_install_destination(Some(DEST.into()));
        p.refresh(core.as_ref());
        a.plugins = p;
        a.prefs = Prefs::at(blocker.join("prefs.json"));
    });
    dir
}

fn problem(s: &mut Setup) -> Option<String> {
    s.elements()
        .into_iter()
        .find(|e| e.role == "text" && e.value.as_deref() == Some("problem"))
        .map(|e| e.label)
}

#[gpui::test]
fn a_failed_write_shows_the_problem_line(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c| c.set_eq_choices(vec![gain()]));
    let dir = unwritable(&mut s);
    assert!(problem(&mut s).is_none());
    s.click("checkbox", SWITCH);
    let p = problem(&mut s).expect("the problem after the switch");
    assert!(p.contains("could not be written"), "{p}");
    // The core took it all the same.
    assert_eq!(
        s.find("checkbox", SWITCH).unwrap().value.as_deref(),
        Some("off")
    );
    s.click("checkbox", SWITCH);
    s.click("menu", PICKER);
    s.click("menuitem", "Test Gain (CLAP)");
    assert!(
        problem(&mut s)
            .expect("the problem after the pick")
            .contains("could not be written")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[gpui::test]
fn the_picker_offers_the_installed_effects_and_sets_the_choice(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c| c.set_eq_choices(vec![gain()]));
    s.click("menu", PICKER);
    assert!(s.find("menuitem", "Multi-Q (built in)").is_some());
    s.click("menuitem", "Test Gain (CLAP)");
    assert_eq!(
        s.find("menu", PICKER).unwrap().value.as_deref(),
        Some("Test Gain (CLAP)")
    );
    assert_eq!(
        s.calls("set_default_eq"),
        [
            "set_default_eq(multi-q)",
            "set_default_eq(clap:vizdaw.test.gain)"
        ]
    );
    assert!(s.has_text(
        "New channels, new buses and the master of new projects start with Test Gain in their EQ slot. Projects you already have are not changed."
    ));
}

#[gpui::test]
fn a_missing_choice_reads_not_available(cx: &mut TestAppContext) {
    let mut s = setup(cx, |_| {});
    // The choice was made when the plug-in was installed; now it is not
    // in the catalogue.
    s.app.update(&mut s.cx, |a, _| {
        let core = a.store.core();
        a.plugins
            .set_default_eq(Some(gain()), &mut a.prefs, core.as_ref())
            .unwrap();
    });
    s.settle();
    assert_eq!(
        s.find("menu", PICKER).unwrap().value.as_deref(),
        Some("Test Gain (not available)")
    );
    s.click("menu", PICKER);
    assert!(s.find("menuitem", "Test Gain (not available)").is_some());
    // The core's own sentence shows under the picker.
    s.core.set_eq_note(Some("Test Gain is not installed."));
    s.click("menuitem", "Multi-Q (built in)");
    assert!(s.has_text("Test Gain is not installed."));
    assert_eq!(
        s.find("menu", PICKER).unwrap().value.as_deref(),
        Some("Multi-Q (built in)")
    );
}

#[gpui::test]
fn the_choices_are_read_off_the_ui_thread_when_the_tab_shows(cx: &mut TestAppContext) {
    let mut s = setup(cx, |c| c.set_eq_choices(vec![gain()]));
    // Before the read lands nothing claims the choice is missing; after,
    // the item is there.
    s.click("menu", PICKER);
    assert!(s.find("menuitem", "Test Gain (CLAP)").is_some());
    let threads = s.core.read_threads("eq_choices");
    assert_eq!(threads.len(), 1, "read once when the tab showed");
    assert_ne!(threads[0], std::thread::current().id(), "off the UI thread");
}

#[gpui::test]
fn install_with_something_there_asks_replace(cx: &mut TestAppContext) {
    let mut s = setup(cx, |_| {});
    s.core.fail_next(
        "install_multiq",
        VizError::Exists {
            path: format!("{DEST}/Multi-Q.component"),
        },
    );
    s.click("button", "Install Multi-Q for other apps");
    s.settle();
    let handle = s.handle();
    let vcx = VisualTestContext::from_window(handle.into(), &s.cx);
    assert!(vcx.has_pending_prompt(), "it asks");
    let (title, detail) = vcx.pending_prompt().expect("the question");
    assert_eq!(title, "Replace the Multi-Q in your Components folder?");
    assert_eq!(detail, "Something is already installed under that name.");
    // Cancel installs nothing more and says nothing.
    vcx.simulate_prompt_answer("Cancel");
    vcx.run_until_parked();
    assert_eq!(
        s.calls("install_multiq"),
        [format!("install_multiq(Some(\"{DEST}\"),false)")]
    );
    assert!(!s.has_text(&format!("Installed in {DEST}.")));

    // Ask again; Replace installs with replace.
    s.core.fail_next(
        "install_multiq",
        VizError::Exists {
            path: format!("{DEST}/Multi-Q.component"),
        },
    );
    s.click("button", "Install Multi-Q for other apps");
    s.settle();
    let vcx = VisualTestContext::from_window(s.handle().into(), &s.cx);
    assert!(vcx.has_pending_prompt());
    vcx.simulate_prompt_answer("Replace");
    vcx.run_until_parked();
    s.settle();
    assert_eq!(
        s.calls("install_multiq").last().unwrap(),
        &format!("install_multiq(Some(\"{DEST}\"),true)")
    );
    assert!(
        s.has_text(&format!("Installed in {DEST}.")),
        "{:#?}",
        s.elements()
    );
}

#[gpui::test]
fn install_runs_off_the_ui_thread_and_reports_its_status(cx: &mut TestAppContext) {
    let mut s = setup(cx, |_| {});
    s.click("button", "Install Multi-Q for other apps");
    s.settle();
    assert_eq!(s.calls("install_multiq").len(), 1);
    let threads = s.core.read_threads("install_multiq");
    assert_eq!(threads.len(), 1);
    assert_ne!(threads[0], std::thread::current().id(), "off the UI thread");
    assert!(s.has_text(&format!("Installed in {DEST}.")));
    // A failure is the status line.
    s.core.fail_next(
        "install_multiq",
        VizError::Io {
            message: "disk full".into(),
        },
    );
    s.click("button", "Install Multi-Q for other apps");
    s.settle();
    assert!(!s.has_text(&format!("Installed in {DEST}.")));
    assert!(
        s.elements()
            .iter()
            .any(|e| e.role == "text" && e.label.contains("disk full"))
    );
}
