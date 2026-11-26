//! The Keys tab and its key recorder (slice 9c Task 3): every command by
//! menu in registry order, a changed key in the accent, record / Replace /
//! Reset / Reset All, Save… and Load…, and the number-row toggle.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use core_control::bindings::{Chord, Keymap};
use core_control::registry::{COMMANDS, KeyContext};
use futures::channel::oneshot;
use gpui::{
    AnyWindowHandle, App, Keystroke, Modifiers, TestAppContext, VisualTestContext, WindowHandle,
};
use vizdaw_app::DocumentStore;
use vizdaw_app::panels::PanelSpec;
use vizdaw_app::testing::{FakeCore, FakeSession, demo_fake};
use vizdaw_gpui::controls::key_recorder::{Take, take_for};
use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::settings_window::{SettingsTab, SettingsWindow, open_settings};
use vizdaw_gpui::{AppState, DocumentWindow};

const RESERVE: &str = "Number keys always go to locators (step entry uses the keypad)";

/// The platform's ⌘ in a keystroke string.
const CMD: &str = if cfg!(target_os = "macos") {
    "cmd"
} else {
    "ctrl"
};

struct Setup {
    _fake: Arc<FakeSession>,
    app: gpui::Entity<AppState>,
    doc: WindowHandle<DocumentWindow>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext) -> Setup {
    let core = Arc::new(FakeCore::new());
    let fake = Arc::new(demo_fake());
    core.hand_out("new_demo", fake.clone());
    let store = DocumentStore::with_core(core);
    let (app, doc) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let doc = vizdaw_gpui::new_demo(cx).expect("a demo window");
        (app, doc)
    });
    let vcx = VisualTestContext::from_window(doc.into(), cx);
    vcx.run_until_parked();
    let mut s = Setup {
        _fake: fake,
        app,
        doc,
        cx: vcx,
    };
    s.open();
    s
}

impl Setup {
    fn open(&mut self) {
        self.cx
            .update(|_, cx| open_settings(Some(SettingsTab::Keys), cx));
        self.cx.run_until_parked();
    }

    fn handle(&mut self) -> Option<WindowHandle<SettingsWindow>> {
        self.app.read_with(&self.cx, |a, _| a.settings_window)
    }

    fn vcx(&mut self) -> VisualTestContext {
        VisualTestContext::from_window(self.handle().unwrap().into(), &self.cx)
    }

    fn elements(&mut self) -> Vec<Element> {
        let h = self.handle().unwrap();
        let mut vcx = self.vcx();
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

    fn click_el(&mut self, e: &Element) {
        let mut vcx = self.vcx();
        vcx.simulate_click(e.bounds.center(), Modifiers::none());
        vcx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        let e = self
            .find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label}"));
        self.click_el(&e);
    }

    fn keys(&mut self, keys: &str) {
        let mut vcx = self.vcx();
        vcx.simulate_keystrokes(keys);
        vcx.run_until_parked();
    }

    /// Clicks the Record button of the command titled `title`.
    fn record(&mut self, title: &str) {
        self.click("button", &format!("Record Key for {title}"));
    }

    /// The text elements of `title`'s row: its keys' text (value
    /// `changed` when drawn in the accent).
    fn keys_of(&mut self, title: &str) -> (String, Option<String>) {
        let all = self.elements();
        let i = all
            .iter()
            .position(|e| e.role == "text" && e.label == title)
            .unwrap_or_else(|| panic!("no row {title}"));
        // The row is: title, [step entry], keys, Record, Reset.
        let keys = all[i + 1..]
            .iter()
            .find(|e| e.role == "text" && e.value.as_deref() != Some("step"))
            .unwrap();
        (keys.label.clone(), keys.value.clone())
    }

