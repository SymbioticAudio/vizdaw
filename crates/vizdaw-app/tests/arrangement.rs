//! The arrangement's maths (Swift `TimeAxisTests`, `ScrollingTests`,
//! `GridTests`, `ArrangementLayoutTests`): no GPUI.

mod support;

use std::sync::Arc;

use support::demo_fake;
use vizdaw_app::DocumentModel;
use vizdaw_app::arrangement::{
    AutoScroll, BLOCK_RULE, Geometry, GridKind, GridLine, MeterBars, PageFollow, Rect,
    RulerSelection, TimeAxis, WheelScroll, a11y_label, a11y_label_in, grid_lines,
    grid_lines_between, label_every, sketch,
};
use vizdaw_ffi::blocks::{BlockKind, BlockRow, BlockRule};
use vizdaw_ffi::events::{EventKindRow, EventRow, MeterEntryRow};
use vizdaw_ffi::selection::TickRange;

const Q: i64 = 15_360;
const BAR: i64 = 4 * Q;

fn meter(tick: i64, num: u8, den: u8) -> MeterEntryRow {
    MeterEntryRow {
        tick,
        num,
        den,
        sharps: 0,
        minor: false,
    }
}

fn four_four() -> Vec<MeterEntryRow> {
    vec![meter(0, 4, 4)]
}

fn bar_numbers(lines: &[GridLine]) -> Vec<i64> {
    lines
        .iter()
        .filter_map(|l| match l.kind {
            GridKind::Bar(n) => Some(n),
            GridKind::Beat => None,
        })
        .collect()
}

fn block(start: i64, end: i64, ids: &[u64], reference: bool, muted: bool) -> BlockRow {
    BlockRow {
        start,
        end,
        kind: if reference {
            BlockKind::Reference
        } else {
            BlockKind::Events
        },
        events: ids.to_vec(),
        label: reference.then(|| "Groove".to_string()),
        ref_seq: reference.then_some(2),
        muted,
    }
}

// MARK: TimeAxis

#[test]
fn ticks_and_points_round_trip() {
    let a = TimeAxis::new(100., 1_000);
    assert_eq!(a.x(1_000), 0.);
    assert_eq!(a.x(2_000), 10.);
    assert_eq!(a.tick_at(10.), 2_000);
    assert_eq!(a.width_of_ticks(15_360), 153.6);
    assert_eq!(a.visible_range(100.), 1_000..=11_000);
}

#[test]
fn zoom_keeps_the_tick_under_the_pointer() {
    let a = TimeAxis::new(100., 1_000);
    let z = a.zoom_about(2., 50.);
    assert_eq!(z.ticks_per_point(), 50.);
    assert_eq!(z.tick_at(50.), a.tick_at(50.));
}

#[test]
fn zoom_clamps_to_its_limits() {
    let a = TimeAxis::default();
    assert_eq!(a.ticks_per_point(), 384.);
    assert_eq!(
        a.zoom_about(1e9, 0.).ticks_per_point(),
        TimeAxis::MIN_TICKS_PER_POINT
    );
    assert_eq!(
        a.zoom_about(1e-9, 0.).ticks_per_point(),
        TimeAxis::MAX_TICKS_PER_POINT
    );
    assert_eq!(TimeAxis::MIN_TICKS_PER_POINT, 2.);
    assert_eq!(TimeAxis::MAX_TICKS_PER_POINT, 61_440.);
    // Nonsense factors leave the axis as it is.
    assert_eq!(a.zoom_about(f64::NAN, 0.), a);
    assert_eq!(a.zoom_about(0., 0.), a);
}

