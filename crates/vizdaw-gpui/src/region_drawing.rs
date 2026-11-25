//! Drawing an audio region (Swift `RegionDrawing.draw`,
//! `WaveformGeometry.bars`, `RegionHandles.fadeInX`/`fadeOutX`): its
//! body, then, clipped to it, the hatching and `Offline` pill of offline
//! media or the waveform bars (`Ready` or `Coarse` rows only) under the
//! gain band, then the fade lines and the outline.
//!
//! The geometry is pure ([`waveform_bars`], [`hatch_lines`],
//! [`fade_lines`], [`offline_pill_rect`]); [`paint_waveform`] paints rows
//! into a rect and is what the growing take draws with its own rows.
//! Nothing here reads the session: rows come from the cache.

use std::sync::Arc;

use gpui::{
    App, BorderStyle, ContentMask, Hsla, PathBuilder, Pixels, Window, fill, point, px, quad,
};
use vizdaw_app::arrangement::{Point, Rect};
use vizdaw_ffi::events::EventRow;
use vizdaw_ffi::{OverviewRows, OverviewStateRow};

use crate::arrangement::{canvas_rect, colors, hsla, paint_text};

/// The gain band's height along a region's top edge (Swift
/// `RegionHandles.band`): the waveform starts under it.
pub const BAND: f32 = 4.;
/// A corner handle's side (Swift `RegionHandles.size`).
pub const HANDLE: f32 = 7.;
/// The offline pill's text (Swift `RegionDrawing.draw`).
pub const OFFLINE: &str = "Offline";
/// The hatching's spacing.
const HATCH_STEP: f32 = 6.;

/// One bar per column and channel in `rect` (Swift
/// `WaveformGeometry.bars`): channels stacked top to bottom, each bar
/// from the column's max to its min around its channel's centre line,
/// clipped to ±1; silence is a hairline; a non-finite value counts as
/// silence. Rows whose arrays do not hold `channels × columns` values
/// draw nothing.
pub fn waveform_bars(rows: &OverviewRows, rect: Rect) -> Vec<Rect> {
    let n = rows.columns as usize;
    let ch = usize::from(rows.channels);
    let Some(total) = n.checked_mul(ch) else {
        return vec![];
    };
    if n == 0
        || ch == 0
        || rows.min.len() < total
        || rows.max.len() < total
        || !positive(rect.width)
        || !positive(rect.height)
    {
        return vec![];
    }
    let w = rect.width / n as f32;
    let h = rect.height / ch as f32;
    let clip = |v: f32| if v.is_finite() { v.clamp(-1., 1.) } else { 0. };
    let mut out = Vec::with_capacity(total);
    for k in 0..ch {
        let mid = rect.y + h * (k as f32 + 0.5);
        for c in 0..n {
            let lo = clip(rows.min[k * n + c]);
            let hi = clip(rows.max[k * n + c]);
            let top = mid - hi.max(lo) * h / 2.;
            let bottom = mid - hi.min(lo) * h / 2.;
            out.push(Rect {
                x: rect.x + c as f32 * w,
                y: top,
                width: w.max(0.5),
                height: (bottom - top).max(0.5),
            });
        }
    }
    out
}

/// More than zero (NaN is not).
fn positive(v: f32) -> bool {
    v > 0.
}

/// Diagonal hatching over `rect` for offline media (Swift
/// `RegionDrawing.hatch`): lines from bottom left to top right, 6 points
/// apart.
pub fn hatch_lines(rect: Rect) -> Vec<(Point, Point)> {
    let h = rect.height;
    if !positive(h) || !positive(rect.width) {
        return vec![];
    }
    let mut out = vec![];
    let mut x = rect.x - h;
    while x < rect.max_x() {
        out.push((
            Point { x, y: rect.max_y() },
            Point {
                x: x + h,
                y: rect.y,
            },
        ));
        x += HATCH_STEP;
    }
    out
}

fn share(frames: i64, r: &EventRow) -> f32 {
    (frames.max(0) as f64 / r.frames.max(1) as f64) as f32
}

/// Where the fade in ends (Swift `RegionHandles.fadeInX`).
pub fn fade_in_x(r: &EventRow, rect: Rect) -> f32 {
    rect.x + share(r.fade_in.length, r).min(1.) * rect.width
}

/// Where the fade out starts (Swift `RegionHandles.fadeOutX`).
pub fn fade_out_x(r: &EventRow, rect: Rect) -> f32 {
    rect.max_x() - share(r.fade_out.length, r).min(1.) * rect.width
}

/// The fade lines (Swift `RegionDrawing.draw`): a crossfade's X over the
/// fade-in area, else the fade in's rising line; the fade out's falling
/// line.
pub fn fade_lines(r: &EventRow, rect: Rect) -> Vec<(Point, Point)> {
    let p = |x, y| Point { x, y };
    let mut out = vec![];
    if r.crossfade {
        let x = fade_in_x(r, rect);
        out.push((p(rect.x, rect.max_y()), p(x, rect.y)));
        out.push((p(rect.x, rect.y), p(x, rect.max_y())));
    } else if r.fade_in.length > 0 {
        out.push((p(rect.x, rect.max_y()), p(fade_in_x(r, rect), rect.y)));
    }
    if r.fade_out.length > 0 {
        out.push((
            p(fade_out_x(r, rect), rect.y),
            p(rect.max_x(), rect.max_y()),
        ));
    }
    out
}

