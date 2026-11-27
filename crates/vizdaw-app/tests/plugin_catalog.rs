//! The plug-in catalog seam (Swift `PluginCatalogProtocol`,
//! `FakeCatalog.swift`) and the session calls that put a plug-in in a
//! slot.

mod support;

use support::{FakeSession, mixer_fake};
use vizdaw_app::DocSession;
use vizdaw_app::plugins::{LivePluginCatalog, PluginCatalogCalls, format_label, plugin_key};
use vizdaw_app::testing::{FakeCatalog, catalog_row};
use vizdaw_ffi::snapshot::{PluginFormatTag, PluginRefRow};
use vizdaw_ffi::{PluginKey, PluginKindTag, PluginStatusRow, ScanProgress, VizError};

fn refused(message: &str) -> VizError {
    VizError::Model {
        message: message.into(),
    }
}

#[test]
fn format_labels_name_vst3_as_vst3() {
    assert_eq!(format_label(PluginFormatTag::Au), "AU");
    assert_eq!(format_label(PluginFormatTag::Clap), "CLAP");
    assert_eq!(format_label(PluginFormatTag::Vst3), "VST3");
}

#[test]
fn plugin_key_is_the_format_and_id() {
    let row = catalog_row(
        "Verb",
        "Acme",
        PluginFormatTag::Vst3,
        PluginKindTag::Effect,
        PluginStatusRow::Ok,
    );
    assert_eq!(
        plugin_key(&row.plugin),
        PluginKey {
            format: PluginFormatTag::Vst3,
            id: row.plugin.id.clone(),
        }
    );
}

#[test]
fn fake_catalog_records_every_call() {
    let verb = catalog_row(
        "Verb",
        "Acme",
        PluginFormatTag::Clap,
        PluginKindTag::Effect,
        PluginStatusRow::Ok,
    );
    let synth = catalog_row(
        "Synth",
        "Acme",
        PluginFormatTag::Au,
        PluginKindTag::Instrument,
        PluginStatusRow::Ok,
    );
    let gone = catalog_row(
        "Gone",
        "Acme",
        PluginFormatTag::Clap,
        PluginKindTag::Effect,
        PluginStatusRow::Missing,
    );
    let cat = FakeCatalog::new();
    {
        let mut s = cat.state();
        s.rows = vec![verb.clone(), synth.clone(), gone.clone()];
        s.problems.push_back("plugins.json was damaged".into());
        s.progress.push_back(ScanProgress {
            running: true,
            done: 1,
            total: 2,
        });
        s.progress.push_back(ScanProgress {
            running: false,
            done: 2,
            total: 2,
        });
    }
    let c: &dyn PluginCatalogCalls = &cat;

    assert_eq!(c.rows().unwrap().len(), 3);
    assert_eq!(
        c.browser(PluginKindTag::Effect).unwrap(),
        vec![verb.clone()],
        "the browser offers usable plug-ins of that kind only"
    );
    assert_eq!(
        c.browser(PluginKindTag::Instrument).unwrap(),
        vec![synth.clone()]
    );
    assert_eq!(
        c.take_problem().unwrap().as_deref(),
        Some("plugins.json was damaged")
    );
    assert_eq!(c.take_problem().unwrap(), None, "a problem is taken once");

    let k = plugin_key(&verb.plugin);
    c.set_enabled(k.clone(), false).unwrap();
    assert_eq!(cat.state().enabled_calls, vec![(k.clone(), false)]);
    assert!(!cat.state().rows[0].enabled);
    assert_eq!(cat.state().rows[0].status, PluginStatusRow::Disabled);
    c.set_enabled(k.clone(), true).unwrap();
    assert_eq!(cat.state().rows[0].status, PluginStatusRow::Ok);

    c.scan(false).unwrap();
    c.scan(true).unwrap();
    assert_eq!(cat.state().scans, vec![false, true]);
    c.rescan(vec![k.clone()]).unwrap();
    assert_eq!(cat.state().rescans, vec![vec![k.clone()]]);

    assert!(c.progress().unwrap().running);
    let last = c.progress().unwrap();
    assert!(!last.running);
    assert_eq!(c.progress().unwrap(), last, "the last progress stays");

    c.clear_blocklist(vec![k.clone()]).unwrap();
    assert_eq!(cat.state().cleared, vec![vec![k.clone()]]);

    assert_eq!(c.reveal_path(k.clone()).unwrap(), Some(verb.path.clone()));
    let au = plugin_key(&synth.plugin);
    assert_eq!(c.reveal_path(au.clone()).unwrap(), None, "AU has no path");
    assert_eq!(cat.state().revealed, vec![k.clone(), au]);

    // Every call can be refused, and a refused call records nothing.
    {
        let mut s = cat.state();
        s.fail_rows = Some(refused("rows"));
        s.fail_browser = Some(refused("browser"));
        s.fail_take_problem = Some(refused("problem"));
        s.fail_set_enabled = Some(refused("enable"));
        s.fail_clear_blocklist = Some(refused("clear"));
        s.fail_rescan = Some(refused("rescan"));
        s.fail_scan = Some(refused("scan"));
        s.fail_progress = Some(refused("progress"));
        s.fail_reveal_path = Some(refused("reveal"));
    }
    assert_eq!(c.rows(), Err(refused("rows")));
    assert_eq!(c.browser(PluginKindTag::Effect), Err(refused("browser")));
    assert_eq!(c.take_problem(), Err(refused("problem")));
    assert_eq!(c.set_enabled(k.clone(), false), Err(refused("enable")));
    assert_eq!(c.clear_blocklist(vec![k.clone()]), Err(refused("clear")));
    assert_eq!(c.rescan(vec![k.clone()]), Err(refused("rescan")));
    assert_eq!(c.scan(true), Err(refused("scan")));
    assert_eq!(c.progress(), Err(refused("progress")));
    assert_eq!(c.reveal_path(k.clone()), Err(refused("reveal")));
    let s = cat.state();
    assert_eq!(s.enabled_calls.len(), 2);
    assert_eq!(s.cleared.len(), 1);
    assert_eq!(s.rescans.len(), 1);
    assert_eq!(s.scans.len(), 2);
    assert_eq!(s.revealed.len(), 2);
}

