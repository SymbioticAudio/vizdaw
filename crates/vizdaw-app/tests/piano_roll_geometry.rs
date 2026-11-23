//! Swift `PianoRollGeometryTests` and the keyboard labels of
//! `KeyboardTests`, plus the long-track hit test.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use vizdaw_app::arrangement::{MeterBars, Point, Rect, TimeAxis};
use vizdaw_app::editor::{Division, Snap, Tool};
use vizdaw_app::piano_roll::{
    CursorKind, Eraser, LaneNotes, NoteDrag, NoteHit, NoteZone, Pencil, PitchAxis, RollGeometry,
    RollHit, a11y_label, band_rect, is_black, key_label,
};
use vizdaw_ffi::edit::{EditCommand, EventEdit, LaneRef};
use vizdaw_ffi::events::{EventKindRow, EventRow, MeterEntryRow};

const Q: i64 = 15_360;
const METER: [MeterEntryRow; 1] = [MeterEntryRow {
    tick: 0,
    num: 4,
    den: 4,
    sharps: 0,
    minor: false,
}];
const LANE_A: LaneRef = LaneRef { seq: 3, lane: 31 };
const LANE_B: LaneRef = LaneRef { seq: 3, lane: 30 };

/// 96 ticks per point: a quarter is 160 pt. Rows of 10 pt, pitch 127 at y 0.
fn time() -> TimeAxis {
    TimeAxis::new(96., 0)
}

fn pitch() -> PitchAxis {
    PitchAxis::new(10., 0.)
}

fn note_len(id: u64, tick: i64, p: u8, length: i64) -> EventRow {
    EventRow {
        id,
        velocity: 0x8000,
        ..Pencil::note(tick, p, length)
    }
}

fn note(id: u64, tick: i64, p: u8) -> EventRow {
    note_len(id, tick, p, Q)
}

fn lane(key: LaneRef, notes: Vec<EventRow>) -> LaneNotes {
    LaneNotes::new(key, notes.into())
}

fn y(p: u8) -> f32 {
    pitch().center(p)
}

fn pt(x: f32, y: f32) -> Point {
    Point { x, y }
}

fn ev(edit: EventEdit) -> Option<EditCommand> {
    Some(EditCommand::Event { edit })
}

fn snap(division: Division) -> Snap {
    Snap {
        division,
        ..Snap::default()
    }
}

#[test]
fn rects_have_a_minimum_width() {
    assert_eq!(
        RollGeometry::rect(&note(1, Q, 60), &time(), &pitch()),
        Rect {
            x: 160.,
            y: 670.,
            width: 160.,
            height: 10.
        }
    );
    assert_eq!(
        RollGeometry::rect(&note_len(1, 0, 60, 0), &time(), &pitch()).width,
        3.
    );
}

#[test]
fn zones_on_wide_and_narrow_notes() {
    let wide = Rect {
        x: 0.,
        y: 0.,
        width: 160.,
        height: 10.,
    };
    assert_eq!(RollGeometry::zone(7., &wide), NoteZone::Start);
    assert_eq!(RollGeometry::zone(8., &wide), NoteZone::Middle);
    assert_eq!(RollGeometry::zone(151., &wide), NoteZone::Middle);
    assert_eq!(RollGeometry::zone(152., &wide), NoteZone::End);
    let narrow = Rect { width: 12., ..wide };
    assert_eq!(RollGeometry::zone(3., &narrow), NoteZone::Start);
    assert_eq!(RollGeometry::zone(6., &narrow), NoteZone::Middle);
    assert_eq!(RollGeometry::zone(9., &narrow), NoteZone::End);
}

