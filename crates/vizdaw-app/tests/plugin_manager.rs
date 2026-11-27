//! The Plugin Manager's model (slice 10a Task 6; Swift
//! `PluginManagerModel`, ported from `PluginManagerTests.swift`): the
//! rows, the filter, the selection and the intents, over the scripted
//! catalog.

use std::sync::{Arc, Mutex};

use vizdaw_app::DocumentStore;
use vizdaw_app::plugin_manager::{
    Click, FOOTER, FormatFilter, ManagerRow, PluginManagerModel, Revealer, reveal_command_for,
};
use vizdaw_app::plugins::AppPlugins;
use vizdaw_app::testing::{FakeCatalog, FakeSession, catalog_row};
use vizdaw_ffi::snapshot::PluginFormatTag;
use vizdaw_ffi::{CatalogRowFfi, PluginKindTag, PluginStatusRow, ScanProgress, VizError};

fn blocklisted(reason: &str) -> PluginStatusRow {
    PluginStatusRow::Blocklisted {
        reason: reason.into(),
    }
}

fn with_id(mut r: CatalogRowFfi, id: &str) -> CatalogRowFfi {
    r.plugin.id = id.into();
    r
}

fn progress(running: bool, done: u32, total: u32) -> ScanProgress {
    ScanProgress {
        running,
        done,
        total,
    }
}

/// Swift's table (`PluginManagerTests.swift:9-14`): an instrument, a
/// crashing effect, an AU not validated yet and a bundle that failed as a
/// whole (no plug-in id).
fn table() -> Vec<CatalogRowFfi> {
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
            blocklisted("crashed (signal 11)"),
        ),
        with_id(
            catalog_row(
                "AUDelay",
                "System",
                PluginFormatTag::Au,
                PluginKindTag::Effect,
                PluginStatusRow::NotValidated,
            ),
            "aufx/dely/sys",
        ),
        with_id(
            catalog_row(
                "Broken",
                "",
                PluginFormatTag::Clap,
                PluginKindTag::Effect,
                blocklisted("timed out"),
            ),
            "",
        ),
    ]
}

struct Setup {
    cat: Arc<FakeCatalog>,
    plugins: AppPlugins,
    store: DocumentStore,
    m: PluginManagerModel,
}

impl Setup {
    fn rows(&self) -> Vec<ManagerRow> {
        self.m.rows(&self.plugins)
    }

    fn set_enabled(&mut self, i: usize, on: bool) -> bool {
        let r = self.rows()[i].clone();
        self.m
            .set_enabled(&mut self.plugins, &mut self.store, &r, on)
    }

    fn select(&mut self, rows: &[usize]) {
        let all = self.rows();
        self.m.selection = rows.iter().map(|&i| all[i].id()).collect();
    }

    fn titles(&mut self) -> Vec<String> {
        std::iter::from_fn(|| self.plugins.next_alert())
            .map(|(t, _)| t)
            .collect()
    }
}

fn store() -> DocumentStore {
    DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|_| Ok(Arc::new(FakeSession::new(vec![])))),
    )
}

fn manager() -> Setup {
    let cat = Arc::new(FakeCatalog::with_rows(table()));
    let mut plugins = AppPlugins::new(cat.clone());
    plugins.start(false);
    Setup {
        cat,
        plugins,
        store: store(),
        m: PluginManagerModel::new(),
    }
}

