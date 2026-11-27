//! The surface relay (Swift `SurfaceSettingsTests`, plan 5f-1 Task 10;
//! slice 9d Task 3): drained surface events become ordered steps, applied
//! to the document whose session took them; jogs within 150 ms are one
//! locate; the count-in reaches the core once per change; the surfaces'
//! focus follows the front document.

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use support::{FakeSession, demo_fake, lane_row, mixer_fake};
use vizdaw_app::commands;
use vizdaw_app::surface_relay::{
    DIALOG_COMMANDS, JOG_WINDOW, RelayStep, SurfaceCommand, SurfaceRelay,
};
use vizdaw_app::testing::FakeCore;
use vizdaw_app::{DocumentModel, DocumentStore, WindowToken};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::{EditCommand, LaneEdit};
use vizdaw_ffi::surfaces::SurfaceEventRow;

/// One detent: a sixteenth (`PPQ / 4`).
const DETENT: i64 = 15_360 / 4;

fn open(fake: FakeSession) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(fake);
    let m = DocumentModel::new(f.clone()).unwrap();
    (f, m)
}

fn command(id: &str, programmatic: bool) -> SurfaceEventRow {
    SurfaceEventRow::Command {
        id: id.into(),
        programmatic,
    }
}

fn step(id: &str, programmatic: bool) -> RelayStep {
    RelayStep::Command {
        id: id.into(),
        programmatic,
    }
}

fn locates(f: &FakeSession) -> Vec<i64> {
    f.calls()
        .iter()
        .filter_map(|c| c.strip_prefix("locate(")?.strip_suffix(')')?.parse().ok())
        .collect()
}

/// Applies every step; the commands the window would run, in order.
fn apply_all(m: &mut DocumentModel, steps: Vec<RelayStep>) -> Vec<SurfaceCommand> {
    steps
        .into_iter()
        .filter_map(|s| m.apply_surface_step(s))
        .collect()
}

#[test]
fn events_select_jog_and_run_commands() {
    let (f, mut m) = open(mixer_fake());
    let mut relay = SurfaceRelay::default();
    let t0 = Instant::now();
    let start = m.last_transport().tick;
    let steps = relay.apply(
        vec![
            command("transport.stop", false),
            SurfaceEventRow::Select { strip: 2 },
            SurfaceEventRow::Jog { detents: 4 },
        ],
        t0,
    );
    assert_eq!(
        steps,
        vec![step("transport.stop", false), RelayStep::Select(2)]
    );
    let ran = apply_all(&mut m, steps);
    assert_eq!(
        ran,
        vec![SurfaceCommand {
            id: "transport.stop".into(),
            programmatic: false,
            bypass: true,
        }]
    );
    assert_eq!(m.selected_strips(), &BTreeSet::from([2]));
    let located = relay.flush(t0 + JOG_WINDOW);
    assert_eq!(located, Some(RelayStep::Locate(4)));
    apply_all(&mut m, located.into_iter().collect());
    assert_eq!(locates(&f), vec![start + 4 * DETENT]);
}

/// Pre-flight M3: a spun wheel is one locate.
#[test]
fn jogs_within_150_ms_are_one_locate() {
    let mut relay = SurfaceRelay::default();
    let t0 = Instant::now();
    assert!(
        relay
            .apply(vec![SurfaceEventRow::Jog { detents: 2 }], t0)
            .is_empty()
    );
    let t1 = t0 + Duration::from_millis(100);
    assert!(
        relay
            .apply(vec![SurfaceEventRow::Jog { detents: 3 }], t1)
            .is_empty()
    );
    assert_eq!(relay.flush(t1), None, "still inside the window");
    assert_eq!(
        relay.flush(t1 + Duration::from_millis(60)),
        Some(RelayStep::Locate(5)),
        "one locate for the spin"
    );
    assert_eq!(relay.flush(t1 + Duration::from_secs(1)), None);
    assert_eq!(JOG_WINDOW, Duration::from_millis(150));
}