    /// The Reset button after `title`'s row.
    fn reset_of(&mut self, title: &str) -> Element {
        let all = self.elements();
        let rec = format!("Record Key for {title}");
        let i = all
            .iter()
            .position(|e| e.role == "button" && e.label == rec)
            .unwrap_or_else(|| panic!("no recorder {title}"));
        all[i + 1..]
            .iter()
            .find(|e| e.role == "button" && e.label == "Reset")
            .unwrap()
            .clone()
    }

    fn open_windows(&mut self) -> bool {
        self.handle().is_some() && self.doc.read_with(&self.cx, |v, _| v.doc()).is_ok()
    }

    fn chords(&mut self, id: &str) -> Vec<String> {
        self.app.read_with(&self.cx, |a, _| {
            a.keys.keymap().chords(id).iter().map(Chord::text).collect()
        })
    }
}

// ---- the list ----

#[gpui::test]
fn keys_tab_lists_commands_by_menu_in_registry_order(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let all = s.elements();
    // The sections are the menus in order of first appearance.
    let mut menus: Vec<&str> = vec![];
    for c in COMMANDS {
        if !menus.contains(&c.menu) {
            menus.push(c.menu);
        }
    }
    let sections: Vec<String> = all
        .iter()
        .filter(|e| e.role == "text" && e.value.as_deref() == Some("section"))
        .map(|e| e.label.clone())
        .collect();
    assert_eq!(sections, menus);
    // Every command has a recorder, in registry order.
    let recorders: Vec<String> = all
        .iter()
        .filter(|e| e.role == "button" && e.label.starts_with("Record Key for "))
        .map(|e| e.label.clone())
        .collect();
    let mut by_menu: Vec<String> = vec![];
    for m in &menus {
        for c in COMMANDS.iter().filter(|c| c.menu == *m) {
            by_menu.push(format!("Record Key for {}", c.title));
        }
    }
    assert_eq!(recorders, by_menu);
    // Idle recorders say "Record"; the keys are shown as menus show them.
    assert_eq!(
        s.find("button", "Record Key for Save").unwrap().value,
        Some("Record".into())
    );
    assert_eq!(s.keys_of("Save").0, "⌘S");
    // The caption of a step-entry command.
    assert!(COMMANDS.iter().any(|c| c.context == KeyContext::StepEntry));
    assert!(
        all.iter()
            .any(|e| e.role == "text" && e.label == "step entry"),
        "a step entry caption"
    );
    // The toggle, the buttons; no Learn MIDI.
    assert!(s.find("checkbox", RESERVE).is_some());
    for b in ["Save…", "Load…", "Reset All"] {
        assert!(s.find("button", b).is_some(), "{b}");
    }
    assert!(!all.iter().any(|e| e.label == "Learn MIDI"));
}

#[gpui::test]
fn the_window_is_720_wide_on_keys_and_500_elsewhere(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let w = |s: &mut Setup| {
        let mut vcx = s.vcx();
        vcx.update(|w, _| {
            (
                f32::from(w.bounds().size.width),
                f32::from(w.bounds().size.height),
            )
        })
    };
    let (width, height) = w(&mut s);
    assert_eq!(width, 720.);
    assert!(height >= 520., "{height}");
    s.click("tab", "Audio");
    assert_eq!(w(&mut s).0, 500.);
    s.click("tab", "Keys");
    assert_eq!(w(&mut s).0, 720.);
}

#[gpui::test]
fn a_changed_key_is_drawn_in_accent_and_reset_is_enabled(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // Unchanged: plain, Reset off.
    assert_eq!(s.keys_of("Save").1, None);
    assert!(!s.reset_of("Save").enabled);
    // Bind ⌥⌘J to a command by the model, as a loaded file would.
    s.app.update(&mut s.cx, |a, cx| {
        let mut prefs = std::mem::replace(&mut a.prefs, vizdaw_app::Prefs::in_memory());
        a.keys.record("file.save", "⌥⌘J", &mut prefs);
        a.prefs = prefs;
        cx.notify();
    });
    s.cx.run_until_parked();
    let (text, value) = s.keys_of("Save");
    assert_eq!(text, "⌥⌘J");
    assert_eq!(value.as_deref(), Some("changed"));
    let reset = s.reset_of("Save");
    assert!(reset.enabled);
    s.click_el(&reset);
    assert_eq!(s.chords("file.save"), ["⌘S"]);
    assert_eq!(s.keys_of("Save").1, None);
    assert!(!s.reset_of("Save").enabled);
}

