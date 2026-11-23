//! The long-throw fader (Swift `Fader`, `FaderDrawing`): the dB scale's
//! labels on the left, then the cap column with the well, the scale's
//! ticks either side of it and the cap in the strip kind's colour. Drag
//! up or down anywhere on the cap column (⌥ fine; ⌥ at the press moves a
//! strip fader alone), double-click for 0 dB, Up and Down (⇧ ×10) when
//! focused. No scroll wheel (Swift has none).

use gpui::{
    AnyElement, App, BorderStyle, Bounds, IntoElement, ParentElement, Pixels, Rgba, Styled, Window,
    canvas, div, fill, point, px, quad, rgb, size,
};
use vizdaw_app::mixer::MixerMath;

use super::{ControlBinding, StepKeys, drag_state, drag_surface, has_focus};
use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// The cap's drawn size (Swift `ControlMetrics.faderCap`).
pub const CAP: (f64, f64) = (29., 42.);
/// The cap's least hit size (Swift `ControlMetrics.faderCapHit`).
pub const CAP_HIT: (f64, f64) = (36., 52.);
/// The scale's label column (Swift `Fader.scaleWidth`).
pub const SCALE_WIDTH: f64 = 24.;
/// The cap column: the cap's hit width.
pub const COLUMN: f64 = CAP_HIT.0;
/// The scale marks: +12 +6 0 −6 −12 −24 −36 −48 −∞ (Swift
/// `StripLayout.faderMarks`).
pub fn marks() -> Vec<(f64, &'static str)> {
    let mut m: Vec<(f64, &'static str)> = [
        (12., "+12"),
        (6., "+6"),
        (0., "0"),
        (-6., "\u{2212}6"),
        (-12., "\u{2212}12"),
        (-24., "\u{2212}24"),
        (-36., "\u{2212}36"),
        (-48., "\u{2212}48"),
    ]
    .into_iter()
    .map(|(db, label)| (MixerMath::fader_position(db), label))
    .collect();
    m.push((0., "\u{2212}\u{221E}"));
    m
}

/// The y (down from the top) of position `v`'s cap centre on a throw of
/// `throw` points.
pub fn cap_y(v: f64, throw: f64) -> f64 {
    let v = if v.is_nan() { 0. } else { v.clamp(0., 1.) };
    CAP.1 / 2. + throw - v * throw
}

/// The whole fader's size for a throw: scale column and cap column wide,
/// the throw and the cap high.
pub fn fader_size(throw: f64) -> (f64, f64) {
    (SCALE_WIDTH + COLUMN, throw.max(0.) + CAP.1)
}

/// Fader `b` with a throw of `throw` points and its cap in `cap`.
pub fn fader(
    b: &ControlBinding,
    throw: f64,
    cap: Rgba,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let throw = throw.max(0.);
    let (w, h) = fader_size(throw);
    let state = drag_state(b, window, cx);
    let focused = has_focus(&state, window, cx);
    let t = tokens();
    let scale = div()
        .relative()
        .w(px(SCALE_WIDTH as f32))
        .h(px(h as f32))
        .flex_none()
        .children(marks().into_iter().map(|(p, label)| {
            div()
                .absolute()
                .left_0()
                .w(px((SCALE_WIDTH - 2.) as f32))
                .top(px((cap_y(p, throw) - 6.) as f32))
                .h(px(12.))
                .flex()
                .justify_end()
                .items_center()
                .text_size(px(9.))
                .line_height(px(12.))
                .text_color(rgb(t.fader_scale))
                .child(label)
        }));
    let drawing = Drawing {
        value: b.position,
        throw,
        cap,
        focused,
        touched: b.touched,
        marks: marks().into_iter().map(|m| m.0).collect(),
    };
    let column = div()
        .relative()
        .w(px(COLUMN as f32))
        .h(px(h as f32))
        .flex_none()
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| drawing.paint(bounds, window),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        );
    let column = drag_surface(column, b, throw.max(1.), StepKeys::Vertical, &state, cx);
    div()
        .relative()
        .flex()
        .flex_row()
        .flex_none()
        .w(px(w as f32))
        .h(px(h as f32))
        .opacity(if b.enabled { 1. } else { 0.45 })
        .child(scale)
        .child(column)
        .child(scope.control_with("slider", &b.label, b.enabled, Some(&b.value), focused))
        .into_any_element()
}

/// The fader's drawing, apart from its behaviour.
struct Drawing {
    value: f64,
    throw: f64,
    cap: Rgba,
    focused: bool,
    touched: bool,
    marks: Vec<f64>,
}

impl Drawing {
    fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let t = tokens();
        let o = bounds.origin;
        let rect = |x: f64, y: f64, w: f64, h: f64| {
            Bounds::new(
                point(o.x + px(x as f32), o.y + px(y as f32)),
                size(px(w as f32), px(h as f32)),
            )
        };
        let r = px(t.radius);
        let cx = COLUMN / 2.;
        // The well, spanning the cap's travel.
        window.paint_quad(quad(
            rect(cx - 2., CAP.1 / 2. - 2., 4., self.throw + 4.),
            r,
            rgb(t.fader_well),
            px(0.),
            rgb(t.fader_well),
            BorderStyle::Solid,
        ));
        // The scale's ticks either side of the well.
        for m in &self.marks {
            let y = cap_y(*m, self.throw) - 0.5;
            window.paint_quad(fill(rect(cx - 10., y, 5., 1.), rgb(t.fader_scale)));
            window.paint_quad(fill(rect(cx + 5., y, 5., 1.), rgb(t.fader_scale)));
        }
        // The cap, outlined, with its value line.
        let cy = cap_y(self.value, self.throw);
        let (cw, ch) = CAP;
        let mut line = rgb(t.fader_cap_line);
        line.a = 0.6;
        window.paint_quad(quad(
            rect(cx - cw / 2., cy - ch / 2., cw, ch),
            r,
            self.cap,
            px(1.),
            line,
            BorderStyle::Solid,
        ));
        window.paint_quad(fill(
            rect(cx - cw / 2. + 2., cy - 1., cw - 4., 2.),
            rgb(t.fader_cap_line),
        ));
        if self.focused {
            window.paint_quad(ring(
                rect(cx - cw / 2. - 2., cy - ch / 2. - 2., cw + 4., ch + 4.),
                t.radius + 2.,
            ));
        }
        if self.touched {
            window.paint_quad(ring(bounds, t.radius));
        }
    }
}

/// A 2-point ring in the focus colour (focus, and the touch ring).
pub(crate) fn ring(bounds: Bounds<Pixels>, radius: f32) -> gpui::PaintQuad {
    let c = rgb(tokens().focus_ring);
    quad(
        bounds,
        px(radius),
        gpui::transparent_black(),
        px(2.),
        c,
        BorderStyle::Solid,
    )
}
