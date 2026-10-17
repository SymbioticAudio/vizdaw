//! Automation lanes in the schedule (Plan 4d Rulings 7–9).

use core_engine::fixtures::{Builder, Q};
use core_engine::midi1::note_on_velocity;
use core_engine::schedule::{
    AutoDest, Automation, CompileOptions, CompileReport, MAX_AUTO_LANES, Schedule, StripParam,
    TargetIdx, TargetKind, TargetTable, compile,
};
use core_model::automation::{AutomationTarget, SendIndex};
use core_model::document::Document;
use core_model::event::{EventBody, EventSpec};
use core_model::ids::{DestId, PluginInstanceId, SeqId, StripId};
use core_model::mixer::{ParamId, PluginRef, StripKind};
use core_model::time::Tick;

const SR: f64 = 48_000.0;
/// A quarter at 120 BPM is 24 000 frames.
const QF: u64 = 24_000;

fn compile_seq(doc: &Document, seq: SeqId) -> (Schedule, CompileReport) {
    let mut table = TargetTable::new();
    compile(doc, seq, &CompileOptions::new(SR), &mut |k: &TargetKind| {
        table.intern(k.clone()).map(Ok)
    })
    .unwrap()
}

fn auto(t: i64, target: AutomationTarget, value: f64) -> EventSpec {
    EventSpec::new(Tick(t), EventBody::Automation { target, value })
}

#[test]
fn lanes_hold_points_in_frames_and_ramp_or_hold() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, 4 * Q)]).unwrap();
    let s = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let lane = b.lane(seq, "Auto", None).unwrap();
    b.events(
        seq,
        lane,
        vec![
            auto(0, AutomationTarget::StripFader(s), 0.0),
            auto(Q, AutomationTarget::StripFader(s), 1.0),
            auto(2 * Q, AutomationTarget::StripFader(s), 0.5),
            auto(Q, AutomationTarget::StripPan(s), 0.25),
            auto(0, AutomationTarget::StripMute(s), 0.0),
            auto(Q, AutomationTarget::StripMute(s), 1.0),
        ],
    )
    .unwrap();
    let (sch, report) = compile_seq(b.doc(), seq);
    let a = &sch.automation;
    assert_eq!(report.automation_lanes, 3);
    assert_eq!(report.automation_points, 6);
    let dests: Vec<AutoDest> = a.lanes().iter().map(|l| l.dest).collect();
    assert_eq!(
        dests,
        vec![
            AutoDest::Strip(s, StripParam::Fader),
            AutoDest::Strip(s, StripParam::Pan),
            AutoDest::Strip(s, StripParam::Mute),
        ]
    );
    let [fader, pan, mute] = [a.lanes()[0], a.lanes()[1], a.lanes()[2]];
    assert!(!fader.stepped && mute.stepped);
    assert_eq!(a.value(&fader, 0, false), 0.0);
    assert_eq!(a.value(&fader, QF / 2, false), 0.5);
    assert_eq!(a.value(&fader, QF, false), 1.0);
    assert_eq!(a.value(&fader, QF + QF / 2, false), 0.75);
    assert_eq!(a.value(&fader, 10 * QF, false), 0.5, "the last value holds");
    assert_eq!(
        a.value(&pan, 0, false),
        0.25,
        "the first value holds before it"
    );
    assert_eq!(a.value(&mute, QF - 1, false), 0.0);
    assert_eq!(a.value(&mute, QF, false), 1.0);
    assert_eq!(a.value(&mute, QF, true), 0.0, "the left limit at the point");
    assert_eq!(a.next_cut(0), Some(QF));
    assert_eq!(a.next_cut(QF), Some(2 * QF));
    assert_eq!(a.next_cut(2 * QF), None);
    assert!(a.drives(AutoDest::Strip(s, StripParam::Pan)));
    assert!(!a.drives(AutoDest::Strip(s, StripParam::Solo)));
    assert!(
        sch.entries
            .iter()
            .all(|e| sch.bytes(&e.msg)[0] & 0xF0 != 0xB0),
        "no MIDI for automation"
    );
}

#[test]
fn velocity_scale_scales_the_note_ons_of_its_destination() {
    let mut b = Builder::new();
    let a = b.dest("A", "builtin:synth", 0).unwrap();
    let other = b.dest("B", "builtin:synth", 1).unwrap();
    let (seq, la) = b.sequence("S", "A", Some(a)).unwrap();
    let lb = b.lane(seq, "B", Some(other)).unwrap();
    b.notes(
        seq,
        la,
        &[
            (0, 60, 0x8000, Q),
            (2 * Q, 62, 0x8000, Q),
            (5 * Q, 64, 0x8000, Q),
        ],
    )
    .unwrap();
    b.notes(seq, lb, &[(2 * Q, 70, 0x8000, Q)]).unwrap();
    let auto_lane = b.lane(seq, "Auto", None).unwrap();
    b.events(
        seq,
        auto_lane,
        vec![
            auto(0, AutomationTarget::VelocityScale(a), 0.5),
            auto(4 * Q, AutomationTarget::VelocityScale(a), 1.0),
        ],
    )
    .unwrap();
    let (sch, report) = compile_seq(b.doc(), seq);
    assert_eq!(report.automation_lanes, 0, "velocity scale is not a lane");
    let on = |pitch: u8| {
        sch.entries
            .iter()
            .map(|e| sch.bytes(&e.msg))
            .find(|m| m[0] & 0xF0 == 0x90 && m[1] == pitch)
            .unwrap()[2]
    };
    assert_eq!(on(60), 64, "0.5 is 100 %");
    assert_eq!(on(62), note_on_velocity(57_344), "0.75 is 175 %");
    assert_eq!(on(64), 127, "250 % clamps");
    assert_eq!(on(70), 64, "another destination is untouched");
}

