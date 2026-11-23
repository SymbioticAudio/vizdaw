//! The strip's measurements and the shared rows: Swift `StripLayoutTests`,
//! `StripHeightTests` and `StripGridTests`, with the explicit row heights
//! of Decision M8.

mod support;

use std::collections::BTreeMap;

use support::strip_row;
use vizdaw_app::mixer::layout::{
    RowHeights, StripGrid, StripLayout, StripRowKind as K, StripSplit, rows_of,
};
use vizdaw_ffi::mixer::StripKindRow;

#[test]
fn the_fader_gets_a_long_throw_when_there_is_room() {
    let s = StripLayout::split(900., 300.);
    assert_eq!(s.throw, 480f64.min(900. - 300. - StripLayout::LOWER_FIXED));
    assert!(!s.scrolls);
}

#[test]
fn a_short_pane_scrolls_the_strip_and_keeps_the_minimum_throw() {
    let scrolls = StripSplit {
        throw: 160.,
        scrolls: true,
    };
    assert_eq!(StripLayout::split(260., 300.), scrolls);
    assert_eq!(StripLayout::split(120., 0.), scrolls);
    let edge = StripLayout::split(300. + StripLayout::LOWER_FIXED + 160., 300.);
    assert_eq!(
        edge,
        StripSplit {
            throw: 160.,
            scrolls: false
        }
    );
}

#[test]
fn at_every_pane_height_the_strip_fits_or_scrolls() {
    for upper in (0..=640).step_by(37) {
        for height in (0..=1400).step_by(7) {
            let (u, h) = (f64::from(upper), f64::from(height));
            let s = StripLayout::split(h, u);
            assert!(
                s.scrolls || StripLayout::content_height(u, s.throw) <= h,
                "height {h}, upper {u}"
            );
        }
    }
    let lower = StripLayout::LEVEL_BOX
        + StripLayout::TOGGLE_ROW
        + StripLayout::MODE_MENU
        + StripLayout::FADER_CAP
        + 4. * StripLayout::GAP
        + 2. * StripLayout::PADDING;
    assert_eq!(StripLayout::LOWER_FIXED, lower);
}

#[test]
fn strips_share_their_rows_and_master_keeps_none_of_its_missing_ones() {
    let synth = strip_row(1, "Synth", StripKindRow::BuiltinSynth);
    let audio = strip_row(5, "Audio 1", StripKindRow::AudioPath { path: 1 });
    let master = strip_row(0, "Master", StripKindRow::Master);
    let m = rows_of(&master, false);
    assert!(!m.contains_key(&K::Sends) && !m.contains_key(&K::Output) && !m.contains_key(&K::Pan));
    assert_eq!(rows_of(&synth, false)[&K::Instrument], RowHeights::SLOT);
    assert!(!rows_of(&audio, false).contains_key(&K::Input));
    assert_eq!(rows_of(&audio, true)[&K::Input], RowHeights::INPUT);
    assert!(
        !rows_of(&synth, false).contains_key(&K::MidiFx),
        "not drawn yet"
    );
    let all = [
        rows_of(&synth, false),
        rows_of(&audio, true),
        rows_of(&master, false),
    ];
    let g = StripGrid::new(&all, 700.);
    // Every row of any strip, in order, one gap apart.
    let order: Vec<K> = g.rows.keys().copied().collect();
    assert_eq!(
        order,
        [
            K::Header,
            K::Instrument,
            K::Input,
            K::Eq,
            K::Inserts,
            K::Sends,
            K::Output,
            K::Pan
        ]
    );
    assert_eq!(g.tops[&K::Header], StripLayout::PADDING);
    assert_eq!(
        g.tops[&K::Instrument],
        StripLayout::PADDING + RowHeights::HEADER + StripGrid::ROW_GAP
    );
    let up = StripGrid::upper(&g.rows);
    assert_eq!(g.fader_top, StripLayout::PADDING + up + StripLayout::GAP);
    // One strip's own rows alone would put its fader higher.
    let alone = StripGrid::new([&rows_of(&master, false)], 700.);
    assert!(alone.fader_top < g.fader_top);
    assert_eq!(g.total, StripLayout::content_height(up, g.throw));
    assert_eq!(
        g.mode_top + StripLayout::MODE_MENU + StripLayout::PADDING,
        g.total
    );
    assert_eq!(StripGrid::upper(&BTreeMap::new()), 0.);
}

#[test]
fn the_divider_sits_in_the_middle_of_the_gap_before_master() {
    let bus_right = StripGrid::x(2, Some(3)) + StripLayout::WIDTH;
    let master_left = StripGrid::x(3, Some(3));
    let mid = StripGrid::divider_x(3) + 0.5;
    assert!((mid - (bus_right + master_left) / 2.).abs() < 0.01);
    assert_eq!(
        StripGrid::width(4, Some(3)),
        master_left + StripLayout::WIDTH
    );
    assert_eq!(StripGrid::width(0, None), 0.);
}

#[test]
fn each_row_is_as_tall_as_the_tallest_strips() {
    let synth = BTreeMap::from([(K::Header, 40.), (K::Inserts, 36.), (K::Sends, 80.)]);
    let audio = BTreeMap::from([(K::Header, 40.), (K::Input, 22.), (K::Inserts, 58.)]);
    let master = BTreeMap::from([(K::Header, 44.), (K::Inserts, 36.), (K::Pan, 0.)]);
    let rows = StripGrid::row_heights([&synth, &audio, &master]);
    assert_eq!(
        rows,
        BTreeMap::from([
            (K::Header, 44.),
            (K::Input, 22.),
            (K::Inserts, 58.),
            (K::Sends, 80.)
        ]),
        "no strip has a pan row of height: it takes no space"
    );
    assert_eq!(
        StripGrid::upper(&rows),
        44. + 22. + 58. + 80. + 3. * StripGrid::ROW_GAP
    );
}
