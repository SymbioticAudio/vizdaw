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

/// `pad()` plus an insert on master (returned) whose state is blob `a`
/// and whose overlay is `{3: 0.5}`, as history.
fn with_insert(a: &[u8]) -> (History, PluginInstanceId) {
    use core_model::ids::AnyId;
    use core_model::mixer::PluginRef;
    let d = pad();
    let master = d.mixer().master();
    let applied = apply(
        &d,
        &Command::Mixer(MixerCmd::AddInsert {
            strip: master,
            index: None,
            plugin: PluginRef::from_legacy("au:fx"),
        }),
    )
    .unwrap();
    let Some(AnyId::Plugin(slot)) = applied.report.created.first().copied() else {
        panic!("no insert created");
    };
    let mut h = History::new(applied.doc);
    h.capture_state(slot, a).unwrap();
    h.execute(
        &Command::Mixer(MixerCmd::SetPluginParams {
            slot,
            params: vec![(ParamId(3), 0.5)],
        }),
        &ViewFilter::default(),
    )
    .unwrap();
    (h, slot)
}

fn overlay(doc: &Document, slot: PluginInstanceId) -> Vec<(ParamId, f64)> {
    let s = doc.plugin_slot(slot).unwrap();
    s.params.iter().map(|(k, v)| (*k, *v)).collect()
}

fn set_state(slot: PluginInstanceId, bytes: &[u8]) -> Command {
    Command::Mixer(MixerCmd::SetPluginState {
        slot,
        bytes: std::sync::Arc::from(bytes),
    })
}

/// Plan 6e Ruling 10: theirs' state goes in as one undo step.
#[test]
fn set_plugin_state_is_one_undoable_step() {
    let (mut h, slot) = with_insert(b"state-A");
    let f = ViewFilter::default();
    let depth = h.undo_depth();
    h.execute(&set_state(slot, b"state-B"), &f).unwrap();
    assert_eq!(h.undo_depth(), depth + 1, "one step");
    assert_eq!(h.undo_label(), Some("Set Plugin Settings"));
    let b = blob_id(b"state-B");
    assert_eq!(h.doc().plugin_slot(slot).unwrap().state, Some(b));
    assert!(overlay(h.doc(), slot).is_empty(), "the overlay is cleared");
    assert_eq!(h.doc().state_blob(b), Some(&b"state-B"[..]));
    assert_eq!(
        h.doc().state_blob(blob_id(b"state-A")),
        None,
        "the old blob is no longer referenced"
    );
    assert!(h.doc().missing_states().is_empty());
    h.undo().unwrap();
    let a = blob_id(b"state-A");
    assert_eq!(h.doc().plugin_slot(slot).unwrap().state, Some(a));
    assert_eq!(overlay(h.doc(), slot), vec![(ParamId(3), 0.5)]);
    assert_eq!(h.doc().state_blob(a), Some(&b"state-A"[..]));
    assert!(h.doc().missing_states().is_empty());
    h.redo().unwrap();
    assert_eq!(h.doc().plugin_slot(slot).unwrap().state, Some(b));
    assert!(overlay(h.doc(), slot).is_empty());
    assert_eq!(h.doc().state_blob(b), Some(&b"state-B"[..]));
    assert!(h.doc().missing_states().is_empty());
}

/// Hostile input (Task 3 fix round 1): a blob ID already held with other
/// bytes is a hash collision. The command is refused, and the slot that
/// holds the blob keeps its bytes. The collision is made by a package
/// whose blob file was edited (load trusts the file name).
#[test]
fn set_plugin_state_refuses_a_colliding_blob() {
    let (h, insert) = with_insert(b"state-A");
    let pkg = temp_package("collide");
    save(h.doc(), &pkg).unwrap();
    let a = blob_id(b"state-A");
    let file = pkg.join("PluginState").join(blob_file_name(a));
    std::fs::write(&file, b"other bytes").unwrap();
    let tampered = load(&pkg).unwrap();
    let _ = std::fs::remove_dir_all(&pkg);
    assert_eq!(tampered.state_blob(a), Some(&b"other bytes"[..]));
    // The instrument's slot asks for bytes whose ID is `a`.
    let cmd = set_state(PluginInstanceId(0), b"state-A");
    let _: core_model::error::ModelError = apply(&tampered, &cmd).unwrap_err();
    let mut h2 = History::new(tampered.clone());
    assert!(h2.execute(&cmd, &ViewFilter::default()).is_err());
    assert_eq!(h2.doc(), &tampered, "unchanged");
    assert_eq!(h2.undo_depth(), 0, "no step");
    assert_eq!(h2.doc().plugin_slot(insert).unwrap().state, Some(a));
    assert_eq!(h2.doc().state_blob(a), Some(&b"other bytes"[..]));
    // Without the edit the same ID holds the same bytes: no collision, and
    // both slots share the blob.
    let same = apply(h.doc(), &set_state(PluginInstanceId(0), b"state-A")).unwrap();
    assert_eq!(same.doc.state_blob(a), Some(&b"state-A"[..]));
    assert_eq!(same.doc.plugin_slot(insert).unwrap().state, Some(a));
}

