//! VCAs through a session (groups-vcas-transport Task 5): the seven VCA
//! commands through `run_mixer`, each one undo step; a VCA changes what
//! the engine plays, never a member's fader or switches; the strip's VCA
//! offset and mark; Review Focus 3 (deleting a member's track is one step
//! and Undo brings the memberships back, the last member of a VCA too);
//! hostile input; and the song's cap of `MAX_VCAS`, on create and on load.
//!
//! No plug-in loads in the session: the strips are audio tracks' with no
//! EQ. Offline only; the heard level is measured by an offline render.

use core_engine::backend::AudioBackend;
use core_engine::engine::{Engine, EngineConfig};
use core_engine::fixtures::Builder;
use core_engine::graph::TestInserts;
use core_engine::offline::{OfflineBackend, OfflineSinks};
use core_model::command::{Command, GroupCmd, MixerCmd};
use core_model::document::Document;
use core_model::groups::MAX_VCAS;
use core_model::ids::{SeqId, StripId, VcaId};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::automation::{
    AutomationModeRow as Mode, reset_automation_settings, set_automation_suspended,
};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{
    Session, StripKindRow, StripRow, SwatchRow, TrackKindRow, VcaRow, set_new_channel_eq,
};

static SERIAL: Mutex<()> = Mutex::new(());

const SR: f64 = 48_000.0;

/// One test at a time; offline; no EQ on new channels, so nothing loads.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(SR);
    reset_host();
    reset_automation_settings();
    set_automation_suspended(false);
    set_new_channel_eq(false);
    g
}

fn int(name: &str, value: i64) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Int { value },
    }
}

fn id(name: &str, value: u64) -> ControlArg {
    int(name, value as i64)
}

fn text(name: &str, value: &str) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Text {
            value: value.into(),
        },
    }
}

fn flag(name: &str, value: bool) -> ControlArg {
    ControlArg {
        name: name.into(),
        value: ControlValue::Flag { value },
    }
}

fn strips(s: &Session) -> Vec<StripRow> {
    s.mixer().unwrap().strips
}

fn strip_row(s: &Session, id: u64) -> StripRow {
    strips(s).into_iter().find(|x| x.id == id).unwrap()
}

fn strip_id(s: &Session, name: &str) -> u64 {
    strips(s)
        .into_iter()
        .find(|x| x.name == name)
        .unwrap_or_else(|| panic!("no strip {name}"))
        .id
}

fn vcas(s: &Session) -> Vec<VcaRow> {
    s.mixer().unwrap().vcas
}

fn undo_label(s: &Session) -> Option<String> {
    s.snapshot().unwrap().undo_label
}

fn step(s: &Session) -> u64 {
    s.snapshot().unwrap().step
}

/// The last edit made one undo step, the first after the step `before`:
/// one undo goes back to `before`, and redo brings the edit back.
fn one_step(s: &Session, before: u64) {
    let after = step(s);
    assert_ne!(after, before, "a step was made");
    s.undo().unwrap();
    assert_eq!(step(s), before, "one undo goes back");
    s.redo().unwrap();
    assert_eq!(step(s), after);
}

/// The mixer's strips, groups and VCAs (not its version).
fn mix(s: &Session) -> (Vec<StripRow>, Vec<vizdaw_ffi::GroupRow>, Vec<VcaRow>) {
    let m = s.mixer().unwrap();
    (m.strips, m.groups, m.vcas)
}

/// A saved song with audio tracks Kick and Snare (each on its own strip
/// at 0 dB, to master) and a bus, in the sequence `seq`.
struct Song {
    s: Arc<Session>,
    seq: u64,
    kick: u64,
    snare: u64,
    bus: u64,
    master: u64,
    path: PathBuf,
    _dir: tempfile::TempDir,
}

fn song() -> Song {
    let s = Session::new_empty().unwrap();
    let seq = s.snapshot().unwrap().sequences[0].id;
    let mut ids = Vec::new();
    for (i, name) in ["Kick", "Snare"].iter().enumerate() {
        s.new_track(seq, TrackKindRow::Audio, None).unwrap();
        let id = strip_id(&s, &format!("Audio {}", i + 1));
        s.rename_strip(id, (*name).into()).unwrap();
        ids.push(id);
    }
    s.add_bus("Bus".into()).unwrap();
    let bus = strip_id(&s, "Bus");
    let master = strips(&s)
        .into_iter()
        .find(|x| x.kind == StripKindRow::Master)
        .unwrap()
        .id;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("VCAs.vizdaw");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    Song {
        seq,
        kick: ids[0],
        snare: ids[1],
        bus,
        master,
        path,
        s,
        _dir: dir,
    }
}

