//! The knob (Swift `Knob`, `KnobDrawing`, `KnobGeometry`): skirt, the
//! track and value arc in the skirt band from the origin (the centre for
//! a pan), the cap and its indicator. Drag up or down (⌥ fine), 200
//! points for the whole range; double-click for the reset (centre, 0 dB);
//! Up/Right and Down/Left (⇧ ×10) when focused.

use gpui::{
    AnyElement, App, BorderStyle, Bounds, IntoElement, ParentElement, PathBuilder, Pixels, Point,
    Rgba, Styled, Window, canvas, div, point, px, quad, rgb,
};
use vizdaw_app::mixer::gesture::DRAG_TRAVEL;

use super::{ControlBinding, StepKeys, drag_state, drag_surface, has_focus};
use crate::controls::fader::ring;
use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// The knob's sweep in degrees, 7:30 to 4:30.
pub const SWEEP: f64 = 270.;
/// The least hit size each way (Swift `ControlMetrics.minimumHit`).
pub const MINIMUM_HIT: f64 = 24.;

/// The three knob sizes (Swift `KnobSize`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnobSize {
    Small,
    Medium,
    Large,
}

impl KnobSize {
    /// The skirt's diameter: 28, 38, 48.
    pub fn diameter(self) -> f64 {
        match self {
            KnobSize::Small => 28.,
            KnobSize::Medium => 38.,
            KnobSize::Large => 48.,
        }
    }

    /// The cap's diameter as a part of the skirt's.
    pub fn cap_ratio(self) -> f64 {
        match self {
            KnobSize::Small | KnobSize::Medium => 10.5 / 17.,
            KnobSize::Large => 23. / 34.,
        }
    }

    /// The hit frame, each way: the diameter, at least 24.
    pub fn hit(self) -> f64 {
        self.diameter().max(MINIMUM_HIT)
    }
}

/// The pointer angle (degrees from 3 o'clock, counter-clockwise) of
/// position `v`: 225 at 0, −45 at 1.
pub fn angle(v: f64) -> f64 {
    let v = if v.is_nan() { 0. } else { v.clamp(0., 1.) };
    225. - SWEEP * v
}

/// Knob `b` of `size`, its value arc in `arc` from `origin` (0, or 0.5
/// for a pan).
pub fn knob(
    b: &ControlBinding,
    size: KnobSize,
    arc: Rgba,
    origin: f64,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = drag_state(b, window, cx);
    let focused = has_focus(&state, window, cx);
    let side = size.hit();
    let drawing = Drawing {
        value: b.position,
        origin,
        size,
        arc,
        focused,
        touched: b.touched,
    };
    let surface = div()
        .relative()
        .flex_none()
        .w(px(side as f32))
        .h(px(side as f32))
        .opacity(if b.enabled { 1. } else { 0.45 })
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
    drag_surface(surface, b, DRAG_TRAVEL, StepKeys::All, &state, cx)
        .child(scope.control_with("slider", &b.label, b.enabled, Some(&b.value), focused))
        .into_any_element()
}

struct Drawing {
    value: f64,
    origin: f64,
    size: KnobSize,
    arc: Rgba,
    focused: bool,
    touched: bool,
}

fn at(c: Point<Pixels>, r: f64, degrees: f64) -> Point<Pixels> {
    let a = degrees.to_radians();
    point(
        c.x + px((r * a.cos()) as f32),
        c.y - px((r * a.sin()) as f32),
    )
}

fn circle(c: Point<Pixels>, r: f64) -> Bounds<Pixels> {
    Bounds::new(
        point(c.x - px(r as f32), c.y - px(r as f32)),
        gpui::size(px((2. * r) as f32), px((2. * r) as f32)),
    )
}

/// The arc from position `a` to `b` along the sweep, stroked `width`.
fn arc_path(c: Point<Pixels>, r: f64, a: f64, b: f64, width: f64) -> Option<gpui::Path> {
    let (a0, a1) = (angle(a), angle(b));
    let steps = (((a1 - a0).abs() / 4.) as usize + 1).max(2);
    let mut p = PathBuilder::stroke(px(width as f32));
    for i in 0..=steps {
        let pt = at(c, r, a0 + (a1 - a0) * i as f64 / steps as f64);
        if i == 0 {
            p.move_to(pt);
        } else {
            p.line_to(pt);
        }
    }
    p.build().ok()
}

impl Drawing {
    fn paint(&self, bounds: Bounds<Pixels>, window: &mut Window) {
        let t = tokens();
        let d = self.size.diameter();
        let c = bounds.center();
        let skirt_r = d / 2.;
        let cap_r = d * self.size.cap_ratio() / 2.;
        let band = (d - d * self.size.cap_ratio()) / 2.;
        let unit = |v: f64| if v.is_nan() { 0. } else { v.clamp(0., 1.) };
        let (v, o) = (unit(self.value), unit(self.origin));
        let disc = |r: f64, colour: Rgba| {
            quad(
                circle(c, r),
                px(r as f32),
                colour,
                px(0.),
                colour,
                BorderStyle::Solid,
            )
        };
        window.paint_quad(disc(skirt_r, rgb(t.knob_skirt)));
        // The track and the value arc in the skirt band.
        let r = cap_r + band / 2.;
        if let Some(track) = arc_path(c, r, 0., 1., 0.7 * band) {
            window.paint_path(track, rgb(t.knob_track));
        }
        let (from, to) = (o.min(v), o.max(v));
        if to > from
            && let Some(filled) = arc_path(c, r, from, to, 0.7 * band)
        {
            window.paint_path(filled, self.arc);
        }
        window.paint_quad(disc(cap_r, rgb(t.knob_cap)));
        // The indicator: a capsule on the radius at the value's angle.
        let cap = 2. * cap_r;
        let (w, h) = ((0.07 * d).max(2.), 0.42 * cap);
        let mid = 0.25 * cap;
        let mut ind = PathBuilder::stroke(px(w as f32));
        ind.move_to(at(c, mid - h / 2. + w / 2., angle(v)));
        ind.line_to(at(c, mid + h / 2. - w / 2., angle(v)));
        if let Ok(path) = ind.build() {
            window.paint_path(path, rgb(t.knob_indicator));
        }
        if self.focused {
            window.paint_quad(ring(circle(c, skirt_r + 2.), (skirt_r + 2.) as f32));
        }
        if self.touched {
            window.paint_quad(ring(bounds, t.radius));
        }
    }
}