/// Hostile input (Task 3 fix round 1): a state is at most
/// `MAX_STATE_BYTES`, the import's per-state cap.
#[test]
fn set_plugin_state_caps_the_size() {
    use core_model::state::MAX_STATE_BYTES;
    assert_eq!(MAX_STATE_BYTES, 64 * 1024 * 1024);
    let (h, slot) = with_insert(b"state-A");
    let cap = usize::try_from(MAX_STATE_BYTES).unwrap();
    let big: std::sync::Arc<[u8]> = vec![7u8; cap + 1].into();
    let over = Command::Mixer(MixerCmd::SetPluginState {
        slot,
        bytes: big.clone(),
    });
    let _: core_model::error::ModelError = apply(h.doc(), &over).unwrap_err();
    let at = Command::Mixer(MixerCmd::SetPluginState {
        slot,
        bytes: big[..cap].into(),
    });
    let d = apply(h.doc(), &at).unwrap().doc;
    let id = d.plugin_slot(slot).unwrap().state.unwrap();
    assert_eq!(d.state_blob(id).map(<[u8]>::len), Some(cap));
}

/// Hostile input: an unknown slot and empty bytes are refused, and the
/// document is unchanged.
#[test]
fn set_plugin_state_refuses() {
    use core_model::error::ModelError;
    let (h, slot) = with_insert(b"state-A");
    let before = h.doc().clone();
    for cmd in [
        set_state(PluginInstanceId(9_999), b"x"),
        set_state(PluginInstanceId(u64::MAX), b"x"),
        set_state(slot, b""),
    ] {
        let _: ModelError = apply(&before, &cmd).unwrap_err();
        let mut h2 = History::new(before.clone());
        assert!(h2.execute(&cmd, &ViewFilter::default()).is_err());
        assert_eq!(h2.doc(), &before, "unchanged");
        assert_eq!(h2.undo_depth(), 0, "no step");
    }
}

/// It works on an instrument destination's own slot and on an EQ slot.
#[test]
fn set_plugin_state_reaches_instrument_and_eq_slots() {
    use core_model::ids::AnyId;
    use core_model::mixer::PluginRef;
    let d = pad();
    let master = d.mixer().master();
    let applied = apply(
        &d,
        &Command::Mixer(MixerCmd::SetEq {
            strip: master,
            plugin: Some(PluginRef::from_legacy("au:eq")),
        }),
    )
    .unwrap();
    let eq = applied
        .report
        .created
        .iter()
        .find_map(|x| match x {
            AnyId::Plugin(p) => Some(*p),
            _ => None,
        })
        .unwrap();
    let mut h = History::new(applied.doc);
    let f = ViewFilter::default();
    let inst = PluginInstanceId(0);
    h.execute(&set_state(inst, b"synth"), &f).unwrap();
    h.execute(&set_state(eq, b"curve"), &f).unwrap();
    assert_eq!(
        h.doc().plugin_slot(inst).unwrap().state,
        Some(blob_id(b"synth"))
    );
    assert_eq!(
        h.doc().plugin_slot(eq).unwrap().state,
        Some(blob_id(b"curve"))
    );
    assert_eq!(h.doc().state_blob(blob_id(b"synth")), Some(&b"synth"[..]));
    assert_eq!(h.doc().state_blob(blob_id(b"curve")), Some(&b"curve"[..]));
    h.undo().unwrap();
    assert!(h.doc().plugin_slot(eq).unwrap().state.is_none());
    assert_eq!(h.doc().state_blob(blob_id(b"curve")), None);
    h.undo().unwrap();
    assert!(h.doc().plugin_slot(inst).unwrap().state.is_none());
}