/// `mixer.new_vca` of `members`, named `name`; its ID.
fn new_vca(s: &Session, members: &[u64], name: &str) -> u64 {
    let list: Vec<String> = members.iter().map(u64::to_string).collect();
    s.run_mixer(
        "mixer.new_vca".into(),
        vec![text("strips", &list.join(",")), text("name", name)],
    )
    .unwrap();
    vcas(s).into_iter().find(|v| v.name == name).unwrap().id
}

fn set_vca(s: &Session, vca: u64, mut more: Vec<ControlArg>) {
    let mut args = vec![id("vca", vca)];
    args.append(&mut more);
    s.run_mixer("mixer.set_vca".into(), args).unwrap();
}

#[test]
fn new_vca_from_strips() {
    let _g = serial();
    let t = song();
    let before = step(&t.s);
    t.s.run_mixer(
        "mixer.new_vca".into(),
        vec![text("strips", &format!("{},{}", t.kick, t.snare))],
    )
    .unwrap();
    one_step(&t.s, before);
    assert_eq!(undo_label(&t.s).as_deref(), Some("New VCA"));
    let v = &vcas(&t.s)[0];
    assert_eq!(
        (
            v.name.as_str(),
            v.colour,
            v.members.clone(),
            v.gain_db,
            v.mute,
            v.solo,
            v.automation
        ),
        (
            "VCA 1",
            SwatchRow::Red,
            vec![t.kick, t.snare],
            0.0,
            false,
            false,
            Mode::Read
        )
    );
    for k in [t.kick, t.snare] {
        let r = strip_row(&t.s, k);
        assert_eq!(
            (r.vcas, r.vca_offset_db, r.vca_mark),
            (vec![v.id], 0.0, false)
        );
    }
    assert!(strip_row(&t.s, t.bus).vcas.is_empty());
    // An empty one (Ruling 11), named and coloured by the count; a bus may
    // join, as may a strip already in a VCA.
    t.s.run_mixer("mixer.new_vca".into(), vec![]).unwrap();
    let w = &vcas(&t.s)[1];
    assert_eq!(
        (w.name.as_str(), w.colour, w.members.len()),
        ("VCA 2", SwatchRow::Orange, 0)
    );
    t.s.run_mixer(
        "mixer.assign_vca".into(),
        vec![
            id("vca", w.id),
            text("strips", &format!("{},{}", t.bus, t.kick)),
        ],
    )
    .unwrap();
    assert_eq!(undo_label(&t.s).as_deref(), Some("Assign to VCA"));
    assert_eq!(strip_row(&t.s, t.kick).vcas, vec![v.id, w.id]);
    t.s.run_mixer(
        "mixer.unassign_vca".into(),
        vec![id("vca", w.id), text("strips", &t.bus.to_string())],
    )
    .unwrap();
    assert_eq!(undo_label(&t.s).as_deref(), Some("Remove from VCA"));
    assert_eq!(vcas(&t.s)[1].members, vec![t.kick]);
    t.s.run_mixer(
        "mixer.rename_vca".into(),
        vec![id("vca", w.id), text("name", "  Kit  ")],
    )
    .unwrap();
    assert_eq!(undo_label(&t.s).as_deref(), Some("Rename VCA"));
    t.s.run_mixer(
        "mixer.set_vca_colour".into(),
        vec![id("vca", w.id), text("colour", "teal")],
    )
    .unwrap();
    assert_eq!(undo_label(&t.s).as_deref(), Some("VCA Colour"));
    let w = &vcas(&t.s)[1];
    assert_eq!((w.name.as_str(), w.colour), ("Kit", SwatchRow::Teal));
}