// ---- the recorder ----

#[gpui::test]
fn a_recorded_key_binds_and_rebinds_the_window_keys(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.record("Save");
    assert_eq!(
        s.find("button", "Record Key for Save").unwrap().value,
        Some("Press a key…".into())
    );
    s.keys("alt-j");
    assert_eq!(s.chords("file.save"), ["⌥J"]);
    assert_eq!(
        s.find("button", "Record Key for Save").unwrap().value,
        Some("Record".into())
    );
    assert_eq!(s.keys_of("Save"), ("⌥J".into(), Some("changed".into())));
    // Saved in the prefs, and the new key runs the command.
    let saved = s.app.read_with(&s.cx, |a, _| a.prefs.raw("keys").is_some());
    assert!(saved);
    let keys = s.app.read_with(&s.cx, |a, _| {
        vizdaw_gpui::menus::bindings(a.keys.keymap()).len()
    });
    assert!(keys > 0);
}

#[gpui::test]
fn recording_cmd_w_binds_it_and_closes_nothing(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.record("Save");
    s.keys(&format!("{CMD}-w"));
    // Close has ⌘W: the recorder took the key, the window stayed, and
    // the clash is offered as Replace / Cancel.
    assert!(s.open_windows(), "no window closed");
    assert_eq!(s.chords("file.save"), ["⌘S"]);
    assert_eq!(
        s.app
            .read_with(&s.cx, |a, _| a.keys.pending().map(|p| p.id.clone())),
        Some("file.save".into())
    );
    s.click("button", "Replace");
    assert!(s.open_windows());
    assert_eq!(s.chords("file.save"), ["⌘W"]);
    assert_eq!(s.chords("file.close"), Vec::<String>::new());
    // Not recording any more: Close lost ⌘W (Save has it), so ⌘W no
    // longer closes the Settings window.
    s.keys(&format!("{CMD}-w"));
    assert!(s.handle().is_some(), "⌘W is Save now, not Close");
}

#[gpui::test]
fn escape_cancels_recording(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.record("Save");
    s.keys("escape");
    assert_eq!(
        s.find("button", "Record Key for Save").unwrap().value,
        Some("Record".into())
    );
    assert_eq!(s.chords("file.save"), ["⌘S"]);
    // Escape does not close anything either.
    assert!(s.open_windows());
}

#[gpui::test]
fn an_unbindable_key_is_ignored_and_recording_goes_on(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.record("Save");
    // Shift alone, and a key VizDAW does not bind (F13).
    s.keys("shift");
    s.keys("f13");
    assert_eq!(
        s.find("button", "Record Key for Save").unwrap().value,
        Some("Press a key…".into())
    );
    s.keys("alt-k");
    assert_eq!(s.chords("file.save"), ["⌥K"]);
}

#[gpui::test]
fn clicking_elsewhere_or_switching_tab_stops_recording(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.record("Save");
    s.click("checkbox", RESERVE);
    assert_eq!(
        s.find("button", "Record Key for Save").unwrap().value,
        Some("Record".into())
    );
    s.record("Save");
    s.click("tab", "Audio");
    s.click("tab", "Keys");
    assert_eq!(
        s.find("button", "Record Key for Save").unwrap().value,
        Some("Record".into())
    );
}

