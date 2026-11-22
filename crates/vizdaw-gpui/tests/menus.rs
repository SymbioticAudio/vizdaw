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
                "Editors",
                "Track",
                "Transport",
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
            ItemSpec::Separator => None,
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
    let track: Vec<Option<&str>> = m.menus[4]
        .items
        .iter()
        .map(|i| match i {
            ItemSpec::Command { id, .. } => Some(*id),
            ItemSpec::Separator => None,
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
    assert!(m.menus[6].items.is_empty());
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
        // A text field's own editing keys act only inside it.
        if b.action().name().starts_with("text_input::") {
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
            ItemSpec::Separator => None,
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
