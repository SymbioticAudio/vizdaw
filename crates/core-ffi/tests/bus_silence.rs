//! A Co-Producer bus the composer's tracks feed (owner, 2026-11-17; plan
//! 8e Task 6c): muting it, setting its fader to −∞, changing its output,
//! or automation that does one of these, asks first, and its item says
//! whose tracks go silent or where they now reach. The same changes on a
//! bus only Co-Producer feeds stay free, as does everything else on it.
//!
//! No plug-in loads: the strips are audio tracks' with no EQ. Offline only.

use core_model::document::Document;
use std::sync::{Arc, Mutex, MutexGuard};
use vizdaw_ffi::co_producer::{CoProducerApproval, NEEDS_APPROVAL, PlannedRow, PreviewRow};
use vizdaw_ffi::control::{ControlArg, ControlValue};
use vizdaw_ffi::edit::{EditCommand, SequenceEdit};
use vizdaw_ffi::host::{force_offline, reset_host};
use vizdaw_ffi::{Session, StripRow, TrackKindRow, VizError, set_new_channel_eq};

static SERIAL: Mutex<()> = Mutex::new(());

const STAMP: &str = "2026-11-17 10-00";
const BAR: i64 = 61_440;

/// One test at a time; offline; no EQ on new channels, so nothing loads.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    vizdaw_ffi::host::set_offline_rate(48_000.0);
    reset_host();
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

fn planned(id: &str, args: Vec<ControlArg>) -> PlannedRow {
    PlannedRow {
        id: id.into(),
        args,
    }
}

