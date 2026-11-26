//! The app's key bindings in GPUI (slice 9c Task 2): the model's keymap
//! drives the menus and the window keys, a change rebinds both at once,
//! saved bindings load at launch, and `keys.*` runs over the channel.

use std::sync::Arc;

use core_control::bindings::Keymap;
use core_control::protocol::{RawRequest, RequestId};
use core_control::{ControlError, ErrorCode};
use gpui::{App, TestAppContext};
use serde_json::{Map, Value, json};
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeCore, demo_fake};
use vizdaw_gpui::AppState;
use vizdaw_gpui::control::{self, Handled};
use vizdaw_gpui::menus::{self, ItemSpec, RunCommand};

const OPT_CMD_J: &str = "⌥⌘J";
const MINE: &str =
    r#"{"vizdaw_keys": 1, "bindings": {"locator.go_2": ["X"], "track.new_audio": ["⌥⌘J"]}}"#;

fn setup(cx: &mut TestAppContext) {
    let core = Arc::new(FakeCore::new());
    core.hand_out("new_demo", Arc::new(demo_fake()));
    let store = DocumentStore::with_core(core);
    cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
        vizdaw_gpui::new_demo(cx).expect("a demo window");
    });
    cx.run_until_parked();
}

fn run(cx: &mut TestAppContext, id: &str, params: Value) -> Result<Value, ControlError> {
    let mut p = Map::new();
    p.insert("id".into(), id.into());
    p.insert("params".into(), params);
    let raw = RawRequest {
        id: RequestId::Num(1),
        cmd: "command.run".into(),
        params: p,
    };
    let command = core_control::validate(&raw)?;
    let r = cx.update(|cx| match control::handle(&command, cx) {
        Handled::Reply(r) => r,
        _ => panic!("expected a reply"),
    });
    cx.run_until_parked();
    r
}

/// The key the menus show for `id`.
fn menu_key(id: &str, cx: &App) -> Option<String> {
    fn find(items: &[ItemSpec], id: &str) -> Option<Option<String>> {
        for i in items {
            match i {
                ItemSpec::Command { id: i, keys, .. } if *i == id => return Some(keys.clone()),
                ItemSpec::Submenu { items, .. } => {
                    if let Some(k) = find(items, id) {
                        return Some(k);
                    }
                }
                _ => {}
            }
        }
        None
    }
    let shown = menus::shown(cx).expect("menus shown");
    shown
        .menus
        .iter()
        .find_map(|m| find(&m.items, id))
        .flatten()
}

/// The keystrokes bound to `id` in the app's keymap.
fn bound(id: &str, cx: &App) -> Vec<String> {
    let map = cx.key_bindings();
    let map = map.borrow();
    map.bindings_for_action(&RunCommand::new(id))
        .flat_map(|b| b.keystrokes().iter().map(|k| k.inner().key.clone()))
        .collect()
}

#[gpui::test]
fn keys_rebind_moves_the_menu_key_and_the_binding(cx: &mut TestAppContext) {
    setup(cx);
    let before = cx.update(|cx| (menu_key("track.new_audio", cx), bound("locator.go_2", cx)));
    assert_eq!(before.0.as_deref(), Some("⌥⌘A"), "{before:?}");
    assert!(before.1.iter().any(|k| k.contains('2')), "{before:?}");

    cx.update(|cx| {
        AppState::global(cx).update(cx, |s, _| {
            let mut prefs = std::mem::replace(&mut s.prefs, vizdaw_app::Prefs::in_memory());
            s.keys.record("locator.go_2", "X", &mut prefs);
            s.keys.record("track.new_audio", "⌥⌘J", &mut prefs);
            s.prefs = prefs;
        });
        menus::rebind(cx);
    });
    cx.run_until_parked();
    let after = cx.update(|cx| (menu_key("track.new_audio", cx), bound("locator.go_2", cx)));
    assert_eq!(after.0.as_deref(), Some(OPT_CMD_J), "{after:?}");
    assert!(
        after.1.iter().all(|k| !k.contains('2')) && after.1.iter().any(|k| k.contains('x')),
        "{after:?}"
    );
}

#[gpui::test]
fn saved_keys_load_at_launch(cx: &mut TestAppContext) {
    setup(cx);
    let mut prefs = vizdaw_app::Prefs::in_memory();
    prefs
        .set_raw("keys", serde_json::from_str(MINE).unwrap())
        .unwrap();
    cx.update(|cx| {
        AppState::global(cx).update(cx, |s, _| s.set_prefs(prefs));
        menus::rebind(cx);
    });
    cx.run_until_parked();
    let moved = cx.update(|cx| {
        AppState::global(cx)
            .read(cx)
            .keys
            .keymap()
            .chords("locator.go_2")
            .iter()
            .map(|c| c.text())
            .collect::<Vec<_>>()
    });
    assert_eq!(moved, vec!["X".to_string()]);
    let key = cx.update(|cx| menu_key("track.new_audio", cx));
    assert_eq!(key.as_deref(), Some(OPT_CMD_J));
    // Damaged saved keys leave the defaults.
    let mut bad = vizdaw_app::Prefs::in_memory();
    bad.set_raw("keys", json!("nonsense")).unwrap();
    cx.update(|cx| AppState::global(cx).update(cx, |s, _| s.set_prefs(bad)));
    let same = cx.update(|cx| {
        AppState::global(cx)
            .read(cx)
            .keys
            .keymap()
            .chords("locator.go_2")
            == Keymap::default().chords("locator.go_2")
    });
    assert!(same);
}

#[gpui::test]
fn keys_load_over_the_channel_dispatches(cx: &mut TestAppContext) {
    setup(cx);
    let dir = std::env::temp_dir().join(format!("vizkeys-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("mine.vizkeys");
    std::fs::write(&file, MINE).unwrap();

    run(cx, "keys.load", json!({"path": file})).expect("loads");
    assert_eq!(
        cx.update(|cx| menu_key("track.new_audio", cx)).as_deref(),
        Some(OPT_CMD_J)
    );
    assert!(
        cx.update(|cx| bound("locator.go_2", cx))
            .iter()
            .any(|k| k.contains('x'))
    );

    // Save writes what is bound now.
    let out = dir.join("out.vizkeys");
    run(cx, "keys.save", json!({"path": out})).expect("saves");
    assert!(
        std::fs::read_to_string(&out)
            .unwrap()
            .contains("locator.go_2")
    );

    run(cx, "keys.reset", json!({})).expect("resets");
    let k = cx.update(|cx| menu_key("track.new_audio", cx));
    assert!(k.is_some() && k.as_deref() != Some(OPT_CMD_J), "{k:?}");

    // A bad file is a Failed with the title, and changes nothing.
    std::fs::write(&file, "not json").unwrap();
    let e = run(cx, "keys.load", json!({"path": file})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(
        e.message.starts_with("Load Key Bindings Failed: "),
        "{}",
        e.message
    );
    let _ = std::fs::remove_dir_all(&dir);
}
