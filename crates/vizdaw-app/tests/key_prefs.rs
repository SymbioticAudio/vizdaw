//! The key bindings model over the preferences (slice 9c Task 1; Swift
//! `KeyBindingsTests`) and the `keys.*` commands.

use core_control::ErrorCode;
use core_control::registry::ArgValue;
use serde_json::json;
use vizdaw_app::Prefs;
use vizdaw_app::key_prefs::{KEYS_KEY, KeyBindings, run_keys};

fn keys_of(k: &KeyBindings, id: &str) -> Vec<String> {
    k.keymap().chords(id).iter().map(|c| c.text()).collect()
}

fn path_arg(p: &std::path::Path) -> Vec<(String, ArgValue)> {
    vec![("path".into(), ArgValue::Text(p.to_string_lossy().into()))]
}

#[test]
fn bindings_persist_in_the_prefs() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("prefs.json");
    let mut prefs = Prefs::at(&file);
    let mut k = KeyBindings::load(&prefs);
    assert_eq!(keys_of(&k, "file.save"), ["⌘S"]);
    k.record("file.save", "⌘K", &mut prefs);
    assert_eq!(keys_of(&k, "file.save"), ["⌘K"]);
    k.set_reserve_number_row(true, &mut prefs);
    let prefs = Prefs::at(&file);
    let again = KeyBindings::load(&prefs);
    assert_eq!(keys_of(&again, "file.save"), ["⌘K"]);
    assert!(again.reserve_number_row(&prefs));
    assert!(prefs.raw(KEYS_KEY).is_some_and(|v| v.is_object()));
}

#[test]
fn a_damaged_pref_keeps_the_defaults() {
    for bad in [json!("garbage"), json!(7), json!({"vizdaw_keys": 9})] {
        let mut prefs = Prefs::in_memory();
        prefs.set_raw(KEYS_KEY, bad).unwrap();
        let k = KeyBindings::load(&prefs);
        assert_eq!(keys_of(&k, "file.save"), ["⌘S"]);
    }
}

#[test]
fn a_taken_key_is_refused_unless_replaced() {
    let mut prefs = Prefs::in_memory();
    let mut k = KeyBindings::load(&prefs);
    k.record("edit.undo", "⌘S", &mut prefs);
    assert_eq!(k.message(), Some("⌘S is the key of “Save”. Replace it?"));
    assert_eq!(keys_of(&k, "edit.undo"), ["⌘Z"]);
    assert_eq!(k.pending().map(|p| p.id.as_str()), Some("edit.undo"));
    k.confirm_replace(&mut prefs);
    assert!(k.pending().is_none() && k.message().is_none());
    assert_eq!(keys_of(&k, "edit.undo"), ["⌘S"]);
    assert!(keys_of(&k, "file.save").is_empty());
    k.reset("file.save", &mut prefs);
    assert!(keys_of(&k, "file.save").is_empty(), "⌘S stays with Undo");
    k.reset_all(&mut prefs);
    assert_eq!(keys_of(&k, "file.save"), ["⌘S"]);
    assert_eq!(keys_of(&k, "edit.undo"), ["⌘Z"]);
    // Persisted after each change.
    assert!(KeyBindings::load(&prefs).keymap().changed().is_empty());
}

#[test]
fn files_save_load_and_refuse_damage() {
    let dir = tempfile::tempdir().unwrap();
    let mut prefs = Prefs::in_memory();
    let mut k = KeyBindings::load(&prefs);
    k.record("file.save", "F5", &mut prefs);
    let path = dir.path().join("mine.vizkeys");
    k.save(&path).unwrap();
    k.reset_all(&mut prefs);
    k.load_file(&path, &mut prefs).unwrap();
    assert_eq!(keys_of(&k, "file.save"), ["F5"]);
    assert!(k.message().is_none());
    assert_eq!(keys_of(&KeyBindings::load(&prefs), "file.save"), ["F5"]);
    let bad = dir.path().join("bad.vizkeys");
    std::fs::write(&bad, "garbage").unwrap();
    assert!(k.load_file(&bad, &mut prefs).is_err());
    assert_eq!(keys_of(&k, "file.save"), ["F5"], "unchanged");
    assert!(k.message().is_some_and(|m| !m.is_empty()));
    // Too large, a directory and a missing file are refused too.
    std::fs::write(&bad, vec![b' '; 300 * 1024]).unwrap();
    assert!(k.load_file(&bad, &mut prefs).is_err());
    assert!(k.load_file(dir.path(), &mut prefs).is_err());
    assert!(k.load_file(&dir.path().join("none"), &mut prefs).is_err());
    assert_eq!(keys_of(&k, "file.save"), ["F5"]);
    assert!(k.save(&dir.path().join("no/such/dir/x")).is_err());
}

