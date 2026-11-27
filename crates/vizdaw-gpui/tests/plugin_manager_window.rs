//! The Plugin Manager window (slice 10a Task 6; Swift `PluginManagerView`,
//! `S/PluginManagerView.swift`): one per app from Window › Plugin
//! Manager, the table, its buttons and the scan's progress, over the
//! scripted catalog.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use core_control::bindings::Keymap;
use gpui::{Modifiers, TestAppContext, VisualTestContext, WindowHandle};
use vizdaw_app::DocumentStore;
use vizdaw_app::plugin_manager::{
    CLEAR_BLOCKLIST, FOOTER, FormatFilter, RESCAN_ALL, RESCAN_SELECTED, REVEAL, Revealer,
};
use vizdaw_app::testing::{FakeCatalog, FakeSession, catalog_row};
use vizdaw_ffi::snapshot::PluginFormatTag;
use vizdaw_ffi::{CatalogRowFfi, PluginKindTag, PluginStatusRow, ScanProgress};
use vizdaw_gpui::menus::{ItemSpec, action_for, menu_model};
use vizdaw_gpui::plugin_manager_window::{PluginManagerWindow, open_plugin_manager};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::{AppState, PLUGIN_POLL_EVERY, start_plugins};

fn progress(running: bool, done: u32, total: u32) -> ScanProgress {
    ScanProgress {
        running,
        done,
        total,
    }
}

fn rows() -> Vec<CatalogRowFfi> {
    vec![
        catalog_row(
            "Lead",
            "Maker",
            PluginFormatTag::Clap,
            PluginKindTag::Instrument,
            PluginStatusRow::Ok,
        ),
        catalog_row(
            "Crasher",
            "Bad",
            PluginFormatTag::Clap,
            PluginKindTag::Effect,
            PluginStatusRow::Blocklisted {
                reason: "timed out".into(),
            },
        ),
        catalog_row(
            "Comp",
            "Maker",
            PluginFormatTag::Vst3,
            PluginKindTag::Effect,
            PluginStatusRow::Ok,
        ),
    ]
}

#[derive(Default)]
struct Seen(Mutex<Vec<String>>);

impl Revealer for Seen {
    fn reveal(&self, path: &str) {
        self.0.lock().unwrap().push(path.to_owned());
    }
}

struct Setup {
    cat: Arc<FakeCatalog>,
    session: Arc<FakeSession>,
    app: gpui::Entity<AppState>,
    cx: VisualTestContext,
}

fn setup(cx: &mut TestAppContext) -> Setup {
    let session = Arc::new(FakeSession::new(vec![]));
    let s2 = session.clone();
    let store = DocumentStore::new(
        Box::new(move || Ok(s2.clone())),
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|_| Ok(Arc::new(FakeSession::new(vec![])))),
    );
    let cat = Arc::new(FakeCatalog::with_rows(rows()));
    let (app, doc) = cx.update(|cx| {
        let app = vizdaw_gpui::install(store, cx);
        let doc = vizdaw_gpui::new_document(cx).expect("a window");
        start_plugins(cat.clone(), false, cx);
        (app, doc)
    });
    let vcx = VisualTestContext::from_window(doc.into(), cx);
    vcx.run_until_parked();
    Setup {
        cat,
        session,
        app,
        cx: vcx,
    }
}

impl Setup {
    fn handle(&mut self) -> Option<WindowHandle<PluginManagerWindow>> {
        self.app.read_with(&self.cx, |a, _| a.plugin_manager_window)
    }

    fn open_from_menu(&mut self) -> WindowHandle<PluginManagerWindow> {
        self.cx
            .update(|window, cx| window.dispatch_action(action_for("window.plugin_manager"), cx));
        self.cx.run_until_parked();
        self.handle().expect("the Plugin Manager is open")
    }

    fn manager_cx(&mut self) -> VisualTestContext {
        let h = self.handle().unwrap();
        VisualTestContext::from_window(h.into(), &self.cx)
    }

    /// The elements the window drew last, without asking it to draw.
    fn drawn(&mut self) -> Vec<Element> {
        let h = self.handle().unwrap();
        h.read_with(&self.cx, |v, _| v.registry().borrow().elements().to_vec())
            .unwrap()
    }

