//! The plug-in browser model, the browser's picks and the instrument menu
//! (Swift `PluginBrowserTests` :16-38; `MixerModelTests` :166-395;
//! `MixerStripTests` `theEqSlotLoadsBypassesAndEmpties` :100,
//! `pickingTheEqAlreadyInTheSlotSendsNothing` :160).

mod support;

use std::sync::Arc;

use support::{FakeSession, dest_row, lane_row, mixer_fake, slot_row, strip_row};
use vizdaw_app::DocumentModel;
use vizdaw_app::alerts::error_message;
use vizdaw_app::mixer::{
    BrowserTarget, InstrumentMenuItem, MixerModel, MixerPlacement, NEW_INSTRUMENT_FAILED,
};
use vizdaw_app::plugin_browser::{NO_MATCH, NO_PLUGINS, PluginBrowserModel, SEARCH};
use vizdaw_app::plugins::AppPlugins;
use vizdaw_app::testing::{FakeCatalog, catalog_row};
use vizdaw_ffi::mixer::StripKindRow;
use vizdaw_ffi::snapshot::{PluginFormatTag, PluginRefRow};
use vizdaw_ffi::{CatalogRowFfi, PluginKindTag, PluginStatusRow, VizError};

fn row(id: &str, name: &str, vendor: &str, kind: PluginKindTag) -> CatalogRowFfi {
    let mut r = catalog_row(
        name,
        vendor,
        PluginFormatTag::Clap,
        kind,
        PluginStatusRow::Ok,
    );
    r.plugin.id = id.into();
    r
}

fn status_row(id: &str, name: &str, kind: PluginKindTag, status: PluginStatusRow) -> CatalogRowFfi {
    let mut r = catalog_row(name, "A", PluginFormatTag::Clap, kind, status);
    r.plugin.id = id.into();
    r
}

/// Swift `PluginBrowserTests.rows`.
fn rows() -> Vec<CatalogRowFfi> {
    let mut delay = row("aufx/dely/appl", "AUDelay", "Apple", PluginKindTag::Effect);
    delay.plugin.format = PluginFormatTag::Au;
    vec![
        row("u.diva", "Diva", "u-he", PluginKindTag::Instrument),
        row("v.zeta", "zeta", "Vendor", PluginKindTag::Effect),
        row("v.alpha", "Alpha", "Vendor", PluginKindTag::Effect),
        row("x.loose", "Loose", "", PluginKindTag::Effect),
        delay,
    ]
}

fn pref(id: &str, name: &str, vendor: &str) -> PluginRefRow {
    PluginRefRow {
        format: PluginFormatTag::Clap,
        id: id.into(),
        name: name.into(),
        vendor: vendor.into(),
        version: "1".into(),
    }
}

fn make_with(f: FakeSession) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(f);
    let mut d = DocumentModel::new(f.clone()).unwrap();
    assert!(d.ensure_mixer());
    (f, d)
}

fn mx<R>(d: &mut DocumentModel, f: impl FnOnce(&mut MixerModel, &mut DocumentModel) -> R) -> R {
    d.with_mixer(f).expect("a mixer")
}

fn model_err(m: &str) -> VizError {
    VizError::Model { message: m.into() }
}

fn none() -> AppPlugins {
    AppPlugins::default()
}

fn names(groups: &[(String, Vec<CatalogRowFfi>)]) -> Vec<String> {
    groups
        .iter()
        .flat_map(|(_, r)| r.iter().map(|x| x.plugin.name.clone()))
        .collect()
}

// PluginBrowserTests.

#[test]
fn groups_by_vendor_with_other_last() {
    let g = PluginBrowserModel::group(&rows(), "");
    let vendors: Vec<&str> = g.iter().map(|(v, _)| v.as_str()).collect();
    assert_eq!(vendors, ["Apple", "u-he", "Vendor", "Other"]);
    assert_eq!(names(&g[2..3]), ["Alpha", "zeta"]);
}

