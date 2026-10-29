use core_model::command::{Command, MixerCmd, RoutingCmd, apply};
use core_model::document::{DestKind, Document};
use core_model::file::{autosave, load, save};
use core_model::history::{History, Timestamp};
use core_model::ids::{PluginInstanceId, StateBlobId};
use core_model::mixer::ParamId;
use core_model::selection::ViewFilter;
use core_model::state::{
    blob_file_name, blob_id, capture, capture_keeping, parse_blob_file_name, states_in_json,
};
use std::path::PathBuf;

fn temp_package(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let p = std::env::temp_dir().join(format!(
        "vizdaw-state-{tag}-{}-{nanos}.vizdaw",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    p
}

/// One plugin instrument (instance 0).
fn pad() -> Document {
    apply(
        &Document::new(),
        &Command::Routing(RoutingCmd::AddDestination {
            name: "Pad".into(),
            kind: DestKind::plugin("au:pad"),
            default_patch: None,
        }),
    )
    .unwrap()
    .doc
}

#[test]
fn blob_ids_are_content_hashes() {
    assert_eq!(blob_id(b"abc"), blob_id(b"abc"));
    assert_ne!(blob_id(b"abc"), blob_id(b"abd"));
    // FNV-1a 64 of the empty input is the offset basis.
    assert_eq!(blob_id(b""), StateBlobId(0xcbf2_9ce4_8422_2325));
    let name = blob_file_name(StateBlobId(0xab));
    assert_eq!(name, "00000000000000ab.bin");
    assert_eq!(parse_blob_file_name(&name), Some(StateBlobId(0xab)));
    assert_eq!(parse_blob_file_name("notes.txt"), None);
}

#[test]
fn capture_sets_state_and_clears_the_overlay_without_an_undo_step() {
    let mut h = History::new(pad());
    let f = ViewFilter::default();
    h.execute(
        &Command::Mixer(MixerCmd::SetPluginParams {
            slot: PluginInstanceId(0),
            params: vec![(ParamId(1), 0.5)],
        }),
        &f,
    )
    .unwrap();
    let depth = h.undo_depth();
    let id = h.capture_state(PluginInstanceId(0), b"state-1").unwrap();
    assert_eq!(h.undo_depth(), depth);
    let slot = h.doc().plugin_slot(PluginInstanceId(0)).unwrap();
    assert_eq!(slot.state, Some(id));
    assert!(slot.params.is_empty());
    assert_eq!(h.doc().state_blob(id), Some(&b"state-1"[..]));
    assert!(h.doc().missing_states().is_empty());
    // A second capture replaces the blob and forgets the first.
    let id2 = h.capture_state(PluginInstanceId(0), b"state-2").unwrap();
    assert_ne!(id, id2);
    assert_eq!(h.doc().state_blob(id), None);
    // Unknown slots and open transactions are refused.
    assert!(h.capture_state(PluginInstanceId(9), b"x").is_err());
    h.begin("gesture").unwrap();
    assert!(h.capture_state(PluginInstanceId(0), b"x").is_err());
    h.abort().unwrap();
    // The pure form leaves its input alone.
    let d = pad();
    let (d2, id3) = capture(&d, PluginInstanceId(0), b"z").unwrap();
    assert!(d.plugin_slot(PluginInstanceId(0)).unwrap().state.is_none());
    assert_eq!(
        d2.plugin_slot(PluginInstanceId(0)).unwrap().state,
        Some(id3)
    );
}

/// Task 9b: a capture keeps the overlay entries it is given (an
/// automated parameter's static value) and drops the rest; a value that
/// is not finite is not kept.
#[test]
fn capture_keeping_keeps_the_given_overlay_entries() {
    let mut h = History::new(pad());
    let f = ViewFilter::default();
    h.execute(
        &Command::Mixer(MixerCmd::SetPluginParams {
            slot: PluginInstanceId(0),
            params: vec![(ParamId(1), 0.5), (ParamId(2), 0.7)],
        }),
        &f,
    )
    .unwrap();
    let depth = h.undo_depth();
    let keep = std::collections::BTreeMap::from([
        (ParamId(1), 0.5),
        (ParamId(3), 0.2),
        (ParamId(4), f64::NAN),
    ]);
    let id = h
        .capture_state_keeping(PluginInstanceId(0), b"state-1", &keep)
        .unwrap();
    assert_eq!(h.undo_depth(), depth, "no undo step");
    let slot = h.doc().plugin_slot(PluginInstanceId(0)).unwrap();
    assert_eq!(slot.state, Some(id));
    let kept: Vec<(ParamId, f64)> = slot.params.iter().map(|(k, v)| (*k, *v)).collect();
    assert_eq!(kept, vec![(ParamId(1), 0.5), (ParamId(3), 0.2)]);
    // Refused like a plain capture.
    assert!(
        h.capture_state_keeping(PluginInstanceId(9), b"x", &keep)
            .is_err()
    );
    // The pure form; an empty map is a plain capture.
    let (d, _) = capture_keeping(&pad(), PluginInstanceId(0), b"z", &keep).unwrap();
    assert_eq!(d.plugin_slot(PluginInstanceId(0)).unwrap().params.len(), 2);
    let (d, _) = capture_keeping(h.doc(), PluginInstanceId(0), b"z", &Default::default()).unwrap();
    assert!(
        d.plugin_slot(PluginInstanceId(0))
            .unwrap()
            .params
            .is_empty()
    );
    // A file with kept entries saves and loads.
    let pkg = temp_package("keep");
    save(h.doc(), &pkg).unwrap();
    let back = load(&pkg).unwrap();
    assert_eq!(
        back.plugin_slot(PluginInstanceId(0)).unwrap().params.len(),
        2
    );
    let _ = std::fs::remove_dir_all(&pkg);
}

#[test]
fn save_writes_blobs_and_load_reads_them() {
    let pkg = temp_package("save");
    let (d, id) = capture(&pad(), PluginInstanceId(0), b"\x00\x01binary\xff").unwrap();
    save(&d, &pkg).unwrap();
    let file = pkg.join("PluginState").join(blob_file_name(id));
    assert_eq!(std::fs::read(&file).unwrap(), b"\x00\x01binary\xff");
    let text = std::fs::read_to_string(pkg.join("project.json")).unwrap();
    assert!(!text.contains("binary"), "blobs are not inline");
    assert_eq!(states_in_json(&text), [id].into_iter().collect());
    assert_eq!(load(&pkg).unwrap(), d);
    std::fs::remove_dir_all(&pkg).unwrap();
}

#[test]
fn unreferenced_blobs_go_on_save_but_autosave_references_stay() {
    let pkg = temp_package("prune");
    let (d1, id1) = capture(&pad(), PluginInstanceId(0), b"one").unwrap();
    autosave(&d1, &pkg, Timestamp(1000)).unwrap();
    let (d2, id2) = capture(&d1, PluginInstanceId(0), b"two").unwrap();
    let stray = pkg.join("PluginState").join(blob_file_name(StateBlobId(7)));
    std::fs::write(&stray, b"stray").unwrap();
    save(&d2, &pkg).unwrap();
    let dir = pkg.join("PluginState");
    assert!(dir.join(blob_file_name(id2)).exists());
    assert!(
        dir.join(blob_file_name(id1)).exists(),
        "an autosave still uses it"
    );
    assert!(!stray.exists());
    std::fs::remove_dir_all(&pkg).unwrap();
}

#[test]
fn missing_blob_loads_and_is_reported() {
    let pkg = temp_package("missing");
    let (d, id) = capture(&pad(), PluginInstanceId(0), b"gone").unwrap();
    save(&d, &pkg).unwrap();
    std::fs::remove_file(pkg.join("PluginState").join(blob_file_name(id))).unwrap();
    let back = load(&pkg).unwrap();
    assert_eq!(
        back.plugin_slot(PluginInstanceId(0)).unwrap().state,
        Some(id)
    );
    assert_eq!(back.missing_states(), vec![id]);
    assert_eq!(back.state_blob(id), None);
    std::fs::remove_dir_all(&pkg).unwrap();
}

/// Saves `d2` over a package whose newest autosave was replaced by `bad`;
/// nothing may be deleted and the save must still succeed.
fn prune_is_skipped_with(tag: &str, bad: impl Fn(&std::path::Path)) {
    let pkg = temp_package(tag);
    let (d1, id1) = capture(&pad(), PluginInstanceId(0), b"one").unwrap();
    let auto = autosave(&d1, &pkg, Timestamp(1000)).unwrap();
    let (d2, id2) = capture(&d1, PluginInstanceId(0), b"two").unwrap();
    let stray = pkg.join("PluginState").join(blob_file_name(StateBlobId(7)));
    std::fs::write(&stray, b"stray").unwrap();
    std::fs::remove_file(&auto).unwrap();
    bad(&auto);
    save(&d2, &pkg).unwrap();
    let dir = pkg.join("PluginState");
    assert!(dir.join(blob_file_name(id1)).exists());
    assert!(dir.join(blob_file_name(id2)).exists());
    assert!(stray.exists(), "no prune happened");
    assert_eq!(load(&pkg).unwrap(), d2, "project.json was updated");
    std::fs::remove_dir_all(&pkg).unwrap();
}

#[test]
fn an_unreadable_autosave_skips_the_prune_but_save_succeeds() {
    prune_is_skipped_with("unreadable", |p| std::fs::create_dir(p).unwrap());
}

#[test]
fn a_garbage_autosave_skips_the_prune_but_save_succeeds() {
    prune_is_skipped_with("garbage", |p| std::fs::write(p, b"not json {{").unwrap());
}

#[test]
fn an_autosave_loads_with_the_packages_blobs() {
    let pkg = temp_package("restore");
    save(&pad(), &pkg).unwrap();
    let (d, id) = capture(&pad(), PluginInstanceId(0), b"autosaved").unwrap();
    let path = autosave(&d, &pkg, Timestamp(7000)).unwrap();
    let restored = core_model::file::load_autosave(&pkg, &path).unwrap();
    assert_eq!(restored, d);
    assert_eq!(restored.state_blob(id), Some(&b"autosaved"[..]));
    assert!(core_model::file::load_autosave(&pkg, &pkg.join("Autosave/none.json")).is_err());
    std::fs::remove_dir_all(&pkg).unwrap();
}