/// The left channel of `doc`'s sequence `seq` played offline for 20
/// blocks, with the test inserts.
fn played(doc: &Document, seq: SeqId) -> Vec<f32> {
    let (mut engine, r) =
        Engine::new(EngineConfig::new(SR), Box::new(OfflineSinks::new(10))).unwrap();
    engine.set_insert_factory(Box::new(TestInserts::new()));
    engine.load(doc, seq).unwrap();
    let mut b = OfflineBackend::new(SR, 512);
    b.start(r).unwrap();
    engine.play().unwrap();
    let mut out = Vec::new();
    for _ in 0..20 {
        b.reserve(512);
        b.block(512);
        out.extend_from_slice(b.output(512).0);
    }
    assert!(engine.faults().is_empty());
    drop(b.stop().unwrap());
    out
}

/// The saved song at `path` with a noise insert on `strip`, then `edits`.
fn noisy(path: &Path, strip: u64, edits: &[Command]) -> Document {
    let mut b = Builder::from_doc(core_model::file::load(path).unwrap());
    b.insert(StripId(strip), TestInserts::plugin("noise=9"))
        .unwrap();
    for c in edits {
        b.run(c.clone()).unwrap();
    }
    b.finish()
}

fn bits(v: &[f32]) -> Vec<u32> {
    v.iter().map(|x| x.to_bits()).collect()
}

/// Spec §2: a VCA's level is heard on its members, and no member's fader
/// moves. Kick's fader stays at its −1.5 while its offset is −3; the
/// engine plays the saved song (noise on Kick) sample for sample as the
/// same song with no VCA level and Kick's own fader at −4.5.
#[test]
fn set_vca_changes_heard_not_faders() {
    let _g = serial();
    let t = song();
    t.s.run_mixer(
        "mixer.set_strip".into(),
        vec![id("strip", t.kick), text("gain_db", "-1.5")],
    )
    .unwrap();
    let vca = new_vca(&t.s, &[t.kick], "Kit");
    let before = step(&t.s);
    set_vca(&t.s, vca, vec![text("gain_db", "-3")]);
    one_step(&t.s, before);
    assert_eq!(undo_label(&t.s).as_deref(), Some("Set VCA"));
    let k = strip_row(&t.s, t.kick);
    assert_eq!((k.gain_db, k.vca_offset_db), (-1.5, -3.0));
    assert_eq!(strip_row(&t.s, t.snare).vca_offset_db, 0.0);
    assert_eq!(vcas(&t.s)[0].gain_db, -3.0);
    t.s.save().unwrap();

    let seq = SeqId(t.seq);
    let heard = played(&noisy(&t.path, t.kick, &[]), seq);
    let by_fader = played(
        &noisy(
            &t.path,
            t.kick,
            &[
                Command::Groups(GroupCmd::SetVcaLevel {
                    vca: VcaId(vca),
                    gain_db: 0.0,
                }),
                Command::Mixer(MixerCmd::SetGain {
                    strip: StripId(t.kick),
                    gain_db: -4.5,
                }),
            ],
        ),
        seq,
    );
    let at_own = played(
        &noisy(
            &t.path,
            t.kick,
            &[Command::Groups(GroupCmd::SetVcaLevel {
                vca: VcaId(vca),
                gain_db: 0.0,
            })],
        ),
        seq,
    );
    assert!(heard.iter().any(|x| *x != 0.0), "the noise plays");
    assert_eq!(bits(&heard), bits(&by_fader), "heard at fader −3");
    assert_ne!(bits(&heard), bits(&at_own), "the VCA is heard");
}