#[test]
fn search_matches_name_vendor_or_id_in_any_case() {
    let vendors = |q: &str| -> Vec<String> {
        PluginBrowserModel::group(&rows(), q)
            .into_iter()
            .map(|(v, _)| v)
            .collect()
    };
    assert_eq!(vendors("DIVA"), ["u-he"]);
    assert_eq!(
        names(&PluginBrowserModel::group(&rows(), " vendor ")).len(),
        2
    );
    assert_eq!(
        names(&PluginBrowserModel::group(&rows(), "dely"))
            .first()
            .map(String::as_str),
        Some("AUDelay")
    );
    assert!(PluginBrowserModel::group(&rows(), "nothing").is_empty());
}

#[test]
fn the_model_lists_its_kind_only() {
    let cat = Arc::new(FakeCatalog::with_rows(rows()));
    let mut plugins = AppPlugins::new(cat.clone());
    plugins.start(false);
    let m = PluginBrowserModel::new(
        PluginKindTag::Instrument,
        plugins.browser(PluginKindTag::Instrument),
    );
    assert_eq!(m.kind, PluginKindTag::Instrument);
    assert_eq!(names(&m.groups("")), ["Diva"]);
    assert!(m.groups("zzz").is_empty());
    // Swift's strings (`PluginBrowserView.swift:16,19-22`).
    assert_eq!(SEARCH, "Search");
    assert_eq!(m.empty_text("zzz"), NO_MATCH);
    assert_eq!(m.empty_text(" "), NO_PLUGINS);
    assert_eq!(
        NO_PLUGINS,
        "No plugins found. Scan in Window > Plugin Manager."
    );
    assert_eq!(NO_MATCH, "No match.");
}

#[test]
fn a_vst3_row_reads_vst3() {
    let mut r = row("v.3", "Three", "V", PluginKindTag::Effect);
    r.plugin.format = PluginFormatTag::Vst3;
    assert_eq!(PluginBrowserModel::format(&r), "VST3");
    r.plugin.format = PluginFormatTag::Au;
    assert_eq!(PluginBrowserModel::format(&r), "AU");
    r.plugin.format = PluginFormatTag::Clap;
    assert_eq!(PluginBrowserModel::format(&r), "CLAP");
}

// MixerModelTests.

#[test]
fn the_browser_pick_goes_where_it_was_opened() {
    let (f, mut d) = make_with(mixer_fake());
    let p = pref("u.diva", "Diva", "u-he");
    mx(&mut d, |m, d| {
        m.open_browser(
            BrowserTarget::Insert { strip: 1 },
            MixerPlacement::Window,
            &none(),
        );
        assert_eq!(m.browser_target(), Some(BrowserTarget::Insert { strip: 1 }));
        assert!(m.shows_browser(MixerPlacement::Window));
        assert!(!m.shows_browser(MixerPlacement::Pane));
        m.pick(d, &p);
        m.open_browser(BrowserTarget::Instrument, MixerPlacement::Window, &none());
        assert!(m.browser_target().unwrap().wants_instrument());
        m.pick(d, &p);
        m.open_browser(
            BrowserTarget::Destination(7),
            MixerPlacement::Window,
            &none(),
        );
        m.pick(d, &p);
        assert_eq!(m.browser_target(), None);
        assert!(m.browser().is_none());
    });
    assert_eq!(
        f.calls(),
        [
            "add_insert(1,None,u.diva)",
            "add_plugin_instrument(Diva,u.diva)",
            "set_destination_plugin(7,u.diva)"
        ]
    );
    assert!(!BrowserTarget::Insert { strip: 1 }.wants_instrument());
    assert!(BrowserTarget::Destination(7).wants_instrument());
    assert!(!BrowserTarget::Eq { strip: 1 }.wants_instrument());
    assert!(d.alert.is_none());
    // A pick with no browser open sends nothing.
    mx(&mut d, |m, d| assert_eq!(m.pick(d, &p), None));
    assert_eq!(f.calls().len(), 3);
}

