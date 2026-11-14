//! New audio, instrument and built-in-synth strips get the bundled EQ in
//! their EQ slot, in the same undo step as the command that made them
//! (plan 6g Task 3).

use core_model::command::{Command, LaneCmd, MixerCmd, Report, RoutingCmd, SequenceCmd};
use core_model::document::{DestKind, Document};
use core_model::history::History;
use core_model::ids::{AnyId, DestId, LaneId, OutputPathId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{BUILTIN_SYNTH_REF, PluginFormat, PluginRef, StripKind};
use core_model::selection::ViewFilter;

fn mq() -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: "com.dusk-audio.multi-q".into(),
        name: "Multi-Q".into(),
        vendor: "Dusk Audio".into(),
        version: "0.10.9".into(),
    }
}

fn user_eq() -> PluginRef {
    PluginRef {
        format: PluginFormat::Clap,
        id: "org.example.user-eq".into(),
        name: "User EQ".into(),
        vendor: "Example".into(),
        version: "1".into(),
    }
}

fn run(h: &mut History, cmd: Command) -> Report {
    h.execute(&cmd, &ViewFilter::default()).unwrap()
}

fn history(eq: Option<PluginRef>) -> History {
    let mut h = History::new(Document::new());
    h.set_new_strip_eq(eq);
    run(
        &mut h,
        Command::Sequence(SequenceCmd::New { name: "S".into() }),
    );
    h
}

fn add_output(h: &mut History, name: &str) -> Report {
    run(
        h,
        Command::Routing(RoutingCmd::AddOutput {
            name: name.into(),
            reference: "audio:1-2".into(),
        }),
    )
}

fn add_dest(h: &mut History, name: &str, kind: DestKind) -> Report {
    run(
        h,
        Command::Routing(RoutingCmd::AddDestination {
            name: name.into(),
            kind,
            default_patch: None,
        }),
    )
}

fn synth() -> DestKind {
    DestKind::MidiEndpoint {
        endpoint_ref: BUILTIN_SYNTH_REF.into(),
        group: 0,
        channel: 0,
    }
}

fn output_of(r: &Report) -> OutputPathId {
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::Output(x) => Some(*x),
            _ => None,
        })
        .unwrap()
}

fn dest_of(r: &Report) -> DestId {
    r.created
        .iter()
        .find_map(|id| match id {
            AnyId::Dest(x) => Some(*x),
            _ => None,
        })
        .unwrap()
}

fn strip(h: &History, kind: StripKind) -> StripId {
    h.doc().mixer().find_kind(kind).unwrap()
}

fn eq_of(h: &History, s: StripId) -> Option<PluginRef> {
    h.doc()
        .mixer()
        .strip(s)
        .unwrap()
        .eq
        .as_ref()
        .map(|p| p.plugin.clone())
}

fn plugins(r: &Report) -> Vec<PluginInstanceId> {
    r.created
        .iter()
        .filter_map(|id| match id {
            AnyId::Plugin(x) => Some(*x),
            _ => None,
        })
        .collect()
}

/// Checks the strip's EQ is a fresh Multi-Q whose ID `r` created.
fn assert_filled(h: &History, s: StripId, r: &Report) {
    let st = h.doc().mixer().strip(s).unwrap();
    let eq = st.eq.as_ref().expect("the EQ slot is filled");
    assert_eq!(eq.plugin, mq());
    assert!(eq.state.is_none());
    assert!(eq.params.is_empty());
    assert!(!eq.bypass);
    assert!(eq.sidechain.is_none());
    assert!(r.created.contains(&AnyId::Plugin(eq.id)));
}

#[test]
fn only_new_audio_instrument_and_synth_strips() {
    let mut h = history(Some(mq()));
    let master = h.doc().mixer().master();

    let r = add_output(&mut h, "Audio 1");
    let audio = strip(&h, StripKind::AudioPath(output_of(&r)));
    assert_filled(&h, audio, &r);

    let r = add_dest(&mut h, "Pad", DestKind::plugin("clap:pad"));
    let pad = dest_of(&r);
    let inst = strip(&h, StripKind::Instrument(pad));
    assert_filled(&h, inst, &r);
    // The instrument's own slot is created too, but only one EQ is filled.
    assert_eq!(plugins(&r).len(), 2);

    let r = add_dest(&mut h, "Synth", synth());
    let syn = strip(&h, StripKind::BuiltinSynth);
    assert_filled(&h, syn, &r);

    let r = run(
        &mut h,
        Command::Mixer(MixerCmd::AddBus {
            name: "Bus".into(),
            index: None,
        }),
    );
    let bus = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Strip(x) => Some(*x),
            _ => None,
        })
        .unwrap();
    assert_eq!(eq_of(&h, bus), None);
    assert!(plugins(&r).is_empty());
    assert_eq!(eq_of(&h, master), None);

    let r = run(
        &mut h,
        Command::Mixer(MixerCmd::AddInstrumentOutput {
            dest: pad,
            index: 1,
            name: "Pad 2".into(),
        }),
    );
    let out2 = strip(&h, StripKind::InstrumentOutput(pad, 1));
    assert_eq!(eq_of(&h, out2), None);
    assert!(plugins(&r).is_empty());

    // Strips that existed before keep their EQ: none, or the user's.
    let mut h = history(None);
    let r = add_output(&mut h, "Audio 1");
    let a1 = strip(&h, StripKind::AudioPath(output_of(&r)));
    let r = add_output(&mut h, "Audio 2");
    let a2 = strip(&h, StripKind::AudioPath(output_of(&r)));
    run(
        &mut h,
        Command::Mixer(MixerCmd::SetEq {
            strip: a2,
            plugin: Some(user_eq()),
        }),
    );
    h.set_new_strip_eq(Some(mq()));
    let r = add_output(&mut h, "Audio 3");
    let a3 = strip(&h, StripKind::AudioPath(output_of(&r)));
    assert_eq!(eq_of(&h, a1), None);
    assert_eq!(eq_of(&h, a2), Some(user_eq()));
    assert_filled(&h, a3, &r);
    assert_eq!(plugins(&r).len(), 1);
}