/// Spec §2a: the mark lights while a VCA mutes or solos the strip; the
/// strip's own switches stay off. `mixer.set_vca` is "Set VCA" whatever
/// it sets (preflight F11), and all four settings are one step.
#[test]
fn vca_mark_lights_when_muting_or_soloing() {
    let _g = serial();
    let t = song();
    let vca = new_vca(&t.s, &[t.kick, t.snare], "Kit");
    let mark = |s: &Session| strip_row(s, t.kick).vca_mark;
    let own = |s: &Session| {
        [t.kick, t.snare]
            .map(|k| strip_row(s, k))
            .iter()
            .all(|r| !r.mute && !r.solo)
    };
    assert!(!mark(&t.s));
    set_vca(&t.s, vca, vec![flag("mute", true)]);
    assert_eq!(undo_label(&t.s).as_deref(), Some("Set VCA"));
    assert!(mark(&t.s) && strip_row(&t.s, t.snare).vca_mark && own(&t.s));
    assert!(vcas(&t.s)[0].mute);
    set_vca(&t.s, vca, vec![flag("mute", false)]);
    assert!(!mark(&t.s) && own(&t.s));
    set_vca(&t.s, vca, vec![flag("solo", true)]);
    assert_eq!(undo_label(&t.s).as_deref(), Some("Set VCA"));
    assert!(mark(&t.s) && own(&t.s));
    // A VCA solo counts as the members' solo: the bus is solo-muted, they
    // are not.
    assert!(strip_row(&t.s, t.bus).solo_muted);
    assert!(!strip_row(&t.s, t.kick).solo_muted);
    assert!(!strip_row(&t.s, t.bus).vca_mark);

    let before = step(&t.s);
    set_vca(
        &t.s,
        vca,
        vec![
            text("gain_db", "-6"),
            flag("mute", true),
            flag("solo", false),
            text("mode", "latch"),
        ],
    );
    one_step(&t.s, before);
    assert_eq!(undo_label(&t.s).as_deref(), Some("Set VCA"));
    let v = &vcas(&t.s)[0];
    assert_eq!(
        (v.gain_db, v.mute, v.solo, v.automation),
        (-6.0, true, false, Mode::Latch)
    );
    set_vca(&t.s, vca, vec![text("mode", "off")]);
    assert_eq!(undo_label(&t.s).as_deref(), Some("Set VCA"));
    t.s.undo().unwrap();
    t.s.undo().unwrap();
    let v = &vcas(&t.s)[0];
    assert_eq!(
        (v.gain_db, v.mute, v.solo, v.automation),
        (0.0, false, true, Mode::Read),
        "one undo takes back all four"
    );
}

#[test]
fn deleting_a_vca_is_one_step() {
    let _g = serial();
    let t = song();
    let vca = new_vca(&t.s, &[t.kick, t.snare], "Kit");
    set_vca(&t.s, vca, vec![text("gain_db", "-3")]);
    let kept = vcas(&t.s);
    let before = step(&t.s);
    t.s.run_mixer("mixer.delete_vca".into(), vec![id("vca", vca)])
        .unwrap();
    one_step(&t.s, before);
    assert_eq!(undo_label(&t.s).as_deref(), Some("Delete VCA"));
    assert!(vcas(&t.s).is_empty());
    for k in [t.kick, t.snare] {
        let r = strip_row(&t.s, k);
        assert_eq!(
            (r.gain_db, r.vcas.len(), r.vca_offset_db),
            (0.0, 0, 0.0),
            "members as their faders say"
        );
    }
    t.s.undo().unwrap();
    assert_eq!(vcas(&t.s), kept, "the VCA and its members come back");
    assert_eq!(strip_row(&t.s, t.kick).vca_offset_db, -3.0);
}

/// Review Focus 3: Kick is in group "Drums" and is the only member of VCA
/// "Kit". Deleting its track is one undo step; "Kit" stays, empty, and
/// "Drums" no longer holds Kick. One undo brings back the track, the
/// strip, and both memberships.
#[test]
fn deleting_a_member_strip_is_one_step_and_undo_restores_membership() {
    let _g = serial();
    let t = song();
    t.s.run_mixer(
        "mixer.new_group".into(),
        vec![
            text("strips", &format!("{},{}", t.kick, t.snare)),
            text("name", "Drums"),
        ],
    )
    .unwrap();
    let vca = new_vca(&t.s, &[t.kick], "Kit");
    let mixer_before = mix(&t.s);
    let lanes_before = t.s.lanes(t.seq).unwrap();
    let lane = lanes_before
        .iter()
        .find(|l| l.name == "Audio 1")
        .unwrap_or(&lanes_before[0])
        .id;
    let before = step(&t.s);
    t.s.edit(
        EditCommand::Lane {
            edit: LaneEdit::Remove { seq: t.seq, lane },
        },
        None,
    )
    .unwrap();
    assert_ne!(step(&t.s), before);
    let m = t.s.mixer().unwrap();
    assert!(m.strips.iter().all(|s| s.id != t.kick), "the strip went");
    assert_eq!(m.vcas.len(), 1);
    assert_eq!((m.vcas[0].id, m.vcas[0].name.as_str()), (vca, "Kit"));
    assert!(m.vcas[0].members.is_empty(), "the VCA stays, empty");
    assert_eq!(m.groups[0].members, vec![t.snare], "Drums lacks Kick");
    assert_eq!(t.s.lanes(t.seq).unwrap().len(), lanes_before.len() - 1);

    t.s.undo().unwrap();
    assert_eq!(step(&t.s), before);
    assert_eq!(t.s.lanes(t.seq).unwrap(), lanes_before, "the track is back");
    let m = t.s.mixer().unwrap();
    assert_eq!(m.groups, mixer_before.1, "Drums holds Kick again");
    assert_eq!(m.vcas, mixer_before.2, "Kit holds Kick again");
    let k = strip_row(&t.s, t.kick);
    assert_eq!((k.group, k.vcas), (Some(m.groups[0].id), vec![vca]));
    assert_eq!(mix(&t.s), mixer_before, "the strip as it was");
}