#[test]
fn the_instrument_menu_groups_by_maker_and_marks_the_current_plugin() {
    let rows = vec![
        row("b.2", "Zeta", "beta", PluginKindTag::Instrument),
        row("a.1", "Mono", "Alpha", PluginKindTag::Instrument),
        row("b.1", "alpha", "beta", PluginKindTag::Instrument),
        row("n.1", "Loose", "", PluginKindTag::Instrument),
    ];
    let current = rows[2].plugin.clone();
    let menu = MixerModel::instrument_menu(&rows, Some(&current));
    let vendors: Vec<&str> = menu.makers.iter().map(|m| m.vendor.as_str()).collect();
    assert_eq!(vendors, ["Alpha", "beta", "Other"]);
    let beta: Vec<&str> = menu.makers[1]
        .choices
        .iter()
        .map(|c| c.plugin.name.as_str())
        .collect();
    assert_eq!(beta, ["alpha", "Zeta"]);
    let checked: Vec<&str> = menu
        .makers
        .iter()
        .flat_map(|m| &m.choices)
        .filter(|c| c.checked)
        .map(|c| c.plugin.id.as_str())
        .collect();
    assert_eq!(checked, ["b.1"]);
    // "Open Editor" first, enabled with a plug-in loaded, then a separator.
    let items = menu.items();
    assert_eq!(items[0], InstrumentMenuItem::OpenEditor { enabled: true });
    assert_eq!(items[1], InstrumentMenuItem::Separator);
    assert_eq!(items.len(), 2 + 3);
    assert_eq!(
        InstrumentMenuItem::OpenEditor { enabled: true }.title(),
        "Open Editor"
    );
    let plain = MixerModel::instrument_menu(&rows, None);
    assert!(
        plain
            .makers
            .iter()
            .flat_map(|m| &m.choices)
            .all(|c| !c.checked)
    );
    assert_eq!(
        plain.items()[0],
        InstrumentMenuItem::OpenEditor { enabled: false }
    );
}

#[test]
fn the_instrument_menu_leaves_out_effects_and_disabled_plugins() {
    let rows = vec![
        status_row("e", "Verb", PluginKindTag::Effect, PluginStatusRow::Ok),
        status_row(
            "d",
            "Off",
            PluginKindTag::Instrument,
            PluginStatusRow::Disabled,
        ),
        status_row(
            "m",
            "Gone",
            PluginKindTag::Instrument,
            PluginStatusRow::Missing,
        ),
        status_row("k", "Keys", PluginKindTag::Instrument, PluginStatusRow::Ok),
    ];
    let mut off_but_ok = status_row("o", "Odd", PluginKindTag::Instrument, PluginStatusRow::Ok);
    off_but_ok.enabled = false;
    let mut all = rows.clone();
    all.push(off_but_ok);
    let menu = MixerModel::instrument_menu(&all, None);
    let vendors: Vec<&str> = menu.makers.iter().map(|m| m.vendor.as_str()).collect();
    assert_eq!(vendors, ["A"]);
    let ids: Vec<&str> = menu.makers[0]
        .choices
        .iter()
        .map(|c| c.plugin.id.as_str())
        .collect();
    assert_eq!(ids, ["k"]);
    let empty = MixerModel::instrument_menu(&[], None);
    assert!(empty.makers.is_empty());
    assert_eq!(
        empty.items(),
        [
            InstrumentMenuItem::OpenEditor { enabled: false },
            InstrumentMenuItem::Separator,
            InstrumentMenuItem::NoInstruments,
        ]
    );
    assert_eq!(
        InstrumentMenuItem::NoInstruments.title(),
        "No Plugin Instruments"
    );
    assert!(!InstrumentMenuItem::NoInstruments.enabled());
    assert!(
        MixerModel::instrument_menu(&rows[..1], None)
            .makers
            .is_empty()
    );
}

#[test]
fn picking_an_instrument_replaces_it_in_that_destination() {
    let (f, mut d) = make_with(mixer_fake());
    let mut strip = strip_row(5, "Keys", StripKindRow::Instrument { dest: 7 });
    strip.instrument = Some(slot_row(50, "Old"));
    let new = pref("u.diva", "Diva", "u-he");
    mx(&mut d, |m, d| m.pick_instrument(d, &new, &strip));
    assert_eq!(f.calls(), ["set_destination_plugin(7,u.diva)"]);
    assert!(d.alert.is_none());
}

