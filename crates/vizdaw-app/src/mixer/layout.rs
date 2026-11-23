//! The channel strip's measurements and the strips' shared rows (Swift
//! `StripLayout`, `StripRowKind`, `StripGrid`). SwiftUI measures the rows
//! above the fader; here each row has an explicit height (Decision M8),
//! and the strips share their rows: each row is as tall as the tallest
//! strip's, and a strip without it keeps the space empty, so every fader,
//! level box and button row lines up.

use std::collections::BTreeMap;

use vizdaw_ffi::mixer::{StripKindRow, StripRow};

/// How the strip's height divides: the fader's throw, and whether the
/// strip scrolls because even the shortest throw does not fit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripSplit {
    pub throw: f64,
    pub scrolls: bool,
}

/// The channel strip's measurements (points).
pub struct StripLayout;

impl StripLayout {
    pub const WIDTH: f64 = 120.;
    pub const MIN_THROW: f64 = 160.;
    pub const MAX_THROW: f64 = 480.;
    /// The fader cap's drawn height: the fader draws `throw + FADER_CAP`
    /// tall.
    pub const FADER_CAP: f64 = 42.;
    /// How far the meter beside the fader reaches past the cap's travel
    /// at each end.
    pub const METER_OVERHANG: f64 = 2.;
    pub const COLOUR_BAR: f64 = 4.;
    /// The level box's hit frame.
    pub const LEVEL_BOX: f64 = 24.;
    /// The M S R I row.
    pub const TOGGLE_ROW: f64 = 24.;
    /// The automation mode menu.
    pub const MODE_MENU: f64 = 24.;
    /// The gap between the strip's sections.
    pub const GAP: f64 = 6.;
    /// The padding above and below.
    pub const PADDING: f64 = 10.;
    /// The padding left and right.
    pub const SIDE_PADDING: f64 = 6.;
    /// Everything but the part above the fader and the throw: the cap
    /// (42), the level box (24), the M S R I row (24), the mode menu (24),
    /// four gaps (24) and the padding above and below (20).
    pub const LOWER_FIXED: f64 = 158.;

    /// The strip's whole height for an `upper` part and a throw.
    pub fn content_height(upper: f64, throw: f64) -> f64 {
        upper + throw + Self::LOWER_FIXED
    }

    /// The throw for a strip `height` tall whose part above the fader is
    /// `upper` tall: what is left, from 160 to 480; the strip scrolls
    /// when even 160 does not fit.
    pub fn split(height: f64, upper: f64) -> StripSplit {
        let room = height - upper - Self::LOWER_FIXED;
        StripSplit {
            throw: room.clamp(Self::MIN_THROW, Self::MAX_THROW),
            scrolls: height < upper + Self::LOWER_FIXED + Self::MIN_THROW,
        }
    }

    /// The width inside the side padding.
    pub fn inner() -> f64 {
        Self::WIDTH - 2. * Self::SIDE_PADDING
    }
}

/// The rows of a strip's part above the fader, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StripRowKind {
    Header,
    Instrument,
    Input,
    Eq,
    Inserts,
    MidiFx,
    Sends,
    Output,
    Pan,
}

impl StripRowKind {
    pub const ALL: [StripRowKind; 9] = [
        StripRowKind::Header,
        StripRowKind::Instrument,
        StripRowKind::Input,
        StripRowKind::Eq,
        StripRowKind::Inserts,
        StripRowKind::MidiFx,
        StripRowKind::Sends,
        StripRowKind::Output,
        StripRowKind::Pan,
    ];
}

/// Each row's height (Decision M8). Slice 6b fills the instrument, input,
/// EQ, insert and send rows; until then they are left blank at these
/// heights (their empty state), so the faders already line up.
pub struct RowHeights;

impl RowHeights {
    /// The colour bar (4), the name (16), the kind (14), two 2-point gaps.
    pub const HEADER: f64 = 38.;
    /// One slot row.
    pub const SLOT: f64 = 20.;
    /// The audio path's input and monitor row.
    pub const INPUT: f64 = 24.;
    /// A section's small heading ("INSERTS", "SENDS").
    pub const HEADING: f64 = 14.;
    /// The inserts: the heading, a 2-point gap and the 24-point "+".
    pub const INSERTS: f64 = Self::HEADING + 2. + 24.;
    /// The sends: the heading, a 2-point gap and the 24-point menu.
    pub const SENDS: f64 = Self::HEADING + 2. + 24.;
    /// The output menu.
    pub const OUTPUT: f64 = 24.;
    /// The medium pan knob's hit size.
    pub const PAN: f64 = 38.;
}

