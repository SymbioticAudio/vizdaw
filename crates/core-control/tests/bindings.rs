//! Key bindings (plan 4f Task 2): chords, defaults from the registry,
//! contexts, binding and resetting, and the bindings document.

use core_control::bindings::{
    BindError, COMMAND, Chord, Keymap, MAX_CHORDS, MAX_JSON, SHIFT, default_chords,
};
use core_control::registry::{COMMANDS, CommandSpec, KeyContext};
use proptest::prelude::*;
use serde_json::json;

fn c(text: &str) -> Chord {
    Chord::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

const fn spec(
    id: &'static str,
    shortcut: Option<&'static str>,
    extra_keys: &'static [&'static str],
    context: KeyContext,
) -> CommandSpec {
    CommandSpec {
        id,
        menu: "Test",
        title: id,
        shortcut,
        extra_keys,
        menu_item: false,
        context,
        document: true,
        params: &[],
        dialogs: &[],
    }
}

/// A global locator and marker key, and step entry keys on the same keys.
static TABLE: &[CommandSpec] = &[
    spec("locator.go_1", Some("1"), &[], KeyContext::Global),
    spec("transport.next", Some("."), &[], KeyContext::Global),
    spec("transport.stop", Some("S"), &[], KeyContext::Global),
    spec("step.whole", Some("1"), &["Keypad1"], KeyContext::StepEntry),
    spec("step.dot", Some("."), &["Keypad."], KeyContext::StepEntry),
    spec("step.rest", None, &[], KeyContext::StepEntry),
];

/// Every chord is held by at most one command per context.
fn no_collisions(k: &Keymap) -> bool {
    let mut seen = std::collections::BTreeSet::new();
    k.table().iter().all(|s| {
        k.chords(s.id)
            .into_iter()
            .all(|ch| seen.insert((s.context, ch)))
    })
}

#[test]
fn chords_read_and_write_as_menus_show_them() {
    let save_as = c("⇧⌘S");
    assert_eq!(
        save_as,
        Chord {
            code: 1,
            mods: SHIFT | COMMAND
        }
    );
    assert_eq!(save_as.text(), "⇧⌘S");
    assert_eq!(c("⌘⇧s"), save_as, "any order, any case");
    assert_eq!(c("⌥⇧1").text(), "⌥⇧1");
    assert_eq!(c("left"), c("←"));
    assert_eq!(c("esc"), c("Escape"));
    assert_eq!(c("Keypad1").code, 83);
    assert_eq!(c("keypad.").code, 65);
    assert_eq!(c("F5").code, 96);
    assert_eq!(c("space").text(), "Space");
    for bad in ["", "⌘", "⌘⌘S", "Hyper", "SS", "⌘ S S", "Keypad", "F13"] {
        assert!(Chord::parse(bad).is_err(), "{bad:?}");
    }
    // Every key VizDAW binds, with every modifier set, round-trips.
    for code in 0..=200u16 {
        for mods in 0..16u8 {
            if let Some(ch) = Chord::new(code, mods) {
                assert_eq!(Chord::parse(&ch.text()), Ok(ch), "{}", ch.text());
            }
        }
    }
    assert_eq!(Chord::new(0xFFFF, 0), None, "a key no US key types");
    assert_eq!(Chord::new(0, 0xFF).map(|x| x.mods), Some(15));
    assert_eq!(c("⌘S").menu_key(), "s");
    assert_eq!(c("Space").menu_key(), " ");
    assert_eq!(c("←").menu_key(), "\u{f702}");
    assert_eq!(c("F5").menu_key(), "\u{f708}");
    assert_eq!(c("F").menu_key(), "f");
    assert_eq!(c("Keypad1").menu_key(), "1");
    assert_eq!(c("Delete").menu_key(), "\u{8}");
    assert_eq!(c("ForwardDelete").menu_key(), "\u{f728}");
    assert_ne!(c("ForwardDelete").menu_key(), c("Delete").menu_key());
    assert_eq!(Chord::of_slot("a", 8), Some(c("⌘A")));
    assert_eq!(Chord::of_slot("F5", 4 | 16), Some(c("⌥F5")));
    assert_eq!(Chord::of_slot("nope", 0), None);
    assert!(c("1").number_row() && c("0").number_row());
    assert!(!c("Keypad1").number_row() && !c("Q").number_row());
}