#[test]
fn extreme_offsets_clamp_instead_of_trapping() {
    let far = TimeAxis::default().scrolled_by(1e30);
    assert_eq!(far.origin(), TimeAxis::LIMIT);
    assert_eq!(far.tick_at(f32::MAX), TimeAxis::LIMIT);
    let back = TimeAxis::default().scrolled_by(-1e30);
    assert_eq!(back.tick_at(0.), 0, "never left of bar 1");
    assert_eq!(TimeAxis::clamp_tick(f64::NAN), 0);
    assert!(TimeAxis::default().x(i64::MAX).is_finite());
    assert_eq!(
        TimeAxis::default().scrolled_to(f64::INFINITY).origin(),
        TimeAxis::LIMIT
    );
    assert_eq!(TimeAxis::LIMIT, 1 << 50);
    assert_eq!(TimeAxis::new(f64::NAN, 0).ticks_per_point(), 2.);
    assert_eq!(
        TimeAxis::new(f64::INFINITY, i64::MIN).origin(),
        -TimeAxis::LIMIT
    );
    assert_eq!(TimeAxis::default().scrolled_by(f32::NAN).origin(), 0);
    assert_eq!(
        TimeAxis::new(2., TimeAxis::LIMIT).zoom_about(1.5, f32::MAX),
        TimeAxis::new(2., TimeAxis::LIMIT).zoom_about(1.5, f32::MAX),
        "no panic at the far end"
    );
}

/// Owner, 2026-11-05: the user never scrolls or zooms left of bar 1; a
/// count-in's view (left of it) only moves right.
#[test]
fn scroll_and_zoom_stop_at_bar_one() {
    let a = TimeAxis::new(10., 500);
    assert_eq!(a.scrolled_by(-100.).origin(), 0);
    assert_eq!(a.zoom_about(0.01, 400.).origin(), 0);
    let count_in = a.scrolled_to(-3_840.);
    assert_eq!(count_in.origin(), -3_840, "following a count-in");
    assert_eq!(count_in.scrolled_by(-10.).origin(), -3_840);
    assert_eq!(count_in.scrolled_by(10.).origin(), -3_740);
}

#[test]
fn visible_range_with_nan_width_does_not_trap() {
    let r = TimeAxis::default().visible_range(f32::NAN);
    assert_eq!(r.start(), r.end());
}

// MARK: Scrolling

#[test]
fn auto_scroll_starts_near_the_edges() {
    assert_eq!(AutoScroll::speed(100., 400.), 0.);
    assert_eq!(AutoScroll::speed(24., 400.), 0.);
    assert_eq!(AutoScroll::speed(14., 400.), -200.);
    assert_eq!(AutoScroll::speed(390., 400.), 280.);
    assert_eq!(AutoScroll::speed(-500., 400.), -AutoScroll::MAX_SPEED);
    assert_eq!(
        (AutoScroll::EDGE, AutoScroll::RATE, AutoScroll::MAX_SPEED),
        (24., 20., 1_200.)
    );
}

#[test]
fn page_follow_jumps_when_the_playhead_leaves() {
    let a = TimeAxis::new(100., 0);
    assert_eq!(PageFollow::origin(5_000., &a, 400.), None);
    assert_eq!(
        PageFollow::origin(41_000., &a, 400.),
        Some((41_000 - 1_600) as f64)
    );
    assert_eq!(
        PageFollow::origin(-100., &a, 400.),
        Some((-100 - 1_600) as f64)
    );
    assert_eq!(PageFollow::MARGIN, 16.);
}

/// Slice 3 has no Snap setting: the range is the raw ticks (Swift snaps).
#[test]
fn ruler_ranges_ignore_direction() {
    let a = TimeAxis::new(128., 0);
    let r = RulerSelection::range(a.tick_at(250.), 10., &a);
    assert_eq!(
        r,
        Some(TickRange {
            start: 1_280,
            end: 32_000
        })
    );
    assert_eq!(RulerSelection::range(640, 5., &a), None, "empty");
}

#[test]
fn wheel_lines_scale_to_points_and_trackpad_points_pass_through() {
    assert_eq!(WheelScroll::points(3., -5., true), (3., -5.));
    assert_eq!(WheelScroll::points(1., -2., false), (12., -24.));
}

#[test]
fn one_wheel_notch_zooms_about_ten_percent() {
    assert!((WheelScroll::zoom_factor(1., false) - 1.1).abs() < 1e-9);
    assert!((WheelScroll::zoom_factor(-1., false) - 1. / 1.1).abs() < 1e-9);
    assert!((WheelScroll::zoom_factor(1., true) - 1.01).abs() < 1e-9);
    assert_eq!(WheelScroll::zoom_factor(0., false), 1.);
}

// MARK: Grid