#[gpui::test]
fn a_taken_key_offers_replace_and_cancel(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let message = "⌘Z is the key of “Undo”. Replace it?";
    s.record("Save");
    s.keys(&format!("{CMD}-z"));
    // Nothing changed yet; the message and both buttons show.
    assert_eq!(s.chords("file.save"), ["⌘S"]);
    assert!(s.find("text", message).is_some());
    assert!(s.find("button", "Replace").is_some());
    s.click("button", "Cancel");
    assert!(s.find("button", "Replace").is_none());
    assert!(s.find("text", message).is_none());
    assert_eq!(s.chords("edit.undo"), ["⌘Z"]);
    // Again, and Replace: Save has ⌘Z, Undo none.
    s.record("Save");
    s.keys(&format!("{CMD}-z"));
    s.click("button", "Replace");
    assert_eq!(s.chords("file.save"), ["⌘Z"]);
    assert_eq!(s.chords("edit.undo"), Vec::<String>::new());
    assert!(s.find("button", "Replace").is_none());
}

#[gpui::test]
fn reset_all_restores_the_defaults(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.record("Save");
    s.keys("alt-j");
    s.app.update(&mut s.cx, |a, _| {
        let mut prefs = std::mem::replace(&mut a.prefs, vizdaw_app::Prefs::in_memory());
        a.keys.record("edit.undo", "⌥K", &mut prefs);
        a.prefs = prefs;
    });
    assert_eq!(s.chords("file.save"), ["⌥J"]);
    s.click("button", "Reset All");
    assert_eq!(s.chords("file.save"), ["⌘S"]);
    assert_eq!(s.chords("edit.undo"), ["⌘Z"]);
    assert_eq!(s.keys_of("Save").1, None);
    let fresh = Keymap::default();
    assert!(
        s.app
            .read_with(&s.cx, |a, _| a.keys.keymap().changed() == fresh.changed())
    );
}

// ---- chords from keystrokes ----

fn ks(text: &'static str) -> Keystroke {
    Keystroke::parse(text).unwrap()
}

#[test]
fn keystrokes_become_chord_text_as_macos_and_linux_name_them() {
    let t = |s: &'static str, mac: bool| take_for(&ks(s), mac);
    assert_eq!(t("cmd-shift-k", true), Take::Key("⇧⌘K".into()));
    assert_eq!(t("ctrl-alt-k", true), Take::Key("⌃⌥K".into()));
    // Elsewhere ctrl is ⌘ (the `secondary` key), as menus bind it.
    assert_eq!(t("ctrl-w", false), Take::Key("⌘W".into()));
    assert_eq!(t("ctrl-shift-s", false), Take::Key("⇧⌘S".into()));
    assert_eq!(t("super-w", false), Take::Ignore);
    assert_eq!(t("space", true), Take::Key("Space".into()));
    assert_eq!(t("enter", true), Take::Key("Return".into()));
    assert_eq!(t("backspace", true), Take::Key("Delete".into()));
    assert_eq!(t("delete", true), Take::Key("ForwardDelete".into()));
    assert_eq!(t("left", true), Take::Key("←".into()));
    assert_eq!(t("pageup", true), Take::Key("PageUp".into()));
    assert_eq!(t("f5", true), Take::Key("F5".into()));
    assert_eq!(t("2", true), Take::Key("2".into()));
    assert_eq!(t(",", true), Take::Key(",".into()));
    // Escape alone cancels; with a modifier it is a key.
    assert_eq!(t("escape", true), Take::Cancel);
    assert_eq!(t("shift-escape", true), Take::Key("⇧Escape".into()));
    // A key VizDAW cannot bind, and a lone modifier.
    assert_eq!(t("f13", true), Take::Ignore);
    assert_eq!(
        take_for(
            &Keystroke {
                key: "shift".into(),
                key_char: None,
                modifiers: Modifiers::none()
            },
            true
        ),
        Take::Ignore
    );
}

// ---- files ----