/// Where a region's `Offline` pill goes for a label `width` × `height`
/// (Swift `RegionDrawing.offlineLabelRect`): top left under the gain
/// band, after the fade-in handle; `None` when the region cannot hold it.
pub fn offline_pill_rect(r: &EventRow, rect: Rect, width: f32, height: f32) -> Option<Rect> {
    let (w, h) = (width + 8., height + 2.);
    let mut x = rect.x + 4.;
    if r.fade_in.length > 0 {
        x = x.max(fade_in_x(r, rect) + HANDLE / 2. + 2.);
    }
    let pill = Rect {
        x,
        y: rect.y + BAND + 2.,
        width: w,
        height: h,
    };
    (pill.max_x() <= rect.max_x() - 2. && pill.max_y() <= rect.max_y() - 2.).then_some(pill)
}

/// The waveform's rect for rows covering `from_x..to_x` of a region in
/// `rect` (Swift: the visible part, under the gain band).
pub fn wave_rect(rect: Rect, from_x: f32, to_x: f32) -> Rect {
    Rect {
        x: from_x,
        y: rect.y + BAND,
        width: (to_x - from_x).max(0.),
        height: (rect.height - BAND - 2.).max(1.),
    }
}

/// Paints `rows` (`Ready` or `Coarse` only) as bars in `wave` (canvas
/// points from origin `o`) in `colour`. The growing take calls it with
/// its own rows.
pub fn paint_waveform(
    rows: &OverviewRows,
    wave: Rect,
    o: gpui::Point<Pixels>,
    colour: Hsla,
    window: &mut Window,
) {
    if !matches!(
        rows.state,
        OverviewStateRow::Ready | OverviewStateRow::Coarse
    ) {
        return;
    }
    for b in waveform_bars(rows, wave) {
        window.paint_quad(fill(canvas_rect(o, b.x, b.y, b.width, b.height), colour));
    }
}

/// One audio region, laid out for painting.
pub(crate) struct RegionDraw {
    pub(crate) row: EventRow,
    pub(crate) rect: Rect,
    pub(crate) offline: bool,
    pub(crate) selected: bool,
    /// The rows held and the rect they cover.
    pub(crate) wave: Option<(Rect, Arc<OverviewRows>)>,
}

fn stroke(lines: &[(Point, Point)], o: gpui::Point<Pixels>, colour: Hsla, window: &mut Window) {
    if lines.is_empty() {
        return;
    }
    let mut p = PathBuilder::stroke(px(1.));
    for (a, b) in lines {
        p.move_to(point(o.x + px(a.x), o.y + px(a.y)));
        p.line_to(point(o.x + px(b.x), o.y + px(b.y)));
    }
    if let Ok(path) = p.build() {
        window.paint_path(path, colour);
    }
}

/// Paints region `d` (canvas points from origin `o`).
pub(crate) fn paint_region(
    d: &RegionDraw,
    o: gpui::Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let r = d.rect;
    let muted = d.row.mute;
    let alpha = colors::REGION_ALPHA * if muted { colors::MUTED_ALPHA } else { 1. };
    let body = canvas_rect(
        o,
        r.x + 0.5,
        r.y + 0.5,
        (r.width - 1.).max(0.),
        (r.height - 1.).max(0.),
    );
    window
        .paint_quad(fill(body, hsla(colors::REGION_FILL, alpha)).corner_radii(px(colors::RADIUS)));
    let inside = canvas_rect(o, r.x, r.y, r.width, r.height);
    window.with_content_mask(Some(ContentMask { bounds: inside }), |window| {
        if d.offline {
            stroke(
                &hatch_lines(r),
                o,
                hsla(colors::OFFLINE, colors::OFFLINE_HATCH_ALPHA),
                window,
            );
        } else if let Some((wave, rows)) = &d.wave {
            paint_waveform(
                rows,
                *wave,
                o,
                hsla(colors::REGION_LINE, colors::WAVEFORM_ALPHA),
                window,
            );
        }
        stroke(
            &fade_lines(&d.row, r),
            o,
            Hsla::from(gpui::rgb(colors::FADE_LINE)),
            window,
        );
    });
    let (line, width) = if d.selected {
        (colors::REGION_SELECTED, 2.)
    } else {
        (colors::REGION_LINE, 1.)
    };
    window.paint_quad(quad(
        body,
        px(colors::RADIUS),
        gpui::transparent_black(),
        px(width),
        gpui::rgb(line),
        if muted {
            BorderStyle::Dashed
        } else {
            BorderStyle::Solid
        },
    ));
    if d.offline {
        paint_pill(d, o, window, cx);
    }
}

/// The `Offline` pill, when the region has room for it.
fn paint_pill(d: &RegionDraw, o: gpui::Point<Pixels>, window: &mut Window, cx: &mut App) {
    let font = window.text_style().font();
    let run = gpui::TextRun {
        len: OFFLINE.len(),
        font,
        color: gpui::rgb(colors::OFFLINE).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(OFFLINE.into(), px(10.), &[run], None);
    let Some(pill) = offline_pill_rect(&d.row, d.rect, f32::from(line.width), 12.) else {
        return;
    };
    window.paint_quad(
        fill(
            canvas_rect(o, pill.x, pill.y, pill.width, pill.height),
            gpui::rgb(colors::OFFLINE_PILL),
        )
        .corner_radii(px(colors::RADIUS)),
    );
    paint_text(
        OFFLINE,
        o.x + px(pill.x + 4.),
        o.y + px(pill.y + 1.),
        colors::OFFLINE,
        window,
        cx,
    );
}