#[test]
fn bars_and_beats_at_normal_zoom() {
    let lines = grid_lines(&TimeAxis::new(128., 0), 960., &four_four());
    assert_eq!(lines.iter().filter(|l| l.kind == GridKind::Beat).count(), 6);
    assert_eq!(bar_numbers(&lines), vec![1, 2, 3]);
    assert_eq!(lines.first().map(|l| l.tick), Some(0));
}

#[test]
fn beats_drop_out_when_they_crowd() {
    let lines = grid_lines_between(&four_four(), 0, 4 * BAR, Q as f64 / 4.);
    assert!(lines.iter().all(|l| l.kind != GridKind::Beat));
    assert_eq!(lines.len(), 5);
}

#[test]
fn bars_thin_out_far_out() {
    // One point per bar: every eighth bar keeps 6 points apart.
    let lines = grid_lines_between(&four_four(), 0, 20 * BAR, BAR as f64);
    assert_eq!(bar_numbers(&lines), vec![1, 9, 17]);
}

#[test]
fn pickup_bars_are_numbered_backwards() {
    let axis = TimeAxis::new(512., 0).scrolled_to((-2 * BAR) as f64);
    let lines = grid_lines(&axis, (2 * BAR / 512) as f32, &four_four());
    let bars: Vec<(i64, i64)> = lines
        .iter()
        .filter_map(|l| match l.kind {
            GridKind::Bar(n) => Some((n, l.tick)),
            GridKind::Beat => None,
        })
        .collect();
    assert_eq!(bars, vec![(-1, -2 * BAR), (0, -BAR), (1, 0)]);
}

#[test]
fn meter_changes_move_the_beats() {
    let m = vec![meter(0, 4, 4), meter(BAR, 3, 4)];
    let lines = grid_lines_between(&m, BAR, BAR + 3 * Q, 128.);
    let ticks: Vec<i64> = lines.iter().map(|l| l.tick).collect();
    assert_eq!(ticks, vec![BAR, BAR + Q, BAR + 2 * Q, BAR + 3 * Q]);
    assert_eq!(lines.last().map(|l| l.kind), Some(GridKind::Bar(3)));
}

#[test]
fn labels_spread_out() {
    assert_eq!(label_every(BAR, 128.), 1);
    assert_eq!(label_every(BAR, BAR as f64 / 10.), 4);
}

#[test]
fn the_grid_is_capped_and_survives_hostile_axes() {
    let lines = grid_lines_between(&four_four(), 0, TimeAxis::LIMIT, 2.);
    assert!(lines.len() <= 4_000);
    let far = TimeAxis::new(61_440., TimeAxis::LIMIT);
    assert!(grid_lines(&far, 10_000., &four_four()).len() <= 4_000);
    assert!(grid_lines(&TimeAxis::default(), f32::NAN, &[]).len() <= 4_000);
}

// MARK: Layout

#[test]
fn blocks_sit_inside_their_rows() {
    let axis = TimeAxis::new(384., 0);
    let g = Geometry::default();
    assert_eq!(g.lane_height, 48.);
    assert_eq!(
        g.block_rect(&block(0, BAR, &[1], false, false), 1, &axis),
        Rect {
            x: 0.,
            y: 50.,
            width: 160.,
            height: 44.
        }
    );
    assert_eq!(
        g.block_rect(&block(BAR, BAR, &[1], false, false), 0, &axis)
            .width,
        4.,
        "zero length stays clickable"
    );
    assert_eq!(g.row_at(47., 2), Some(0));
    assert_eq!(g.row_at(48., 2), Some(1));
    assert_eq!(g.row_at(96., 2), None);
    let scrolled = Geometry {
        vertical_offset: 48.,
        ..g
    };
    assert_eq!(scrolled.row_at(0., 2), Some(1));
    assert_eq!(scrolled.row_top(1), 0.);
    assert_eq!(g.content_height(2), 144., "one spare row for adding tracks");
}

