//! The strip's peak meter (Swift `StripMeter`): one canvas, the left and
//! right bars side by side, each filled from −60 to +6 dBFS in its band's
//! colour over the meter background. Not in the tree (Swift hides it). No
//! clip latch and no peak-hold line (Swift has none).

use gpui::{
    AnyElement, Bounds, IntoElement, Pixels, Styled, Window, canvas, fill, point, px, rgb, size,
};
use vizdaw_app::mixer::meters::SILENCE;
use vizdaw_app::mixer::{MeterLevel, MixerMath};

use crate::strip_palette::tokens;

/// The meter's width: two bars and the gap between them.
pub const WIDTH: f64 = 14.;
/// The gap between the bars (`spacing.xxs`).
pub const GAP: f64 = 2.;
/// How far the meter reaches past the fader's throw at each end.
pub const OVERHANG: f64 = 2.;

/// The meter showing `level` (the mixer's `level_at(now)`), `height`
/// points high. While it shows a signal (above the −60 dBFS bottom) it
/// asks for the next frame, so it falls between two reads.
pub fn meter(level: MeterLevel, height: f64, window: &mut Window) -> AnyElement {
    if wants_frame(level) {
        window.request_animation_frame();
    }
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| paint(level, bounds, window),
    )
    .flex_none()
    .w(px(WIDTH as f32))
    .h(px(height.max(0.) as f32))
    .into_any_element()
}

/// Whether a meter showing `level` asks for the next frame: while either
/// bar is above the −60 dBFS bottom (it is still falling).
pub fn wants_frame(level: MeterLevel) -> bool {
    level.left >= SILENCE || level.right >= SILENCE
}

/// Each bar's fill height for `level` in a meter `height` high.
pub fn fills(level: MeterLevel, height: f64) -> (f64, f64) {
    (
        height * MixerMath::meter_fill(level.left),
        height * MixerMath::meter_fill(level.right),
    )
}

fn paint(level: MeterLevel, bounds: Bounds<Pixels>, window: &mut Window) {
    let t = tokens();
    let h = f64::from(f32::from(bounds.size.height));
    let bar_w = (WIDTH - GAP) / 2.;
    let (left, right) = fills(level, h);
    for (i, (peak, filled)) in [(level.left, left), (level.right, right)]
        .into_iter()
        .enumerate()
    {
        let x = bounds.origin.x + px((i as f64 * (bar_w + GAP)) as f32);
        let bar = |y: f64, hh: f64| {
            Bounds::new(
                point(x, bounds.origin.y + px(y as f32)),
                size(px(bar_w as f32), px(hh as f32)),
            )
        };
        window.paint_quad(fill(bar(0., h), rgb(t.meter_background)));
        if filled > 0. {
            window.paint_quad(fill(bar(h - filled, filled), t.meter(peak)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_fill_from_minus_60_to_plus_6_dbfs() {
        let (l, r) = fills(
            MeterLevel {
                left: 1.0,
                right: 0.0,
            },
            132.,
        );
        assert!(
            (l - 120.).abs() < 1e-6,
            "0 dBFS is 60/66 of the height: {l}"
        );
        assert_eq!(r, 0.);
        let (top, _) = fills(
            MeterLevel {
                left: MixerMath::MAX_METER_PEAK,
                right: 0.,
            },
            132.,
        );
        assert!((top - 132.).abs() < 1e-3, "{top}");
    }
}