#[test]
fn replace_is_offered_only_for_a_taken_key_and_resets_forget_it() {
    let mut prefs = Prefs::in_memory();
    let mut k = KeyBindings::load(&prefs);
    k.record("no.such", "X", &mut prefs);
    assert!(k.pending().is_none(), "nothing to replace");
    assert!(
        k.message()
            .is_some_and(|m| !m.is_empty() && !m.ends_with("Replace it?"))
    );
    for which in 0..2 {
        k.record("transport.stop", "Space", &mut prefs);
        assert!(k.pending().is_some());
        if which == 0 {
            k.reset("transport.stop", &mut prefs);
        } else {
            k.reset_all(&mut prefs);
        }
        assert!(k.pending().is_none() && k.message().is_none());
    }
    k.record("transport.stop", "Space", &mut prefs);
    k.cancel_replace();
    assert!(k.pending().is_none() && k.message().is_none());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("k.vizkeys");
    k.save(&path).unwrap();
    k.record("transport.stop", "Space", &mut prefs);
    k.load_file(&path, &mut prefs).unwrap();
    assert!(
        k.pending().is_none() && k.message().is_none(),
        "a load forgets it"
    );
}

#[test]
fn a_successful_bind_clears_the_message() {
    let mut prefs = Prefs::in_memory();
    let mut k = KeyBindings::load(&prefs);
    k.record("no.such", "X", &mut prefs);
    assert!(k.message().is_some());
    k.record("transport.stop", "X", &mut prefs);
    assert!(k.message().is_none());
}

#[test]
fn a_file_with_new_ids_loads_and_lists_what_it_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.vizkeys");
    let mut prefs = Prefs::in_memory();
    let mut k = KeyBindings::load(&prefs);
    std::fs::write(
        &path,
        r#"{"vizdaw_keys": 1, "bindings": {"transport.rewind": ["⌥,"]}}"#,
    )
    .unwrap();
    k.load_file(&path, &mut prefs).unwrap();
    assert_eq!(keys_of(&k, "transport.rewind"), ["⌥,"]);
    assert!(k.message().is_none());
    std::fs::write(
        &path,
        r#"{"vizdaw_keys": 1, "bindings": {"a": ["F1"], "b": ["F2"]}}"#,
    )
    .unwrap();
    k.load_file(&path, &mut prefs).unwrap();
    assert_eq!(
        k.message(),
        Some("Skipped: no command \"a\"; no command \"b\"")
    );
    assert_eq!(
        keys_of(&k, "file.save"),
        ["⌘S"],
        "a load replaces the bindings"
    );
}

#[test]
fn keys_commands_need_a_path_and_report_failures() {
    let mut prefs = Prefs::in_memory();
    let mut k = KeyBindings::load(&prefs);
    for (id, dialog) in [("keys.save", "Save"), ("keys.load", "Open")] {
        let e = run_keys(id, &[], &mut k, &mut prefs).unwrap_err();
        assert_eq!(e.code, ErrorCode::DialogRequired);
        assert_eq!(e.dialog.as_deref(), Some(dialog));
    }
    let dir = tempfile::tempdir().unwrap();
    let e = run_keys(
        "keys.load",
        &path_arg(&dir.path().join("none")),
        &mut k,
        &mut prefs,
    )
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(
        e.message.starts_with("Load Key Bindings Failed: "),
        "{}",
        e.message
    );
    let e = run_keys(
        "keys.save",
        &path_arg(&dir.path().join("no/such/x")),
        &mut k,
        &mut prefs,
    )
    .unwrap_err();
    assert!(
        e.message.starts_with("Save Key Bindings Failed: "),
        "{}",
        e.message
    );
    // The round trip, and the reset.
    k.record("file.save", "F5", &mut prefs);
    let file = dir.path().join("k.vizkeys");
    run_keys("keys.save", &path_arg(&file), &mut k, &mut prefs).unwrap();
    run_keys("keys.reset", &[], &mut k, &mut prefs).unwrap();
    assert_eq!(keys_of(&k, "file.save"), ["⌘S"]);
    run_keys("keys.load", &path_arg(&file), &mut k, &mut prefs).unwrap();
    assert_eq!(keys_of(&k, "file.save"), ["F5"]);
    let e = run_keys("keys.nope", &[], &mut k, &mut prefs).unwrap_err();
    assert_eq!(e.code, ErrorCode::UnknownId);
}