    fn elements(&mut self) -> Vec<Element> {
        let mut vcx = self.manager_cx();
        vcx.run_until_parked();
        vcx.update(|w, _| w.refresh());
        vcx.run_until_parked();
        self.drawn()
    }

    fn find(&mut self, role: &str, label: &str) -> Option<Element> {
        self.elements()
            .into_iter()
            .find(|e| e.role == role && e.label == label)
    }

    fn get(&mut self, role: &str, label: &str) -> Element {
        self.find(role, label)
            .unwrap_or_else(|| panic!("no {role} {label}: {:?}", self.labels()))
    }

    fn labels(&mut self) -> Vec<(String, String)> {
        self.drawn()
            .into_iter()
            .map(|e| (e.role, e.label))
            .collect()
    }

    fn click_with(&mut self, role: &str, label: &str, mods: Modifiers) {
        let e = self.get(role, label);
        let mut vcx = self.manager_cx();
        vcx.simulate_click(e.bounds.center(), mods);
        vcx.run_until_parked();
    }

    fn click(&mut self, role: &str, label: &str) {
        self.click_with(role, label, Modifiers::none());
    }

    fn selected_rows(&mut self) -> Vec<String> {
        self.elements()
            .into_iter()
            .filter(|e| e.role == "row" && e.selected)
            .map(|e| e.label)
            .collect()
    }

    fn tick(&mut self) {
        self.cx
            .executor()
            .advance_clock(PLUGIN_POLL_EVERY + Duration::from_millis(1));
        self.cx.run_until_parked();
    }
}

#[gpui::test]
fn the_window_menu_opens_one_plugin_manager(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    // Window › Plugin Manager, with its key, with or without a document.
    let keymap = Keymap::default();
    let m = menu_model(None, &keymap);
    let window = m
        .menus
        .iter()
        .find(|m| m.title == "Window")
        .expect("a Window menu");
    let item = window
        .items
        .iter()
        .find_map(|i| match i {
            ItemSpec::Command {
                id,
                title,
                keys,
                enabled,
                ..
            } if title == "Plugin Manager" => Some((*id, keys.clone(), *enabled)),
            _ => None,
        })
        .expect("Plugin Manager");
    assert_eq!(
        item,
        ("window.plugin_manager", Some("⌥⌘P".to_owned()), true)
    );
    assert!(s.handle().is_none());
    assert!(!s.app.read_with(&s.cx, |a, _| a.plugin_manager_open));
    let first = s.open_from_menu();
    assert!(
        s.app.read_with(&s.cx, |a, _| a.plugin_manager_open),
        "the poll runs"
    );
    // Again: the same window comes to the front, no second one.
    let again = s.open_from_menu();
    assert_eq!(first.window_id(), again.window_id());
    s.cx.update(|_, cx| open_plugin_manager(cx));
    s.cx.run_until_parked();
    assert_eq!(s.handle().unwrap().window_id(), first.window_id());
    let windows = s.cx.update(|_, cx| cx.windows().len());
    assert_eq!(windows, 2, "the document and one Manager");
    // Its title bar (the test platform reports no titles: the constant
    // the window opens with).
    assert_eq!(vizdaw_app::plugin_manager::TITLE, "Plugin Manager");
    // The table, the buttons and the footer.
    for col in [
        "On", "Name", "Vendor", "Format", "Version", "Type", "Status",
    ] {
        assert!(s.find("header", col).is_some(), "{col}");
    }
    for b in [RESCAN_SELECTED, RESCAN_ALL, CLEAR_BLOCKLIST, REVEAL] {
        assert!(s.find("button", b).is_some(), "{b}");
    }
    assert!(s.find("text", FOOTER).is_some());
    for f in FormatFilter::available() {
        assert!(s.find("radio", f.label()).is_some(), "{}", f.label());
    }
    assert!(s.find("textfield", "Search").is_some());
    // The least size.
    let size = s.manager_cx().update(|w, _| w.viewport_size());
    assert!(f32::from(size.width) >= 760. && f32::from(size.height) >= 360.);
    // ⌘W closes it; the poll stops.
    let mut vcx = s.manager_cx();
    vcx.simulate_keystrokes("secondary-w");
    vcx.run_until_parked();
    assert!(s.handle().is_none());
    assert!(!s.app.read_with(&s.cx, |a, _| a.plugin_manager_open));
}