#[test]
fn rows_show_the_spec_statuses_and_filter() {
    let mut s = manager();
    let rows = s.rows();
    assert_eq!(
        rows.iter().map(ManagerRow::status_text).collect::<Vec<_>>(),
        [
            "OK",
            "Blocklisted: crashed (signal 11)",
            "Not validated",
            "Blocklisted: timed out"
        ]
    );
    assert_eq!(rows[0].kind_text(), "Instrument");
    assert_eq!(rows[1].kind_text(), "Effect");
    assert_eq!(rows[0].format_text(), "CLAP");
    assert_eq!(rows[2].format_text(), "AU");
    assert_eq!(rows[0].id(), "clap:x.Lead|/p/x.Lead");
    s.m.format = FormatFilter::Au;
    assert_eq!(
        s.rows()
            .iter()
            .map(|r| r.name().to_owned())
            .collect::<Vec<_>>(),
        ["AUDelay"]
    );
    s.m.format = FormatFilter::All;
    s.m.search = " MAKER ".into();
    assert_eq!(
        s.rows()
            .iter()
            .map(|r| r.name().to_owned())
            .collect::<Vec<_>>(),
        ["Lead"]
    );
    // The free form the window draws from.
    let free = vizdaw_app::plugin_manager::rows(&s.plugins, FormatFilter::Clap, "cra");
    assert_eq!(free.len(), 1);
    assert_eq!(free[0].name(), "Crasher");
    // No kind known: a dash.
    s.cat.state().rows[3].kind = None;
    s.plugins.scan_all();
    s.m.search.clear();
    assert_eq!(s.rows()[3].kind_text(), "—");
    // A VST3 reads VST3, and its own filter finds it.
    s.cat.state().rows.push(catalog_row(
        "Comp",
        "Maker",
        PluginFormatTag::Vst3,
        PluginKindTag::Effect,
        PluginStatusRow::Ok,
    ));
    s.plugins.scan_all();
    s.m.format = FormatFilter::Vst3;
    let v = s.rows();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].format_text(), "VST3");
    assert_eq!(v[0].id(), "vst3:x.Comp|/p/x.Comp");
    // Missing and Disabled.
    s.cat.state().rows[0].status = PluginStatusRow::Missing;
    s.cat.state().rows[1].status = PluginStatusRow::Disabled;
    s.plugins.scan_all();
    s.m.format = FormatFilter::All;
    assert_eq!(s.rows()[0].status_text(), "Missing");
    assert_eq!(s.rows()[1].status_text(), "Disabled");
    assert!(FOOTER.starts_with("Turning a plugin off unloads it"));
}

#[test]
fn the_format_filter_offers_vst3_and_au_only_on_macos() {
    let labels: Vec<&str> = FormatFilter::available()
        .iter()
        .map(|f| f.label())
        .collect();
    if cfg!(target_os = "macos") {
        assert_eq!(labels, ["All", "AU", "CLAP", "VST3"]);
    } else {
        assert_eq!(labels, ["All", "CLAP", "VST3"]);
    }
    assert_eq!(PluginManagerModel::new().format, FormatFilter::All);
}

#[test]
fn disabling_and_enabling_reach_the_catalog() {
    let mut s = manager();
    assert!(s.set_enabled(0, false));
    assert_eq!(
        s.cat
            .state()
            .enabled_calls
            .iter()
            .map(|c| c.1)
            .collect::<Vec<_>>(),
        [false]
    );
    assert_eq!(s.rows()[0].status_text(), "Disabled");
    assert!(!s.rows()[0].is_enabled());
    // A bundle that failed as a whole has nothing to enable.
    assert!(!s.rows()[3].has_plugin());
    assert!(!s.set_enabled(3, false));
    assert_eq!(s.cat.state().enabled_calls.len(), 1);
}

