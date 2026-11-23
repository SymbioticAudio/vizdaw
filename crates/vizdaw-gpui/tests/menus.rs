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
        // Recording, the click, overdub, punch and arming come later.
        let later = matches!(
            id,
            "transport.record"
                | "transport.overdub"
                | "transport.punch"
                | "transport.click"
                | "transport.go_punch"
                | "transport.arm_previous"
                | "transport.arm_next"
        );
        if later {
            assert!(!enabled, "{id} is disabled");
        }
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
    // No document: all off.
    let m = menu_of(&menu_model(None, &keymap), "Mixer").to_vec();
    assert_eq!(m[3], mixer(MixerOp::NewVca, "New VCA", false));
    assert_eq!(m[0], mixer(MixerOp::Group, "Group", false));
}
