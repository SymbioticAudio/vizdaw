//! The Editing tab (slice 9c Task 4; Swift `EditingSettingsView`): the
//! nudge steps, the crossfade, zero crossings and the external editor.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use futures::channel::oneshot;
use gpui::{AnyWindowHandle, App, Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use vizdaw_app::DocumentStore;
use vizdaw_app::panels::PanelSpec;
use vizdaw_app::testing::{FakeCore, FakeSession, demo_fake};
use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::settings_window::{SettingsTab, SettingsWindow, open_settings};
use vizdaw_gpui::{AppState, DocumentWindow};

struct Setup {
    _fake: Arc<FakeSession>,
    app: gpui::Entity<AppState>,
    _doc: WindowHandle<DocumentWindow>,
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
        _doc: doc,
        cx: vcx,
    };
    s.cx.update(|_, cx| open_settings(Some(SettingsTab::Editing), cx));
    s.cx.run_until_parked();
    s
}

impl Setup {
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

    fn choose(&mut self, menu: &str, item: &str) {
        self.click("menu", menu);
        self.click("menuitem", item);
    }

    fn has_text(&mut self, t: &str) -> bool {
        self.find("text", t).is_some()
    }

    fn value(&mut self, role: &str, label: &str) -> Option<String> {
        self.find(role, label).and_then(|e| e.value)
    }

    fn enabled(&mut self, role: &str, label: &str) -> bool {
        self.find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label}"))
            .enabled
    }
}

const FOLLOW: &str = "Follow the sequence's key signature";
const SCALE_STEPS: &str = "Scale Steps";

#[gpui::test]
fn editing_tab_disables_root_and_scale_unless_scale_steps_and_not_following(
    cx: &mut TestAppContext,
) {
    let mut s = setup(cx);
    for t in [
        "Nudge in Time",
        "Nudge in Pitch",
        "Crossfades",
        "External Sample Editor",
        "Amount: 1",
        "Default crossfade",
        "10 ms",
        "Moves a trimmed start or end to the nearest zero crossing within 3 ms, so the cut does not click.",
        "None",
    ] {
        assert!(s.has_text(t), "text {t}");
    }
    assert_eq!(s.value("menu", "Nudge in time step").unwrap(), "Edit Grid");
    assert_eq!(s.value("menu", "Nudge in pitch step").unwrap(), "Semitones");
    // Semitones: the follow switch, Root and Scale are all off.
    assert!(!s.enabled("checkbox", FOLLOW));
    assert!(!s.enabled("menu", "Root"));
    assert!(!s.enabled("menu", "Scale"));
    s.choose("Nudge in pitch step", SCALE_STEPS);
    assert!(s.enabled("checkbox", FOLLOW));
    assert_eq!(s.value("checkbox", FOLLOW).as_deref(), Some("on"));
    // Following the key signature: Root and Scale stay off.
    assert!(!s.enabled("menu", "Root"));
    assert!(!s.enabled("menu", "Scale"));
    s.click("checkbox", FOLLOW);
    assert!(s.enabled("menu", "Root"));
    assert!(s.enabled("menu", "Scale"));
    assert_eq!(s.value("menu", "Root").unwrap(), "C");
    assert_eq!(s.value("menu", "Scale").unwrap(), "Major");
    s.click("menu", "Scale");
    let items: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "menuitem")
        .map(|e| e.label)
        .collect();
    assert_eq!(
        items,
        [
            "Major",
            "Natural Minor",
            "Harmonic Minor",
            "Dorian",
            "Mixolydian",
            "Major Pentatonic",
            "Minor Pentatonic"
        ]
    );
    s.click("menuitem", "Dorian");
    s.choose("Root", "F#");
    s.click("button", "Increase Amount");
    assert!(s.has_text("Amount: 2"));
    // All of it is kept.
    let n = s.app.read_with(&s.cx, |a, _| a.editing.nudge().clone());
    assert!(n.pitch_scale_steps && !n.follow_key_signature);
    assert_eq!((n.amount, n.root), (2, 6));
    assert_eq!(n.scale, vizdaw_app::editing_prefs::ScaleKind::Dorian);
    // Back to semitones: the amount stays, the rest is off again.
    s.choose("Nudge in pitch step", "Semitones");
    assert!(s.has_text("Amount: 2"));
    assert!(!s.enabled("menu", "Root"));
    assert!(!s.enabled("checkbox", FOLLOW));
}