#[test]
fn the_active_lane_wins_and_the_latest_note_wins() {
    let lanes = [
        lane(LANE_B, vec![note_len(7, 0, 60, 4 * Q)]),
        lane(LANE_A, vec![note_len(1, 0, 60, 2 * Q), note(2, Q, 60)]),
    ];
    let p = pt(200., y(60));
    let RollHit::Note(h) = RollGeometry::hit(p, &lanes, Some(LANE_A), &time(), &pitch()) else {
        panic!("no hit");
    };
    assert!(h.lane == LANE_A && h.row.id == 2 && h.zone == NoteZone::Middle);
    let RollHit::Note(hb) = RollGeometry::hit(p, &lanes, Some(LANE_B), &time(), &pitch()) else {
        panic!("no hit");
    };
    assert_eq!(hb.row.id, 7);
    assert_eq!(
        RollGeometry::hit(pt(200., y(61)), &lanes, Some(LANE_A), &time(), &pitch()),
        RollHit::Empty {
            tick: 19_200,
            pitch: 61
        }
    );
}

#[test]
fn the_paint_order_puts_the_active_lane_on_top() {
    let c = LaneRef { seq: 3, lane: 32 };
    let lanes = [lane(LANE_B, vec![]), lane(LANE_A, vec![]), lane(c, vec![])];
    let order: Vec<LaneRef> = RollGeometry::paint_order(&lanes, Some(LANE_A))
        .iter()
        .map(|l| l.lane)
        .collect();
    assert_eq!(order, [c, LANE_B, LANE_A]);
}

#[test]
fn rubber_band_finds_notes_of_every_lane() {
    let lanes = [
        lane(LANE_B, vec![note(7, 0, 64)]),
        lane(LANE_A, vec![note(1, 0, 60), note(2, 4 * Q, 60)]),
    ];
    let r = band_rect(pt(100., y(65)), pt(10., y(59)));
    let found = RollGeometry::notes_in(&r, &lanes, &time(), &pitch());
    assert_eq!(found, [(LANE_B, 7), (LANE_A, 1)]);
}

#[test]
fn the_dragged_set_is_the_selection_when_the_pressed_note_is_selected() {
    let lanes = [
        lane(LANE_B, vec![note(7, 0, 64)]),
        lane(LANE_A, vec![note(1, 0, 60), note(2, Q, 62)]),
    ];
    let sel = |ids: &[u64]| ids.iter().copied().collect::<HashSet<u64>>();
    assert_eq!(NoteDrag::events(1, &sel(&[1, 7]), &lanes), [1, 7]);
    assert_eq!(NoteDrag::events(2, &sel(&[1, 7]), &lanes), [2]);
    assert_eq!(
        NoteDrag::events(1, &sel(&[1, 99]), &lanes),
        [1],
        "hidden lanes are not dragged"
    );
}

fn drag(zone: NoteZone, copy: bool, lowest: u8, highest: u8) -> NoteDrag {
    NoteDrag::new(
        zone,
        note(1, Q, 60),
        LANE_A,
        vec![1, 2],
        lowest,
        highest,
        copy,
    )
}

fn plain(zone: NoteZone) -> NoteDrag {
    drag(zone, false, 60, 64)
}

#[test]
fn start_zone_moves_in_time_snapping_the_pressed_start() {
    let s = snap(Division::Quarter);
    let d = plain(NoteZone::Start);
    assert_eq!(d.label(), "Move Notes");
    assert_eq!(
        d.command(9_000, -40., 10., &s, &METER),
        ev(EventEdit::Move {
            events: vec![1, 2],
            delta: Q,
            to_lane: None
        })
    );
    assert_eq!(d.command(100, 0., 10., &s, &METER), None);
    let copy = drag(NoteZone::Start, true, 60, 64);
    assert_eq!(
        copy.command(-Q, 0., 10., &s, &METER),
        ev(EventEdit::Duplicate {
            events: vec![1, 2],
            ticks: -Q,
            semitones: 0
        })
    );
    assert_eq!(copy.label(), "Copy Notes");
}