#[test]
fn replacing_the_instrument_renames_the_strip_and_the_track_destination_at_once() {
    let f = mixer_fake();
    f.set_destinations(vec![dest_row(4, "Built-in Synth")]);
    let mut l = lane_row(30, "Chords", false);
    l.midi_dest = Some(4);
    l.destination = Some("Built-in Synth".into());
    f.set_lanes(3, vec![l]);
    let (f, mut d) = make_with(f);
    assert_eq!(d.selected_seq(), Some(3));
    assert_eq!(d.lanes()[0].destination.as_deref(), Some("Built-in Synth"));
    let new = pref("u.pend", "Pendulate", "V");
    let slot = mx(&mut d, |m, d| {
        let synth = m.strip(1).cloned().unwrap();
        m.pick_instrument(d, &new, &synth)
    });
    // No refresh call: everything already shows the plug-in's name.
    {
        let m = d.mixer().unwrap();
        let s = m
            .strips()
            .find(|s| s.kind == StripKindRow::Instrument { dest: 4 })
            .unwrap();
        assert_eq!(
            (s.name.as_str(), s.kind_label.as_str()),
            ("Pendulate", "Pendulate")
        );
        assert_eq!(s.instrument.as_ref().unwrap().plugin.name, "Pendulate");
        assert_eq!(slot, s.instrument.as_ref().map(|x| x.id));
        assert_eq!(m.destinations()[0].name, "Pendulate");
    }
    assert_eq!(d.lanes()[0].destination.as_deref(), Some("Pendulate"));
    // A renamed destination keeps the user's name.
    let mut dest = d.mixer().unwrap().destinations()[0].clone();
    dest.name = "Lead".into();
    f.set_destinations(vec![dest]);
    let mut l = d.lanes()[0].clone();
    l.destination = Some("Lead".into());
    f.set_lanes(3, vec![l]);
    let other = pref("u.other", "Other", "V");
    mx(&mut d, |m, d| {
        m.pick_instrument_on_destination(d, &other, 4)
    });
    assert_eq!(d.mixer().unwrap().destinations()[0].name, "Lead");
    assert_eq!(d.lanes()[0].destination.as_deref(), Some("Lead"));
    assert!(d.alert.is_none());
}

#[test]
fn a_pick_returns_the_new_slot_to_open_and_a_failure_returns_none() {
    let f = mixer_fake();
    f.set_destinations(vec![dest_row(4, "Built-in Synth")]);
    let (f, mut d) = make_with(f);
    let new = pref("u.pend", "Pendulate", "V");
    let synth = d.mixer().unwrap().strip(1).cloned().unwrap();
    assert_eq!(
        mx(&mut d, |m, d| m.pick_instrument(d, &new, &synth)),
        Some(904)
    );
    // The same plug-in again: no change, nothing to open.
    assert_eq!(
        mx(&mut d, |m, d| m.pick_instrument_on_destination(d, &new, 4)),
        None
    );
    f.set_call_error("set_destination_plugin", Some(model_err("load failed")));
    let other = pref("u.other", "Other", "V");
    assert_eq!(
        mx(&mut d, |m, d| m
            .pick_instrument_on_destination(d, &other, 4)),
        None
    );
    assert_eq!(d.alert.as_ref().unwrap().title, "Instrument Failed");
}

#[test]
fn picking_the_loaded_instrument_or_on_a_non_instrument_strip_does_nothing() {
    let (f, mut d) = make_with(mixer_fake());
    let mut strip = strip_row(5, "Keys", StripKindRow::Instrument { dest: 7 });
    strip.instrument = Some(slot_row(50, "Old"));
    let loaded = strip.instrument.as_ref().unwrap().plugin.clone();
    let bus = strip_row(6, "Bus", StripKindRow::Bus);
    mx(&mut d, |m, d| {
        assert_eq!(m.pick_instrument(d, &loaded, &strip), None);
        assert_eq!(m.pick_instrument(d, &loaded, &bus), None);
        assert_eq!(m.instrument_destination(&bus), None);
    });
    assert!(f.calls().is_empty());
    assert!(d.alert.is_none());
}