#[test]
fn disabling_reaches_the_catalog_and_idles_every_document() {
    let mut s = manager();
    let a = Arc::new(FakeSession::new(vec![]));
    let b = Arc::new(FakeSession::new(vec![]));
    let (fa, fb) = (a.clone(), b.clone());
    let next = Arc::new(Mutex::new(vec![fa, fb]));
    s.store = DocumentStore::new(
        Box::new(move || {
            let f = next.lock().unwrap().remove(0);
            Ok(f as Arc<dyn vizdaw_app::DocSession>)
        }),
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|_| Ok(Arc::new(FakeSession::new(vec![])))),
    );
    s.store.new_empty().expect("a");
    s.store.new_empty().expect("b");
    let before = (a.idle_calls(), b.idle_calls());
    assert!(s.set_enabled(0, false));
    assert_eq!(s.cat.state().enabled_calls.len(), 1);
    assert_eq!(
        (a.idle_calls(), b.idle_calls()),
        (before.0 + 1, before.1 + 1),
        "every open document idles at once: the plug-in unloads now"
    );
    // On again: they idle again (the waiting slots load).
    assert!(s.set_enabled(0, true));
    assert_eq!(
        (a.idle_calls(), b.idle_calls()),
        (before.0 + 2, before.1 + 2)
    );
    // A refused toggle changes nothing, so nothing idles.
    s.cat.state().fail_set_enabled = Some(VizError::Busy {
        message: "editor opening".into(),
    });
    assert!(!s.set_enabled(0, false));
    assert_eq!(
        (a.idle_calls(), b.idle_calls()),
        (before.0 + 2, before.1 + 2)
    );
}