/// Pre-flight M21: arm needs no mixer window; a strip that is no
/// AudioPath does nothing.
#[test]
fn arm_toggles_the_strips_input_track() {
    let fake = demo_fake();
    let mut vox = lane_row(40, "Vox", false);
    vox.audio_out = Some(7);
    fake.set_lanes(3, vec![vox]);
    let (f, mut m) = open(fake);
    m.select(Some(3));
    assert_eq!(m.input_lane(7).map(|l| l.id), Some(40));
    let n = f.edits().len();
    let mut relay = SurfaceRelay::default();
    let steps = relay.apply(
        vec![
            SurfaceEventRow::Arm {
                strip: 9,
                path: Some(7),
            },
            SurfaceEventRow::Arm {
                strip: 10,
                path: None,
            },
        ],
        Instant::now(),
    );
    assert_eq!(steps, vec![RelayStep::Arm(Some(7)), RelayStep::Arm(None)]);
    assert!(apply_all(&mut m, steps).is_empty());
    assert_eq!(
        f.edits().len(),
        n + 1,
        "a strip that is no AudioPath does nothing"
    );
    assert_eq!(
        f.edits().last(),
        Some(&EditCommand::Lane {
            edit: LaneEdit::SetFlags {
                seq: 3,
                lane: 40,
                mute: None,
                solo: None,
                arm: Some(true),
            },
        })
    );
}

/// Final review R3: a command that can open a dialog runs without the
/// keyboard bypass; steps keep their order across batches.
#[test]
fn dialog_commands_do_not_bypass() {
    let (_f, mut m) = open(mixer_fake());
    let mut relay = SurfaceRelay::default();
    let now = Instant::now();
    let mut steps = relay.apply(vec![command("file.save", false)], now);
    steps.extend(relay.apply(vec![command("transport.stop", false)], now));
    let ran = apply_all(&mut m, steps);
    assert_eq!(
        ran,
        vec![
            SurfaceCommand {
                id: "file.save".into(),
                programmatic: false,
                bypass: false,
            },
            SurfaceCommand {
                id: "transport.stop".into(),
                programmatic: false,
                bypass: true,
            },
        ]
    );
    for id in [
        "file.open",
        "file.close",
        "file.save",
        "file.save_as",
        "file.project_settings",
        "keys.save",
        "keys.load",
        "media.import",
        "media.relink",
        "media.rename",
        "record.recover",
        "region.normalize",
        "file.bounce",
    ] {
        assert!(DIALOG_COMMANDS.contains(&id), "{id}");
        assert!(!SurfaceCommand::new(id, false).bypass, "{id}");
    }
    assert_eq!(DIALOG_COMMANDS.len(), 13);
}

/// Ruling T9-1: a surface button is not a keystroke, so it acts while a
/// text field has the keyboard; the keys still wait.
#[test]
fn a_surface_button_acts_while_a_text_field_has_the_keyboard() {
    let (f, mut m) = open(mixer_fake());
    m.begin_text_editing("track-rename");
    assert!(!m.transport_keys_enabled());
    assert!(commands::run("transport.play", &[], &mut m).is_err());
    assert!(!f.calls().iter().any(|c| c.starts_with("play(")));
    m.set_surface_acting(true);
    assert!(m.transport_keys_enabled() && m.return_key_enabled());
    commands::run("transport.play", &[], &mut m).expect("the surface's Play runs");
    assert!(f.calls().iter().any(|c| c.starts_with("play(")));
    m.set_surface_acting(false);
    assert!(
        !m.transport_keys_enabled(),
        "the keys still wait for the field"
    );
}

/// Fix round 1 I1: a jog then Play in one batch locates first.
#[test]
fn a_jog_is_located_before_a_later_command_in_the_batch() {
    let mut relay = SurfaceRelay::default();
    let steps = relay.apply(
        vec![
            SurfaceEventRow::Jog { detents: 4 },
            command("transport.play", false),
        ],
        Instant::now(),
    );
    assert_eq!(
        steps,
        vec![RelayStep::Locate(4), step("transport.play", false)]
    );
}