#[test]
fn the_registrys_defaults_parse_and_never_collide() {
    let k = Keymap::default();
    for s in COMMANDS {
        let n = usize::from(s.shortcut.is_some()) + s.extra_keys.len();
        assert_eq!(
            default_chords(s).len(),
            n,
            "{}: a default does not parse",
            s.id
        );
        for ch in default_chords(s) {
            assert_eq!(
                k.holder(ch, s.context),
                Some(s.id),
                "{} {}",
                s.id,
                ch.text()
            );
        }
    }
    assert!(no_collisions(&k));
    assert_eq!(
        k.lookup(c("Space"), false, false),
        Some("transport.play_stop")
    );
    assert_eq!(k.lookup(c("⌘S"), true, true), Some("file.save"));
    assert_eq!(
        k.lookup(c("⌥⌘1"), false, false),
        Some("editors.piano_roll_window")
    );
    assert_eq!(k.lookup(c("X"), false, false), None);
}

#[test]
fn step_entry_keys_win_while_it_is_on_unless_the_number_row_is_reserved() {
    let k = Keymap::new(TABLE);
    assert!(no_collisions(&k));
    assert_eq!(k.lookup(c("1"), false, false), Some("locator.go_1"));
    assert_eq!(k.lookup(c("1"), true, false), Some("step.whole"));
    assert_eq!(
        k.lookup(c("1"), true, true),
        Some("locator.go_1"),
        "the row is the locators'"
    );
    assert_eq!(
        k.lookup(c("Keypad1"), true, true),
        Some("step.whole"),
        "the keypad still enters"
    );
    assert_eq!(
        k.lookup(c("Keypad1"), false, false),
        None,
        "step keys are off outside step entry"
    );
    assert_eq!(
        k.lookup(c("."), true, true),
        Some("step.dot"),
        "only the number row is reserved"
    );
    assert_eq!(
        k.lookup(c("S"), true, false),
        Some("transport.stop"),
        "global keys still work"
    );
}

#[test]
fn binding_refuses_a_taken_key_unless_it_replaces() {
    let mut k = Keymap::default();
    k.bind("file.save", &[c("⌘K"), c("⌘K")], false).unwrap();
    assert_eq!(k.chords("file.save"), vec![c("⌘K")], "deduplicated");
    assert_eq!(k.lookup(c("⌘S"), false, false), None, "⌘S is free now");
    let e = k.bind("edit.undo", &[c("⌘K")], false).unwrap_err();
    assert_eq!(
        e,
        BindError::Taken {
            chord: c("⌘K"),
            other: "file.save"
        }
    );
    assert_eq!(e.to_string(), "⌘K is the key of “Save”");
    k.bind("edit.undo", &[c("⌘K"), c("⌘Z")], true).unwrap();
    assert_eq!(k.chords("file.save"), Vec::<Chord>::new(), "taken away");
    assert_eq!(k.lookup(c("⌘K"), false, false), Some("edit.undo"));
    assert_eq!(
        k.bind("nope.nope", &[], false),
        Err(BindError::UnknownCommand("nope.nope".into()))
    );
    let five: Vec<Chord> = ["F1", "F2", "F3", "F4", "F5"]
        .iter()
        .map(|t| c(t))
        .collect();
    assert_eq!(k.bind("file.save", &five, false), Err(BindError::TooMany));
    assert_eq!(five.len(), MAX_CHORDS + 1);
    // Contexts are separate: a step key may share a global key, not
    // another step key.
    let mut t = Keymap::new(TABLE);
    t.bind("step.rest", &[c("S")], false).unwrap();
    assert_eq!(
        t.bind("step.rest", &[c("1")], false),
        Err(BindError::Taken {
            chord: c("1"),
            other: "step.whole"
        })
    );
    assert!(no_collisions(&k) && no_collisions(&t));
}