#[test]
fn middle_zone_transposes() {
    let s = Snap::default();
    assert_eq!(plain(NoteZone::Middle).label(), "Transpose Notes");
    assert_eq!(
        plain(NoteZone::Middle).command(50_000, -21., 10., &s, &METER),
        ev(EventEdit::Nudge {
            events: vec![1, 2],
            ticks: 0,
            semitones: 2
        })
    );
    assert_eq!(
        drag(NoteZone::Middle, true, 60, 64).command(0, 30., 10., &s, &METER),
        ev(EventEdit::Duplicate {
            events: vec![1, 2],
            ticks: 0,
            semitones: -3
        })
    );
    assert_eq!(drag(NoteZone::Middle, true, 60, 64).label(), "Copy Notes");
    assert_eq!(plain(NoteZone::Middle).sounding_pitch(-21., 10.), 62);
    assert_eq!(plain(NoteZone::Start).sounding_pitch(-21., 10.), 60);
}

#[test]
fn transpose_clamps_the_group_at_the_edges() {
    let d = drag(NoteZone::Middle, false, 60, 125);
    assert_eq!(d.semitones(10), 2);
    assert_eq!(d.semitones(-100), -60);
    assert_eq!(
        d.command(0, -1_000., 10., &Snap::default(), &METER),
        ev(EventEdit::Nudge {
            events: vec![1, 2],
            ticks: 0,
            semitones: 2
        })
    );
    let top = drag(NoteZone::Middle, false, 127, 127);
    assert_eq!(top.command(0, -50., 10., &Snap::default(), &METER), None);
}

#[test]
fn end_zone_resizes_snapping_the_end_and_keeps_one_tick() {
    let mut s = snap(Division::Sixteenth);
    let d = drag(NoteZone::End, true, 60, 64);
    assert_eq!(d.label(), "Resize Notes", "⌥ is ignored at the end");
    assert_eq!(
        d.command(3_000, 0., 10., &s, &METER),
        ev(EventEdit::Resize {
            events: vec![1, 2],
            delta: 3_840
        })
    );
    assert_eq!(d.duration_delta(-10 * Q, &s, &METER), 1 - Q);
    s.enabled = false;
    assert_eq!(d.duration_delta(7, &s, &METER), 7);
    assert_eq!(d.duration_delta(-10 * Q, &s, &METER), 1 - Q);
}

#[test]
fn pencil_starts_on_the_grid_and_paints_a_length() {
    let mut s = snap(Division::Eighth);
    assert_eq!(Pencil::start(10_000, &s, &METER), 7_680);
    assert_eq!(Pencil::step(0, &s, &METER), 7_680);
    assert_eq!(
        Pencil::length(7_680, 7_000, &s, &METER),
        7_680,
        "at least one step"
    );
    assert_eq!(Pencil::length(7_680, 30_000, &s, &METER), 23_040);
    s.enabled = false;
    assert_eq!(Pencil::step(0, &s, &METER), Q);
    assert_eq!(Pencil::length(0, 100, &s, &METER), Q / 8);
    assert_eq!(Pencil::length(0, 5_000, &s, &METER), 5_000);
    let row = Pencil::note(7_680, 64, 7_680);
    assert!(
        row.kind == EventKindRow::Note
            && row.velocity == 51_200
            && row.duration == 7_680
            && row.pitch == 64
            && row.release_velocity == 0
            && !row.mute
            && row.dest.is_none()
            && row.id == 0
    );
    assert_eq!(
        Some(Pencil::command(LANE_A, 7_680, 64, 7_680)),
        ev(EventEdit::Insert {
            seq: 3,
            lane: 31,
            events: vec![row]
        })
    );
}