/// Fix round 1 I1: the same across two batches inside the window.
#[test]
fn a_jog_is_located_before_a_command_in_the_next_batch() {
    let (f, mut m) = open(mixer_fake());
    let start = m.last_transport().tick;
    let mut relay = SurfaceRelay::default();
    let t0 = Instant::now();
    assert!(
        relay
            .apply(vec![SurfaceEventRow::Jog { detents: 2 }], t0)
            .is_empty()
    );
    let t1 = t0 + Duration::from_millis(40);
    assert_eq!(relay.flush(t1), None);
    let steps = relay.apply(vec![command("transport.play", true)], t1);
    assert_eq!(
        steps,
        vec![RelayStep::Locate(2), step("transport.play", true)]
    );
    apply_all(&mut m, steps);
    assert_eq!(locates(&f), vec![start + 2 * DETENT]);
    assert_eq!(relay.flush(t1 + JOG_WINDOW), None, "nothing left to locate");
}

/// Review M2: the jog counts from where the transport stands now (a
/// core-side move the last poll has not seen), and not below 0.
#[test]
fn a_jog_counts_from_the_current_tick_and_stops_at_zero() {
    let (f, mut m) = open(mixer_fake());
    f.set_transport(|t| t.tick = 7680);
    m.apply_surface_step(RelayStep::Locate(1));
    assert_eq!(locates(&f), vec![7680 + DETENT]);
    assert_eq!(DETENT, 3840);
    f.set_transport(|t| t.tick = 0);
    m.apply_surface_step(RelayStep::Locate(-10));
    assert_eq!(locates(&f).last(), Some(&0));
}

/// A refused locate is the status line's "Locate Failed" message.
#[test]
fn a_refused_locate_says_why_in_the_status_line() {
    let (f, mut m) = open(mixer_fake());
    f.set_locate_error(Some(VizError::Model {
        message: "nope".into(),
    }));
    m.apply_surface_step(RelayStep::Locate(1));
    assert_eq!(m.status(), Some("nope"));
    assert!(m.alert.is_none());
}

/// Review M5: `idle` drains the session's surface events into steps.
#[test]
fn idle_drains_surface_events() {
    let (f, mut m) = open(mixer_fake());
    f.push_surface_events(vec![
        SurfaceEventRow::Select { strip: 2 },
        command("transport.stop", false),
    ]);
    m.idle();
    let steps = m.take_surface_steps();
    assert_eq!(
        steps,
        vec![RelayStep::Select(2), step("transport.stop", false)]
    );
    let ran = apply_all(&mut m, steps);
    assert_eq!(m.selected_strips(), &BTreeSet::from([2]));
    assert_eq!(
        ran.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
        ["transport.stop"]
    );
    m.idle();
    assert!(m.take_surface_steps().is_empty(), "drained");
}

/// Plan 5f-4 Ruling 16: the keyboard's Count-in toggles the record
/// count-in between off and its last bar count.
#[test]
fn a_count_in_press_toggles_the_record_count_in() {
    let (_f, mut m) = open(mixer_fake());
    m.record_mut().set_count_in(3);
    m.record_mut().set_count_in(0);
    let mut relay = SurfaceRelay::default();
    let steps = relay.apply(vec![SurfaceEventRow::CountIn], Instant::now());
    assert_eq!(steps, vec![RelayStep::CountIn]);
    m.apply_surface_step(RelayStep::CountIn);
    assert_eq!(m.record().count_in_bars(), 3, "its last bar count");
    m.apply_surface_step(RelayStep::CountIn);
    assert_eq!(m.record().count_in_bars(), 0);
    m.apply_surface_step(RelayStep::CountIn);
    assert_eq!(m.record().count_in_bars(), 3);
}