/// Each refused call changes nothing: the same step, the same mixer.
#[test]
fn run_vca_commands_refuse_hostile_input() {
    let _g = serial();
    let t = song();
    let vca = new_vca(&t.s, &[t.kick], "Kit");
    let mixer = mix(&t.s);
    let at = step(&t.s);
    let huge = "x".repeat(70_000);
    let cases: Vec<(&str, Vec<ControlArg>)> = vec![
        (
            "mixer.set_vca",
            vec![id("vca", u64::MAX), flag("mute", true)],
        ),
        ("mixer.set_vca", vec![int("vca", -1), flag("mute", true)]),
        (
            "mixer.set_vca",
            vec![id("vca", vca + 1), flag("mute", true)],
        ),
        ("mixer.delete_vca", vec![id("vca", u64::MAX)]),
        (
            "mixer.rename_vca",
            vec![id("vca", u64::MAX), text("name", "x")],
        ),
        (
            "mixer.set_vca",
            vec![id("vca", vca), text("gain_db", "1e3")],
        ),
        (
            "mixer.set_vca",
            vec![id("vca", vca), text("gain_db", "NaN")],
        ),
        (
            "mixer.set_vca",
            vec![id("vca", vca), text("gain_db", "12.5")],
        ),
        ("mixer.set_vca", vec![id("vca", vca), text("gain_db", "")]),
        (
            "mixer.set_vca",
            vec![id("vca", vca), text("gain_db", "inf")],
        ),
        ("mixer.set_vca", vec![id("vca", vca), int("gain_db", -3)]),
        ("mixer.set_vca", vec![id("vca", vca), text("mode", "write")]),
        ("mixer.set_vca", vec![id("vca", vca), text("mode", "trim")]),
        ("mixer.set_vca", vec![id("vca", vca), text("mode", "Read")]),
        (
            "mixer.set_vca",
            vec![id("vca", vca), text("gain_db", "-3"), text("mode", "trim")],
        ),
        ("mixer.set_vca", vec![id("vca", vca)]),
        (
            "mixer.new_vca",
            vec![text("strips", &format!("{},{}", t.snare, t.master))],
        ),
        (
            "mixer.assign_vca",
            vec![id("vca", vca), text("strips", &t.master.to_string())],
        ),
        (
            "mixer.assign_vca",
            vec![id("vca", vca), text("strips", &t.kick.to_string())],
        ),
        (
            "mixer.assign_vca",
            vec![id("vca", vca), text("strips", "999999")],
        ),
        ("mixer.assign_vca", vec![id("vca", vca)]),
        (
            "mixer.unassign_vca",
            vec![id("vca", vca), text("strips", &t.snare.to_string())],
        ),
        ("mixer.new_vca", vec![text("strips", "1,1")]),
        ("mixer.new_vca", vec![text("name", &huge)]),
        ("mixer.new_vca", vec![text("name", "   ")]),
        ("mixer.new_vca", vec![text("name", "a\u{7}b")]),
        (
            "mixer.rename_vca",
            vec![id("vca", vca), text("name", &huge)],
        ),
        ("mixer.new_vca", vec![text("colour", "none")]),
        (
            "mixer.set_vca_colour",
            vec![id("vca", vca), text("colour", "mauve")],
        ),
    ];
    for (cmd, args) in cases {
        let shown = format!("{cmd} {args:?}");
        let shown: String = shown.chars().take(200).collect();
        assert!(t.s.run_mixer(cmd.into(), args).is_err(), "{shown}");
        assert_eq!(step(&t.s), at, "{shown}");
        assert_eq!(mix(&t.s), mixer, "{shown}");
    }
    // Write and Trim name Ruling 6's modes.
    let e =
        t.s.run_mixer(
            "mixer.set_vca".into(),
            vec![id("vca", vca), text("mode", "trim")],
        )
        .unwrap_err();
    assert!(format!("{e:?}").contains("mode"), "{e:?}");
}