#[test]
fn second_tracks_add_no_strip() {
    let mut h = history(Some(mq()));
    let r = add_output(&mut h, "Audio 1");
    let out = output_of(&r);
    let audio = strip(&h, StripKind::AudioPath(out));
    add_dest(&mut h, "Synth", synth());
    let syn = strip(&h, StripKind::BuiltinSynth);
    // The user removes the synth strip's EQ.
    run(
        &mut h,
        Command::Mixer(MixerCmd::SetEq {
            strip: syn,
            plugin: None,
        }),
    );
    let audio_eq = h.doc().mixer().strip(audio).unwrap().eq.clone();
    let strips = h.doc().mixer().strips().len();

    // A second audio track: the output path exists.
    let r = run(
        &mut h,
        Command::Lane(LaneCmd::Add {
            seq: SeqId(0),
            name: "Audio 2".into(),
            index: None,
        }),
    );
    let lane = r
        .created
        .iter()
        .find_map(|id| match id {
            AnyId::Lane(x) => Some(*x),
            _ => None,
        })
        .unwrap();
    let r2 = run(
        &mut h,
        Command::Lane(LaneCmd::SetOutput {
            seq: SeqId(0),
            lane,
            output: Some(out),
        }),
    );
    assert!(plugins(&r).is_empty() && plugins(&r2).is_empty());

    // A second built-in-synth track: the destination's strip exists.
    let r = add_dest(&mut h, "Synth 2", synth());
    assert!(plugins(&r).is_empty());

    // Duplicate Track.
    let r = run(
        &mut h,
        Command::Lane(LaneCmd::Duplicate {
            seq: SeqId(0),
            lane: LaneId(0),
        }),
    );
    assert!(plugins(&r).is_empty());

    assert_eq!(h.doc().mixer().strips().len(), strips);
    assert_eq!(h.doc().mixer().strip(audio).unwrap().eq, audio_eq);
    assert_eq!(eq_of(&h, syn), None);
}

#[test]
fn the_fill_is_part_of_the_same_step() {
    let mut h = history(Some(mq()));
    let depth = h.undo_depth();
    let r = add_output(&mut h, "Audio 1");
    let out = output_of(&r);
    let s = strip(&h, StripKind::AudioPath(out));
    let id = h.doc().mixer().strip(s).unwrap().eq.as_ref().unwrap().id;
    assert_eq!(h.undo_depth(), depth + 1);

    h.undo().unwrap();
    assert!(
        h.doc()
            .mixer()
            .find_kind(StripKind::AudioPath(out))
            .is_none()
    );
    assert!(h.doc().find_slot(id).is_none());

    h.redo().unwrap();
    assert_eq!(
        h.doc().mixer().strip(s).unwrap().eq.as_ref().unwrap().id,
        id
    );

    // Redo restores the stored version; it does not re-run the command.
    h.undo().unwrap();
    h.set_new_strip_eq(None);
    assert_eq!(h.new_strip_eq(), None);
    h.redo().unwrap();
    assert_eq!(eq_of(&h, s), Some(mq()));
    assert_eq!(
        h.doc().mixer().strip(s).unwrap().eq.as_ref().unwrap().id,
        id
    );
}

#[test]
fn switch_off_leaves_the_slot_empty() {
    let mut h = history(None);
    assert_eq!(h.new_strip_eq(), None);
    let mut created = Vec::new();
    let r = add_output(&mut h, "Audio 1");
    created.extend(r.created.clone());
    let audio = strip(&h, StripKind::AudioPath(output_of(&r)));
    let r = add_dest(&mut h, "Synth", synth());
    created.extend(r.created.clone());
    let r = add_dest(&mut h, "Pad", DestKind::plugin("clap:pad"));
    let pad = dest_of(&r);
    let inst_plugins = plugins(&r);
    created.extend(r.created.clone());
    assert_eq!(eq_of(&h, audio), None);
    assert_eq!(eq_of(&h, strip(&h, StripKind::BuiltinSynth)), None);
    assert_eq!(eq_of(&h, strip(&h, StripKind::Instrument(pad))), None);
    // Only the instrument's own slot was created: no EQ.
    assert_eq!(inst_plugins.len(), 1);
    let eqs = created
        .iter()
        .filter(|id| matches!(id, AnyId::Plugin(_)))
        .count();
    assert_eq!(eqs, 1);
}

#[test]
fn a_command_that_fills_its_own_eq_keeps_it() {
    let mut h = history(Some(mq()));
    h.begin("Add Instrument").unwrap();
    let r = add_dest(&mut h, "Pad", DestKind::plugin("clap:pad"));
    let pad = dest_of(&r);
    let s = strip(&h, StripKind::Instrument(pad));
    let first = h.doc().mixer().strip(s).unwrap().eq.as_ref().unwrap().id;
    let r2 = run(
        &mut h,
        Command::Mixer(MixerCmd::SetEq {
            strip: s,
            plugin: Some(user_eq()),
        }),
    );
    h.commit().unwrap();

    assert_eq!(eq_of(&h, s), Some(user_eq()));
    assert!(h.doc().find_slot(first).is_none());
    assert!(r2.removed.contains(&AnyId::Plugin(first)));
    assert_eq!(h.undo_label(), Some("Add Instrument"));
}