/// One strip's rows and heights. `input`: the strip has an input track.
/// The MIDI FX section is not drawn in this slice and takes no row.
pub fn rows_of(s: &StripRow, input: bool) -> BTreeMap<StripRowKind, f64> {
    let mut rows = BTreeMap::from([
        (StripRowKind::Header, RowHeights::HEADER),
        (StripRowKind::Eq, RowHeights::SLOT),
        (StripRowKind::Inserts, RowHeights::INSERTS),
    ]);
    if s.instrument.is_some() || s.kind == StripKindRow::BuiltinSynth {
        rows.insert(StripRowKind::Instrument, RowHeights::SLOT);
    }
    if input {
        rows.insert(StripRowKind::Input, RowHeights::INPUT);
    }
    if s.kind != StripKindRow::Master {
        rows.insert(StripRowKind::Sends, RowHeights::SENDS);
        rows.insert(StripRowKind::Output, RowHeights::OUTPUT);
        rows.insert(StripRowKind::Pan, RowHeights::PAN);
    }
    rows
}

/// Where the shared rows and the parts below the fader go, for strips in
/// a placement `height` tall.
#[derive(Debug, Clone, PartialEq)]
pub struct StripGrid {
    /// Each row's shared height; a row no strip has is left out.
    pub rows: BTreeMap<StripRowKind, f64>,
    /// Each row's top, down from the strip's top.
    pub tops: BTreeMap<StripRowKind, f64>,
    pub throw: f64,
    /// Every strip's height.
    pub total: f64,
    pub fader_top: f64,
    pub level_top: f64,
    pub buttons_top: f64,
    pub mode_top: f64,
}

impl StripGrid {
    /// The gap between two rows above the fader.
    pub const ROW_GAP: f64 = 5.;
    /// The gap between two strips.
    pub const SPACING: f64 = 6.;

    /// Each row's height: the most any strip has; a row no strip has is
    /// left out (it takes no space).
    pub fn row_heights<'a>(
        strips: impl IntoIterator<Item = &'a BTreeMap<StripRowKind, f64>>,
    ) -> BTreeMap<StripRowKind, f64> {
        let mut rows: BTreeMap<StripRowKind, f64> = BTreeMap::new();
        for s in strips {
            for (k, h) in s {
                if *h > 0. {
                    let r = rows.entry(*k).or_default();
                    *r = r.max(*h);
                }
            }
        }
        rows
    }

    /// The part above the fader: the rows and the gaps between them.
    pub fn upper(rows: &BTreeMap<StripRowKind, f64>) -> f64 {
        if rows.is_empty() {
            return 0.;
        }
        rows.values().sum::<f64>() + (rows.len() - 1) as f64 * Self::ROW_GAP
    }

    /// The grid for `strips`' rows in a placement `height` tall.
    pub fn new<'a>(
        strips: impl IntoIterator<Item = &'a BTreeMap<StripRowKind, f64>>,
        height: f64,
    ) -> StripGrid {
        let rows = Self::row_heights(strips);
        let mut tops = BTreeMap::new();
        let mut y = StripLayout::PADDING;
        for (k, h) in &rows {
            tops.insert(*k, y);
            y += h + Self::ROW_GAP;
        }
        let up = Self::upper(&rows);
        let throw = StripLayout::split(height, up).throw;
        let total = StripLayout::content_height(up, throw);
        let fader_top = StripLayout::PADDING + up + StripLayout::GAP;
        let level_top = fader_top + throw + StripLayout::FADER_CAP + StripLayout::GAP;
        let buttons_top = level_top + StripLayout::LEVEL_BOX + StripLayout::GAP;
        let mode_top = buttons_top + StripLayout::TOGGLE_ROW + StripLayout::GAP;
        StripGrid {
            rows,
            tops,
            throw,
            total,
            fader_top,
            level_top,
            buttons_top,
            mode_top,
        }
    }

    /// Column `column`'s left edge: one strip and gap per column, and the
    /// divider's own slot (a point and a gap) from `divider_before` on.
    pub fn x(column: usize, divider_before: Option<usize>) -> f64 {
        let divider =
            divider_before.map_or(0., |d| if column >= d { 1. + Self::SPACING } else { 0. });
        column as f64 * (StripLayout::WIDTH + Self::SPACING) + divider
    }

    /// The divider's left edge: the middle of the gap before
    /// `divider_before`.
    pub fn divider_x(divider_before: usize) -> f64 {
        Self::x(divider_before, Some(divider_before)) - Self::SPACING - 1.
    }

    /// The width of `columns` columns.
    pub fn width(columns: usize, divider_before: Option<usize>) -> f64 {
        if columns == 0 {
            return 0.;
        }
        Self::x(columns - 1, divider_before) + StripLayout::WIDTH
    }
}