#[gpui::test]
fn the_manager_shows_scanning_progress(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.open_from_menu();
    assert!(s.find("text", "Scanning 1 of 3…").is_none());
    let before = s
        .handle()
        .unwrap()
        .read_with(&s.cx, |v, _| v.refreshes())
        .unwrap();
    s.cat.state().progress = [progress(true, 1, 3)].into();
    s.tick();
    // The poll's change redraws the Manager by itself.
    let after = s
        .handle()
        .unwrap()
        .read_with(&s.cx, |v, _| v.refreshes())
        .unwrap();
    assert!(after > before, "the poll's notify reached the Manager");
    assert!(
        s.drawn()
            .iter()
            .any(|e| e.role == "text" && e.label == "Scanning 1 of 3…"),
        "drawn without a refresh: {:?}",
        s.labels()
    );
    assert!(!s.get("button", RESCAN_ALL).enabled, "one scan at a time");
    // The scan ends: the text goes, the rows are read again.
    s.cat.state().rows.push(catalog_row(
        "New",
        "Maker",
        PluginFormatTag::Clap,
        PluginKindTag::Effect,
        PluginStatusRow::Ok,
    ));
    s.cat.state().progress = [progress(false, 3, 3)].into();
    s.tick();
    assert!(s.find("text", "Scanning 1 of 3…").is_none());
    assert!(s.find("row", "New").is_some());
    assert!(s.get("button", RESCAN_ALL).enabled);
}

#[gpui::test]
fn the_table_toggles_selects_and_runs_its_buttons(cx: &mut TestAppContext) {
    let mut s = setup(cx);
    s.open_from_menu();
    assert!(!s.get("button", RESCAN_SELECTED).enabled);
    assert!(!s.get("button", CLEAR_BLOCKLIST).enabled);
    assert!(!s.get("button", REVEAL).enabled);
    // The On toggle: the catalog hears it and the document idles at once.
    let idles = s.session.idle_calls();
    assert_eq!(s.get("checkbox", "Lead").value.as_deref(), Some("on"));
    s.click("checkbox", "Lead");
    assert_eq!(s.cat.state().enabled_calls.len(), 1);
    assert!(!s.cat.state().enabled_calls[0].1);
    assert!(s.session.idle_calls() > idles, "unloaded at once");
    assert_eq!(s.get("checkbox", "Lead").value.as_deref(), Some("off"));
    assert!(s.find("text", "Disabled").is_some());
    // Click, ⇧-click and ⌘-click select.
    let secondary = s.cx.update(|_, cx| Modifiers::secondary_key(cx));
    s.click("row", "Lead");
    assert_eq!(s.selected_rows(), ["Lead"]);
    s.click_with("row", "Comp", Modifiers::shift());
    assert_eq!(s.selected_rows(), ["Lead", "Crasher", "Comp"]);
    s.click_with("row", "Crasher", secondary);
    assert_eq!(s.selected_rows(), ["Lead", "Comp"]);
    s.click_with("row", "Crasher", secondary);
    assert!(s.get("button", CLEAR_BLOCKLIST).enabled);
    s.click("button", CLEAR_BLOCKLIST);
    assert_eq!(s.cat.state().cleared.len(), 1);
    s.click("button", RESCAN_SELECTED);
    assert_eq!(s.cat.state().rescans.len(), 1);
    assert_eq!(s.cat.state().rescans[0].len(), 3);
    // Reveal goes through the app's revealer: the first selected bundle.
    let seen = Arc::new(Seen::default());
    let r: Arc<dyn Revealer> = seen.clone();
    s.app.update(&mut s.cx, |a, _| a.plugin_revealer = r);
    s.click("row", "Comp");
    s.click("button", REVEAL);
    assert_eq!(*seen.0.lock().unwrap(), ["/p/x.Comp"]);
    s.click("button", RESCAN_ALL);
    assert_eq!(s.cat.state().scans, [true]);
    // The format picker.
    s.click("radio", "VST3");
    let shown: Vec<String> = s
        .elements()
        .into_iter()
        .filter(|e| e.role == "row")
        .map(|e| e.label)
        .collect();
    assert_eq!(shown, ["Comp"]);
    assert!(s.get("radio", "VST3").selected);
    // A row's tooltip is its path: in the tree as the row's value.
    assert_eq!(s.get("row", "Comp").value.as_deref(), Some("/p/x.Comp"));
}