#[test]
fn a_destinations_instrument_is_replaced_unless_it_is_the_loaded_one() {
    let f = mixer_fake();
    let old = pref("u.old", "Old", "V");
    let mut seven = dest_row(7, "Old");
    seven.plugin = Some(old.clone());
    f.set_destinations(vec![seven, dest_row(4, "Built-in Synth")]);
    let (f, mut d) = make_with(f);
    let new = pref("u.diva", "Diva", "u-he");
    mx(&mut d, |m, d| {
        m.pick_instrument_on_destination(d, &old, 7);
        assert!(f.calls().is_empty());
        m.pick_instrument_on_destination(d, &new, 7);
        m.pick_instrument_on_destination(d, &old, 4);
        assert_eq!(m.instrument_plugin_of_destination(7), Some(&new));
        assert_eq!(m.instrument_plugin_of_destination(4), Some(&old));
    });
    assert_eq!(
        f.calls(),
        [
            "set_destination_plugin(7,u.diva)",
            "set_destination_plugin(4,u.old)"
        ]
    );
}

#[test]
fn a_failing_instrument_pick_alerts() {
    let (f, mut d) = make_with(mixer_fake());
    f.set_call_error("set_destination_plugin", Some(model_err("no such plugin")));
    let strip = strip_row(5, "Keys", StripKindRow::Instrument { dest: 7 });
    let got = mx(&mut d, |m, d| {
        m.pick_instrument(d, &pref("x", "X", "V"), &strip)
    });
    assert_eq!(got, None);
    let a = d.alert.as_ref().unwrap();
    assert_eq!(a.title, "Instrument Failed");
    assert_eq!(a.message, "no such plugin");
}

#[test]
fn browser_models_are_reused_while_open_and_reset_on_close() {
    let cat = Arc::new(FakeCatalog::with_rows(rows()));
    let mut plugins = AppPlugins::new(cat.clone());
    plugins.start(false);
    let (_f, mut d) = make_with(mixer_fake());
    mx(&mut d, |m, _| {
        assert!(m.browser().is_none());
        m.open_browser(BrowserTarget::Instrument, MixerPlacement::Window, &plugins);
        let a = m.browser().unwrap() as *const PluginBrowserModel;
        assert_eq!(names(&m.browser().unwrap().groups("")), ["Diva"]);
        // A scan adds a plug-in while the browser shows: it keeps its rows.
        cat.state()
            .rows
            .push(row("n.new", "Newer", "N", PluginKindTag::Instrument));
        m.open_browser(
            BrowserTarget::Destination(4),
            MixerPlacement::Window,
            &plugins,
        );
        let b = m.browser().unwrap() as *const PluginBrowserModel;
        assert_eq!(a, b);
        assert_eq!(names(&m.browser().unwrap().groups("")), ["Diva"]);
        m.close_browser();
        assert!(m.browser().is_none());
        assert_eq!(m.browser_target(), None);
        m.open_browser(BrowserTarget::Instrument, MixerPlacement::Window, &plugins);
        assert_eq!(names(&m.browser().unwrap().groups("")), ["Newer", "Diva"]);
        // Another kind is another model.
        m.open_browser(
            BrowserTarget::Insert { strip: 1 },
            MixerPlacement::Pane,
            &plugins,
        );
        assert_eq!(m.browser().unwrap().kind, PluginKindTag::Effect);
        assert!(m.shows_browser(MixerPlacement::Pane));
    });
}

// MixerStripTests.

#[test]
fn the_eq_slot_loads_bypasses_and_empties() {
    let (f, mut d) = make_with(mixer_fake());
    let eq = pref("x.EQ", "EQ", "V");
    let target = BrowserTarget::Eq { strip: 1 };
    assert!(!target.wants_instrument());
    let slot = mx(&mut d, |m, d| {
        m.open_browser(target, MixerPlacement::Window, &none());
        assert_eq!(m.browser().unwrap().kind, PluginKindTag::Effect);
        m.pick(d, &eq)
    });
    assert_eq!(f.calls(), ["set_eq(1,Some(\"x.EQ\"))"]);
    let id = d.mixer().unwrap().strip(1).unwrap().eq.as_ref().unwrap().id;
    assert_eq!(slot, Some(id));
    // The same plug-in again is no step.
    mx(&mut d, |m, d| {
        m.open_browser(target, MixerPlacement::Window, &none());
        assert_eq!(m.pick(d, &eq), None);
    });
    assert_eq!(f.calls().len(), 1, "the plug-in already in the slot");
    mx(&mut d, |m, d| m.toggle_bypass(d, id));
    assert_eq!(f.calls().last().unwrap(), &format!("set_bypass({id},true)"));
    mx(&mut d, |m, d| m.clear_eq(d, 1));
    assert_eq!(f.calls().last().unwrap(), "set_eq(1,None)");
    f.set_call_error("set_eq", Some(model_err("no")));
    mx(&mut d, |m, d| {
        m.open_browser(target, MixerPlacement::Window, &none());
        assert_eq!(m.pick(d, &eq), None);
    });
    assert_eq!(d.alert.as_ref().unwrap().title, "Load EQ Failed");
}