/// The cap: a song holds at most `MAX_VCAS` VCAs, so the next is refused
/// with a sentence and changes nothing; deleting one makes room.
#[test]
fn a_vca_past_the_cap_is_refused() {
    let _g = serial();
    let t = song();
    for _ in 0..MAX_VCAS {
        t.s.run_mixer("mixer.new_vca".into(), vec![]).unwrap();
    }
    assert_eq!(vcas(&t.s).len(), MAX_VCAS);
    let (mixer, at) = (mix(&t.s), step(&t.s));
    let e =
        t.s.run_mixer(
            "mixer.new_vca".into(),
            vec![text("strips", &t.kick.to_string())],
        )
        .unwrap_err();
    assert!(
        format!("{e:?}").contains("A song holds at most 64 VCAs; delete one first."),
        "{e:?}"
    );
    assert_eq!((mix(&t.s), step(&t.s)), (mixer, at));
    let first = vcas(&t.s)[0].id;
    t.s.run_mixer("mixer.delete_vca".into(), vec![id("vca", first)])
        .unwrap();
    t.s.run_mixer("mixer.new_vca".into(), vec![]).unwrap();
    assert_eq!(vcas(&t.s).len(), MAX_VCAS);
}

/// A hostile file with more than `MAX_VCAS` VCAs opens with the first 64
/// by ID, the rest dropped, and says so on the first `idle`. A strip that
/// was only in a dropped VCA hears no offset.
#[test]
fn a_file_with_too_many_vcas_keeps_the_first_64() {
    let _g = serial();
    let t = song();
    let first = new_vca(&t.s, &[t.kick], "First");
    t.s.save().unwrap();
    let file = t.path.join(core_model::file::PROJECT_FILE);
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    let list = json["document"]["mixer"]["vcas"].as_array_mut().unwrap();
    let template = list[0].clone();
    for n in 2..=70u64 {
        let mut v = template.clone();
        v["id"] = n.into();
        v["name"] = format!("Extra {n}").into();
        v["members"] = if n == 70 {
            serde_json::json!([t.snare])
        } else {
            serde_json::json!([])
        };
        v["gain_db"] = (-6.0).into();
        list.push(v);
    }
    json["document"]["counters"]["vca"] = 71.into();
    std::fs::write(&file, serde_json::to_string(&json).unwrap()).unwrap();
    t.s.close().unwrap();

    let s = Session::open(t.path.to_string_lossy().into_owned()).unwrap();
    let kept = vcas(&s);
    assert_eq!(kept.len(), MAX_VCAS);
    assert_eq!(kept[0].id, first);
    assert_eq!(kept.last().unwrap().id, MAX_VCAS as u64);
    assert_eq!(strip_row(&s, t.snare).vca_offset_db, 0.0);
    assert!(strip_row(&s, t.snare).vcas.is_empty());
    let problems = s.idle().unwrap().problems;
    let line =
        "This song has 70 VCAs; a song holds at most 64, so the last 6 (by ID) were left out.";
    assert!(problems.iter().any(|p| p == line), "{problems:?}");
    // The same file read by the model directly is capped too.
    let doc = core_model::file::load(&t.path).unwrap();
    assert_eq!(doc.mixer().vcas().len(), MAX_VCAS);
    let (_, notes) = core_model::file::load_noted(&t.path).unwrap();
    assert_eq!(notes, vec![line.to_string()]);
    s.close().unwrap();
}
