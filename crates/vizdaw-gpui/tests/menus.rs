//! The menu model and key bindings built from the command registry.

use std::sync::Arc;

use core_control::bindings::{Chord, Keymap};
use core_control::registry;
use gpui::KeyBindingContextPredicate;
use vizdaw_app::DocumentModel;
use vizdaw_app::commands;
use vizdaw_app::testing::demo_fake;
use vizdaw_gpui::menus::{self, ItemSpec, MenuModel, UNMODIFIED_KEYS, keystroke, menu_model};

fn titles(m: &MenuModel) -> Vec<&str> {
    m.menus.iter().map(|s| s.title.as_str()).collect()
}

/// The item titled `title` in menu `menu`: (id, title, keys, enabled).
fn item<'a>(
    m: &'a MenuModel,
    menu: &str,
    title: &str,
) -> (&'a str, &'a str, Option<&'a str>, bool) {
    let spec = m
        .menus
        .iter()
        .find(|s| s.title == menu)
        .unwrap_or_else(|| panic!("no menu {menu}"));
    spec.items
        .iter()
        .find_map(|i| match i {
            ItemSpec::Command {
                id,
                title: t,
                keys,
                enabled,
                ..
            } if t == title => Some((*id, t.as_str(), keys.as_deref(), *enabled)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no item {title} in {menu}"))
}

fn demo_doc() -> DocumentModel {
    DocumentModel::new(Arc::new(demo_fake())).expect("a document")
}

#[test]
fn the_menu_model_follows_the_swift_order() {
    let keymap = Keymap::default();
    let doc = demo_doc();
    for front in [None, Some(&doc)] {
        let m = menu_model(front, &keymap);
        assert_eq!(
            titles(&m),
            [
                "VizDAW",
                "File",
                "Edit",
                "View",
                "Editors",
                "Track",
                "Transport",
                "Automation",
                "Mixer",
                "Window"
            ]
        );
    }
    let m = menu_model(Some(&doc), &keymap);
    let edit: Vec<Option<&str>> = m.menus[2]
        .items
        .iter()
        .map(|i| match i {
            ItemSpec::Command { title, .. } => Some(title.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        edit,
        [
            Some("Undo"),
            Some("Redo"),
            None,
            Some("Cut"),
            Some("Copy"),
            Some("Paste"),
            Some("Select All"),
        ]
    );
    let track: Vec<Option<&str>> = m.menus[5]
        .items
        .iter()
        .map(|i| match i {
            ItemSpec::Command { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(
        track,
        [
            Some("track.new_audio"),
            Some("track.new_instrument"),
            Some("track.new_midi"),
            None,
            Some("track.duplicate"),
            Some("track.delete"),
            Some("track.rename"),
        ]
    );

    let ids = |i: usize| -> Vec<Option<&str>> {
        m.menus[i]
            .items
            .iter()
            .map(|i| match i {
                ItemSpec::Command { id, .. } => Some(*id),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        ids(3),
        [
            Some("ui.view_arrange"),
            Some("ui.view_mix"),
            None,
            Some("ui.toggle_left"),
            Some("ui.toggle_bottom"),
            Some("ui.toggle_right"),
        ]
    );
    assert_eq!(ids(9), [Some("window.mixer")]);
    assert_eq!(
        ids(4),
        [
            Some("editors.piano_roll"),
            Some("editors.list"),
            None,
            Some("editors.close_detail"),
            Some("editors.insert_event"),
            None,
            Some("editors.snap"),
        ]
    );
}

#[test]
fn track_items_take_their_titles_and_keys_from_the_registry() {
    let keymap = Keymap::default();
    let doc = demo_doc();
    let m = menu_model(Some(&doc), &keymap);
    for id in [
        "track.new_audio",
        "track.new_instrument",
        "track.new_midi",
        "track.duplicate",
        "track.delete",
        "track.rename",
    ] {
        let spec = registry::find(id).unwrap();
        let (got, _, keys, _) = item(&m, "Track", spec.title);
        assert_eq!(got, id);
        let want = spec.shortcut.map(|s| Chord::parse(s).unwrap().text());
        assert_eq!(keys.map(String::from), want, "{id}");
    }
    let shortcut = registry::find("track.new_audio").unwrap().shortcut.unwrap();
    let chord = Chord::parse(shortcut).unwrap();
    let bound: Vec<gpui::Keystroke> = menus::bindings(&keymap)
        .iter()
        .filter(|b| {
            b.action()
                .partial_eq(&menus::RunCommand::new("track.new_audio"))
        })
        .flat_map(|b| b.keystrokes().iter().map(|k| k.inner().clone()))
        .collect();
    let os = if cfg!(target_os = "macos") {
        gpui::OperatingSystem::Mac
    } else {
        gpui::OperatingSystem::Linux
    };
    let want = gpui::Keystroke::parse_with_os(&keystroke(&chord).unwrap(), os).unwrap();
    assert_eq!(bound, vec![want]);
}

#[test]
fn undo_shows_its_label_and_disables_without_one() {
    let keymap = Keymap::default();
    let fake = Arc::new(demo_fake());
    let mut doc = DocumentModel::new(fake.clone()).unwrap();
    let m = menu_model(Some(&doc), &keymap);
    let (_, _, keys, enabled) = item(&m, "Edit", "Undo");
    assert!(!enabled);
    assert_eq!(keys, Some("⌘Z"));
    assert!(!item(&m, "Edit", "Redo").3);
    // With no document: the registry's titles, disabled.
    let none = menu_model(None, &keymap);
    assert!(!item(&none, "Edit", "Undo").3);

    let mut next = fake.snap();
    next.undo_label = Some("New Track".into());
    fake.bump(next);
    commands::run("track.new_audio", &[], &mut doc).unwrap();
    let m = menu_model(Some(&doc), &keymap);
    let (id, _, _, enabled) = item(&m, "Edit", "Undo New Track");
    assert_eq!(id, "edit.undo");
    assert!(enabled);
}

#[test]
fn track_items_need_a_selected_track() {
    let keymap = Keymap::default();
    let mut doc = demo_doc();
    let m = menu_model(Some(&doc), &keymap);
    assert!(item(&m, "Track", "New Audio Track").3);
    for t in ["Duplicate Track", "Delete Track", "Rename Track"] {
        assert!(!item(&m, "Track", t).3, "{t}");
    }
    doc.select_track(30, false);
    let m = menu_model(Some(&doc), &keymap);
    for t in ["Duplicate Track", "Delete Track", "Rename Track"] {
        assert!(item(&m, "Track", t).3, "{t}");
    }
    // No document: nothing in Track runs.
    let none = menu_model(None, &keymap);
    assert!(!item(&none, "Track", "New Audio Track").3);
}

#[test]
fn single_keys_are_off_in_text_fields() {
    let want = KeyBindingContextPredicate::parse(UNMODIFIED_KEYS).unwrap();
    let all = menus::bindings(&Keymap::default());
    let mut checked = 0;
    for b in &all {
        // A text field's own editing keys act only inside it, the List
        // table's only inside the table, the roll's only in its canvas.
        if b.action().name().starts_with("text_input::") {
            continue;
        }
        if b.action().name().starts_with("list_editor::") {
            let table =
                KeyBindingContextPredicate::parse(vizdaw_gpui::list_editor::CONTEXT).unwrap();
            assert_eq!(b.predicate().as_deref(), Some(&table));
            continue;
        }
        // The Piano Roll's Delete keys act only inside its canvas.
        if b.action().name().starts_with("piano_roll::") {
            let roll = KeyBindingContextPredicate::parse(vizdaw_gpui::piano_roll::CONTEXT).unwrap();
            assert_eq!(b.predicate().as_deref(), Some(&roll));
            continue;
        }
        if b.keystrokes().iter().any(|k| k.modifiers().modified()) {
            continue;
        }
        checked += 1;
        assert_eq!(
            b.predicate().as_deref(),
            Some(&want),
            "{} has context {:?}",
            b.action().name(),
            b.predicate()
        );
    }
    // Space and S at least.
    assert!(checked >= 2);
    // Every document command's chords are bound.
    for id in commands::DOCUMENT_COMMANDS {
        let n = all
            .iter()
            .filter(|b| b.action().partial_eq(&menus::RunCommand::new(*id)))
            .count();
        assert_eq!(n, Keymap::default().chords(id).len(), "{id}");
    }
}

#[test]
fn keystroke_spells_modifiers_for_gpui() {
    let chord = Chord::parse("⌥⌘A").unwrap();
    let want = if cfg!(target_os = "macos") {
        "cmd-alt-a"
    } else {
        "ctrl-alt-a"
    };
    assert_eq!(keystroke(&chord).as_deref(), Some(want));
    assert_eq!(
        keystroke(&Chord::parse("Space").unwrap()).as_deref(),
        Some("space")
    );
    assert_eq!(
        keystroke(&Chord::parse("⇧⌘Z").unwrap()).as_deref(),
        Some(if cfg!(target_os = "macos") {
            "cmd-shift-z"
        } else {
            "ctrl-shift-z"
        })
    );
    assert_eq!(
        keystroke(&Chord::parse("F5").unwrap()).as_deref(),
        Some("f5")
    );
}

#[test]
fn the_transport_menu_lists_every_transport_command() {
    let keymap = Keymap::default();
    let doc = demo_doc();
    let m = menu_model(Some(&doc), &keymap);
    let transport = m.menus.iter().find(|s| s.title == "Transport").unwrap();
    let ids: Vec<&str> = transport
        .items
        .iter()
        .filter_map(|i| match i {
            ItemSpec::Command { id, .. } => Some(*id),
            _ => None,
        })
        .collect();
    // Every transport command with a menu item, in registry order.
    let want: Vec<&str> = registry::COMMANDS
        .iter()
        .filter(|c| c.id.starts_with("transport.") && c.menu_item)
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, want);
    for c in registry::COMMANDS
        .iter()
        .filter(|c| c.id.starts_with("transport.") && c.menu_item)
    {
        let (id, _, keys, enabled) = item(&m, "Transport", c.title);
        assert_eq!(id, c.id);
        let want = c.shortcut.map(|s| Chord::parse(s).unwrap().text());
        assert_eq!(keys.map(String::from), want, "{id}");
        assert_eq!(enabled, commands::enabled(id, &doc), "{id}");
    }
    // Stopped, with a sequence: these run.
    for t in [
        "Play / Stop",
        "Rewind",
        "Fast Forward",
        "Return to Start",
        "Loop",
        "Go to Position Field",
        "Go to Edit-In Field",
        "Overdub",
        "Punch",
        "Go to Punch Fields",
        "Record",
        "Click",
        "Arm Previous Track",
        "Arm Next Track",
    ] {
        assert!(item(&m, "Transport", t).3, "{t}");
    }
    // Nothing is playing: Stop does not run; scrubbing is not here yet.
    assert!(!item(&m, "Transport", "Stop").3);
    assert!(!item(&m, "Transport", "Scrub").3);
    // Every transport command's keys are bound, but the held keys (the
    // bar's shuttle handles them) and the keypad twins (GPUI cannot tell
    // the keypad from the main keys).
    let all = menus::bindings(&keymap);
    for &id in commands::TRANSPORT_COMMANDS {
        if matches!(
            id,
            "transport.rewind"
                | "transport.fast_forward"
                | "transport.play_stop"
                | "transport.stop"
        ) {
            continue;
        }
        let n = all
            .iter()
            .filter(|b| b.action().partial_eq(&menus::RunCommand::new(id)))
            .count();
        assert_eq!(n, keymap.chords(id).len(), "{id}");
    }
}

/// `item`'s check mark.
fn checked(m: &MenuModel, menu: &str, title: &str) -> Option<bool> {
    m.menus
        .iter()
        .find(|s| s.title == menu)
        .and_then(|s| {
            s.items.iter().find_map(|i| match i {
                ItemSpec::Command {
                    title: t, checked, ..
                } if t == title => Some(*checked),
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("no item {title} in {menu}"))
}

#[test]
fn the_view_menu_checks_the_main_view_and_the_shown_panes() {
    let keymap = Keymap::default();
    let mut doc = demo_doc();
    let m = menu_model(Some(&doc), &keymap);
    // Off macOS ⌘1 and ⌘2 own Ctrl+1 and Ctrl+2: the views show no key.
    let mac = cfg!(target_os = "macos");
    assert_eq!(item(&m, "View", "Arrange View").2, mac.then_some("⌃1"));
    assert_eq!(item(&m, "View", "Mix View").2, mac.then_some("⌃2"));
    assert_eq!(item(&m, "View", "Left Pane").2, Some("⌃⌘←"));
    assert_eq!(item(&m, "View", "Right Pane").2, Some("⌃⌘→"));
    assert_eq!(checked(&m, "View", "Arrange View"), Some(true));
    assert_eq!(checked(&m, "View", "Mix View"), Some(false));
    assert_eq!(checked(&m, "View", "Left Pane"), Some(true));
    assert_eq!(checked(&m, "View", "Bottom Pane"), Some(true));
    assert_eq!(checked(&m, "View", "Right Pane"), Some(false));
    for title in [
        "Arrange View",
        "Mix View",
        "Left Pane",
        "Bottom Pane",
        "Right Pane",
    ] {
        assert!(item(&m, "View", title).3, "{title}");
    }
    commands::run("ui.view_mix", &[], &mut doc).unwrap();
    let m = menu_model(Some(&doc), &keymap);
    assert_eq!(checked(&m, "View", "Arrange View"), Some(false));
    assert_eq!(checked(&m, "View", "Mix View"), Some(true));
    // The mix view's own panes: all hidden at first.
    assert_eq!(checked(&m, "View", "Left Pane"), Some(false));
    assert_eq!(checked(&m, "View", "Bottom Pane"), Some(false));
    // The Mixer window: enabled with a document, with its key.
    let (id, _, keys, enabled) = item(&m, "Window", "Mixer");
    assert_eq!((id, keys, enabled), ("window.mixer", Some("⌥⌘M"), true));
    let none = menu_model(None, &keymap);
    assert!(!item(&none, "Window", "Mixer").3);
    assert!(!item(&none, "View", "Mix View").3);
}

/// The main views' ⌃ keys and the editors' ⌘ keys never land on one
/// keystroke: off macOS, where both are Ctrl, the ⌘ key keeps it.
#[test]
fn main_view_keys_do_not_collide_with_the_editor_keys() {
    let keymap = Keymap::default();
    let all = menus::bindings(&keymap);
    let strokes = |id: &str| -> Vec<String> {
        all.iter()
            .filter(|b| {
                b.action()
                    .partial_eq(&menus::RunCommand::new(id.to_owned()))
            })
            .flat_map(|b| b.keystrokes().iter().map(|k| format!("{:?}", k.inner())))
            .collect()
    };
    let roll = strokes("editors.piano_roll");
    let arrange = strokes("ui.view_arrange");
    assert_eq!(roll.len(), 1, "{roll:?}");
    assert!(
        arrange.iter().all(|k| !roll.contains(k)),
        "{arrange:?} {roll:?}"
    );
    if cfg!(target_os = "macos") {
        assert_eq!(arrange.len(), 1, "{arrange:?}");
        assert_eq!(strokes("ui.view_mix").len(), 1);
    }
    // No keystroke runs two of the routed commands in one context.
    let routed: Vec<&gpui::KeyBinding> = all
        .iter()
        .filter(|b| b.action().as_any().is::<menus::RunCommand>())
        .collect();
    for (n, a) in routed.iter().enumerate() {
        for b in &routed[..n] {
            let same_keys = format!("{:?}", a.keystrokes()) == format!("{:?}", b.keystrokes());
            let same_context = format!("{:?}", a.predicate()) == format!("{:?}", b.predicate());
            assert!(
                !(same_keys && same_context) || a.action().partial_eq(b.action()),
                "{:?} runs two commands",
                a.keystrokes()
            );
        }
    }
    for id in commands::VIEW_COMMANDS
        .iter()
        .chain(commands::MIXER_COMMANDS)
    {
        let bound = strokes(id).len();
        let chords = keymap.chords(id).len();
        assert!(bound <= chords, "{id}");
        if cfg!(target_os = "macos") {
            assert_eq!(bound, chords, "{id}");
        }
    }
}

/// Off macOS (`mac` false; run on every platform) ⌃1 and ⌃2 belong to ⌘1
/// and ⌘2: the View menu shows no key for the main views, and the
/// channel's ⌃1 runs what the keyboard's Ctrl+1 runs. On macOS nothing
/// changes.
#[test]
fn off_macos_the_command_keys_own_the_control_keys() {
    let keymap = Keymap::default();
    let c = |s: &str| Chord::parse(s).unwrap();
    assert_eq!(menus::owner(&c("⌃1"), &keymap, false), Some(c("⌘1")));
    assert_eq!(menus::owner(&c("⌃2"), &keymap, false), Some(c("⌘2")));
    assert_eq!(menus::owner(&c("⌃1"), &keymap, true), None);
    // A ⌘ chord, or a ⌃ chord no ⌘ chord shares, is its own.
    assert_eq!(menus::owner(&c("⌘1"), &keymap, false), None);
    assert_eq!(menus::owner(&c("⌃⌘←"), &keymap, false), None);
    assert_eq!(menus::owner(&c("⌃9"), &keymap, false), None);
    assert_eq!(menus::key_text("ui.view_arrange", &keymap, false), None);
    assert_eq!(menus::key_text("ui.view_mix", &keymap, false), None);
    assert_eq!(
        menus::key_text("ui.view_arrange", &keymap, true).as_deref(),
        Some("⌃1")
    );
    assert_eq!(
        menus::key_text("editors.piano_roll", &keymap, false).as_deref(),
        Some("⌘1")
    );
    assert_eq!(
        menus::key_text("ui.toggle_left", &keymap, false).as_deref(),
        Some("⌃⌘←")
    );
    // This platform's menu follows its rule.
    let m = menu_model(Some(&demo_doc()), &keymap);
    let mac = cfg!(target_os = "macos");
    assert_eq!(
        item(&m, "View", "Arrange View").2,
        menus::key_text("ui.view_arrange", &keymap, mac).as_deref()
    );
}

/// A document over the groups' mixer: Kick (2), Snare (3), Bass (4), Bus 1
/// (5); VCA 1 (9) holds Snare; Kick and Snare are in Drums (7) and
/// selected.
fn groups_doc() -> DocumentModel {
    use vizdaw_app::testing::{group_row, groups_fake, vca_row};
    let f = groups_fake(
        vec![vca_row(9, "VCA 1", &[3], 0.0)],
        vec![group_row(7, "Drums", &[2, 3])],
    );
    let mut d = DocumentModel::new(Arc::new(f)).expect("a document");
    assert!(d.ensure_mixer());
    d.set_selected_strips([2, 3].into());
    d
}

fn menu_of<'a>(m: &'a MenuModel, title: &str) -> &'a [ItemSpec] {
    &m.menus
        .iter()
        .find(|s| s.title == title)
        .unwrap_or_else(|| panic!("no menu {title}"))
        .items
}

#[test]
fn the_automation_menu_sets_the_selected_strips_and_suspends() {
    let keymap = Keymap::default();
    let mut doc = demo_doc();
    let m = menu_model(Some(&doc), &keymap);
    let items = menu_of(&m, "Automation");
    assert_eq!(items[0], ItemSpec::Heading("Selected Strips".into()));
    let modes: Vec<(&str, &str, bool)> = items[1..7]
        .iter()
        .map(|i| match i {
            ItemSpec::Command {
                id, title, enabled, ..
            } => (*id, title.as_str(), *enabled),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        modes,
        [
            ("automation.mode.off", "Off", false),
            ("automation.mode.read", "Read", false),
            ("automation.mode.touch", "Touch", false),
            ("automation.mode.latch", "Latch", false),
            ("automation.mode.write", "Write", false),
            ("automation.mode.trim", "Trim", false),
        ],
        "off without a strip selection"
    );
    assert_eq!(items[7], ItemSpec::Separator);
    let ItemSpec::Command {
        id,
        title,
        enabled,
        checked,
        ..
    } = &items[8]
    else {
        panic!("{:?}", items[8])
    };
    assert_eq!(
        (*id, title.as_str(), *enabled, *checked),
        (
            "automation.suspend",
            "Suspend Automation",
            true,
            Some(automation_suspended_now())
        )
    );
    assert_eq!(items.len(), 9);
    doc.set_selected_strips([0].into());
    let m = menu_model(Some(&doc), &keymap);
    assert!(matches!(
        menu_of(&m, "Automation")[4],
        ItemSpec::Command { enabled: true, .. }
    ));
}

fn automation_suspended_now() -> bool {
    vizdaw_ffi::automation::automation_suspended()
}

#[test]
fn the_mixer_menu_follows_the_selection_groups_and_vcas() {
    use vizdaw_gpui::menus::MixerOp;
    let keymap = Keymap::default();
    let mut doc = groups_doc();
    let m = menu_model(Some(&doc), &keymap);
    let mixer = |op, title: &str, enabled| ItemSpec::Mixer {
        op,
        title: title.into(),
        enabled,
    };
    assert_eq!(
        menu_of(&m, "Mixer"),
        [
            // Both are grouped already: Group is off; Ungroup takes Drums.
            mixer(MixerOp::Group, "Group", false),
            mixer(MixerOp::Ungroup, "Ungroup", true),
            ItemSpec::Separator,
            mixer(MixerOp::NewVca, "New VCA", true),
            ItemSpec::Submenu {
                title: "Assign to VCA".into(),
                enabled: true,
                items: vec![
                    mixer(MixerOp::Assign { vca: 9 }, "VCA 1", true),
                    ItemSpec::Separator,
                    mixer(MixerOp::NewVca, "New VCA", true),
                ],
            },
            ItemSpec::Submenu {
                title: "Remove from VCA".into(),
                enabled: true,
                items: vec![mixer(MixerOp::Unassign { vca: 9 }, "VCA 1", true)],
            },
        ]
    );
    // Bass alone: Group on, Ungroup off, VCA 1 takes it, none to remove.
    doc.set_selected_strips([4].into());
    let m = menu_of(&menu_model(Some(&doc), &keymap), "Mixer").to_vec();
    assert_eq!(m[0], mixer(MixerOp::Group, "Group", true));
    assert_eq!(m[1], mixer(MixerOp::Ungroup, "Ungroup", false));
    assert!(matches!(&m[5], ItemSpec::Submenu { enabled: false, items, .. } if items.is_empty()));
    // Snare alone: VCA 1 holds it, so Assign's VCA 1 is off.
    doc.set_selected_strips([3].into());
    let m = menu_of(&menu_model(Some(&doc), &keymap), "Mixer").to_vec();
    let ItemSpec::Submenu { items, .. } = &m[4] else {
        panic!()
    };
    assert_eq!(items[0], mixer(MixerOp::Assign { vca: 9 }, "VCA 1", false));
    // Nothing selected: Assign to VCA ▸ is off; New VCA stays on.
    doc.set_selected_strips(Default::default());
    let m = menu_of(&menu_model(Some(&doc), &keymap), "Mixer").to_vec();
    assert!(matches!(&m[4], ItemSpec::Submenu { enabled: false, .. }));
    assert_eq!(m[3], mixer(MixerOp::NewVca, "New VCA", true));
    // A document whose mixer was never made: New VCA is off (Swift's
    // `mixer == nil`).
    let plain = demo_doc();
    assert!(plain.mixer().is_none());
    let m = menu_of(&menu_model(Some(&plain), &keymap), "Mixer").to_vec();
    assert_eq!(m[3], mixer(MixerOp::NewVca, "New VCA", false));
    // No document: all off.
    let m = menu_of(&menu_model(None, &keymap), "Mixer").to_vec();
    assert_eq!(m[3], mixer(MixerOp::NewVca, "New VCA", false));
    assert_eq!(m[0], mixer(MixerOp::Group, "Group", false));
}

// ---------------------------------------------------------------------
// The File menu (slice 7b Task 3; Swift `AppCommands`'s File groups)

/// One File menu row: a command's (id, title, keys, enabled), a
/// submenu's title and enabled with its rows, or a separator.
#[derive(Debug, PartialEq)]
enum Row {
    Item(&'static str, String, Option<String>, bool),
    Sub(String, bool, Vec<Row>),
    Sep,
}

fn rows(items: &[ItemSpec]) -> Vec<Row> {
    items
        .iter()
        .map(|i| match i {
            ItemSpec::Command {
                id,
                title,
                keys,
                enabled,
                checked,
            } => {
                assert_eq!(*checked, None, "{id} is not a toggle");
                Row::Item(id, title.clone(), keys.clone(), *enabled)
            }
            ItemSpec::Submenu {
                title,
                enabled,
                items,
            } => Row::Sub(title.clone(), *enabled, rows(items)),
            ItemSpec::Separator => Row::Sep,
            other => panic!("unexpected {other:?}"),
        })
        .collect()
}

fn file_rows(front: Option<&DocumentModel>) -> Vec<Row> {
    let m = menu_model(front, &Keymap::default());
    rows(menu_of(&m, "File"))
}

/// The keys `id` shows: the registry's shortcut.
fn keys(id: &str) -> Option<String> {
    registry::find(id)
        .and_then(|c| c.shortcut)
        .map(|s| Chord::parse(s).unwrap().text())
}

/// The File menu Swift shows, with `on(id)` saying which items are
/// enabled and `export` whether the Export submenu opens.
fn swift_file_menu(on: impl Fn(&str) -> bool, export: bool) -> Vec<Row> {
    let item = |id: &'static str, title: &str, shows_keys: bool| {
        Row::Item(
            id,
            title.to_owned(),
            if shows_keys { keys(id) } else { None },
            on(id),
        )
    };
    vec![
        item("file.new", "New", true),
        item("file.open", "Open\u{2026}", true),
        item("file.open_copy", "Open Co-Producer Copy\u{2026}", true),
        item("media.import", "Import Audio\u{2026}", true),
        item("file.import_midi", "Import MIDI File\u{2026}", false),
        Row::Sub(
            "Import Project".into(),
            true,
            vec![
                item("file.import_dawproject", "DAWproject\u{2026}", false),
                item("file.import_aaf", "AAF\u{2026}", false),
            ],
        ),
        item("file.create_demo", "Create Demo Project", false),
        item(
            "file.create_demo_audio",
            "Create Demo Project with Audio",
            false,
        ),
        Row::Sep,
        item("file.close", "Close", true),
        item("file.save", "Save", true),
        item("file.save_as", "Save As\u{2026}", true),
        Row::Sep,
        item("file.project_settings", "Project Settings\u{2026}", false),
        item("file.bounce", "Bounce\u{2026}", false),
        Row::Sub(
            "Export".into(),
            export,
            vec![
                item("file.export_dawproject", "DAWproject\u{2026}", false),
                item("file.export_aaf", "AAF\u{2026}", false),
                item("file.export_midi", "MIDI File\u{2026}", false),
                item("file.export_midi_clip", "MIDI Clip File\u{2026}", false),
                item("file.export_for_daw", "For Another DAW\u{2026}", false),
            ],
        ),
        item(
            "file.interchange_report",
            "Last Import or Export Report\u{2026}",
            false,
        ),
        item("media.relink", "Relink Media\u{2026}", true),
    ]
}

/// Always on, with or without a document.
const ALWAYS: [&str; 7] = [
    "file.new",
    "file.open",
    "file.import_dawproject",
    "file.import_aaf",
    "file.create_demo",
    "file.create_demo_audio",
    "file.close",
];

/// On with a document whatever its state.
const WITH_DOC: [&str; 11] = [
    "media.import",
    "file.import_midi",
    "file.save",
    "file.save_as",
    "file.project_settings",
    "file.bounce",
    "file.export_dawproject",
    "file.export_aaf",
    "file.export_midi",
    "file.export_midi_clip",
    "file.export_for_daw",
];

fn media(id: u64, offline: Option<vizdaw_ffi::OfflineReasonRow>) -> vizdaw_ffi::MediaRow {
    vizdaw_ffi::MediaRow {
        id,
        name: format!("m{id}.wav"),
        path: format!("Audio/m{id}.caf"),
        channels: 2,
        frames: 48_000,
        sample_rate: 48_000,
        origin: vizdaw_ffi::MediaOriginRow::Imported,
        original_name: None,
        offline,
        uses: 1,
        used_percent: 100.0,
        peak: None,
    }
}

#[test]
fn the_file_menu_has_swifts_items_titles_keys_and_submenus_with_no_document() {
    assert_eq!(
        file_rows(None),
        swift_file_menu(|id| ALWAYS.contains(&id), false)
    );
    // Slice 1's own titles and ID are gone.
    let m = menu_model(None, &Keymap::default());
    for r in rows(menu_of(&m, "File")) {
        if let Row::Item(id, title, ..) = r {
            assert_ne!(id, "file.new_demo");
            assert!(title != "New Demo Project" && title != "Close Window");
            assert!(registry::find(id).is_some(), "{id} is a registry ID");
        }
    }
    // Swift's keys: ⌘N, ⌘O, ⇧⌘I, ⌘W, ⌘S, ⇧⌘S.
    assert_eq!(keys("media.import").as_deref(), Some("⇧⌘I"));
    assert_eq!(keys("file.close").as_deref(), Some("⌘W"));
}

#[test]
fn the_file_menu_with_a_document_and_each_state_enables_as_swift_does() {
    use vizdaw_ffi::OfflineReasonRow as Off;
    use vizdaw_ffi::session::CopiesSizeRow;
    let base = |id: &str| ALWAYS.contains(&id) || WITH_DOC.contains(&id);
    // An untitled document.
    let d = demo_doc();
    assert_eq!(file_rows(Some(&d)), swift_file_menu(base, true));

    // Copies, untitled: Open Co-Producer Copy… stays off; saved: on.
    let copies = CopiesSizeRow {
        count: 2,
        bytes: 10,
    };
    let f = Arc::new(demo_fake());
    f.set_copies_size(copies.clone());
    assert_eq!(file_rows(Some(&doc_of(&f))), swift_file_menu(base, true));
    let f = Arc::new(demo_fake());
    f.set_path(Some("/m/Song.vizdaw"));
    f.set_copies_size(copies);
    assert_eq!(
        file_rows(Some(&doc_of(&f))),
        swift_file_menu(|id| base(id) || id == "file.open_copy", true)
    );
    // Saved, no copies.
    let f = Arc::new(demo_fake());
    f.set_path(Some("/m/Song.vizdaw"));
    assert_eq!(file_rows(Some(&doc_of(&f))), swift_file_menu(base, true));

    // A report the UI kept.
    let f = Arc::new(demo_fake());
    let mut d = doc_of(&f);
    f.script_job(
        vizdaw_app::testing::JobScript::done().report(vizdaw_ffi::ReportRow {
            summary: "Exported".into(),
            items: vec![],
        }),
    );
    let args = vec![(
        "path".to_owned(),
        core_control::registry::ArgValue::Text("/m/a.mid".into()),
    )];
    let Ok(vizdaw_app::interchange_commands::InterchangeRequest::Job(job)) =
        vizdaw_app::interchange_commands::request("file.export_midi", &args, &d)
    else {
        panic!("an export job")
    };
    d.start_job(job.kind(), job.refusal_title(), |s| job.start(s));
    d.poll_jobs();
    assert_eq!(
        file_rows(Some(&d)),
        swift_file_menu(|id| base(id) || id == "file.interchange_report", true)
    );

    // Relinkable media; another format is not; a held merge holds it.
    let f = Arc::new(demo_fake());
    f.set_media(vec![media(1, Some(Off::Format))]);
    assert_eq!(file_rows(Some(&doc_of(&f))), swift_file_menu(base, true));
    let f = Arc::new(demo_fake());
    f.set_media(vec![media(1, None), media(2, Some(Off::Missing))]);
    let mut d = doc_of(&f);
    assert_eq!(
        file_rows(Some(&d)),
        swift_file_menu(|id| base(id) || id == "media.relink", true)
    );
    d.set_merge_held(true);
    assert_eq!(file_rows(Some(&d)), swift_file_menu(base, true));
}

fn doc_of(f: &Arc<vizdaw_app::testing::FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

#[test]
fn file_items_dispatch_their_actions() {
    use vizdaw_gpui::menus::action_for;
    let is = |id: &str, a: &dyn gpui::Action| action_for(id).partial_eq(a);
    assert!(is("file.new", &vizdaw_gpui::NewDocument));
    assert!(is("file.open", &vizdaw_gpui::OpenDocument));
    assert!(is("file.create_demo", &vizdaw_gpui::NewDemo));
    assert!(is("file.close", &vizdaw_gpui::CloseWindow));
    assert!(is("file.save", &vizdaw_gpui::Save));
    assert!(is("file.save_as", &vizdaw_gpui::SaveAs));
    for id in menus::APP_FILE_COMMANDS {
        assert!(
            is(id, &vizdaw_gpui::AppFileCommand { id: (*id).into() }),
            "{id}"
        );
    }
    for id in [
        "file.open_copy",
        "media.import",
        "file.import_midi",
        "file.project_settings",
        "file.bounce",
        "file.export_dawproject",
        "file.export_aaf",
        "file.export_midi",
        "file.export_midi_clip",
        "file.export_for_daw",
        "file.interchange_report",
        "media.relink",
    ] {
        assert!(is(id, &vizdaw_gpui::FileCommand { id: id.into() }), "{id}");
    }
}

#[test]
fn import_audio_is_bound_to_its_registry_key() {
    let keymap = Keymap::default();
    let chord = Chord::parse("⇧⌘I").unwrap();
    let os = if cfg!(target_os = "macos") {
        gpui::OperatingSystem::Mac
    } else {
        gpui::OperatingSystem::Linux
    };
    let want = gpui::Keystroke::parse_with_os(&keystroke(&chord).unwrap(), os).unwrap();
    let context = KeyBindingContextPredicate::parse(menus::WINDOW_CONTEXT).unwrap();
    let bound: Vec<_> = menus::bindings(&keymap)
        .into_iter()
        .filter(|b| {
            b.action().partial_eq(&vizdaw_gpui::FileCommand {
                id: "media.import".into(),
            })
        })
        .collect();
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].keystrokes()[0].inner(), &want);
    assert_eq!(bound[0].predicate().as_deref(), Some(&context));
}

/// What the File items do: New asks for the format first (a sheet on the
/// front window, a dialog window with none), the imports show their
/// panel and run, the demos and Import Project need no document, and the
/// items whose sheets come later do nothing.
mod file_flows {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::Arc;

    use core_control::ControlError;
    use core_control::server::Call;
    use futures::channel::oneshot;
    use gpui::{AnyWindowHandle, App, Modifiers, TestAppContext, VisualTestContext, WindowHandle};
    use serde_json::Value;
    use vizdaw_app::DocumentStore;
    use vizdaw_app::panels::PanelSpec;
    use vizdaw_app::testing::{FakeCore, FakeSession, JobRequest, demo_fake};
    use vizdaw_ffi::{BitDepthRow, JobKindRow, ProjectFormatRow};
    use vizdaw_gpui::control::{self, CallSource};
    use vizdaw_gpui::file_panels::{Chosen, PanelBackend, PanelPlace, set_backend};
    use vizdaw_gpui::sheet_host::SheetDialog;
    use vizdaw_gpui::{AppFileCommand, AppState, DocumentWindow, FileCommand, NewDocument};

    /// Panels a test answers itself.
    #[derive(Default)]
    struct FakePanels {
        shown: RefCell<Vec<(&'static str, oneshot::Sender<Chosen>)>>,
    }

    impl PanelBackend for FakePanels {
        fn show(
            &self,
            spec: &PanelSpec,
            _: &PanelPlace,
            _: Option<AnyWindowHandle>,
            done: oneshot::Sender<Chosen>,
            _: &mut App,
        ) {
            self.shown.borrow_mut().push((spec.row, done));
        }
    }

    impl FakePanels {
        fn rows(&self) -> Vec<&'static str> {
            self.shown.borrow().iter().map(|(r, _)| *r).collect()
        }

        fn answer(&self, paths: &[&str]) {
            let (_, done) = self.shown.borrow_mut().remove(0);
            done.send(Ok(Some(paths.iter().map(PathBuf::from).collect())))
                .unwrap();
        }
    }

    struct Setup {
        core: Arc<FakeCore>,
        fake: Arc<FakeSession>,
        window: WindowHandle<DocumentWindow>,
        panels: Rc<FakePanels>,
        cx: VisualTestContext,
    }

    fn setup(cx: &mut TestAppContext) -> Setup {
        let core = Arc::new(FakeCore::new());
        let fake = Arc::new(demo_fake());
        core.hand_out("new_demo", fake.clone());
        let store = DocumentStore::with_core(core.clone());
        let panels = Rc::new(FakePanels::default());
        let backend: Rc<dyn PanelBackend> = panels.clone();
        let window = cx.update(|cx| {
            vizdaw_gpui::install(store, cx);
            set_backend(backend, cx);
            vizdaw_gpui::new_demo(cx).expect("the demo's window")
        });
        let vcx = VisualTestContext::from_window(window.into(), cx);
        vcx.run_until_parked();
        Setup {
            core,
            fake,
            window,
            panels,
            cx: vcx,
        }
    }

    fn sheet_label(s: &mut Setup) -> Option<String> {
        s.window.read_with(&s.cx, |v, _| v.sheet_label()).unwrap()
    }

    fn node(s: &mut Setup, role: &str, label: &str) -> Option<Value> {
        let tree = s
            .window
            .read_with(&s.cx, |v, _| v.registry().borrow().tree())
            .unwrap();
        tree["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["role"] == role && n["label"] == label)
            .cloned()
    }

    fn click(s: &mut Setup, role: &str, label: &str) {
        let id = node(s, role, label).unwrap_or_else(|| panic!("no {role} {label}"))["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let at = s
            .window
            .read_with(&s.cx, |v, _| {
                v.registry().borrow().resolve(&id).unwrap().bounds.center()
            })
            .unwrap();
        s.cx.simulate_click(at, Modifiers::none());
        s.cx.run_until_parked();
    }

    fn made(s: &Setup) -> Vec<String> {
        s.core.made().into_iter().map(|(c, _)| c).collect()
    }

    fn new_project_calls(s: &Setup) -> Vec<String> {
        s.core
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("new_project("))
            .collect()
    }

    fn default_format(s: &mut Setup) -> (ProjectFormatRow, ProjectFormatRow) {
        s.cx.update(|_, cx| {
            let a = AppState::global(cx);
            let a = a.read(cx);
            (a.store.default_format(), a.prefs.default_format())
        })
    }

    fn windows(s: &mut Setup) -> usize {
        s.cx.update(|_, cx| AppState::global(cx).read(cx).windows().len())
    }

    #[gpui::test]
    fn new_asks_for_the_format_on_the_front_window_and_remembers_it(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        s.cx.dispatch_action(NewDocument);
        s.cx.run_until_parked();
        assert_eq!(sheet_label(&mut s).as_deref(), Some("New Project"));
        assert!(new_project_calls(&s).is_empty(), "asked before made");
        assert!(node(&mut s, "text", "New Project").is_some());
        assert!(
            node(
                &mut s,
                "text",
                "Every audio file of the project is stored in this format."
            )
            .is_some()
        );
        // It starts at the default format, 48 kHz / 24-bit.
        let rate = node(&mut s, "menu", "Sample rate").unwrap();
        assert_eq!(rate["value"], "48 kHz");
        assert_eq!(
            node(&mut s, "menu", "Bit depth").unwrap()["value"],
            "24-bit"
        );
        assert_eq!(node(&mut s, "button", "Create").unwrap()["enabled"], true);
        assert!(node(&mut s, "button", "Cancel").is_some());
        // Its items, the chosen one selected.
        click(&mut s, "menu", "Sample rate");
        for r in vizdaw_ffi::project_rates() {
            let title = vizdaw_app::formats::rate_title(r);
            let n = node(&mut s, "menuitem", &title).unwrap_or_else(|| panic!("{title}"));
            assert_eq!(n["selected"] == true, r == 48_000, "{title}");
        }
        click(&mut s, "menuitem", "44.1 kHz");
        assert!(node(&mut s, "menuitem", "88.2 kHz").is_none(), "it closes");
        assert_eq!(
            node(&mut s, "menu", "Sample rate").unwrap()["value"],
            "44.1 kHz"
        );
        click(&mut s, "menu", "Bit depth");
        for t in ["16-bit", "24-bit", "32-bit float"] {
            assert!(node(&mut s, "menuitem", t).is_some(), "{t}");
        }
        click(&mut s, "menuitem", "32-bit float");
        let before = windows(&mut s);
        // Return is Create.
        s.cx.simulate_keystrokes("enter");
        s.cx.run_until_parked();
        assert_eq!(sheet_label(&mut s), None);
        assert_eq!(new_project_calls(&s), ["new_project(44100,Float32)"]);
        assert_eq!(windows(&mut s), before + 1, "the project's window");
        let chosen = ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Float32,
        };
        assert_eq!(default_format(&mut s), (chosen, chosen));
        // The next New starts at it.
        s.cx.dispatch_action(NewDocument);
        s.cx.run_until_parked();
        let front = s.cx.update(|_, cx| cx.active_window()).unwrap();
        let front = front.downcast::<DocumentWindow>().unwrap();
        let labels = front
            .read_with(&s.cx, |v, _| {
                let r = v.registry().borrow();
                let rate = r
                    .elements()
                    .iter()
                    .find(|e| e.role == "menu" && e.label == "Sample rate")
                    .and_then(|e| e.value.clone());
                (v.sheet_label(), rate)
            })
            .unwrap();
        assert_eq!(
            labels,
            (Some("New Project".into()), Some("44.1 kHz".into()))
        );
    }

    #[gpui::test]
    fn cancel_makes_nothing_and_a_second_new_waits_for_the_first(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        s.cx.dispatch_action(NewDocument);
        s.cx.run_until_parked();
        // The sheet keeps the window's focus; ⌘N reaches the app anyway.
        s.cx.simulate_keystrokes("secondary-n");
        s.cx.run_until_parked();
        assert!(dialogs(&mut s.cx).is_empty(), "no second panel");
        assert_eq!(windows(&mut s), 1);
        s.cx.simulate_keystrokes("escape");
        s.cx.run_until_parked();
        assert_eq!(sheet_label(&mut s), None);
        assert!(new_project_calls(&s).is_empty());
        assert_eq!(windows(&mut s), 1);
        // Answered: New asks again.
        s.cx.dispatch_action(NewDocument);
        s.cx.run_until_parked();
        assert_eq!(sheet_label(&mut s).as_deref(), Some("New Project"));
        click(&mut s, "button", "Create");
        assert_eq!(new_project_calls(&s), ["new_project(48000,Int24)"]);
    }

    fn menus_on(s: &mut Setup) -> bool {
        fn on(i: &vizdaw_gpui::menus::ItemSpec) -> bool {
            use vizdaw_gpui::menus::ItemSpec;
            match i {
                ItemSpec::Command { enabled, .. } | ItemSpec::Mixer { enabled, .. } => *enabled,
                ItemSpec::Submenu { enabled, items, .. } => *enabled || items.iter().any(on),
                _ => false,
            }
        }
        s.cx.update(|_, cx| {
            vizdaw_gpui::menus::shown(cx)
                .unwrap()
                .menus
                .iter()
                .any(|m| m.items.iter().any(on))
        })
    }

    /// Swift's format alert was app-modal: while the New Project format
    /// panel is up (as a sheet or as its dialog window) every menu item
    /// is off.
    #[gpui::test]
    fn the_menus_are_off_while_the_format_panel_is_up(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        assert!(menus_on(&mut s));
        s.cx.dispatch_action(NewDocument);
        s.cx.run_until_parked();
        assert_eq!(sheet_label(&mut s).as_deref(), Some("New Project"));
        assert!(!menus_on(&mut s), "the sheet");
        s.cx.simulate_keystrokes("escape");
        s.cx.run_until_parked();
        assert!(menus_on(&mut s));
        // The dialog window (the front window shows a sheet).
        s.window
            .update(&mut s.cx, |v, window, cx| {
                let ask = vizdaw_gpui::sheet_host::Ask::alert(gpui::PromptLevel::Info, "Busy", "");
                let (sheet, _) = vizdaw_gpui::sheet_host::AskSheet::new(ask);
                assert!(v.show_sheet(Box::new(sheet), window, cx).is_ok());
            })
            .unwrap();
        s.cx.dispatch_action(NewDocument);
        s.cx.run_until_parked();
        let shown = dialogs(&mut s.cx);
        assert_eq!(shown.len(), 1);
        assert!(!menus_on(&mut s), "the dialog window");
        let mut dcx = VisualTestContext::from_window(shown[0].into(), &s.cx);
        dcx.simulate_keystrokes("escape");
        dcx.run_until_parked();
        assert!(menus_on(&mut s));
    }

    /// With no window open nothing draws: the panel's coming and going
    /// sets the menus itself.
    #[gpui::test]
    fn with_no_window_the_menus_are_off_while_the_format_dialog_is_up(cx: &mut TestAppContext) {
        let s = setup(cx);
        let window = s.window;
        drop(s);
        window
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        let on = |cx: &mut TestAppContext| {
            cx.update(|cx| {
                vizdaw_gpui::menus::shown(cx)
                    .unwrap()
                    .menus
                    .iter()
                    .flat_map(|m| &m.items)
                    .any(|i| {
                        matches!(
                            i,
                            vizdaw_gpui::menus::ItemSpec::Command { enabled: true, .. }
                        )
                    })
            })
        };
        assert!(on(cx));
        cx.update(vizdaw_gpui::file_menu::new_project);
        cx.run_until_parked();
        let shown = dialogs(cx);
        assert_eq!(shown.len(), 1);
        assert!(!on(cx));
        let mut dcx = VisualTestContext::from_window(shown[0].into(), cx);
        dcx.simulate_keystrokes("escape");
        dcx.run_until_parked();
        assert!(on(cx));
    }

    fn dialogs(cx: &mut TestAppContext) -> Vec<WindowHandle<SheetDialog>> {
        cx.update(|cx| {
            cx.windows()
                .into_iter()
                .filter_map(|w| w.downcast::<SheetDialog>())
                .collect()
        })
    }

    #[gpui::test]
    fn with_no_window_open_the_format_panel_is_a_dialog_window(cx: &mut TestAppContext) {
        let s = setup(cx);
        let core = s.core.clone();
        let window = s.window;
        drop(s);
        window
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        assert_eq!(
            cx.update(|cx| AppState::global(cx).read(cx).windows().len()),
            0
        );
        // What File › New's global handler runs with no window open.
        cx.update(vizdaw_gpui::file_menu::new_project);
        cx.run_until_parked();
        let shown = dialogs(cx);
        assert_eq!(shown.len(), 1, "the dialog window");
        let label = shown[0].read_with(cx, |d, _| d.label()).unwrap();
        assert_eq!(label.as_deref(), Some("New Project"));
        let has_rate = shown[0]
            .read_with(cx, |d, _| {
                d.registry()
                    .borrow()
                    .elements()
                    .iter()
                    .any(|e| e.role == "menu" && e.label == "Sample rate")
            })
            .unwrap();
        assert!(has_rate);
        assert!(core.calls().iter().all(|c| !c.starts_with("new_project")));
        let mut dcx = VisualTestContext::from_window(shown[0].into(), cx);
        dcx.simulate_keystrokes("enter");
        dcx.run_until_parked();
        assert!(dialogs(cx).is_empty(), "it closed");
        assert!(
            core.calls()
                .contains(&"new_project(48000,Int24)".to_owned())
        );
        assert_eq!(
            cx.update(|cx| AppState::global(cx).read(cx).windows().len()),
            1
        );
    }

    #[gpui::test]
    fn closing_the_dialog_window_cancels(cx: &mut TestAppContext) {
        let s = setup(cx);
        let core = s.core.clone();
        let window = s.window;
        drop(s);
        window
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        // What File › New's global handler runs with no window open.
        cx.update(vizdaw_gpui::file_menu::new_project);
        cx.run_until_parked();
        let shown = dialogs(cx);
        let mut dcx = VisualTestContext::from_window(shown[0].into(), cx);
        // The window's close button: Cancel.
        assert!(dcx.simulate_close());
        dcx.run_until_parked();
        assert!(core.calls().iter().all(|c| !c.starts_with("new_project")));
        assert!(!cx.update(|cx| vizdaw_gpui::file_menu::format_panel_up(cx)));
        shown[0]
            .update(cx, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        // Answered: New asks again; Esc is Cancel too.
        cx.update(vizdaw_gpui::file_menu::new_project);
        cx.run_until_parked();
        let shown = dialogs(cx);
        assert_eq!(shown.len(), 1);
        let mut dcx = VisualTestContext::from_window(shown[0].into(), cx);
        dcx.simulate_keystrokes("escape");
        dcx.run_until_parked();
        assert!(dialogs(cx).is_empty());
        assert!(core.calls().iter().all(|c| !c.starts_with("new_project")));
        assert!(!cx.update(|cx| vizdaw_gpui::file_menu::format_panel_up(cx)));
    }

    #[gpui::test]
    fn a_front_window_showing_a_sheet_gets_the_dialog_window(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        s.cx.dispatch_action(NewDocument);
        s.cx.run_until_parked();
        click(&mut s, "button", "Cancel");
        // Another sheet is up on the front window.
        s.window
            .update(&mut s.cx, |v, window, cx| {
                let ask = vizdaw_gpui::sheet_host::Ask::alert(gpui::PromptLevel::Info, "Busy", "");
                let (sheet, _) = vizdaw_gpui::sheet_host::AskSheet::new(ask);
                assert!(v.show_sheet(Box::new(sheet), window, cx).is_ok());
            })
            .unwrap();
        s.cx.dispatch_action(NewDocument);
        s.cx.run_until_parked();
        assert_eq!(sheet_label(&mut s).as_deref(), Some("Busy"));
        let shown = dialogs(&mut s.cx);
        assert_eq!(shown.len(), 1);
    }

    struct NoCalls;

    impl CallSource for NoCalls {
        fn next_call(&self) -> Option<Call> {
            None
        }
        fn reply(&self, _: u64, _: Result<Value, ControlError>) -> bool {
            true
        }
    }

    #[gpui::test]
    fn under_the_control_channel_new_asks_nothing_and_uses_the_default(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        s.cx.update(|_, cx| control::pump(&NoCalls, cx));
        s.cx.dispatch_action(NewDocument);
        s.cx.run_until_parked();
        assert_eq!(sheet_label(&mut s), None);
        assert!(dialogs(&mut s.cx).is_empty());
        assert_eq!(new_project_calls(&s), ["new_project(48000,Int24)"]);
    }

    #[gpui::test]
    fn import_audio_shows_its_panel_and_imports_the_chosen_files(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        s.cx.simulate_keystrokes("secondary-shift-i");
        s.cx.run_until_parked();
        assert_eq!(s.panels.rows(), ["7"]);
        s.panels.answer(&["/m/a.wav", "/m/b.wav"]);
        s.cx.run_until_parked();
        let started = s.fake.jobs_started();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].kind, JobKindRow::Import);
        let JobRequest::Import { paths, .. } = &started[0].request else {
            panic!("an import: {:?}", started[0].request)
        };
        assert_eq!(paths, &["/m/a.wav".to_owned(), "/m/b.wav".to_owned()]);
    }

    #[gpui::test]
    fn import_midi_shows_its_panel_and_reads_the_file(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        s.cx.dispatch_action(FileCommand {
            id: "file.import_midi".into(),
        });
        s.cx.run_until_parked();
        assert_eq!(s.panels.rows(), ["3"]);
        s.panels.answer(&["/m/Song.mid"]);
        s.cx.run_until_parked();
        // Swift `beginImport`: the file is read (here it cannot be: the
        // alert "Import Failed"); the sheet is slice 7c's.
        assert_eq!(s.fake.reads("midi_file_info"), 1);
        let prompt = s.cx.pending_prompt().map(|p| p.0);
        assert_eq!(prompt.as_deref(), Some("Import Failed"));
    }

    #[gpui::test]
    fn import_project_opens_the_chosen_file_as_a_new_project(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        for (id, row, path, read) in [
            (
                "file.import_dawproject",
                "11",
                "/m/Song.dawproject",
                "dawproject_info(/m/Song.dawproject)",
            ),
            (
                "file.import_aaf",
                "14",
                "/m/Song.aaf",
                "aaf_info(/m/Song.aaf)",
            ),
        ] {
            s.cx.dispatch_action(AppFileCommand { id: id.into() });
            s.cx.run_until_parked();
            assert_eq!(s.panels.rows(), [row], "{id}");
            s.panels.answer(&[path]);
            s.cx.run_until_parked();
            assert!(s.core.calls().contains(&read.to_owned()), "{id}");
            // The file cannot be read here: its alert.
            if s.cx.has_pending_prompt() {
                s.cx.simulate_prompt_answer("OK");
                s.cx.run_until_parked();
            }
        }
    }

    #[gpui::test]
    fn the_demos_need_no_document(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        s.cx.dispatch_action(vizdaw_gpui::NewDemo);
        s.cx.run_until_parked();
        s.cx.dispatch_action(AppFileCommand {
            id: "file.create_demo_audio".into(),
        });
        s.cx.run_until_parked();
        assert_eq!(made(&s), ["new_demo", "new_demo", "new_demo_audio"]);
        assert_eq!(windows(&mut s), 3);
    }

    #[gpui::test]
    fn the_items_whose_sheets_come_later_do_nothing_yet(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        for id in vizdaw_gpui::file_menu::LATER {
            s.cx.dispatch_action(FileCommand { id: (*id).into() });
            s.cx.run_until_parked();
        }
        assert!(s.panels.rows().is_empty());
        assert_eq!(sheet_label(&mut s), None);
        assert!(!s.cx.has_pending_prompt());
        assert!(s.fake.jobs_started().is_empty());
    }

    #[gpui::test]
    fn the_in_window_menu_bar_opens_import_projects_items(cx: &mut TestAppContext) {
        let mut s = setup(cx);
        s.window
            .update(&mut s.cx, |v, _, cx| v.set_menu_bar_shown(true, cx))
            .unwrap();
        s.cx.run_until_parked();
        click(&mut s, "menu", "File");
        assert_eq!(
            node(&mut s, "menuitem", "Create Demo Project").unwrap()["enabled"],
            true
        );
        assert_eq!(
            node(&mut s, "menuitem", "Relink Media\u{2026}").unwrap()["enabled"],
            false
        );
        click(&mut s, "menuitem", "Import Project");
        click(&mut s, "menuitem", "DAWproject\u{2026}");
        assert_eq!(s.panels.rows(), ["11"]);
    }
}