#[test]
fn resetting_gives_defaults_back_but_not_ones_taken_since() {
    let mut k = Keymap::default();
    k.bind("edit.undo", &[c("⌘S")], true).unwrap();
    k.reset_one("file.save");
    assert_eq!(
        k.chords("file.save"),
        Vec::<Chord>::new(),
        "⌘S stays with Undo"
    );
    k.reset_one("edit.undo");
    assert_eq!(k.chords("edit.undo"), vec![c("⌘Z")]);
    k.reset_one("file.save");
    assert_eq!(k.chords("file.save"), vec![c("⌘S")]);
    assert!(k.changed().is_empty(), "back to the defaults");
    k.bind("file.save", &[], false).unwrap();
    assert_eq!(k.changed(), vec!["file.save"], "unbound is a change");
    k.reset();
    assert_eq!(k, Keymap::default());
}

#[test]
fn the_document_holds_only_changes_and_reads_back() {
    let mut k = Keymap::default();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&k.to_json()).unwrap(),
        json!({"vizdaw_keys": 1, "bindings": {}})
    );
    k.bind("file.save", &[c("⌘K"), c("F5")], false).unwrap();
    k.bind("edit.redo", &[], false).unwrap();
    let text = k.to_json();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text).unwrap(),
        json!({"vizdaw_keys": 1, "bindings": {"edit.redo": [], "file.save": ["⌘K", "F5"]}})
    );
    let (back, skipped) = Keymap::from_json(COMMANDS, &text).unwrap();
    assert_eq!(back, k);
    assert!(skipped.is_empty(), "{skipped:?}");
}

#[test]
fn a_damaged_document_loads_what_it_can() {
    let doc = json!({"vizdaw_keys": 1, "bindings": {
        "nope.nope": ["⌘K"],
        "edit.undo": "⌘Z",
        "edit.redo": ["⌘⌘Z", 7, "⇧⌘Z"],
        "file.new": ["F1", "F2", "F3", "F4", "F5", "F6"],
        "transport.play_stop": ["⌘O"],
        "file.save": ["⌘Y"],
        "file.save_as": ["⌘S"],
    }});
    let (k, skipped) = Keymap::from_json(COMMANDS, &doc.to_string()).unwrap();
    assert!(no_collisions(&k));
    assert_eq!(k.chords("edit.redo"), vec![c("⇧⌘Z")]);
    assert_eq!(k.chords("edit.undo"), vec![c("⌘Z")], "unreadable: defaults");
    assert_eq!(k.chords("file.new").len(), MAX_CHORDS);
    assert_eq!(
        k.chords("transport.play_stop"),
        Vec::<Chord>::new(),
        "⌘O is still Open's"
    );
    assert_eq!(k.chords("file.save"), vec![c("⌘Y")]);
    assert_eq!(k.chords("file.save_as"), vec![c("⌘S")], "keys may move");
    let joined = skipped.join("\n");
    for part in [
        "no command \"nope.nope\"",
        "edit.undo: the keys are not a list",
        "edit.redo: \"⌘⌘Z\" repeats a modifier",
        "edit.redo: a key is not text",
        "file.new: more than 4 keys",
        "transport.play_stop: ⌘O is already the key of file.open",
    ] {
        assert!(joined.contains(part), "{part:?} in\n{joined}");
    }
    for bad in [
        "garbage".to_string(),
        json!({"vizdaw_keys": 2, "bindings": {}}).to_string(),
        json!({"vizdaw_keys": 1}).to_string(),
        json!([1, 2]).to_string(),
        format!(
            "{{\"vizdaw_keys\":1,\"bindings\":{{}},\"pad\":\"{}\"}}",
            "x".repeat(MAX_JSON)
        ),
    ] {
        assert!(
            Keymap::from_json(COMMANDS, &bad).is_err(),
            "{}",
            &bad[..bad.len().min(40)]
        );
    }
}

