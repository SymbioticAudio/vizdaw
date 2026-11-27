//! The app's plug-in catalog (Swift `AppPlugins`, ported from
//! `PluginBrowserTests.swift` `AppPluginsTests` :43-142): the launch scan,
//! the progress poll and the catalog's alerts.

use std::sync::Arc;

use vizdaw_app::plugins::{AppPlugins, plugin_key};
use vizdaw_app::testing::{FakeCatalog, catalog_row};
use vizdaw_ffi::snapshot::PluginFormatTag;
use vizdaw_ffi::{PluginKindTag, PluginStatusRow, ScanProgress, VizError};

fn row(name: &str) -> vizdaw_ffi::CatalogRowFfi {
    catalog_row(
        name,
        "V",
        PluginFormatTag::Clap,
        PluginKindTag::Effect,
        PluginStatusRow::Ok,
    )
}

fn progress(running: bool, done: u32, total: u32) -> ScanProgress {
    ScanProgress {
        running,
        done,
        total,
    }
}

fn plugins(cat: &Arc<FakeCatalog>) -> AppPlugins {
    AppPlugins::new(cat.clone())
}

fn missing_scanner() -> VizError {
    VizError::NotFound {
        path: "/x/vizdaw-scan".into(),
    }
}

fn titles(p: &mut AppPlugins) -> Vec<String> {
    std::iter::from_fn(|| p.next_alert())
        .map(|(t, _)| t)
        .collect()
}

#[test]
fn start_opens_scans_and_shows_a_problem_once() {
    let cat = Arc::new(FakeCatalog::new());
    cat.state()
        .problems
        .push_back("plugin settings not read: newer".into());
    cat.state().progress.push_back(progress(true, 0, 3));
    let mut p = plugins(&cat);
    p.start(true);
    assert_eq!(cat.state().scans, vec![false]);
    assert!(p.progress().running);
    assert!(p.needs_poll());
    assert_eq!(
        p.next_alert(),
        Some((
            "Plugin Settings".to_owned(),
            "plugin settings not read: newer".to_owned()
        ))
    );
    p.poll();
    assert_eq!(p.next_alert(), None);
}

#[test]
fn start_without_scan_scans_nothing() {
    let cat = Arc::new(FakeCatalog::with_rows(vec![row("A")]));
    let mut p = plugins(&cat);
    p.start(false);
    assert!(cat.state().scans.is_empty());
    assert_eq!(p.rows().len(), 1);
    assert!(!p.needs_poll());
    assert_eq!(p.next_alert(), None);
}

#[test]
fn a_finished_scan_refreshes_the_rows() {
    let cat = Arc::new(FakeCatalog::new());
    cat.state().progress.push_back(progress(true, 0, 2));
    let mut p = plugins(&cat);
    p.start(true);
    assert!(p.rows().is_empty());
    {
        let mut s = cat.state();
        s.rows = vec![row("A")];
        s.progress = [progress(false, 2, 2)].into();
    }
    assert!(p.poll());
    assert_eq!(p.rows().len(), 1);
    assert!(!p.progress().running);
    assert!(!p.needs_poll());
}

#[test]
fn actions_reach_the_catalog_and_errors_become_alerts() {
    let cat = Arc::new(FakeCatalog::with_rows(vec![row("A")]));
    let mut p = plugins(&cat);
    p.start(false);
    let key = plugin_key(&cat.state().rows[0].plugin);
    assert!(p.set_enabled(key.clone(), false));
    assert_eq!(p.rows()[0].status, PluginStatusRow::Disabled);
    p.rescan(vec![key.clone()]);
    p.clear_blocklist(vec![key.clone()]);
    assert_eq!(cat.state().rescans, vec![vec![key.clone()]]);
    assert_eq!(cat.state().cleared, vec![vec![key.clone()]]);
    assert_eq!(p.reveal_path(key).as_deref(), Some("/p/x.A"));
    assert_eq!(p.next_alert(), None);
    cat.state().fail_scan = Some(missing_scanner());
    p.scan_all();
    assert_eq!(cat.state().scans, Vec::<bool>::new());
    assert_eq!(
        p.next_alert().map(|(t, _)| t).as_deref(),
        Some("Scan Failed")
    );
    cat.state().fail_rescan = Some(VizError::Io {
        message: "denied".into(),
    });
    p.rescan(vec![]);
    assert_eq!(
        p.next_alert(),
        Some(("Rescan Failed".to_owned(), "denied".to_owned()))
    );
    cat.state().fail_clear_blocklist = Some(VizError::Io {
        message: "no".into(),
    });
    p.clear_blocklist(vec![]);
    assert_eq!(
        p.next_alert(),
        Some(("Rescan Failed".to_owned(), "no".to_owned()))
    );
}