#[test]
fn selection_drives_rescan_clear_and_reveal() {
    let mut s = manager();
    assert!(!s.m.can_rescan_selected(&s.plugins));
    assert!(!s.m.can_clear_blocklist(&s.plugins));
    assert!(!s.m.can_reveal(&s.plugins));
    assert!(PluginManagerModel::can_rescan_all(&s.plugins));
    s.select(&[1, 3]);
    assert!(s.m.can_clear_blocklist(&s.plugins));
    s.m.clear_blocklist(&mut s.plugins);
    assert_eq!(
        s.cat
            .state()
            .cleared
            .iter()
            .map(|k| k.iter().map(|k| k.id.clone()).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        [["x.Crasher"]]
    );
    assert!(s.titles().is_empty());
    s.select(&[0]);
    s.m.rescan_selected(&mut s.plugins);
    assert_eq!(
        s.cat.state().rescans[0]
            .iter()
            .map(|k| k.id.clone())
            .collect::<Vec<_>>(),
        ["x.Lead"]
    );
    assert!(s.m.can_reveal(&s.plugins));
    assert_eq!(s.m.reveal_path(&s.plugins).as_deref(), Some("/p/x.Lead"));
    // An AU has no bundle to show.
    s.select(&[2]);
    assert!(!s.m.can_reveal(&s.plugins));
    s.select(&[0]);
    s.cat.state().progress = [progress(true, 0, 2)].into();
    PluginManagerModel::rescan_all(&mut s.plugins);
    assert_eq!(s.cat.state().scans, [true]);
    assert_eq!(
        PluginManagerModel::progress_text(&s.plugins),
        "Scanning 0 of 2…"
    );
    assert!(!s.m.can_rescan_selected(&s.plugins), "one scan at a time");
    assert!(!PluginManagerModel::can_rescan_all(&s.plugins));
    // Done: the text clears.
    s.cat.state().progress = [progress(false, 2, 2)].into();
    s.plugins.poll();
    assert_eq!(PluginManagerModel::progress_text(&s.plugins), "");
}

#[test]
fn reveal_goes_to_the_revealer_with_the_bundle() {
    #[derive(Default)]
    struct Seen(Mutex<Vec<String>>);
    impl Revealer for Seen {
        fn reveal(&self, path: &str) {
            self.0.lock().unwrap().push(path.to_owned());
        }
    }
    let mut s = manager();
    let seen = Seen::default();
    assert!(!s.m.reveal(&s.plugins, &seen), "nothing selected");
    s.select(&[2, 0]);
    assert!(s.m.reveal(&s.plugins, &seen));
    assert_eq!(*seen.0.lock().unwrap(), ["/p/x.Lead"]);
    assert_eq!(
        reveal_command_for("/a/b/X.clap", true),
        ("open", vec!["-R".to_owned(), "/a/b/X.clap".to_owned()])
    );
    assert_eq!(
        reveal_command_for("/a/b/X.clap", false),
        ("xdg-open", vec!["/a/b".to_owned()])
    );
}

#[test]
fn re_enabling_works_and_a_blocklisted_row_can_be_switched_off_and_stays_off() {
    let mut s = manager();
    s.set_enabled(0, false);
    assert_eq!(s.rows()[0].status_text(), "Disabled");
    s.set_enabled(0, true);
    assert_eq!(
        s.cat
            .state()
            .enabled_calls
            .iter()
            .map(|c| c.1)
            .collect::<Vec<_>>(),
        [false, true]
    );
    assert_eq!(s.rows()[0].status_text(), "OK");
    assert!(s.rows()[0].is_enabled());
    // The crashing plug-in: off stays off, the status still says why.
    assert!(s.rows()[1].is_enabled());
    s.set_enabled(1, false);
    assert!(!s.rows()[1].is_enabled());
    assert_eq!(
        s.rows()[1].status_text(),
        "Blocklisted: crashed (signal 11)"
    );
    s.set_enabled(1, true);
    assert!(s.rows()[1].is_enabled());
}

#[test]
fn selecting_several_blocklisted_plugins_clears_them_in_one_call_without_an_alert() {
    let mut s = manager();
    s.cat.state().rows.push(catalog_row(
        "Crasher2",
        "Bad",
        PluginFormatTag::Clap,
        PluginKindTag::Effect,
        blocklisted("timed out"),
    ));
    s.plugins.scan_all();
    s.cat.state().scans.clear();
    s.m.selection = s
        .rows()
        .iter()
        .filter(|r| r.is_blocklisted() && r.has_plugin())
        .map(ManagerRow::id)
        .collect();
    assert_eq!(s.m.selection.len(), 2);
    s.m.clear_blocklist(&mut s.plugins);
    let cleared = s.cat.state().cleared.clone();
    assert_eq!(cleared.len(), 1);
    let mut ids: Vec<String> = cleared[0].iter().map(|k| k.id.clone()).collect();
    ids.sort();
    assert_eq!(ids, ["x.Crasher", "x.Crasher2"]);
    assert!(s.titles().is_empty());
    // The bundles scan again.
    s.cat.state().progress = [progress(true, 0, 2)].into();
    s.plugins.poll();
    assert!(!s.m.can_clear_blocklist(&s.plugins), "a scan is running");
    // A second clear during the scan would fail; the model never asks.
    s.m.clear_blocklist(&mut s.plugins);
    assert_eq!(s.cat.state().cleared.len(), 1);
    assert!(s.titles().is_empty());
}

#[test]
fn busy_during_a_toggle_raises_no_alert_and_keeps_the_row() {
    let mut s = manager();
    s.cat.state().fail_set_enabled = Some(VizError::Busy {
        message: "editor opening".into(),
    });
    s.set_enabled(0, false);
    assert!(s.titles().is_empty());
    assert!(s.rows()[0].is_enabled());
}

#[test]
fn clicks_select_one_toggle_with_the_command_key_and_extend_with_shift() {
    let mut s = manager();
    let ids: Vec<String> = s.rows().iter().map(ManagerRow::id).collect();
    s.m.click(&ids[1], Click::Plain, &s.plugins);
    assert_eq!(s.m.selection, [ids[1].clone()].into());
    s.m.click(&ids[3], Click::Extend, &s.plugins);
    assert_eq!(
        s.m.selection,
        [ids[1].clone(), ids[2].clone(), ids[3].clone()].into()
    );
    s.m.click(&ids[2], Click::Toggle, &s.plugins);
    assert_eq!(s.m.selection, [ids[1].clone(), ids[3].clone()].into());
    s.m.click(&ids[0], Click::Toggle, &s.plugins);
    assert_eq!(s.m.selection.len(), 3);
    // Shift from the last toggled row (the anchor) back to the first.
    s.m.click(&ids[0], Click::Plain, &s.plugins);
    s.m.click(&ids[2], Click::Extend, &s.plugins);
    assert_eq!(
        s.m.selection,
        [ids[0].clone(), ids[1].clone(), ids[2].clone()].into()
    );
}
