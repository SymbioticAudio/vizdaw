//! The Strip Chart canvas geometry (Swift `T/StripChartViewTests.swift`;
//! slice 10c Task 4): rows ↔ y, content points ↔ ticks and values, and the
//! stroke preview's track space. The default zoom is 160 points a quarter,
//! rows are 96 points tall.

use vizdaw_app::arrangement::TimeAxis;
use vizdaw_app::editor::Snap;
use vizdaw_app::strip_chart::{
    StripChartSettings, StripFrame, StripLane, StripMode, StripOp, StripPoint, TITLE_HEIGHT,
    TITLE_WIDTH,
};

const Q: i64 = 15_360;

fn frame(origin: i64, offset: f32) -> StripFrame {
    StripFrame {
        axis: TimeAxis::new(96., origin),
        row_height: 96.,
        offset,
    }
}

fn point(f: &StripFrame, x: f32, y: f32, s: &StripChartSettings) -> StripPoint {
    f.point(x, y, 0, StripLane::Velocity, s, &Snap::default(), &[])
}

#[test]
fn rows_map_points() {
    let f = frame(0, 0.);
    assert_eq!(f.row_at(10., 2), Some(0));
    assert_eq!(f.row_at(100., 2), Some(1));
    assert_eq!(f.row_at(250., 2), None);
    assert_eq!(f.row_at(-1., 2), None);
    assert_eq!(f.row_at(f32::NAN, 2), None);
    let s = StripChartSettings::default();
    assert_eq!(
        point(&f, 170., 3., &s),
        StripPoint {
            tick: Q,
            value: 0xFFFF
        }
    );
    assert_eq!(f.content_height(2), 192.);
    assert_eq!((TITLE_HEIGHT, TITLE_WIDTH), (18., 200.));
    assert!(f.in_title(199., 17., 0) && f.in_title(10., 96. + 5., 1));
    assert!(!f.in_title(200., 5., 0) && !f.in_title(10., 18., 0));
}

#[test]
fn points_follow_the_scrolled_offsets_and_clamp_at_the_edges() {
    let s = StripChartSettings::default();
    let f = frame(Q, 0.);
    assert_eq!(
        point(&f, 0., 30., &s).tick,
        Q,
        "the origin scrolled a quarter"
    );
    let f = frame(Q, 50.);
    assert_eq!(f.row_at(10., 2), Some(0), "rows move up with the offset");
    assert_eq!(f.row_at(60., 2), Some(1));
    assert_eq!(f.row_top(1), 46.);
    // Above a row's top and below its bottom: the range's ends.
    let (lo, hi) = StripLane::Velocity.range();
    assert_eq!(point(&f, 0., f.row_top(0) - 40., &s).value, hi);
    assert_eq!(point(&f, 0., f.row_top(0) + 400., &s).value, lo);
    assert_eq!(point(&f, 0., f32::INFINITY, &s).value, lo);
    // Far right of the axis's limit: no overflow.
    assert!(point(&f, f32::MAX, 30., &s).tick >= 0);
}

#[test]
fn ticks_left_of_the_origin_clamp_to_zero() {
    let s = StripChartSettings::default();
    assert_eq!(point(&frame(0, 0.), -50., 30., &s).tick, 0);
    let off = Snap {
        enabled: false,
        ..Snap::default()
    };
    let f = frame(0, 0.);
    assert_eq!(
        f.point(-50., 30., 0, StripLane::Velocity, &s, &off, &[])
            .tick,
        0
    );
    assert_eq!(
        f.point(170., 30., 0, StripLane::Velocity, &s, &off, &[])
            .tick,
        170 * 96
    );
}

#[test]
fn modify_mode_reads_the_operations_amount() {
    let s = StripChartSettings {
        mode: StripMode::Modify,
        op: StripOp::Scale,
        ..StripChartSettings::default()
    };
    let f = frame(0, 0.);
    let a = point(&f, 220., 3., &s);
    let b = point(&f, 330., 93., &s);
    assert_eq!((a.value, b.value), (200, 0), "top is 200 %, bottom 0 %");
    assert_eq!((a.tick, b.tick), (Q, 2 * Q));
    // Compress reads values, not amounts.
    let c = StripChartSettings {
        op: StripOp::Compress,
        ..s
    };
    assert_eq!(point(&f, 220., 3., &c).value, 0xFFFF);
}

#[test]
fn the_stroke_preview_stays_on_its_values_when_the_view_scrolls() {
    let before = frame(0, 0.);
    let after = frame(160 * 96, 10.);
    for (x, y) in [(220., 30.), (320., 48.)] {
        let p = before.lane_point(x, y);
        assert_eq!(before.content_point(p), (x, y));
        let (ax, ay) = after.content_point(p);
        assert!((ax - (x - 160.)).abs() < 1., "{ax}");
        assert!((ay - (y - 10.)).abs() < 1e-6, "{ay}");
    }
}