#[test]
fn set_enabled_reports_whether_the_catalog_took_it() {
    let cat = Arc::new(FakeCatalog::with_rows(vec![row("A")]));
    let key = plugin_key(&cat.state().rows[0].plugin);
    let mut p = plugins(&cat);
    assert!(!p.set_enabled(key.clone(), false), "not started yet");
    assert!(cat.state().enabled_calls.is_empty());
    p.start(false);
    assert!(p.set_enabled(key.clone(), false));
    cat.state().fail_set_enabled = Some(VizError::Io {
        message: "not saved".into(),
    });
    assert!(!p.set_enabled(key, true));
    assert_eq!(
        p.next_alert(),
        Some(("Plugin Settings".to_owned(), "not saved".to_owned()))
    );
}

#[test]
fn a_catalog_that_cannot_open_is_an_alert() {
    let cat = Arc::new(FakeCatalog::new());
    {
        let denied = Some(VizError::Io {
            message: "denied".into(),
        });
        let mut s = cat.state();
        s.fail_take_problem = denied.clone();
        s.fail_rows = denied.clone();
        s.fail_browser = denied.clone();
        s.fail_scan = denied.clone();
        s.fail_progress = denied;
        s.rows = vec![row("A")];
    }
    let mut p = plugins(&cat);
    p.start(true);
    assert!(!p.is_open());
    assert!(cat.state().scans.is_empty());
    assert_eq!(
        p.next_alert(),
        Some(("Plugins Unavailable".to_owned(), "denied".to_owned()))
    );
    assert_eq!(p.next_alert(), None);
    assert!(p.browser(PluginKindTag::Effect).is_empty());
    assert!(!p.poll());
    assert_eq!(p.next_alert(), None);
}

#[test]
fn a_problem_and_a_failed_scan_are_both_shown_in_order() {
    let cat = Arc::new(FakeCatalog::new());
    {
        let mut s = cat.state();
        s.problems
            .push_back("plugin settings not read: newer".into());
        s.fail_scan = Some(missing_scanner());
    }
    let mut p = plugins(&cat);
    p.start(true);
    assert_eq!(titles(&mut p), vec!["Plugin Settings", "Scan Failed"]);
}

#[test]
fn a_repeated_problem_is_queued_once() {
    let cat = Arc::new(FakeCatalog::new());
    cat.state()
        .problems
        .extend(std::iter::repeat_n("not saved".to_owned(), 3));
    let mut p = plugins(&cat);
    p.start(false);
    p.poll();
    p.poll();
    assert_eq!(titles(&mut p), vec!["Plugin Settings"]);
}

#[test]
fn busy_never_alerts_and_automatic_running_scan_is_silent() {
    let cat = Arc::new(FakeCatalog::new());
    cat.state().fail_scan = Some(VizError::Busy {
        message: "busy".into(),
    });
    let mut p = plugins(&cat);
    p.start(true);
    p.scan_all();
    assert_eq!(p.next_alert(), None);
    // The core's refusal while a scan runs (`core-ffi/src/catalog.rs:261`).
    cat.state().fail_scan = Some(VizError::Engine {
        message: "a scan is already running".into(),
    });
    p.start(true);
    p.scan_new();
    assert_eq!(p.next_alert(), None);
    p.rescan(vec![]);
    p.scan_all();
    assert_eq!(
        p.next_alert(),
        Some((
            "Scan Failed".to_owned(),
            "a scan is already running".to_owned()
        ))
    );
    assert_eq!(p.next_alert(), None);
}

#[test]
fn a_missing_scanner_is_one_alert() {
    let cat = Arc::new(FakeCatalog::new());
    cat.state().fail_scan = Some(missing_scanner());
    let mut p = plugins(&cat);
    p.start(true);
    for _ in 0..10 {
        p.poll();
    }
    let (title, message) = p.next_alert().expect("Scan Failed");
    assert_eq!(title, "Scan Failed");
    assert!(message.contains("/x/vizdaw-scan"), "{message}");
    assert_eq!(p.next_alert(), None);
    // Rescan All raises it again; the polls still add none.
    p.scan_all();
    for _ in 0..10 {
        p.poll();
    }
    assert_eq!(titles(&mut p), vec!["Scan Failed"]);
}

#[test]
fn the_poll_refreshes_only_on_change() {
    let cat = Arc::new(FakeCatalog::with_rows(vec![row("A")]));
    {
        let mut s = cat.state();
        s.progress.extend([
            progress(true, 0, 3),
            progress(true, 0, 3),
            progress(true, 1, 3),
            progress(true, 1, 3),
            progress(false, 3, 3),
        ]);
    }
    let mut p = plugins(&cat);
    p.start(true);
    // The scan's first read (running, 0 of 3) is taken by the start.
    assert!(p.needs_poll());
    let reads = || cat.state().rows_reads;
    let before = reads();
    assert!(!p.poll(), "running, 0 of 3 again: nothing changed");
    assert_eq!(reads(), before);
    assert!(p.poll(), "1 of 3");
    assert_eq!(reads(), before + 1);
    assert!(!p.poll());
    assert_eq!(reads(), before + 1);
    assert!(p.poll(), "finished");
    assert_eq!(reads(), before + 2);
    assert!(!p.needs_poll());
    // The last read sticks: nothing more changes.
    assert!(!p.poll());
    assert_eq!(reads(), before + 2);
}