#[test]
fn picking_the_eq_already_in_the_slot_sends_nothing() {
    let f = mixer_fake();
    let eq = pref("x.EQ", "EQ", "V");
    f.edit_mixer(|m| {
        let mut x = slot_row(77, "EQ");
        x.plugin = eq.clone();
        m.strips[0].eq = Some(x);
    });
    let (f, mut d) = make_with(f);
    mx(&mut d, |m, d| {
        m.open_browser(
            BrowserTarget::Eq { strip: 1 },
            MixerPlacement::Window,
            &none(),
        );
        assert_eq!(m.pick(d, &eq), None);
        assert!(m.browser().is_none());
        assert_eq!(m.browser_target(), None);
    });
    assert!(f.calls().is_empty());
    assert!(d.alert.is_none());
}

// New.

/// Review Focus 1: a scan marks the plug-in Missing while the browser
/// shows; the browser keeps its rows, and the core's refusal of the add is
/// the alert "Add Plugin Failed".
#[test]
fn a_pick_after_the_plugin_went_missing_is_an_alert() {
    let cat = Arc::new(FakeCatalog::with_rows(vec![row(
        "x.Verb",
        "Verb",
        "V",
        PluginKindTag::Effect,
    )]));
    let mut plugins = AppPlugins::new(cat.clone());
    plugins.start(false);
    let (f, mut d) = make_with(mixer_fake());
    mx(&mut d, |m, _| {
        m.open_browser(
            BrowserTarget::Insert { strip: 1 },
            MixerPlacement::Window,
            &plugins,
        )
    });
    cat.state().rows[0].status = PluginStatusRow::Missing;
    plugins.poll();
    let shown = d.mixer().unwrap().browser().unwrap().groups("");
    assert_eq!(names(&shown), ["Verb"], "the open browser keeps its rows");
    let verb = shown[0].1[0].plugin.clone();
    let refusal = model_err("plugin x.Verb is missing");
    f.set_call_error("add_insert", Some(refusal.clone()));
    let got = mx(&mut d, |m, d| m.pick(d, &verb));
    assert_eq!(got, None);
    let a = d.alert.as_ref().unwrap();
    assert_eq!(a.title, "Add Plugin Failed");
    assert_eq!(a.message, error_message(&refusal));
    assert!(d.mixer().unwrap().browser().is_none());
    // A reopened browser no longer lists it.
    mx(&mut d, |m, _| {
        m.open_browser(
            BrowserTarget::Insert { strip: 1 },
            MixerPlacement::Window,
            &plugins,
        );
        assert!(m.browser().unwrap().groups("").is_empty());
    });
}

#[test]
fn an_insert_pick_returns_the_new_slot_and_a_new_instrument_refusal_alerts() {
    let (f, mut d) = make_with(mixer_fake());
    let p = pref("x.Gain", "Gain", "V");
    let slot = mx(&mut d, |m, d| {
        m.open_browser(
            BrowserTarget::Insert { strip: 1 },
            MixerPlacement::Window,
            &none(),
        );
        m.pick(d, &p)
    });
    let inserted = d.mixer().unwrap().strip(1).unwrap().inserts[0].id;
    assert_eq!(slot, Some(inserted));
    f.set_call_error("add_plugin_instrument", Some(model_err("no")));
    mx(&mut d, |m, d| {
        m.open_browser(BrowserTarget::Instrument, MixerPlacement::Window, &none());
        assert_eq!(m.pick(d, &p), None);
    });
    assert_eq!(d.alert.as_ref().unwrap().title, NEW_INSTRUMENT_FAILED);
    assert_eq!(NEW_INSTRUMENT_FAILED, "New Instrument Failed");
}