fn doc(s: &Session) -> Document {
    core_model::file::from_json(&s.debug_document_json()).unwrap()
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

fn items(p: &PreviewRow) -> Vec<(String, String, bool)> {
    p.items
        .iter()
        .map(|i| (i.thing.clone(), i.change.clone(), i.composers_work))
        .collect()
}

/// The composer's audio tracks Kick, Snare, Bass, Hat and Clap (each on
/// its own strip, to master) in a second sequence (`automation.write`
/// refuses sequence 0), then Co-Producer's buses Drums and Verb and its
/// own audio track Loop, made in a request of their own.
struct Song {
    s: Arc<Session>,
    seq: u64,
    kick: u64,
    snare: u64,
    bass: u64,
    hat: u64,
    clap: u64,
    drums: u64,
    verb: u64,
    lp: u64,
}

fn song() -> Song {
    let s = Session::new_empty().unwrap();
    let first = s.snapshot().unwrap().sequences[0].id;
    s.edit(
        EditCommand::Sequence {
            edit: SequenceEdit::New {
                name: "Song".into(),
            },
        },
        None,
    )
    .unwrap();
    let seq = s
        .snapshot()
        .unwrap()
        .sequences
        .iter()
        .map(|q| q.id)
        .find(|q| *q != first)
        .unwrap();
    let mut ids = Vec::new();
    for (i, name) in ["Kick", "Snare", "Bass", "Hat", "Clap"].iter().enumerate() {
        s.new_track(seq, TrackKindRow::Audio, None).unwrap();
        let id = strip_id(&s, &format!("Audio {}", i + 1));
        s.rename_strip(id, (*name).into()).unwrap();
        ids.push(id);
    }
    s.co_producer_begin("add the buses".into()).unwrap();
    s.co_producer_enter(STAMP.into()).unwrap();
    s.add_bus("Drums".into()).unwrap();
    s.add_bus("Verb".into()).unwrap();
    s.new_track(seq, TrackKindRow::Audio, None).unwrap();
    s.co_producer_leave();
    s.co_producer_end().unwrap();
    let lp = strip_id(&s, "Audio 6 (Co-Producer)");
    Song {
        seq,
        kick: ids[0],
        snare: ids[1],
        bass: ids[2],
        hat: ids[3],
        clap: ids[4],
        drums: strip_id(&s, "Drums (Co-Producer)"),
        verb: strip_id(&s, "Verb (Co-Producer)"),
        lp,
        s,
    }
}

/// One Co-Producer call, as the app's apply makes it: the take with
/// `approval`, then each command through `run_mixer` or `run_creative`.
fn take_with(
    s: &Session,
    token: String,
    plan: &[PlannedRow],
    approval: CoProducerApproval,
) -> Result<(), VizError> {
    s.co_producer_enter(STAMP.into()).unwrap();
    let r = s
        .co_producer_take(token, plan.to_vec(), approval)
        .and_then(|()| {
            for p in plan {
                if p.id == "automation.write" {
                    s.run_creative(p.id.clone(), p.args.clone())?;
                } else {
                    s.run_mixer(p.id.clone(), p.args.clone())?;
                }
            }
            Ok(())
        });
    s.co_producer_leave();
    r
}

fn mute(strip: u64, on: bool) -> PlannedRow {
    planned(
        "mixer.set_strip",
        vec![id("strip", strip), flag("mute", on)],
    )
}

fn fader(strip: u64, db: &str) -> PlannedRow {
    planned(
        "mixer.set_strip",
        vec![id("strip", strip), text("gain_db", db)],
    )
}

fn output(strip: u64, bus: u64) -> PlannedRow {
    planned("mixer.set_output", vec![id("strip", strip), id("bus", bus)])
}

/// `automation.write` of `target` over bar 1 of `seq`.
fn automate(seq: u64, target: &str, points: &str) -> PlannedRow {
    planned(
        "automation.write",
        vec![
            id("seq", seq),
            text("target", target),
            int("start", 0),
            int("end", BAR),
            text("points", points),
        ],
    )
}

/// The previewed item of `thing`, which must be the only one.
fn item_of(p: &PreviewRow, thing: &str) -> (String, bool) {
    let found: Vec<_> = items(p).into_iter().filter(|i| i.0 == thing).collect();
    assert_eq!(found.len(), 1, "{thing}: {p:?}");
    (found[0].1.clone(), found[0].2)
}

const DRUMS: &str = "Strip Drums (Co-Producer)";

#[test]
fn muting_a_bus_the_composer_feeds_asks() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.set_output(t.kick, Some(t.drums)).unwrap();
    s.co_producer_begin("mute the drums".into()).unwrap();
    let p = s.co_producer_preview(vec![mute(t.drums, true)]).unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert_eq!(
        item_of(&p, DRUMS),
        ("muted; Kick goes silent through it".into(), true)
    );
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn setting_a_bus_the_composer_feeds_to_minus_infinity_asks() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    // A send feeds it as an output does.
    s.add_send(t.kick, t.drums).unwrap();
    s.add_send(t.snare, t.drums).unwrap();
    s.co_producer_begin("pull the drums".into()).unwrap();
    let p = s.co_producer_preview(vec![fader(t.drums, "-144")]).unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert_eq!(
        item_of(&p, DRUMS),
        (
            "volume 0.0 dB → −∞ dB; Kick and Snare go silent through it".into(),
            true
        )
    );
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn changing_the_output_of_a_bus_the_composer_feeds_asks() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    for strip in [t.kick, t.snare, t.bass, t.hat, t.clap] {
        s.set_output(strip, Some(t.drums)).unwrap();
    }
    s.co_producer_begin("route the drums".into()).unwrap();
    let p = s
        .co_producer_preview(vec![output(t.drums, t.verb)])
        .unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert_eq!(
        item_of(&p, DRUMS),
        (
            "output Master → Verb (Co-Producer); Kick, Snare, Bass and 2 more now reach Verb (Co-Producer) through it"
                .into(),
            true
        )
    );
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn mute_automation_on_a_bus_the_composer_feeds_asks() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.set_output(t.kick, Some(t.drums)).unwrap();
    s.co_producer_begin("automate the drums".into()).unwrap();
    let target = format!("mute:{}", t.drums);
    let p = s
        .co_producer_preview(vec![automate(t.seq, &target, "0:1")])
        .unwrap();
    assert!(p.needs_yes, "{p:?}");
    let (change, asks) = item_of(&p, DRUMS);
    assert!(asks, "{p:?}");
    assert!(
        change.ends_with("; Kick goes silent through it"),
        "{change}"
    );
    // A fader point at −144 does too.
    let target = format!("fader:{}", t.drums);
    let p = s
        .co_producer_preview(vec![automate(t.seq, &target, "0:0 30720:-144")])
        .unwrap();
    assert!(p.needs_yes, "{p:?}");
    let (change, asks) = item_of(&p, DRUMS);
    assert!(asks, "{p:?}");
    assert!(
        change.ends_with("; Kick goes silent through it"),
        "{change}"
    );
    // Mute points that never turn it on stay free.
    let target = format!("mute:{}", t.drums);
    let p = s
        .co_producer_preview(vec![automate(t.seq, &target, "0:0")])
        .unwrap();
    assert!(!p.needs_yes, "{p:?}");
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn the_same_changes_on_a_bus_only_co_producer_feeds_stay_free() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.co_producer_begin("mix the loop".into()).unwrap();
    // Co-Producer's own Loop feeds Drums.
    let p = s.co_producer_preview(vec![output(t.lp, t.drums)]).unwrap();
    assert!(!p.needs_yes, "{p:?}");
    take_with(
        s,
        p.token,
        &[output(t.lp, t.drums)],
        CoProducerApproval::None,
    )
    .unwrap();
    let (mute_target, fader_target) = (format!("mute:{}", t.drums), format!("fader:{}", t.drums));
    for plan in [
        mute(t.drums, true),
        fader(t.drums, "-144"),
        output(t.drums, t.verb),
        automate(t.seq, &mute_target, "0:1"),
        automate(t.seq, &fader_target, "0:-144"),
    ] {
        let p = s.co_producer_preview(vec![plan.clone()]).unwrap();
        assert!(!p.needs_yes, "{}: {p:?}", plan.id);
        assert!(
            p.items.iter().all(|i| !i.change.contains("through it")),
            "{p:?}"
        );
    }
    // With the composer's Kick feeding it, everything else stays free:
    // unmute, other fader moves.
    s.co_producer_end().unwrap();
    s.set_output(t.kick, Some(t.drums)).unwrap();
    s.set_strip_mute(t.drums, true).unwrap();
    s.co_producer_begin("mix the drums".into()).unwrap();
    for plan in [mute(t.drums, false), fader(t.drums, "-60")] {
        let p = s.co_producer_preview(vec![plan.clone()]).unwrap();
        assert!(!p.needs_yes, "{}: {p:?}", plan.id);
    }
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn a_feed_through_a_second_bus_counts() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    // Kick → Verb (Co-Producer's) → Drums.
    s.set_output(t.kick, Some(t.verb)).unwrap();
    s.co_producer_begin("chain the buses".into()).unwrap();
    let p = s
        .co_producer_preview(vec![output(t.verb, t.drums)])
        .unwrap();
    take_with(
        s,
        p.token,
        &[output(t.verb, t.drums)],
        CoProducerApproval::Sheet,
    )
    .unwrap();
    assert_eq!(strip_row(s, t.verb).output, Some(t.drums));
    let p = s.co_producer_preview(vec![mute(t.drums, true)]).unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert_eq!(
        item_of(&p, DRUMS),
        ("muted; Kick goes silent through it".into(), true)
    );
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn a_silencing_preview_matches_what_the_apply_does() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.set_output(t.kick, Some(t.drums)).unwrap();
    s.add_send(t.snare, t.drums).unwrap();
    s.co_producer_begin("mute the drums".into()).unwrap();
    let plan = vec![mute(t.drums, true)];
    let p = s.co_producer_preview(plan.clone()).unwrap();
    assert!(p.needs_yes);
    // Without VizDAW's own approval the take is refused and nothing runs.
    let before = s.debug_document_json();
    let e = take_with(s, p.token, &plan, CoProducerApproval::None).unwrap_err();
    assert_eq!(e.to_string(), NEEDS_APPROVAL);
    assert_eq!(s.debug_document_json(), before);
    // With it, the apply does what the preview said.
    let p = s.co_producer_preview(plan.clone()).unwrap();
    let start = doc(s);
    take_with(s, p.token, &plan, CoProducerApproval::Sheet).unwrap();
    assert!(strip_row(s, t.drums).mute);
    let said: Vec<(String, String)> = core_model::diff::describe(&start, &doc(s))
        .into_iter()
        .map(|i| {
            let line = if i.thing == DRUMS {
                "; Kick and Snare go silent through it"
            } else {
                ""
            };
            (i.thing, format!("{}{line}", i.change))
        })
        .collect();
    let shown: Vec<(String, String)> = p.items.into_iter().map(|i| (i.thing, i.change)).collect();
    assert_eq!(said, shown);
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

/// Fix round 1: removing a Co-Producer bus the composer's sound reaches,
/// here through another bus's send, asks as muting it does.
#[test]
fn removing_a_bus_the_composer_reaches_through_a_send_asks() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    // Kick → Verb (Co-Producer's), and Verb sends to Drums.
    s.set_output(t.kick, Some(t.verb)).unwrap();
    s.co_producer_begin("send the verb".into()).unwrap();
    let send = planned(
        "mixer.add_send",
        vec![id("strip", t.verb), id("bus", t.drums)],
    );
    let p = s.co_producer_preview(vec![send.clone()]).unwrap();
    take_with(s, p.token, &[send], CoProducerApproval::Sheet).unwrap();
    let remove = planned("mixer.remove_bus", vec![id("strip", t.drums)]);
    let p = s.co_producer_preview(vec![remove]).unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert_eq!(
        item_of(&p, DRUMS),
        ("deleted; Kick goes silent through it".into(), true)
    );
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

// ---- groups-vcas-transport Task 7 fix round 1: what is heard decides.
// A VCA's mute, or its level at −∞, silences a Co-Producer bus as the
// bus's own mute or fader would. ----

fn vca_named(s: &Session, name: &str) -> u64 {
    s.mixer()
        .unwrap()
        .vcas
        .into_iter()
        .find(|v| v.name == name)
        .unwrap_or_else(|| panic!("no VCA {name}"))
        .id
}

fn set_vca(vca: u64, more: Vec<ControlArg>) -> PlannedRow {
    let mut args = vec![id("vca", vca)];
    args.extend(more);
    planned("mixer.set_vca", args)
}

fn assign(vca: u64, strip: u64) -> PlannedRow {
    planned(
        "mixer.assign_vca",
        vec![id("vca", vca), text("strips", &strip.to_string())],
    )
}

/// Co-Producer's empty VCA "Ride" (an add, run directly in the open
/// request), then `bus` assigned to it through a preview (free: Ride
/// holds only Co-Producer's bus at 0 dB, which changes nothing heard).
fn ride_with(s: &Session, bus: u64) -> u64 {
    s.co_producer_enter(STAMP.into()).unwrap();
    s.run_mixer("mixer.new_vca".into(), vec![text("name", "Ride")])
        .unwrap();
    s.co_producer_leave();
    let ride = vca_named(s, "Ride");
    let plan = vec![assign(ride, bus)];
    let p = s.co_producer_preview(plan.clone()).unwrap();
    assert!(!p.needs_yes, "{p:?}");
    take_with(s, p.token, &plan, CoProducerApproval::None).unwrap();
    ride
}

#[test]
fn muting_a_vca_that_holds_a_bus_the_composer_feeds_asks() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.add_send(t.kick, t.drums).unwrap();
    s.co_producer_begin("ride the drums".into()).unwrap();
    let ride = ride_with(s, t.drums);
    let p = s
        .co_producer_preview(vec![set_vca(ride, vec![flag("mute", true)])])
        .unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert_eq!(
        item_of(&p, DRUMS),
        ("muted by a VCA; Kick goes silent through it".into(), true)
    );
    let e = take_with(
        s,
        p.token,
        &[set_vca(ride, vec![flag("mute", true)])],
        CoProducerApproval::None,
    )
    .unwrap_err();
    assert_eq!(e.to_string(), NEEDS_APPROVAL);
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn a_vca_taking_a_bus_the_composer_feeds_to_minus_infinity_asks() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.add_send(t.kick, t.drums).unwrap();
    s.co_producer_begin("ride the drums".into()).unwrap();
    let ride = ride_with(s, t.drums);
    let p = s
        .co_producer_preview(vec![set_vca(ride, vec![text("gain_db", "-144")])])
        .unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert_eq!(
        item_of(&p, DRUMS),
        (
            "heard 0.0 dB → −∞ dB; Kick goes silent through it".into(),
            true
        )
    );
    // Lower, but still heard: free.
    let p = s
        .co_producer_preview(vec![set_vca(ride, vec![text("gain_db", "-20")])])
        .unwrap();
    assert!(!p.needs_yes, "{p:?}");
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn assigning_a_bus_the_composer_feeds_to_a_silent_vca_asks() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    s.add_send(t.kick, t.drums).unwrap();
    s.co_producer_begin("ride the drums".into()).unwrap();
    // A VCA of Co-Producer's other bus only, muted, then one at −∞.
    let ride = ride_with(s, t.verb);
    let plan = vec![set_vca(ride, vec![flag("mute", true)])];
    let p = s.co_producer_preview(plan.clone()).unwrap();
    assert!(
        !p.needs_yes,
        "nothing of the composer's reaches Verb: {p:?}"
    );
    take_with(s, p.token, &plan, CoProducerApproval::None).unwrap();
    let p = s.co_producer_preview(vec![assign(ride, t.drums)]).unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert_eq!(
        item_of(&p, DRUMS),
        ("muted by a VCA; Kick goes silent through it".into(), true)
    );
    let plan = vec![set_vca(
        ride,
        vec![flag("mute", false), text("gain_db", "-144")],
    )];
    let p = s.co_producer_preview(plan.clone()).unwrap();
    assert!(!p.needs_yes, "{p:?}");
    take_with(s, p.token, &plan, CoProducerApproval::None).unwrap();
    let p = s.co_producer_preview(vec![assign(ride, t.drums)]).unwrap();
    assert!(p.needs_yes, "{p:?}");
    assert_eq!(
        item_of(&p, DRUMS),
        (
            "heard 0.0 dB → −∞ dB; Kick goes silent through it".into(),
            true
        )
    );
    // Unassigning Verb, which only Co-Producer feeds, stays free.
    let p = s
        .co_producer_preview(vec![planned(
            "mixer.unassign_vca",
            vec![id("vca", ride), text("strips", &t.verb.to_string())],
        )])
        .unwrap();
    assert!(!p.needs_yes, "{p:?}");
    s.co_producer_end().unwrap();
    s.close().unwrap();
}

#[test]
fn a_vca_silencing_a_bus_only_co_producer_feeds_stays_free() {
    let _g = serial();
    let t = song();
    let s = &t.s;
    // Co-Producer's own track Loop feeds Drums; no composer's sound does.
    s.set_output(t.lp, Some(t.drums)).unwrap();
    s.co_producer_begin("ride the drums".into()).unwrap();
    let ride = ride_with(s, t.drums);
    for plan in [
        set_vca(ride, vec![flag("mute", true)]),
        set_vca(ride, vec![text("gain_db", "-144")]),
    ] {
        let p = s.co_producer_preview(vec![plan]).unwrap();
        assert!(!p.needs_yes, "{p:?}");
    }
    s.co_producer_end().unwrap();
    s.close().unwrap();
}