#[test]
fn missing_targets_are_skipped_and_reported() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    b.notes(seq, notes, &[(0, 60, 0x8000, Q)]).unwrap();
    let s = b.strip_of(StripKind::BuiltinSynth).unwrap();
    b.events(
        seq,
        notes,
        vec![
            auto(0, AutomationTarget::StripFader(StripId(999)), 0.5),
            auto(0, AutomationTarget::SendLevel(s, SendIndex(3)), 0.5),
            auto(
                0,
                AutomationTarget::PluginParam(PluginInstanceId(77), ParamId(0)),
                0.5,
            ),
            auto(0, AutomationTarget::VelocityScale(DestId(55)), 0.5),
            auto(0, AutomationTarget::StripFader(s), 0.5),
        ],
    )
    .unwrap();
    let (sch, report) = compile_seq(b.doc(), seq);
    assert_eq!(report.automation_missing, 4);
    assert_eq!(report.automation_lanes, 1);
    assert_eq!(
        sch.automation.lanes()[0].dest,
        AutoDest::Strip(s, StripParam::Fader)
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("4 automation point(s) skipped")),
        "{:?}",
        report.warnings
    );
}

#[test]
fn at_most_max_lanes_play_and_plugin_lanes_bind() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (seq, notes) = b.sequence("S", "Notes", Some(d)).unwrap();
    let s = b.strip_of(StripKind::BuiltinSynth).unwrap();
    let slot = b.insert(s, PluginRef::from_legacy("clap:x")).unwrap();
    let events: Vec<EventSpec> = (0..MAX_AUTO_LANES as u64 + 6)
        .map(|p| auto(0, AutomationTarget::PluginParam(slot, ParamId(p)), 0.5))
        .collect();
    b.events(seq, notes, events).unwrap();
    let (mut sch, report) = compile_seq(b.doc(), seq);
    assert_eq!(report.automation_lanes, MAX_AUTO_LANES);
    assert_eq!(report.automation_dropped_lanes, 6);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("6 automation lane(s) dropped"))
    );
    sch.automation
        .mark_stepped(|d| d == AutoDest::Plugin(slot, ParamId(1)));
    sch.automation
        .bind_instruments(|id| (id == slot).then_some(TargetIdx(3)));
    let lanes = sch.automation.lanes();
    assert!(lanes[1].stepped && !lanes[0].stepped);
    assert!(lanes.iter().all(|l| l.instrument == Some(TargetIdx(3))));
    // An empty set of lanes plays nothing.
    let (empty, dropped) = Automation::build(Vec::new());
    assert!(empty.is_empty() && dropped == 0 && empty.next_cut(0).is_none());
}

#[test]
fn a_plugin_instrument_lane_resolves_its_target_without_routed_notes() {
    let mut b = Builder::new();
    let (d, inst) = b
        .plugin_dest("Synth", core_engine::graph::TestInstrument::plugin("synth"))
        .unwrap();
    let (seq, lane) = b.sequence("S", "Auto", None).unwrap();
    // No notes anywhere: only automation of the instrument and of an insert.
    let s = b.strip_of(StripKind::Master).unwrap();
    let ins = b.insert(s, PluginRef::from_legacy("clap:x")).unwrap();
    let _ = d;
    b.events(
        seq,
        lane,
        vec![
            auto(0, AutomationTarget::PluginParam(inst, ParamId(0)), 0.5),
            auto(0, AutomationTarget::PluginParam(ins, ParamId(0)), 0.5),
        ],
    )
    .unwrap();
    let mut table = TargetTable::new();
    let mut asked: Vec<TargetKind> = Vec::new();
    let (sch, report) = compile(
        b.doc(),
        seq,
        &CompileOptions::new(SR),
        &mut |k: &TargetKind| {
            asked.push(k.clone());
            table.intern(k.clone()).map(Ok)
        },
    )
    .unwrap();
    assert_eq!(report.automation_lanes, 2);
    assert_eq!(report.automation_missing, 0);
    assert!(report.unroutable.is_empty());
    assert_eq!(
        asked,
        vec![TargetKind::PluginInstrument(inst)],
        "the insert needs no target"
    );
    let t = table.find(&TargetKind::PluginInstrument(inst));
    assert!(t.is_some());
    // The same binding Engine::load makes: by the slot's owner.
    let mut sch = sch;
    let doc = b.doc();
    sch.automation
        .bind_instruments(|id| match doc.find_slot(id) {
            Some(core_model::document::SlotOwner::Instrument(_)) => {
                table.find(&TargetKind::PluginInstrument(id))
            }
            _ => None,
        });
    let by_dest = |p| {
        sch.automation
            .lanes()
            .iter()
            .find(|l| l.dest == AutoDest::Plugin(p, ParamId(0)))
            .unwrap()
            .instrument
    };
    assert_eq!(by_dest(inst), t);
    assert_eq!(by_dest(ins), None, "an insert is found in the graph");
}