#[test]
fn fake_catalog_refuses_to_clear_while_a_scan_runs() {
    let cat = FakeCatalog::new();
    cat.state().progress.push_back(ScanProgress {
        running: true,
        done: 0,
        total: 1,
    });
    let k = PluginKey {
        format: PluginFormatTag::Clap,
        id: "x.Verb".into(),
    };
    assert_eq!(
        cat.clear_blocklist(vec![k]),
        Err(VizError::Engine {
            message: "a scan is already running".into()
        })
    );
    assert!(cat.state().cleared.is_empty());
}

fn plugin(name: &str) -> PluginRefRow {
    catalog_row(
        name,
        "Acme",
        PluginFormatTag::Clap,
        PluginKindTag::Effect,
        PluginStatusRow::Ok,
    )
    .plugin
}

#[test]
fn fake_session_records_the_new_slot_calls() {
    let f: FakeSession = mixer_fake();
    let verb = plugin("Verb");
    let synth = plugin("Synth");

    let before = f.mixer().unwrap().strips[0].inserts.len();
    f.add_insert(1, Some(0), verb.clone()).unwrap();
    assert_eq!(f.add_inserts(), vec![(1, Some(0), verb.clone())]);
    let strip = &f.mixer().unwrap().strips[0];
    assert_eq!(strip.inserts.len(), before + 1);
    assert_eq!(strip.inserts[0].plugin, verb);

    f.add_plugin_instrument("Synth".into(), synth.clone())
        .unwrap();
    assert_eq!(
        f.plugin_instruments(),
        vec![("Synth".to_owned(), synth.clone())]
    );

    f.set_destination_plugin(7, synth.clone()).unwrap();
    assert_eq!(f.destination_plugins(), vec![(7, synth.clone())]);

    // Each can be refused; a refusal records nothing.
    f.set_call_error("add_insert", Some(refused("no")));
    f.set_call_error("add_plugin_instrument", Some(refused("no")));
    f.set_call_error("set_destination_plugin", Some(refused("no")));
    assert!(f.add_insert(1, None, verb.clone()).is_err());
    assert!(
        f.add_plugin_instrument("Synth".into(), synth.clone())
            .is_err()
    );
    assert!(f.set_destination_plugin(7, synth).is_err());
    assert_eq!(f.add_inserts().len(), 1);
    assert_eq!(f.plugin_instruments().len(), 1);
    assert_eq!(f.destination_plugins().len(), 1);
}

#[test]
fn a_catalog_that_cannot_open_refuses_every_call() {
    let why = VizError::Io {
        message: "plugins.json: permission denied".into(),
    };
    let c = LivePluginCatalog::from_open(Err(why.clone()));
    let k = PluginKey {
        format: PluginFormatTag::Clap,
        id: "x.Verb".into(),
    };
    assert_eq!(c.take_problem(), Err(why.clone()));
    assert_eq!(c.rows(), Err(why.clone()));
    assert_eq!(c.browser(PluginKindTag::Effect), Err(why.clone()));
    assert_eq!(c.set_enabled(k.clone(), false), Err(why.clone()));
    assert_eq!(c.clear_blocklist(vec![k.clone()]), Err(why.clone()));
    assert_eq!(c.rescan(vec![k.clone()]), Err(why.clone()));
    assert_eq!(c.scan(true), Err(why.clone()));
    assert_eq!(c.progress(), Err(why.clone()));
    assert_eq!(c.reveal_path(k), Err(why));
}
