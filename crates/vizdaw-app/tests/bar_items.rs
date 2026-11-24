//! The bar's Click menu, DSP Load and Audio Device texts and command
//! button faces (Swift `TransportRowTests.theClickItemSetsLevelAndMode`,
//! `FormattingTests`, `TransportCustomizeTests`).

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::bar_items::{
    self, BarAction, CommandButtonFace, EntryKind, MenuEntry, STALE_TOOLTIP, cannot_run,
    click_entry, click_is_on, click_menu, command_enabled, device_text, dsp_text,
};
use vizdaw_ffi::ClickRow;
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::session::click::ClickModeRow;

fn demo() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(demo_fake());
    let doc = DocumentModel::new(f.clone()).unwrap();
    (f, doc)
}

fn checked(entries: &[MenuEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.checked)
        .map(|e| e.title.clone())
        .collect()
}

fn sub<'a>(entries: &'a [MenuEntry], title: &str) -> &'a [MenuEntry] {
    match &entries
        .iter()
        .find(|e| e.title == title)
        .unwrap_or_else(|| panic!("no {title}"))
        .kind
    {
        EntryKind::Submenu(s) => s,
        EntryKind::Action(_) => panic!("{title} is not a submenu"),
    }
}

fn action(entries: &[MenuEntry], title: &str) -> BarAction {
    match entries.iter().find(|e| e.title == title).unwrap().kind {
        EntryKind::Action(a) => a,
        EntryKind::Submenu(_) => panic!("{title} is a submenu"),
    }
}

fn text(name: &str, v: &str) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Text { value: v.into() },
    }
}

/// Swift `theClickItemSetsLevelAndMode`.
#[test]
fn the_click_item_sets_level_and_mode() {
    let (f, mut m) = demo();
    let row = ClickRow {
        on: false,
        level_db: -6.0,
        mode: ClickModeRow::PlayAndRecord,
    };
    f.set_click_row(row);
    f.set_transport(|t| t.click = Some(row));
    m.poll_transport().unwrap();
    let menu = click_menu(&m);
    assert_eq!(
        menu.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(),
        ["Level", "Mode"]
    );
    assert!(menu.iter().all(|e| e.enabled));
    assert_eq!(
        sub(&menu, "Level")
            .iter()
            .map(|e| e.title.as_str())
            .collect::<Vec<_>>(),
        [
            "0 dB",
            "\u{2212}3 dB",
            "\u{2212}6 dB",
            "\u{2212}12 dB",
            "\u{2212}18 dB",
            "\u{2212}24 dB",
            "\u{2212}36 dB",
            "\u{2212}48 dB",
            "\u{2212}60 dB"
        ]
    );
    assert_eq!(
        checked(sub(&menu, "Level")),
        ["\u{2212}6 dB"],
        "a new song's level"
    );
    assert_eq!(checked(sub(&menu, "Mode")), ["Play and Record"]);
    bar_items::run(action(sub(&menu, "Level"), "\u{2212}12 dB"), &mut m);
    let last = f.clicks_run().last().cloned().unwrap();
    assert_eq!(last.0, "transport.set_click_level");
    assert_eq!(last.1, [text("level_db", "-12")]);
    assert_eq!(
        checked(sub(&click_menu(&m), "Level")),
        ["\u{2212}12 dB"],
        "the check follows"
    );
    bar_items::run(action(sub(&click_menu(&m), "Mode"), "Record Only"), &mut m);
    let last = f.clicks_run().last().cloned().unwrap();
    assert_eq!(last.0, "transport.set_click_mode");
    assert_eq!(last.1, [text("mode", "record_only")]);
    assert_eq!(checked(sub(&click_menu(&m), "Mode")), ["Record Only"]);
    // The item's toggle.
    bar_items::run(BarAction::ToggleClick, &mut m);
    let last = f.clicks_run().last().cloned().unwrap();
    assert_eq!((last.0.as_str(), last.1.len()), ("transport.click", 0));
    assert!(click_is_on(&m));
    // Folded into »: the same check and submenus.
    let folded = click_entry(&m);
    assert_eq!((folded.title.as_str(), folded.checked), ("Click", true));
    let EntryKind::Submenu(entries) = &folded.kind else {
        panic!("a submenu")
    };
    assert_eq!(
        entries.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(),
        ["Click", "Level", "Mode"]
    );
    assert!(entries[0].checked);
    assert_eq!(action(entries, "Click"), BarAction::ToggleClick);
    assert_eq!(checked(sub(entries, "Level")), ["\u{2212}12 dB"]);
    assert_eq!(checked(sub(entries, "Mode")), ["Record Only"]);
    bar_items::run(action(sub(entries, "Level"), "\u{2212}18 dB"), &mut m);
    assert_eq!(f.clicks_run().last().unwrap().1, [text("level_db", "-18")]);
    // No settings from the session yet: off, and nothing to choose.
    f.set_transport(|t| t.click = None);
    m.poll_transport().unwrap();
    let none = click_menu(&m);
    assert!(none.iter().all(|e| !e.enabled));
    assert!(sub(&none, "Level").iter().all(|e| !e.enabled && !e.checked));
    assert!(!click_is_on(&m));
    let folded = click_entry(&m);
    assert!(folded.enabled, "» still opens it");
    let EntryKind::Submenu(entries) = &folded.kind else {
        panic!("a submenu")
    };
    assert!(entries.iter().all(|e| !e.enabled));
}