#[test]
fn the_vertical_offset_stays_inside_the_content() {
    let g = Geometry::default();
    // Two tracks and the add row: 144 high, 100 shown.
    assert_eq!(g.clamp_vertical(-10., 2, 100.), 0.);
    assert_eq!(g.clamp_vertical(30., 2, 100.), 30.);
    assert_eq!(g.clamp_vertical(500., 2, 100.), 44.);
    assert_eq!(g.clamp_vertical(500., 2, 1_000.), 0.);
    assert_eq!(g.clamp_vertical(f32::NAN, 2, 100.), 0.);
}

#[test]
fn accessibility_labels_name_kind_lane_and_bars() {
    assert_eq!(
        a11y_label(
            &block(0, 2 * BAR, &[1, 2], false, false),
            "Chords",
            &four_four()
        ),
        "Block on Chords, bars 1 to 2, 2 events"
    );
    assert_eq!(
        a11y_label(
            &block(BAR, 2 * BAR, &[9], true, true),
            "Groove",
            &four_four()
        ),
        "Reference to Groove on Groove, bar 2, muted"
    );
    assert_eq!(
        a11y_label(&block(0, BAR, &[4], false, false), "Kick", &four_four()),
        "Block on Kick, bar 1, 1 event"
    );
}

#[test]
fn blocks_are_split_at_four_beats_of_silence() {
    assert_eq!(BLOCK_RULE, BlockRule::SilenceGap { ticks: 4 * Q });
}

#[test]
fn the_sketch_spans_the_blocks_lowest_to_highest_note() {
    let axis = TimeAxis::new(384., 0);
    let note = |id, tick, pitch| EventRow {
        pitch,
        duration: Q,
        ..EventRow::blank(id, tick, EventKindRow::Note)
    };
    let events = vec![note(1, 0, 60), note(2, Q, 72), note(3, 3 * BAR, 40)];
    let b = block(0, 2 * Q, &[1, 2], false, false);
    let r = Geometry::default().block_rect(&b, 0, &axis);
    let bars = sketch(&b, &r, &events, &axis);
    assert_eq!(bars.len(), 2, "the note after the block is not drawn");
    // The lowest note sits 5 above the bottom, the highest 5 below the top.
    assert_eq!(bars[0].y, r.y + r.height - 5. - 1.);
    assert_eq!(bars[1].y, r.y + 5. - 1.);
    assert_eq!(bars[0].height, 2.);
    assert_eq!(bars[0].width, 40.);
}

// MARK: The model

#[test]
fn the_model_reads_blocks_and_selects_ruler_ranges() {
    let f = Arc::new(demo_fake());
    f.set_blocks(3, 31, vec![block(0, BAR, &[1], false, false)]);
    let mut m = DocumentModel::new(f.clone()).unwrap();
    assert_eq!(m.blocks(31).len(), 1);
    assert!(m.blocks(30).is_empty());
    m.select_range(TickRange { start: 0, end: BAR });
    let s = m.selection();
    assert_eq!(s.time_range, Some(TickRange { start: 0, end: BAR }));
    let lanes: Vec<u64> = s.lanes.iter().map(|l| l.lane).collect();
    assert_eq!(lanes, vec![30, 31], "every track of the shown sequence");
    assert!(s.events.is_empty());
}

#[test]
fn reads_of_one_snapshot_share_their_rows() {
    let f = Arc::new(demo_fake());
    f.set_blocks(3, 31, vec![block(0, BAR, &[1], false, false)]);
    f.set_events(3, 31, vec![EventRow::blank(1, 0, EventKindRow::Note)]);
    let m = DocumentModel::new(f.clone()).unwrap();
    assert!(Arc::ptr_eq(&m.blocks(31), &m.blocks(31)));
    assert!(Arc::ptr_eq(&m.lane_events(31), &m.lane_events(31)));
    assert_eq!(m.lane_events(31).len(), 1);
}

#[test]
fn one_meter_table_answers_every_bar() {
    let m = vec![meter(0, 4, 4), meter(BAR, 3, 4)];
    let bars = MeterBars::new(&m);
    assert_eq!(bars.bar_at(BAR + 3 * Q).number, 3);
    assert_eq!(bars.bar_at(-1).number, 0);
    let b = block(0, 2 * BAR, &[1, 2], false, false);
    assert_eq!(
        a11y_label_in(&b, "Chords", &bars),
        a11y_label(&b, "Chords", &m)
    );
}