/// A panel backend that answers each panel with the next queued path.
#[derive(Default)]
struct Answers {
    paths: RefCell<Vec<PathBuf>>,
    shown: RefCell<Vec<(String, Option<&'static str>)>>,
}

impl PanelBackend for Answers {
    fn show(
        &self,
        spec: &PanelSpec,
        _place: &PanelPlace,
        _window: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        _cx: &mut App,
    ) {
        self.shown
            .borrow_mut()
            .push((spec.prompt.to_owned(), spec.default_name));
        let answer = self.paths.borrow_mut().pop().map(|p| vec![p]);
        done.send(Ok(answer)).ok();
    }
}

fn answering(s: &mut Setup, paths: Vec<PathBuf>) -> Rc<Answers> {
    let a = Rc::new(Answers {
        paths: RefCell::new(paths),
        ..Default::default()
    });
    let backend: Rc<dyn PanelBackend> = a.clone();
    s.cx.update(|_, cx| set_backend(backend, cx));
    a
}

#[gpui::test]
fn save_and_load_use_the_panels_and_the_bindings_file(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("mine.vizkeys");
    let panels = answering(&mut s, vec![file.clone()]);
    s.record("Save");
    s.keys("alt-j");
    s.click("button", "Save…");
    assert_eq!(
        panels.shown.borrow().as_slice(),
        [("Save".to_owned(), Some("Key Bindings.vizkeys"))]
    );
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("vizdaw_keys") && text.contains("⌥J"),
        "{text}"
    );
    // Load it back after a reset.
    s.click("button", "Reset All");
    assert_eq!(s.chords("file.save"), ["⌘S"]);
    *panels.paths.borrow_mut() = vec![file.clone()];
    s.click("button", "Load…");
    assert_eq!(panels.shown.borrow()[1].0, "Open");
    assert_eq!(s.chords("file.save"), ["⌥J"]);
    assert_eq!(s.keys_of("Save").1.as_deref(), Some("changed"));
    // A cancelled panel changes nothing.
    s.click("button", "Reset All");
    s.click("button", "Load…");
    assert_eq!(s.chords("file.save"), ["⌘S"]);
}

#[gpui::test]
fn a_failed_save_or_load_says_why_on_the_message_line(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let dir = tempfile::tempdir().unwrap();
    // A folder that does not exist: the save fails.
    let missing = dir.path().join("nope").join("a.vizkeys");
    answering(&mut s, vec![missing.clone()]);
    s.click("button", "Save…");
    let line = s
        .elements()
        .into_iter()
        .find(|e| e.role == "text" && e.label.contains("nope"))
        .expect("the message line");
    assert!(line.label.contains("a.vizkeys"), "{}", line.label);
    // A file that is no bindings file: nothing changes and it says so.
    let junk = dir.path().join("junk.vizkeys");
    std::fs::write(&junk, "hello").unwrap();
    answering(&mut s, vec![junk]);
    s.click("button", "Load…");
    assert_eq!(s.chords("file.save"), ["⌘S"]);
    assert!(
        s.elements().iter().all(|e| !e.label.contains("nope")),
        "the new message replaced the old one"
    );
}

// ---- the number row ----

#[gpui::test]
fn the_reserve_toggle_saves_and_routes(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    assert_eq!(
        s.find("checkbox", RESERVE).unwrap().value.as_deref(),
        Some("off")
    );
    s.click("checkbox", RESERVE);
    assert_eq!(
        s.find("checkbox", RESERVE).unwrap().value.as_deref(),
        Some("on")
    );
    let (on, saved) = s.app.read_with(&s.cx, |a, _| {
        (
            a.keys.reserve_number_row(&a.prefs),
            a.prefs.reserve_number_row(),
        )
    });
    assert!(on && saved);
    // The app's routing reads it: during step entry "2" stays locator 2.
    let two = Chord::parse("2").unwrap();
    let route = |s: &mut Setup| {
        s.app.read_with(&s.cx, |a, _| {
            a.keys
                .keymap()
                .lookup(two, true, a.keys.reserve_number_row(&a.prefs))
        })
    };
    assert_eq!(route(&mut s), Some("locator.go_2"));
    s.click("checkbox", RESERVE);
    assert_ne!(route(&mut s), Some("locator.go_2"));
}