/// A level within 0.05 dB is checked; a refusal goes to the status line.
#[test]
fn a_level_near_a_step_is_checked_and_a_refusal_shows() {
    let (f, mut m) = demo();
    f.set_transport(|t| {
        t.click = Some(ClickRow {
            on: true,
            level_db: -12.04,
            mode: ClickModeRow::RecordOnly,
        })
    });
    m.poll_transport().unwrap();
    assert_eq!(checked(sub(&click_menu(&m), "Level")), ["\u{2212}12 dB"]);
    f.fail_next(
        "run_click",
        vizdaw_ffi::VizError::Model {
            message: "no".into(),
        },
    );
    bar_items::run(BarAction::ClickLevel(-3), &mut m);
    assert_eq!(m.status(), Some("no"));
}

/// Swift `dspText`: a whole percent, clamped to 0…99.99 (as a fraction),
/// and 0 for a load that is not a number.
#[test]
fn dsp_text_rows() {
    for (load, text) in [
        (0.0, "DSP 0%"),
        (0.12, "DSP 12%"),
        (0.126, "DSP 13%"),
        (1.0, "DSP 100%"),
        (-0.5, "DSP 0%"),
        (f32::NAN, "DSP 0%"),
        (f32::INFINITY, "DSP 0%"),
        (200.0, "DSP 9999%"),
    ] {
        assert_eq!(dsp_text(load), text, "{load}");
    }
}

/// Swift `FormattingTests`: `<device> · <rate>`, or `No output` before
/// the first Play.
#[test]
fn the_device_text() {
    let (f, m) = demo();
    f.set_transport(|t| {
        t.device = "Offline".into();
        t.sample_rate = 48_000.0;
    });
    m.poll_transport().unwrap();
    assert_eq!(device_text(&m.last_transport()), "Offline \u{b7} 48 kHz");
    f.set_transport(|t| t.sample_rate = 44_100.0);
    m.poll_transport().unwrap();
    assert_eq!(device_text(&m.last_transport()), "Offline \u{b7} 44.1 kHz");
    f.set_transport(|t| {
        t.device = String::new();
        t.sample_rate = 0.0;
    });
    m.poll_transport().unwrap();
    assert_eq!(device_text(&m.last_transport()), "No output");
}

/// Swift `staleCommandsStayGreyed`: an id the registry does not have is
/// greyed with its tooltip, labelled by its label or its id; a live one
/// is the registry's title.
#[test]
fn stale_commands_stay_greyed() {
    let (_f, m) = demo();
    let face = CommandButtonFace::make("gone.command", None, Some(&m));
    assert!(!face.enabled && face.stale);
    assert_eq!(face.tooltip, STALE_TOOLTIP);
    assert_eq!(face.tooltip, "This command is no longer in VizDAW");
    assert_eq!(face.label, "gone.command", "the id when there is no label");
    let labelled = CommandButtonFace::make("gone.command", Some("Old"), Some(&m));
    assert!(labelled.label == "Old" && !labelled.enabled);
    let live = CommandButtonFace::make("window.mixer", None, Some(&m));
    assert!(live.enabled && !live.stale);
    assert_eq!(
        (
            live.label.as_str(),
            live.tooltip.as_str(),
            live.accessibility_label.as_str()
        ),
        ("Mixer", "Mixer", "Mixer")
    );
    let given = CommandButtonFace::make("window.mixer", Some("Mix"), Some(&m));
    assert_eq!(
        (given.label.as_str(), given.tooltip.as_str()),
        ("Mix", "Mixer")
    );
    assert_eq!(given.accessibility_label, "Mixer");
}

/// Swift `commandAvailabilityFollowsTheMenus`: as its menu item is
/// enabled; with no document only an app-wide command is.
#[test]
fn command_availability_follows_the_menus() {
    let empty = DocumentModel::new(Arc::new(FakeSession::new(vec![]))).unwrap();
    assert!(!command_enabled("transport.play", Some(&empty)));
    let (_f, mut m) = demo();
    assert!(command_enabled("transport.play", Some(&m)));
    assert!(command_enabled("window.mixer", Some(&m)));
    assert!(command_enabled("file.bounce", Some(&m)));
    for id in [
        "window.mixer",
        "file.save",
        "transport.play",
        "track.new_audio",
        "region.normalize",
    ] {
        assert!(!command_enabled(id, None), "{id}");
    }
    assert!(
        command_enabled("file.new", None),
        "an app-wide command needs no document"
    );
    // Undo follows its menu item.
    assert!(!command_enabled("edit.undo", Some(&m)));
    m.begin_text_editing("x");
    assert!(
        !command_enabled("transport.play_stop", Some(&m)),
        "a typed key waits, as its menu item"
    );
    assert_eq!(
        cannot_run("Bounce\u{2026}"),
        "Bounce\u{2026} can't run from a button right now."
    );
}
