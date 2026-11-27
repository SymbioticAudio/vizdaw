//! The launch scan and the plug-in poll (slice 10a Task 2; Swift
//! `openCatalog`, `startScan` and `PluginTicker`, `S/VizDAWApp.swift:
//! 308,515-610`), over the scripted catalog.

use std::sync::Arc;
use std::time::Duration;

use gpui::TestAppContext;
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeCatalog, FakeSession, catalog_row, demo_fake};
use vizdaw_ffi::snapshot::PluginFormatTag;
use vizdaw_ffi::{PluginKindTag, PluginStatusRow, ScanProgress};
use vizdaw_gpui::alerts::{Showing, showing};
use vizdaw_gpui::control::LaunchOptions;
use vizdaw_gpui::{AppState, PLUGIN_POLL_EVERY, start_plugins};

fn install(cx: &mut TestAppContext) {
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(demo_fake()))),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
        vizdaw_gpui::new_document(cx).expect("a window");
    });
    cx.run_until_parked();
}

fn progress(running: bool, done: u32, total: u32) -> ScanProgress {
    ScanProgress {
        running,
        done,
        total,
    }
}

fn tick(cx: &mut TestAppContext) {
    cx.executor()
        .advance_clock(PLUGIN_POLL_EVERY + Duration::from_millis(1));
    cx.run_until_parked();
}

/// The catalog alerts showing, by title.
fn alert_titles(cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        showing(cx)
            .into_iter()
            .filter_map(|(_, s)| match s {
                Showing::Alert(_, a) => Some(a.title),
                _ => None,
            })
            .collect()
    })
}

#[gpui::test]
fn no_scan_is_honoured_at_launch(cx: &mut TestAppContext) {
    install(cx);
    for (args, scans) in [
        (vec!["--no-scan"], vec![]),
        (vec!["--smoke"], vec![]),
        (vec![], vec![false]),
    ] {
        let options = LaunchOptions::parse(args.iter().map(|a| a.to_string()), None);
        let cat = Arc::new(FakeCatalog::new());
        cx.update(|cx| start_plugins(cat.clone(), !options.no_scan, cx));
        assert_eq!(cat.state().scans, scans, "{args:?}");
    }
}

#[gpui::test]
fn the_poll_applies_a_scan_and_shows_the_catalog_alerts(cx: &mut TestAppContext) {
    install(cx);
    let cat = Arc::new(FakeCatalog::new());
    cat.state()
        .progress
        .extend([progress(true, 0, 1), progress(false, 1, 1)]);
    cx.update(|cx| start_plugins(cat.clone(), true, cx));
    let plugins = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let s = AppState::global(cx).read(cx);
            (s.app_plugins.rows().len(), s.app_plugins.needs_poll())
        })
    };
    assert_eq!(plugins(cx), (0, true));
    cat.state().rows = vec![catalog_row(
        "A",
        "V",
        PluginFormatTag::Clap,
        PluginKindTag::Effect,
        PluginStatusRow::Ok,
    )];
    tick(cx);
    assert_eq!(plugins(cx), (1, false), "the finished scan's rows");
    // No scan runs and the Plugin Manager is closed: no poll.
    cat.state().problems.push_back("not saved".into());
    tick(cx);
    tick(cx);
    assert!(alert_titles(cx).is_empty());
    assert_eq!(cat.state().problems.len(), 1);
    // The Manager open: the poll runs and the problem shows.
    cx.update(|cx| AppState::global(cx).update(cx, |s, _| s.plugin_manager_open = true));
    tick(cx);
    assert_eq!(alert_titles(cx), vec!["Plugin Settings"]);
}