#[test]
fn a_segment_finds_every_note_it_crosses_in_order() {
    let n = |id, tick, p| note_len(id, tick, p, 960);
    // Three notes on one row, 10 pt wide, 20 pt apart; one row above.
    let lanes = [lane(
        LaneRef { seq: 1, lane: 1 },
        vec![
            n(1, 0, 60),
            n(2, 1_920, 60),
            n(4, 1_920, 61),
            n(3, 3_840, 60),
        ],
    )];
    let y = pitch().center(60);
    let crossed = |a: Point, b: Point| Eraser::crossed(a, b, &lanes, &time(), &pitch());
    // Sampled points 5 and 45 only touch 1 and 3; the segment crosses 1, 2, 3.
    assert_eq!(crossed(pt(5., y), pt(45., y)), [1, 2, 3]);
    // Right to left: the order the pointer meets them.
    assert_eq!(crossed(pt(45., y), pt(5., y)), [3, 2, 1]);
    // A miss, and a single point.
    let above = pitch().center(70);
    assert!(crossed(pt(0., above), pt(60., above)).is_empty());
    assert_eq!(crossed(pt(25., y), pt(25., y)), [2]);
    assert_eq!(Eraser::adding(&[2, 1, 3], &[1]), [1, 2, 3]);
}

#[test]
fn the_eraser_collects_each_note_once() {
    let hit = RollHit::Note(Box::new(NoteHit {
        lane: LANE_A,
        row: note(2, 0, 60),
        zone: NoteZone::Middle,
    }));
    let once = Eraser::adding_hit(&hit, &[1]);
    assert_eq!(once, [1, 2]);
    assert_eq!(Eraser::adding_hit(&hit, &once), [1, 2]);
    assert_eq!(
        Eraser::adding_hit(&RollHit::Empty { tick: 0, pitch: 1 }, &once),
        [1, 2]
    );
    assert_eq!(Eraser::command(&[]), None);
    assert_eq!(
        Eraser::command(&[1, 2]),
        ev(EventEdit::Delete { events: vec![1, 2] })
    );
}

#[test]
fn cursors_follow_zones_and_tools() {
    let n = |zone| {
        RollHit::Note(Box::new(NoteHit {
            lane: LANE_A,
            row: note(1, 0, 60),
            zone,
        }))
    };
    let empty = RollHit::Empty { tick: 0, pitch: 0 };
    assert_eq!(
        CursorKind::of(&n(NoteZone::Start), Tool::Arrow),
        CursorKind::OpenHand
    );
    assert_eq!(
        CursorKind::of(&n(NoteZone::Middle), Tool::Pencil),
        CursorKind::UpDown
    );
    assert_eq!(
        CursorKind::of(&n(NoteZone::End), Tool::Arrow),
        CursorKind::ResizeRight
    );
    assert_eq!(
        CursorKind::of(&n(NoteZone::End), Tool::Eraser),
        CursorKind::Eraser
    );
    assert_eq!(CursorKind::of(&empty, Tool::Pencil), CursorKind::Crosshair);
    assert_eq!(CursorKind::of(&empty, Tool::Arrow), CursorKind::Arrow);
}

#[test]
fn extreme_inputs_clamp_instead_of_trapping() {
    let s = Snap::default();
    let big = i64::MAX;
    let d = plain(NoteZone::End);
    let _ = d.duration_delta(big, &s, &METER);
    let _ = d.duration_delta(i64::MIN, &s, &METER);
    let _ = NoteDrag::time_delta(big, big, &s, &METER);
    let _ = NoteDrag::time_delta(i64::MIN, i64::MIN, &s, &METER);
    let _ = Pencil::length(i64::MIN, big, &s, &METER);
    let _ = Pencil::start(i64::MIN, &s, &METER);
    assert_eq!(NoteDrag::raw_semitones(f32::NAN, 10.), 0);
    assert_eq!(NoteDrag::raw_semitones(f32::INFINITY, 10.), 0);
    assert_eq!(NoteDrag::raw_semitones(-1e30, 10.), 1_000);
    assert_eq!(NoteDrag::raw_semitones(-10., 0.), 0);
    let bad = drag(NoteZone::Middle, false, 255, 255);
    assert!(bad.semitones(-1_000).unsigned_abs() <= 127);
    let _ = bad.command(0, 1e30, 10., &s, &METER);
    let _ = bad.sounding_pitch(1e30, 10.);
    let nan = f32::NAN;
    let lanes = [lane(LANE_A, vec![note(1, 0, 60)])];
    let _ = RollGeometry::hit(pt(nan, nan), &lanes, Some(LANE_A), &time(), &pitch());
    let _ = RollGeometry::rect(&note_len(1, i64::MAX, 255, i64::MAX), &time(), &pitch());
    let _ = RollGeometry::notes_in(
        &band_rect(pt(nan, 0.), Point::default()),
        &lanes,
        &time(),
        &pitch(),
    );
    let _ = Eraser::crossed(pt(nan, nan), pt(0., 0.), &lanes, &time(), &pitch());
}

