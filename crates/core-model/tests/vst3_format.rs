//! VST3 slots in the model (host track H1 Task 1): the `vst3` plug-in
//! format, its 32-hex class ID, and the song-format bump to version 12 that
//! makes an older build refuse such a song cleanly (owner ruling §2b).

use core_model::ModelError;
use core_model::command::{Command, MixerCmd, apply};
use core_model::document::Document;
use core_model::file::{VERSION, from_json, migrate_v11_to_v12, to_json};
use core_model::mixer::{PluginFormat, PluginRef, vst3_class_id};
use serde_json::{Value, json};

/// The last version a build without VST3 reads.
const LAST_WITHOUT_VST3: u64 = 11;

const TUID: &str = "0123456789ABCDEF0123456789ABCDEF";

fn vst3(id: &str) -> PluginRef {
    PluginRef {
        format: PluginFormat::Vst3,
        id: id.into(),
        name: "Again".into(),
        vendor: "Example".into(),
        version: "1.0".into(),
    }
}

fn with_vst3_insert() -> Document {
    let d = Document::new();
    let master = d.mixer().master();
    apply(
        &d,
        &Command::Mixer(MixerCmd::AddInsert {
            strip: master,
            index: None,
            plugin: vst3(TUID),
        }),
    )
    .unwrap()
    .doc
}

#[test]
fn the_format_is_written_as_vst3() {
    assert_eq!(
        serde_json::to_value(PluginFormat::Vst3).unwrap(),
        json!("vst3")
    );
    assert_eq!(
        serde_json::from_value::<PluginFormat>(json!("vst3")).unwrap(),
        PluginFormat::Vst3
    );
    let r = vst3(TUID);
    let back: PluginRef = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
    assert_eq!(back, r);
}

#[test]
fn a_vst3_slot_saves_as_version_12_and_loads_back() {
    assert_eq!(VERSION, 12);
    let d = with_vst3_insert();
    let text = to_json(&d).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["version"], json!(12));
    assert!(v["version"].as_u64().unwrap() > LAST_WITHOUT_VST3);
    assert!(text.contains("\"format\": \"vst3\""), "{text}");
    assert_eq!(from_json(&text).unwrap(), d);
}

#[test]
fn a_version_11_song_migrates_unchanged() {
    let d = Document::new();
    let mut v: Value = serde_json::from_str(&to_json(&d).unwrap()).unwrap();
    v["version"] = json!(11);
    assert_eq!(migrate_v11_to_v12(v.clone()).unwrap(), v);
    assert_eq!(from_json(&v.to_string()).unwrap(), d);
}

#[test]
fn a_newer_song_is_refused() {
    let mut v: Value = serde_json::from_str(&to_json(&with_vst3_insert()).unwrap()).unwrap();
    v["version"] = json!(VERSION + 1);
    assert_eq!(
        from_json(&v.to_string()),
        Err(ModelError::NewerFormat { found: VERSION + 1 })
    );
}

#[test]
fn class_ids_are_32_hex_digits_in_upper_case() {
    assert_eq!(vst3_class_id(TUID).as_deref(), Some(TUID));
    assert_eq!(
        vst3_class_id("0123456789abcdef0123456789abcdef").as_deref(),
        Some(TUID)
    );
    assert_eq!(vst3_class_id(" 0123456789ABCDEF0123456789ABCDEF "), None);
    assert_eq!(vst3_class_id("0123456789ABCDEF0123456789ABCDE"), None);
    assert_eq!(vst3_class_id("0123456789ABCDEF0123456789ABCDEF0"), None);
    assert_eq!(vst3_class_id("0123456789ABCDEF0123456789ABCDEG"), None);
    assert_eq!(vst3_class_id(""), None);
}

#[test]
fn legacy_references_are_never_vst3() {
    // Version-1 strings predate VST3; they stay AU or CLAP.
    assert_eq!(
        PluginRef::from_legacy(&format!("vst3:{TUID}")).format,
        PluginFormat::Clap
    );
}