/// Ruling 16: the document tells the core its count-in, once per change.
#[test]
fn idle_tells_the_surfaces_the_count_in() {
    let (f, mut m) = open(mixer_fake());
    m.record_mut().set_count_in(0);
    m.idle();
    assert_eq!(f.surface_count_ins(), vec![false], "the first idle says it");
    m.idle();
    assert_eq!(f.surface_count_ins(), vec![false], "unchanged: not again");
    m.record_mut().set_count_in(2);
    m.idle();
    m.record_mut().set_count_in(1);
    m.idle();
    m.record_mut().set_count_in(0);
    m.idle();
    assert_eq!(f.surface_count_ins(), vec![false, true, false]);
}

/// Final review T7 m3: a count-in the core refused is said again.
#[test]
fn a_refused_count_in_is_said_again() {
    let (f, mut m) = open(mixer_fake());
    m.record_mut().set_count_in(2);
    f.fail_next(
        "set_surface_count_in",
        VizError::Model {
            message: "busy".into(),
        },
    );
    m.idle();
    assert_eq!(f.surface_count_ins(), vec![true]);
    m.idle();
    assert_eq!(f.surface_count_ins(), vec![true, true], "said again");
    m.idle();
    assert_eq!(f.surface_count_ins(), vec![true, true], "then once only");
}

/// Pre-flight I9: a command `surface.inject` or `midi.inject` caused
/// never takes the key path; a real button still runs as a key.
#[test]
fn an_injected_save_opens_no_dialog() {
    let (_f, mut m) = open(mixer_fake());
    let mut relay = SurfaceRelay::default();
    let steps = relay.apply(
        vec![command("file.save", true), command("edit.undo", false)],
        Instant::now(),
    );
    let ran = apply_all(&mut m, steps);
    assert_eq!(
        ran,
        vec![
            SurfaceCommand {
                id: "file.save".into(),
                programmatic: true,
                bypass: false,
            },
            SurfaceCommand {
                id: "edit.undo".into(),
                programmatic: false,
                bypass: true,
            },
        ]
    );
}

/// Review Focus 5: the surfaces' focus moves only when the front document
/// changes, and is kept only once the core took it; a refusal is retried
/// at the next call.
#[test]
fn focus_follows_the_front_document_and_retries_after_a_refusal() {
    let core = Arc::new(FakeCore::new());
    let a = Arc::new(demo_fake());
    let b = Arc::new(demo_fake());
    core.hand_out("new_demo", a.clone());
    core.hand_out("new_demo", b.clone());
    let mut store = DocumentStore::with_core(core.clone());
    let da = store.new_demo().unwrap();
    let db = store.new_demo().unwrap();
    store.follow_surface_focus();
    assert_eq!(
        (a.focus_calls(), b.focus_calls()),
        (0, 0),
        "nothing in front"
    );
    store.focus_document(da, WindowToken(1));
    store.follow_surface_focus();
    store.follow_surface_focus();
    assert_eq!((a.focus_calls(), b.focus_calls()), (1, 0));
    store.focus_document(db, WindowToken(2));
    store.follow_surface_focus();
    store.follow_surface_focus();
    assert_eq!(
        (a.focus_calls(), b.focus_calls()),
        (1, 1),
        "once per change"
    );
    a.fail_next(
        "focus_surfaces",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    store.focus_document(da, WindowToken(1));
    store.follow_surface_focus();
    assert_eq!(a.focus_calls(), 2, "refused");
    store.follow_surface_focus();
    assert_eq!(a.focus_calls(), 3, "retried");
    store.follow_surface_focus();
    assert_eq!(a.focus_calls(), 3, "then kept");
    // With no document open the focus is forgotten: the next one is
    // focused even though nothing changed in between.
    store.close(da);
    store.close(db);
    store.follow_surface_focus();
    let c = Arc::new(demo_fake());
    core.hand_out("new_demo", c.clone());
    let dc = store.new_demo().unwrap();
    store.focus_document(dc, WindowToken(3));
    store.follow_surface_focus();
    assert_eq!(c.focus_calls(), 1);
}