#[test]
fn accessibility_labels() {
    let bars = MeterBars::new(&METER);
    let mut n = note(1, 2 * 4 * Q, 60);
    n.velocity = 100 << 9;
    assert_eq!(
        a11y_label(&n, None, &bars),
        "Note C4, bar 3 beat 1, velocity 100"
    );
    n.tick += Q + 240;
    n.mute = true;
    assert_eq!(
        a11y_label(&n, Some("Chords"), &bars),
        "Note C4, bar 3 beat 2 tick 240, velocity 100, Chords, muted"
    );
}

#[test]
fn only_cs_are_labelled_and_black_keys_are_known() {
    assert_eq!(key_label(60).as_deref(), Some("C4"));
    assert_eq!(key_label(0).as_deref(), Some("C-1"));
    assert_eq!(key_label(61), None);
    let black: Vec<u8> = (60..72).filter(|p| is_black(*p)).collect();
    assert_eq!(black, [61, 63, 66, 68, 70]);
    assert!(!is_black(127) && is_black(126));
}

// Beyond the Swift suites.

/// 100 000 quarter notes, one per eighth, cycling through two octaves.
fn long_lane() -> LaneNotes {
    let notes: Vec<EventRow> = (0..100_000u64)
        .map(|i| note(i + 1, i as i64 * Q / 2, 48 + (i % 24) as u8))
        .collect();
    lane(LANE_A, notes)
}

#[test]
fn hits_on_a_long_track_find_the_right_note() {
    let lanes = [long_lane()];
    let t = time();
    let p = pitch();
    // Note 50 001 starts at tick 25 000 * Q at pitch 48 + (50 000 % 24).
    let start = 25_000 * Q;
    let pitch_of = 48 + (50_000 % 24) as u8;
    let at = pt(t.x(start) + 40., p.center(pitch_of));
    let RollHit::Note(h) = RollGeometry::hit(at, &lanes, None, &t, &p) else {
        panic!("no hit");
    };
    assert_eq!(h.row.id, 50_001);
    // The previous note (an eighth earlier, a row lower) still reaches
    // here: its row is not this one, so it is not hit.
    let lower = pt(at.x, p.center(pitch_of - 1));
    let RollHit::Note(h) = RollGeometry::hit(lower, &lanes, None, &t, &p) else {
        panic!("no hit");
    };
    assert_eq!(h.row.id, 50_000);
    let band = band_rect(at, pt(at.x + 1., at.y + 1.));
    assert_eq!(RollGeometry::notes_in(&band, &lanes, &t, &p).len(), 1);
}

#[test]
fn hit_testing_a_long_track_is_quick() {
    let lanes = [long_lane()];
    let t = time();
    let p = pitch();
    let began = Instant::now();
    let mut found = 0;
    for i in 0..10_000 {
        let x = (i as f32) * 37.;
        let y = p.center(48 + (i % 30) as u8);
        if matches!(
            RollGeometry::hit(pt(x, y), &lanes, None, &t, &p),
            RollHit::Note(_)
        ) {
            found += 1;
        }
        let _ = Eraser::crossed(pt(x, y), pt(x + 20., y + 15.), &lanes, &t, &p);
        let _ = RollGeometry::notes_in(&band_rect(pt(x, y), pt(x + 30., y + 30.)), &lanes, &t, &p);
    }
    let took = began.elapsed();
    assert!(found > 0);
    // A full scan per test would take tens of seconds in a debug build.
    assert!(
        took < Duration::from_secs(2),
        "30 000 hit tests over 100 000 notes took {took:?}"
    );
}
