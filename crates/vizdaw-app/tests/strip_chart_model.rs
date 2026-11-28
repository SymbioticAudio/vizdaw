//! What each Strip Chart shows (Swift `T/StripChartModelTests.swift`,
//! `T/LinePanelTests.swift`, `T/AutomationRowsTests.swift`; slice 10c
//! Task 3): the mixer's automation targets and their one per-document
//! cache, the track and rows a chart shows, and the write in progress.

mod support;

use std::sync::Arc;

use support::{
    FakeSession, dest_row, mixer_fake, note_row, roll_fake, slot_row, strip_row, vca_row,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::editor::GESTURE_OPEN;
use vizdaw_app::editor::{DetailEditorKind, EditorTarget};
use vizdaw_app::select_modify::{GestureOrigin, PanelTask};
use vizdaw_app::strip_chart::{
    AutomationTargets, ChartSlot, ERASE_NOTES, MAX_ROWS, StripLane, StripMenu, StripMode, StripOp,
    StripPoint, erase_command, line_command, only_target, opening, v32,
};
use vizdaw_ffi::VizError;
use vizdaw_ffi::automation::PreviewPointRow;
use vizdaw_ffi::edit::{EditCommand, LaneRef};
use vizdaw_ffi::events::{AutomationRow, AutomationTargetRow as T, EventKindRow, EventRow};
use vizdaw_ffi::mixer::{BusRow, MixerSnapshot, StripKindRow, VcaRow};
use vizdaw_ffi::modify::{ModifyRow, SelectModeRow};
use vizdaw_ffi::selection::SelectionRow;
use vizdaw_ffi::snapshot::ParamRow;
use vizdaw_ffi::strip::{LineOpRow, LineRow, LineShapeRow, LineTargetRow, SeriesRow};

const PANE: ChartSlot = ChartSlot::Pane;
const CONTROLLERS: ChartSlot = ChartSlot::Controllers;

fn busy() -> VizError {
    VizError::Busy {
        message: "busy".into(),
    }
}

/// `roll_fake` with the pane's Strip Chart on `target`.
fn make(target: EditorTarget) -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(roll_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(target, Some(DetailEditorKind::StripChart));
    (f, m)
}

fn chords() -> EditorTarget {
    EditorTarget::Lane { seq: 3, lane: 31 }
}

fn param(id: u64, name: &str, hidden: bool) -> ParamRow {
    ParamRow {
        id,
        name: name.into(),
        module: String::new(),
        min: 0.,
        max: 2.,
        default: 1.,
        value: 1.,
        stepped: false,
        automatable: true,
        hidden,
        read_only: false,
    }
}

/// Swift's `mixerFake`: Synth (1) with its instrument slot "Gain" (10),
/// Bus 1 (2) and Master (0); `vcas` as given.
fn mixer_with(vcas: Vec<VcaRow>) -> FakeSession {
    let f = mixer_fake();
    let mut synth = strip_row(1, "Synth", StripKindRow::BuiltinSynth);
    let mut gain = slot_row(10, "Gain");
    gain.instrument = true;
    synth.instrument = Some(gain);
    f.set_mixer(MixerSnapshot {
        version: 0,
        strips: vec![
            synth,
            strip_row(2, "Bus 1", StripKindRow::Bus),
            strip_row(0, "Master", StripKindRow::Master),
        ],
        buses: vec![BusRow {
            id: 2,
            name: "Bus 1".into(),
        }],
        groups: vec![],
        vcas,
    });
    f
}

fn auto(t: T) -> StripLane {
    StripLane::Automation(t)
}

/// An automation point of `target` at `tick`.
fn auto_row(id: u64, tick: i64, target: T, v: f64) -> EventRow {
    EventRow {
        automation: Some(AutomationRow { target, value: v }),
        value: v32(v),
        duration: 0,
        ..EventRow::blank(id, tick, EventKindRow::Automation)
    }
}

fn titles(t: &AutomationTargets) -> Vec<String> {
    t.menus.iter().map(|m| m.title.clone()).collect()
}

#[test]
fn shows_the_target_lane_and_its_rows() {
    let (_, mut m) = make(chords());
    assert_eq!(m.strip_lane(PANE), Some((3, 31)));
    assert_eq!(m.strip_rows(PANE), vec![StripLane::Velocity]);
    let ids: Vec<_> = m
        .strip_values(PANE, StripLane::Velocity)
        .iter()
        .map(|v| v.id)
        .collect();
    assert_eq!(ids, vec![Some(1), Some(2)]);
    m.add_strip_row(PANE);
    assert_eq!(
        m.strip_rows(PANE),
        vec![StripLane::Velocity, StripLane::Controller(7)]
    );
    m.set_strip_row(PANE, 1, StripLane::PitchBend);
    m.remove_strip_row(PANE, 0);
    assert_eq!(m.strip_rows(PANE), vec![StripLane::PitchBend]);
    m.remove_strip_row(PANE, 0);
    assert_eq!(
        m.strip_rows(PANE),
        vec![StripLane::PitchBend],
        "one row stays"
    );
    for _ in 0..6 {
        m.add_strip_row(PANE);
    }
    assert_eq!(m.strip_rows(PANE).len(), MAX_ROWS);
    // The rows are saved with the detail state; another editor keeps them.
    m.open_detail(chords(), Some(DetailEditorKind::PianoRoll));
    let saved = m.layout().detail.as_ref().and_then(|d| d.strip_rows());
    assert_eq!(
        saved.and_then(|r| r.first().copied()),
        Some(StripLane::PitchBend)
    );
    assert_eq!(m.strip_meter(PANE).len(), 1);
}

#[test]
fn a_sequence_target_shows_the_chosen_lane_else_the_first() {
    let (_, mut m) = make(EditorTarget::Sequence { seq: 3 });
    assert_eq!(m.strip_lane(PANE), Some((3, 30)));
    m.set_strip_chosen_lane(PANE, (3, 32));
    assert_eq!(m.strip_lane(PANE), Some((3, 32)));
    m.set_strip_chosen_lane(PANE, (3, 99));
    assert_eq!(
        m.strip_lane(PANE),
        Some((3, 30)),
        "a track that is gone falls back"
    );
    assert_eq!(
        m.strip_target(PANE),
        Some(EditorTarget::Sequence { seq: 3 })
    );
}

#[test]
fn automation_targets_come_from_the_mixer() {
    let f = mixer_with(vec![]);
    f.set_params(10, vec![param(4, "Gain", false), param(5, "Secret", true)]);
    f.set_destinations(vec![dest_row(2, "Piano")]);
    let t = AutomationTargets::load(&f).unwrap();
    assert_eq!(
        titles(&t),
        ["Synth", "Gain · Synth", "Bus 1", "Master", "Velocity Scale"]
    );
    assert_eq!(
        t.menus[0].lanes,
        vec![
            auto(T::StripFader { strip: 1 }),
            auto(T::StripPan { strip: 1 }),
            auto(T::StripMute { strip: 1 }),
            auto(T::StripSolo { strip: 1 }),
        ]
    );
    assert_eq!(
        t.menus[1].lanes,
        vec![auto(T::PluginParam { slot: 10, param: 4 })],
        "hidden parameters are left out"
    );
    assert!(
        !t.menus[3].lanes.contains(&auto(T::StripPan { strip: 0 })),
        "master has no pan"
    );
    assert_eq!(
        t.names.title(&T::PluginParam { slot: 10, param: 4 }),
        "Gain · Gain"
    );
    assert_eq!(t.names.dests.get(&2).map(String::as_str), Some("Piano"));
}

#[test]
fn a_strips_sends_are_its_last_items_and_name_their_bus() {
    let f = mixer_with(vec![]);
    let mut m = f.mixer_snapshot();
    m.strips[0].sends = vec![
        vizdaw_ffi::mixer::SendRow {
            target: 2,
            level_db: 0.,
            pre_fader: false,
            enabled: true,
        },
        vizdaw_ffi::mixer::SendRow {
            target: 9,
            level_db: 0.,
            pre_fader: false,
            enabled: true,
        },
    ];
    f.set_mixer(m);
    let t = AutomationTargets::load(&f).unwrap();
    assert_eq!(
        t.menus[0].lanes[4..],
        [
            auto(T::SendLevel { strip: 1, send: 0 }),
            auto(T::SendLevel { strip: 1, send: 1 })
        ]
    );
    assert_eq!(
        t.names.title(&T::SendLevel { strip: 1, send: 1 }),
        "Send 2 to Bus 9 · Synth"
    );
}

#[test]
fn a_refused_part_reads_as_empty_and_busy_is_an_error() {
    let f = mixer_with(vec![]);
    f.set_destinations(vec![dest_row(2, "Piano")]);
    f.set_call_error(
        "mixer",
        Some(VizError::Engine {
            message: "no".into(),
        }),
    );
    let t = AutomationTargets::load(&f).unwrap();
    assert_eq!(titles(&t), ["Velocity Scale"]);
    f.set_call_error("mixer", None);
    f.set_call_error("params", Some(busy()));
    assert!(AutomationTargets::load(&f).is_err());
}

#[test]
fn saved_rows_of_deleted_targets_are_dropped() {
    // Parked T12: a row saved for a strip or slot that is gone.
    let f = Arc::new(mixer_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(chords(), Some(DetailEditorKind::StripChart));
    let save = |m: &mut DocumentModel, rows: Vec<StripLane>| {
        m.update_layout(|l| {
            if let Some(d) = l.detail.as_mut() {
                d.set_strip_rows(Some(rows));
            }
        });
    };
    save(
        &mut m,
        vec![
            auto(T::StripFader { strip: 1 }),
            auto(T::StripFader { strip: 99 }),
            auto(T::PluginParam { slot: 77, param: 1 }),
            StripLane::Controller(7),
        ],
    );
    assert_eq!(
        m.strip_rows(PANE),
        vec![auto(T::StripFader { strip: 1 }), StripLane::Controller(7)]
    );
    m.set_strip_row(PANE, 1, StripLane::PitchBend);
    assert_eq!(
        m.strip_rows(PANE),
        vec![auto(T::StripFader { strip: 1 }), StripLane::PitchBend],
        "edits act on the rows shown"
    );
    save(&mut m, vec![auto(T::StripMute { strip: 99 })]);
    assert_eq!(m.strip_rows(PANE), vec![StripLane::Velocity]);
}

#[test]
fn a_busy_read_is_not_cached_and_the_next_call_fills_the_menus() {
    let f = Arc::new(mixer_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(chords(), Some(DetailEditorKind::StripChart));
    f.set_call_error("mixer", Some(busy()));
    assert!(m.automation_targets().menus.is_empty());
    f.set_call_error("mixer", None);
    assert_eq!(
        titles(&m.automation_targets()).first().map(String::as_str),
        Some("Synth"),
        "retried without a version bump"
    );
    f.set_call_error("mixer", Some(busy()));
    assert!(
        !m.automation_targets().menus.is_empty(),
        "a later Busy keeps the previous menus"
    );
}

#[test]
fn the_targets_are_read_once_per_version() {
    let f = Arc::new(mixer_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    let a = m.automation_targets();
    let reads = f.mixer_reads();
    let b = m.automation_targets();
    assert!(Arc::ptr_eq(&a, &b));
    assert_eq!(f.mixer_reads(), reads);
}

#[test]
fn automation_targets_refresh_when_a_plugin_finishes_loading() {
    // Final review Minor 4: the cache is keyed on the slots version too.
    let f = Arc::new(mixer_with(vec![]));
    let mut m = DocumentModel::new(f.clone()).unwrap();
    assert!(
        !titles(&m.automation_targets()).contains(&"Gain · Synth".to_owned()),
        "not loaded yet"
    );
    f.set_params(10, vec![param(4, "Gain", false)]);
    f.set_idle_slots_changed();
    m.idle();
    assert_eq!(m.slots_version(), 1);
    let t = m.automation_targets();
    assert!(titles(&t).contains(&"Gain · Synth".to_owned()));
    assert_eq!(
        t.names.title(&T::PluginParam { slot: 10, param: 4 }),
        "Gain · Gain"
    );
}

#[test]
fn vca_fader_targets_are_named() {
    let f = mixer_with(vec![vca_row(3, "Drums", &[1], 0.)]);
    let loaded = AutomationTargets::load(&f).unwrap();
    assert_eq!(loaded.names.vcas.get(&3).map(String::as_str), Some("Drums"));
    assert_eq!(
        loaded.menus.last(),
        Some(&StripMenu {
            title: "VCAs".into(),
            lanes: vec![auto(T::VcaFader { vca: 3 })],
        })
    );
}

/// Plan 5e-2 Ruling 9: the write in progress shows on its row while the
/// chart's sequence plays, and nowhere else.
#[test]
fn the_preview_shows_only_while_its_target_is_written() {
    let (f, m) = make(chords());
    let row = auto(T::StripFader { strip: 1 });
    f.set_preview_rows(vec![
        PreviewPointRow {
            tick: 0,
            value: 0.5,
        },
        PreviewPointRow {
            tick: 960,
            value: 1.,
        },
    ]);
    assert!(m.strip_preview(PANE, row, None).is_empty(), "stopped");
    let p = m.strip_preview(PANE, row, Some(3));
    assert_eq!(p.iter().map(|v| v.tick).collect::<Vec<_>>(), [0, 960]);
    assert_eq!(
        p.iter().map(|v| v.value).collect::<Vec<_>>(),
        [v32(0.5), v32(1.)]
    );
    assert!(p.iter().all(|v| v.id.is_none() && v.ramp && v.length == 0));
    assert!(f.calls().contains(&"automation_preview".to_owned()));
    assert!(
        m.strip_preview(PANE, StripLane::Velocity, Some(3))
            .is_empty(),
        "only automation rows"
    );
    assert!(
        m.strip_preview(PANE, row, Some(4)).is_empty(),
        "another sequence plays"
    );
    f.set_call_error("automation_preview", Some(busy()));
    assert!(
        m.strip_preview(PANE, row, Some(3)).is_empty(),
        "an error is empty"
    );
}

/// Groove (track 30) holding `events`, shown in the pane's chart.
fn groove(events: Vec<EventRow>) -> DocumentModel {
    let f = Arc::new(roll_fake());
    f.set_events(3, 30, events);
    let mut m = DocumentModel::new(f).unwrap();
    m.open_detail(
        EditorTarget::Lane { seq: 3, lane: 30 },
        Some(DetailEditorKind::StripChart),
    );
    m
}

/// Ruling 10: a track holding only one target's automation opens on that
/// target's row.
#[test]
fn an_automation_lane_opens_on_its_target() {
    let pan = T::StripPan { strip: 1 };
    let m = groove(vec![auto_row(1, 0, pan, 0.5), auto_row(2, 960, pan, 0.25)]);
    assert_eq!(
        m.strip_rows(PANE),
        vec![auto(pan), StripLane::Velocity],
        "its target first, then the saved rows"
    );
    let m2 = groove(vec![auto_row(1, 0, pan, 0.5), note_row(3, 0, 60)]);
    assert_eq!(
        m2.strip_rows(PANE),
        vec![StripLane::Velocity],
        "a track with notes keeps its rows"
    );
    let fader = T::StripFader { strip: 1 };
    let m3 = groove(vec![auto_row(1, 0, pan, 0.5), auto_row(2, 0, fader, 0.5)]);
    assert_eq!(
        m3.strip_rows(PANE),
        vec![StripLane::Velocity],
        "two targets: no guess"
    );
    assert_eq!(only_target(&[]), None);
}

/// Ruling 10 holds only until the chart's rows are saved; then the saved
/// rows are used as they are.
#[test]
fn an_automation_lanes_own_row_gives_way_to_saved_rows() {
    let pan = T::StripPan { strip: 1 };
    let mut m = groove(vec![auto_row(1, 0, pan, 0.5), auto_row(2, 960, pan, 0.25)]);
    assert_eq!(m.strip_rows(PANE), vec![auto(pan), StripLane::Velocity]);
    m.remove_strip_row(PANE, 0);
    assert_eq!(
        m.strip_rows(PANE),
        vec![StripLane::Velocity],
        "removed, and it stays removed"
    );
    m.add_strip_row(PANE);
    m.add_strip_row(PANE);
    m.add_strip_row(PANE);
    assert_eq!(
        m.strip_rows(PANE),
        vec![
            StripLane::Velocity,
            StripLane::Controller(7),
            StripLane::Controller(7),
            StripLane::Controller(7)
        ],
        "a full saved list is kept whole"
    );
    m.set_strip_row(PANE, 0, StripLane::Controller(1));
    assert_eq!(
        m.strip_rows(PANE).first(),
        Some(&StripLane::Controller(1)),
        "a replaced row is not added back"
    );
    assert_eq!(m.strip_rows(PANE).len(), MAX_ROWS);
}

/// Rows saved on another track do not stop an automation track opening on
/// its target; on the track they were saved for, they are used as saved.
#[test]
fn rows_saved_on_another_lane_let_an_automation_lane_open_on_its_target() {
    let pan = T::StripPan { strip: 1 };
    let f = Arc::new(roll_fake());
    f.set_events(
        3,
        32,
        vec![auto_row(1, 0, pan, 0.5), auto_row(2, 960, pan, 0.25)],
    );
    let mut m = DocumentModel::new(f).unwrap();
    let groove = EditorTarget::Lane { seq: 3, lane: 30 };
    let bass = EditorTarget::Lane { seq: 3, lane: 32 };
    m.open_detail(groove, Some(DetailEditorKind::StripChart));
    m.add_strip_row(PANE);
    assert_eq!(
        m.strip_rows(PANE),
        vec![StripLane::Velocity, StripLane::Controller(7)],
        "saved on track 30"
    );
    let rows_lane = |m: &DocumentModel| m.layout().detail.as_ref()?.strip_rows_lane();
    assert_eq!(rows_lane(&m), Some((3, 30)));
    m.open_detail(bass, Some(DetailEditorKind::StripChart));
    assert_eq!(
        m.strip_rows(PANE),
        vec![auto(pan), StripLane::Velocity, StripLane::Controller(7)],
        "its target first, then the saved rows"
    );
    m.remove_strip_row(PANE, 0);
    assert_eq!(
        m.strip_rows(PANE),
        vec![StripLane::Velocity, StripLane::Controller(7)],
        "removed, and it stays removed"
    );
    assert_eq!(rows_lane(&m), Some((3, 32)), "saved for this track now");
    m.open_detail(groove, Some(DetailEditorKind::StripChart));
    assert_eq!(
        m.strip_rows(PANE),
        vec![StripLane::Velocity, StripLane::Controller(7)],
        "track 30: as saved"
    );
}

/// The target's row goes first once, the saved rows after it as they are
/// (none dropped by the row cap).
#[test]
fn the_targets_row_joins_the_saved_rows_once() {
    let pan = T::StripPan { strip: 1 };
    assert_eq!(
        opening(pan, &[StripLane::Velocity, auto(pan)]),
        vec![auto(pan), StripLane::Velocity]
    );
    let full = [
        StripLane::Velocity,
        StripLane::Controller(1),
        StripLane::Controller(7),
        StripLane::Duration,
    ];
    let mut want = vec![auto(pan)];
    want.extend(full);
    assert_eq!(opening(pan, &full), want);
}

/// Review Focus 3: the Controllers chart writes its rows under the key of
/// the track it read.
#[test]
fn the_controllers_chart_keeps_rows_per_track() {
    let (_, mut m) = make(chords());
    m.select_track(31, false);
    assert_eq!(m.strip_lane(CONTROLLERS), Some((3, 31)));
    m.set_strip_row(CONTROLLERS, 0, StripLane::PitchBend);
    let rows = |m: &DocumentModel, k: &str| m.layout().controllers.get(k).cloned();
    assert_eq!(rows(&m, "3:31"), Some(vec![StripLane::PitchBend]));
    // The pane's rows are its own.
    assert_eq!(m.strip_rows(PANE), vec![StripLane::Velocity]);
    m.select_track(32, false);
    let state = m.strip_state(CONTROLLERS).unwrap();
    assert_eq!(state.kind, DetailEditorKind::StripChart);
    assert_eq!(state.target, EditorTarget::Lane { seq: 3, lane: 32 });
    assert_eq!(state.strip_rows_lane(), Some((3, 32)));
    m.add_strip_row(CONTROLLERS);
    assert_eq!(
        rows(&m, "3:32"),
        Some(vec![StripLane::Velocity, StripLane::Controller(7)])
    );
    assert_eq!(rows(&m, "3:31"), Some(vec![StripLane::PitchBend]));
    m.select_track(31, false);
    assert_eq!(m.strip_rows(CONTROLLERS), vec![StripLane::PitchBend]);
}

/// Review Focus 3: with no focused track the Controllers chart shows
/// nothing and writes nothing.
#[test]
fn with_no_focused_track_the_controllers_chart_writes_nothing() {
    let (_, mut m) = make(chords());
    assert!(m.strip_state(CONTROLLERS).is_none());
    assert_eq!(m.strip_lane(CONTROLLERS), None);
    m.add_strip_row(CONTROLLERS);
    m.set_strip_row(CONTROLLERS, 0, StripLane::Tempo);
    m.remove_strip_row(CONTROLLERS, 0);
    assert!(m.layout().controllers.is_empty());
}

/// Task 2 review: a `controllers` entry longer than `stripRows` allows
/// reads as no saved rows.
#[test]
fn an_overlong_controllers_entry_reads_as_velocity() {
    let (_, mut m) = make(chords());
    m.select_track(31, false);
    m.update_layout(|l| {
        l.controllers
            .insert("3:31".into(), vec![StripLane::Controller(1); 9]);
    });
    assert_eq!(m.strip_rows(CONTROLLERS), vec![StripLane::Velocity]);
}

#[test]
fn the_bar_settings_are_the_layouts() {
    let (_, mut m) = make(chords());
    m.update_strip_settings(|s| s.row_height = 120.);
    assert_eq!(m.strip_settings().row_height, 120.);
    assert_eq!(m.layout().strip_chart.row_height, 120.);
}

/// A VCA already named "VCA …" is not named twice, as the core names its
/// track (`vca_label`); learn titles keep that.
#[test]
fn a_vca_named_vca_is_not_named_twice() {
    let f = Arc::new(mixer_with(vec![vca_row(1, "VCA 1", &[1], 0.)]));
    let m = DocumentModel::new(f).unwrap();
    assert_eq!(
        vizdaw_app::learn::target_title(&T::VcaFader { vca: 1 }, &m),
        "VCA 1 Fader"
    );
}

/// Fix round 1 (review Minor 2, data loss): while the targets have never
/// been read, saved automation rows are unknown, not gone, and an edit
/// in that window keeps them.
#[test]
fn a_busy_first_read_keeps_saved_automation_rows_through_an_edit() {
    let f = Arc::new(mixer_fake());
    let mut m = DocumentModel::new(f.clone()).unwrap();
    m.open_detail(chords(), Some(DetailEditorKind::StripChart));
    let saved = vec![
        auto(T::StripFader { strip: 1 }),
        auto(T::PluginParam { slot: 77, param: 1 }),
        StripLane::Controller(7),
    ];
    m.update_layout(|l| {
        if let Some(d) = l.detail.as_mut() {
            d.set_strip_rows(Some(saved.clone()));
        }
    });
    f.set_call_error("mixer", Some(busy()));
    assert_eq!(m.strip_rows(PANE), saved, "unknown yet, not gone");
    m.set_strip_row(PANE, 2, StripLane::PitchBend);
    let written = m.layout().detail.as_ref().and_then(|d| d.strip_rows());
    assert_eq!(
        written,
        Some(vec![
            auto(T::StripFader { strip: 1 }),
            auto(T::PluginParam { slot: 77, param: 1 }),
            StripLane::PitchBend,
        ])
    );
    f.set_call_error("mixer", None);
    assert_eq!(
        m.strip_rows(PANE),
        vec![auto(T::StripFader { strip: 1 }), StripLane::PitchBend],
        "once read, a gone target's row is dropped"
    );
}

// MARK: Strokes (Task 4)

const Q: i64 = 15_360;

fn pt(tick: i64, value: i64) -> StripPoint {
    StripPoint { tick, value }
}

/// `make(chords())` with the pane's seed 7 and no calls yet.
fn stroking() -> (Arc<FakeSession>, DocumentModel) {
    let (f, mut m) = make(chords());
    m.set_strip_seed_for_test(PANE, 7);
    f.clear_calls();
    (f, m)
}

fn gesture_label(m: &DocumentModel) -> Option<String> {
    m.gesture().map(|g| g.label.clone())
}

#[test]
fn a_draw_drag_is_one_gesture_on_the_shown_lane() {
    let (f, mut m) = stroking();
    m.strip_stroke_began(PANE, 0, pt(0, 100), false);
    assert!(
        f.calls().is_empty(),
        "nothing opens before the pointer moves"
    );
    m.strip_stroke_moved(PANE, pt(Q, 200));
    assert_eq!(m.selection().lanes, vec![LaneRef { seq: 3, lane: 31 }]);
    assert_eq!(gesture_label(&m).as_deref(), Some("Change Velocity"));
    assert_eq!(m.gesture().unwrap().origin, GestureOrigin::StripChart);
    let line = LineRow {
        from: 0,
        to: Q,
        start: 100,
        end: 200,
        step: Q / 8,
        shape: LineShapeRow::Straight,
    };
    assert_eq!(
        f.gesture_edits().last(),
        Some(&line_command(
            StripLane::Velocity,
            LineOpRow::Set,
            line,
            false
        ))
    );
    m.strip_stroke_ended(PANE, pt(Q, 200));
    assert!(m.gesture().is_none());
    assert_eq!(f.calls().last().map(String::as_str), Some("end_edit"));
    assert!(m.strip_stroke(PANE).is_none());
}

#[test]
fn a_click_in_draw_mode_draws_one_point() {
    let (f, mut m) = stroking();
    m.set_strip_row(PANE, 0, StripLane::Controller(1));
    f.clear_calls();
    m.strip_stroke_began(PANE, 0, pt(Q, 5), false);
    m.strip_stroke_ended(PANE, pt(Q, 5));
    let line = LineRow {
        from: Q,
        to: Q,
        start: 5,
        end: 5,
        step: Q / 8,
        shape: LineShapeRow::Straight,
    };
    assert_eq!(
        f.gesture_edits().last(),
        Some(&line_command(
            StripLane::Controller(1),
            LineOpRow::Draw,
            line,
            false
        ))
    );
    let calls = f.calls();
    assert_eq!(
        calls.first().map(String::as_str),
        Some("begin_edit(Draw Controllers)")
    );
    assert_eq!(calls.last().map(String::as_str), Some("end_edit"));
    // A click in Modify mode does nothing.
    m.update_strip_settings(|s| s.mode = StripMode::Modify);
    f.clear_calls();
    m.strip_stroke_began(PANE, 0, pt(Q, 5), false);
    m.strip_stroke_ended(PANE, pt(Q, 5));
    assert!(f.calls().is_empty(), "{:?}", f.calls());
    assert!(m.strip_stroke(PANE).is_none());
}

#[test]
fn a_modify_line_keeps_the_selected_lanes_and_option_refines() {
    let (f, mut m) = stroking();
    m.set_selection(SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: 3, lane: 32 }],
        events: vec![3],
    });
    let sets = f.count("set_selection");
    m.update_strip_settings(|s| {
        s.mode = StripMode::Modify;
        s.op = StripOp::Add;
    });
    m.strip_stroke_began(PANE, 0, pt(0, -5), true);
    m.strip_stroke_moved(PANE, pt(Q, 5));
    assert_eq!(f.count("set_selection"), sets, "the selection's lanes stay");
    let lanes: Vec<_> = m.gesture().unwrap().lanes.iter().copied().collect();
    assert_eq!(lanes, vec![(3, 32)]);
    match f.gesture_edits().last() {
        Some(EditCommand::SelectAndModify { mode, .. }) => assert_eq!(*mode, SelectModeRow::Refine),
        other => panic!("no line: {other:?}"),
    }
    m.strip_stroke_cancelled(PANE);
    assert!(m.gesture().is_none());
}

#[test]
fn a_refused_stroke_leaves_the_selection_alone() {
    // Another editor's gesture is open, so the draw is refused and the
    // selection keeps its tracks.
    let (f, mut m) = stroking();
    let before = SelectionRow {
        time_range: None,
        lanes: vec![LaneRef { seq: 3, lane: 32 }],
        events: vec![3],
    };
    m.set_selection(before.clone());
    assert!(m.begin_gesture_from("Other", vec![(3, 32)], GestureOrigin::Notes));
    let sets = f.count("set_selection");
    m.strip_stroke_began(PANE, 0, pt(0, 100), false);
    m.strip_stroke_moved(PANE, pt(Q, 200));
    assert_eq!(m.status(), Some(GESTURE_OPEN));
    assert_eq!(f.count("set_selection"), sets, "no selection change");
    assert_eq!(m.selection(), &before);
    assert_eq!(gesture_label(&m).as_deref(), Some("Other"));
    assert!(m.strip_stroke(PANE).is_none());
    // Its release ends nothing of the other gesture.
    m.strip_stroke_ended(PANE, pt(Q, 200));
    assert_eq!(gesture_label(&m).as_deref(), Some("Other"));
    m.cancel_gesture();
}

#[test]
fn a_new_stroke_cancels_one_whose_mouse_up_was_lost() {
    let (f, mut m) = stroking();
    m.strip_stroke_began(PANE, 0, pt(0, 1), false);
    m.strip_stroke_moved(PANE, pt(Q, 2));
    assert!(m.gesture().is_some());
    m.strip_stroke_began(PANE, 0, pt(2 * Q, 3), false);
    assert_eq!(f.calls().last().map(String::as_str), Some("abort_edit"));
    assert!(m.gesture().is_none());
    m.strip_stroke_moved(PANE, pt(3 * Q, 4));
    assert_eq!(
        gesture_label(&m).as_deref(),
        Some("Change Velocity"),
        "the new stroke draws"
    );
    m.strip_stroke_ended(PANE, pt(3 * Q, 4));
    assert!(m.gesture().is_none());
    assert_eq!(f.calls().last().map(String::as_str), Some("end_edit"));
    assert!(!f.edit_is_open());
}

#[test]
fn escape_leaves_no_undo_step() {
    let (f, mut m) = stroking();
    m.strip_stroke_began(PANE, 0, pt(0, 1), false);
    m.strip_stroke_moved(PANE, pt(Q, 2));
    m.strip_stroke_cancelled(PANE);
    assert_eq!(f.calls().last().map(String::as_str), Some("abort_edit"));
    assert!(m.gesture().is_none() && m.strip_stroke(PANE).is_none());
    assert!(!f.edit_is_open());
}

#[test]
fn erase_on_a_note_row_says_why() {
    let (f, mut m) = stroking();
    m.update_strip_settings(|s| s.mode = StripMode::Erase);
    f.clear_calls();
    m.strip_stroke_began(PANE, 0, pt(0, 1), false);
    m.strip_stroke_moved(PANE, pt(Q, 2));
    assert_eq!(m.status(), Some(ERASE_NOTES));
    assert_eq!(
        ERASE_NOTES,
        "Erase works on controller, program and automation rows."
    );
    assert!(f.calls().is_empty() && m.gesture().is_none());
    m.set_strip_row(PANE, 0, StripLane::Controller(7));
    m.strip_stroke_began(PANE, 0, pt(0, 1), false);
    m.strip_stroke_moved(PANE, pt(Q, 2));
    assert_eq!(gesture_label(&m).as_deref(), Some("Erase Events"));
    assert_eq!(
        f.gesture_edits().last(),
        erase_command(StripLane::Controller(7), 0, Q).as_ref()
    );
    m.strip_stroke_ended(PANE, pt(Q, 2));
    assert_eq!(f.calls().last().map(String::as_str), Some("end_edit"));
}

#[test]
fn a_chart_that_stops_showing_drops_its_stroke() {
    let (f, mut m) = stroking();
    m.strip_stroke_began(PANE, 0, pt(0, 1), false);
    m.strip_stroke_moved(PANE, pt(Q, 2));
    assert!(m.gesture().is_some());
    m.show_detail(DetailEditorKind::List);
    assert_eq!(f.calls().last().map(String::as_str), Some("abort_edit"));
    assert!(m.gesture().is_none() && m.strip_stroke(PANE).is_none());
    // A press while the pane shows another editor draws nothing.
    f.clear_calls();
    m.strip_stroke_began(PANE, 0, pt(0, 1), false);
    m.strip_stroke_moved(PANE, pt(Q, 2));
    assert!(m.strip_stroke(PANE).is_none() && f.calls().is_empty());

    m.show_detail(DetailEditorKind::StripChart);
    m.strip_stroke_began(PANE, 0, pt(0, 1), false);
    m.strip_stroke_moved(PANE, pt(Q, 2));
    assert!(m.gesture().is_some());
    m.close_detail();
    assert_eq!(f.calls().last().map(String::as_str), Some("abort_edit"));
    assert!(m.gesture().is_none() && m.strip_stroke(PANE).is_none());
    assert!(!f.edit_is_open());

    // A stroke that never opened goes too.
    m.show_detail(DetailEditorKind::StripChart);
    m.strip_stroke_began(PANE, 0, pt(0, 1), false);
    assert!(m.strip_stroke(PANE).is_some());
    m.open_detail(chords(), Some(DetailEditorKind::PianoRoll));
    assert!(m.strip_stroke(PANE).is_none());
}

// MARK: Exact Mode (Task 7, `T/StripChartModelTests.swift`)

#[test]
fn exact_mode_opens_the_panel_instead_of_applying() {
    let (f, mut m) = stroking();
    m.set_exact_mode(true);
    m.set_strip_row(PANE, 0, StripLane::Controller(7));
    f.clear_calls();
    m.strip_stroke_began(PANE, 0, pt(0, 0), false);
    m.strip_stroke_moved(PANE, pt(Q, 99));
    m.strip_stroke_ended(PANE, pt(Q, 99));
    // The last edit call (the panel's selection follows it).
    let edit_calls: Vec<String> = f
        .calls()
        .into_iter()
        .filter(|c| c.starts_with("begin_edit") || c == "edit_gesture" || c.ends_with("_edit"))
        .collect();
    assert_eq!(
        edit_calls.last().map(String::as_str),
        Some("abort_edit"),
        "previewed, then put back"
    );
    assert!(f.edits().is_empty() && !f.edit_is_open());
    assert!(m.gesture().is_none() && m.strip_stroke(PANE).is_none());
    assert!(m.take_modify_present());
    assert!(m.select_modify().is_open());
    assert_eq!(m.select_modify().task, PanelTask::Change);
    match m.select_modify().config.modification() {
        Some(ModifyRow::Line { target, op, line }) => {
            assert_eq!(
                target,
                LineTargetRow::Series {
                    series: SeriesRow::Controller { index: 7 }
                }
            );
            assert_eq!(op, LineOpRow::Draw);
            assert_eq!((line.from, line.to), (0, Q));
        }
        other => panic!("no line in the panel: {other:?}"),
    }
    // The selection is the stroke's range on the shown track.
    let sel = m.selection();
    assert_eq!(sel.lanes, vec![LaneRef { seq: 3, lane: 31 }]);
    assert_eq!(sel.time_range.map(|r| (r.start, r.end)), Some((0, Q + 1)));
}

#[test]
fn escape_during_an_exact_strip_stroke_opens_nothing() {
    let (f, mut m) = stroking();
    m.set_exact_mode(true);
    m.strip_stroke_began(PANE, 0, pt(0, 1), false);
    m.strip_stroke_moved(PANE, pt(Q, 2));
    m.strip_stroke_cancelled(PANE);
    assert_eq!(f.calls().last().map(String::as_str), Some("abort_edit"));
    assert!(!m.take_modify_present());
    assert!(!m.select_modify().is_open());
    // The release that follows opens nothing either.
    m.strip_stroke_ended(PANE, pt(Q, 2));
    assert!(!m.take_modify_present() && !m.select_modify().is_open());
}