#[gpui::test]
fn ticks_stepper_reads_n_ticks_per_quarter(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.choose("Nudge in time step", "Ticks");
    let per = vizdaw_app::arrangement::PPQ;
    assert!(
        s.has_text(&format!("240 ticks ({per} per quarter note)")),
        "{:#?}",
        s.elements()
    );
    s.click("button", "Increase Ticks");
    assert!(s.has_text(&format!("300 ticks ({per} per quarter note)")));
    // Back to the grid: the stepper goes; Note Value brings the picker
    // and Triplet.
    s.choose("Nudge in time step", "Note Value");
    assert!(s.find("button", "Increase Ticks").is_none());
    assert_eq!(s.value("menu", "Note Value").unwrap(), "1/16");
    assert_eq!(s.value("checkbox", "Triplet").as_deref(), Some("off"));
    s.choose("Note Value", "1/8");
    s.click("checkbox", "Triplet");
    let t = s.app.read_with(&s.cx, |a, _| a.editing.nudge().time);
    assert_eq!(
        t,
        vizdaw_app::editing_prefs::NudgeTime::Division(vizdaw_app::editor::Division::Eighth, true)
    );
}

#[derive(Default)]
struct Answers {
    paths: RefCell<Vec<PathBuf>>,
    shown: RefCell<Vec<(String, Option<String>)>>,
}

impl PanelBackend for Answers {
    fn show(
        &self,
        spec: &PanelSpec,
        place: &PanelPlace,
        _window: Option<AnyWindowHandle>,
        done: oneshot::Sender<Chosen>,
        _cx: &mut App,
    ) {
        self.shown
            .borrow_mut()
            .push((spec.prompt.to_owned(), spec.message_for(&place.name)));
        let answer = self.paths.borrow_mut().pop().map(|p| vec![p]);
        done.send(Ok(answer)).ok();
    }
}

#[gpui::test]
fn choose_and_clear_the_external_editor(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    let a = Rc::new(Answers {
        paths: RefCell::new(vec![PathBuf::from("/Applications/Wave Editor.app")]),
        ..Default::default()
    });
    let backend: Rc<dyn PanelBackend> = a.clone();
    s.cx.update(|_, cx| set_backend(backend, cx));
    assert!(!s.enabled("button", "Clear"));
    s.click("button", "Choose…");
    assert_eq!(
        a.shown.borrow().as_slice(),
        [(
            "Choose".to_owned(),
            Some(
                "Choose the application that edits audio files for Open in External Editor."
                    .to_owned()
            )
        )]
    );
    assert!(s.has_text("Wave Editor"), "{:#?}", s.elements());
    let kept = s.app.read_with(&s.cx, |a, _| {
        a.editing.external_editor().map(|p| p.to_owned())
    });
    assert_eq!(kept, Some(PathBuf::from("/Applications/Wave Editor.app")));
    assert!(s.enabled("button", "Clear"));
    // A cancelled panel changes nothing.
    s.click("button", "Choose…");
    assert!(s.has_text("Wave Editor"));
    s.click("button", "Clear");
    assert!(s.has_text("None"));
    assert!(!s.enabled("button", "Clear"));
    let kept = s.app.read_with(&s.cx, |a, _| {
        a.editing.external_editor().map(|p| p.to_owned())
    });
    assert_eq!(kept, None);
}

#[gpui::test]
fn crossfade_and_zero_crossings_and_a_failed_write(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.click("button", "Increase Default crossfade");
    assert!(s.has_text("11 ms"));
    for _ in 0..12 {
        s.click("button", "Decrease Default crossfade");
    }
    assert!(s.has_text("1 ms"));
    assert!(!s.enabled("button", "Decrease Default crossfade"));
    s.click("checkbox", "Snap audio edits to zero crossings");
    assert_eq!(
        s.value("checkbox", "Snap audio edits to zero crossings")
            .as_deref(),
        Some("off")
    );
    // A preferences file that cannot be written: the line says why.
    let dir = tempfile::tempdir().unwrap();
    let blocker = dir.path().join("f");
    std::fs::write(&blocker, "x").unwrap();
    s.app.update(&mut s.cx, |a, _| {
        a.prefs = vizdaw_app::Prefs::at(blocker.join("p.json"));
    });
    s.click("checkbox", "Snap audio edits to zero crossings");
    assert!(
        s.elements()
            .iter()
            .any(|e| e.role == "text" && e.value.as_deref() == Some("problem")),
        "{:#?}",
        s.elements()
    );
}
