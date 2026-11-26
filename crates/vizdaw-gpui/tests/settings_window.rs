//! The Settings window (slice 9a Task 4): one per app, the nine tabs in
//! Swift's order, placeholders for the tabs not built yet, and none of a
//! document's keys.

use std::sync::Arc;

use core_control::bindings::Keymap;
use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeSession, demo_fake};
use vizdaw_gpui::menus::{ItemSpec, menu_model};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::settings_window::{SettingsTab, SettingsWindow, open_settings};
use vizdaw_gpui::{AppState, DocumentWindow, OpenSettings};

struct Setup {
    fake: Arc<FakeSession>,
    app: gpui::Entity<AppState>,
    doc: Option<WindowHandle<DocumentWindow>>,
    cx: VisualTestContext,
}

/// An app with one demo document (its window front), or none.
fn setup(cx: &mut TestAppContext, with_doc: bool) -> Setup {
    let fake = Arc::new(demo_fake());
    let shared = fake.clone();
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
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