fn any_key_text() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("⌘S".to_string()),
        Just("1".to_string()),
        Just("Space".to_string()),
        Just("Keypad1".to_string()),
        Just("⌥⇧1".to_string()),
        Just("⌘⌘".to_string()),
        "[⌘⌥⇧⌃]{0,3}[A-Za-z0-9.←]{0,2}",
        ".{0,6}",
    ]
}

fn any_id() -> impl Strategy<Value = String> {
    prop_oneof![
        (0..COMMANDS.len()).prop_map(|i| COMMANDS[i].id.to_string()),
        "[a-z._]{0,12}",
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// A hostile document loads or is refused; a loaded keymap never has
    /// a key twice in a context, and writes back to itself.
    #[test]
    fn hostile_documents_never_collide(
        entries in prop::collection::vec((any_id(), prop::collection::vec(any_key_text(), 0..6)), 0..12),
        version in prop_oneof![Just(json!(1)), Just(json!(2)), Just(json!("1")), Just(json!(null))],
    ) {
        let mut b = serde_json::Map::new();
        for (id, keys) in entries {
            b.insert(id, json!(keys));
        }
        let doc = json!({"vizdaw_keys": version, "bindings": b}).to_string();
        if let Ok((k, _)) = Keymap::from_json(COMMANDS, &doc) {
            prop_assert!(no_collisions(&k));
            let (again, skipped) = Keymap::from_json(COMMANDS, &k.to_json()).unwrap();
            prop_assert_eq!(&again, &k);
            prop_assert!(skipped.is_empty());
        }
    }

    /// Any sequence of binds and resets keeps every key in one command
    /// per context.
    #[test]
    fn binds_and_resets_never_collide(
        ops in prop::collection::vec((0..COMMANDS.len(), prop::collection::vec(any_key_text(), 0..5), any::<bool>(), 0..8u8), 0..24),
    ) {
        let mut k = Keymap::default();
        for (i, keys, replace, op) in ops {
            let id = COMMANDS[i].id;
            let chords: Vec<Chord> = keys.iter().filter_map(|t| Chord::parse(t).ok()).collect();
            match op {
                0 => k.reset_one(id),
                1 => k.reset(),
                _ => {
                    let before = k.clone();
                    if k.bind(id, &chords, replace).is_err() {
                        prop_assert_eq!(&k, &before, "a refused bind changes nothing");
                    }
                }
            }
            prop_assert!(no_collisions(&k));
        }
    }
}

#[test]
fn a_slot_clashes_with_a_global_or_a_step_entry_key() {
    let k = Keymap::default();
    assert_eq!(k.slot_clash("s", 8), Some("file.save"), "a global key");
    assert_eq!(k.slot_clash("=", 0), Some("step.tie"), "a step-only key");
    assert_eq!(k.slot_clash("/", 0), Some("step.triplet"));
    assert_eq!(k.slot_clash("4", 0), Some("locator.go_4"), "global first");
    assert_eq!(k.slot_clash("q", 2), None, "a free key");
    assert_eq!(k.slot_clash("nope", 0), None, "not a key");
}

/// A slot cannot take a key macOS or the app menu keeps (final review
/// M-4): the clash names that key's title.
#[test]
fn a_slot_cannot_take_a_system_key() {
    let k = Keymap::default();
    assert_eq!(k.slot_clash("q", 8), Some("Quit VizDAW"));
    assert_eq!(k.slot_clash("h", 8), Some("Hide VizDAW"));
    assert_eq!(k.slot_clash("h", 12), Some("Hide Others"));
    assert_eq!(k.slot_clash("m", 8), Some("Minimize"));
    assert_eq!(k.slot_clash(",", 8), Some("Settings…"));
    assert_eq!(k.slot_clash("tab", 8), Some("Switch Apps"));
    assert_eq!(k.slot_clash("w", 8), Some("file.close"), "a command first");
    let mut k = k;
    k.bind("file.close", &[], false).unwrap();
    assert_eq!(k.slot_clash("w", 8), Some("Close Window"), "kept unbound");
    assert_eq!(k.slot_clash("q", 0), None, "plain Q is free");
    assert_eq!(k.slot_clash("q", 4), None, "⌥Q is free");
}
