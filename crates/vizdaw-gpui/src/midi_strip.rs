//! An external MIDI destination's strip (Swift `MidiStripCells` in a
//! mixer placement): the colour bar, the destination's name and "MIDI ·
//! Ch n"; the pan (CC 10: a medium knob from the centre and its box); the
//! 0…127 fader (CC 7, linear, its scale 127 100 64 32 0) beside the
//! 6-point note-activity meter; the volume box; M (the destination's
//! mute) and R (the track that plays it). A drag previews the controller
//! and its release commits once; unset values are dimmed. The rows above
//! the fader it has no part in stay empty. In the left pane a MIDI track
//! with no destination shows "No Destination", only R working; in the
//! mixer a strip a focused track plays is marked selected.

use gpui::{
    AnyElement, App, Bounds, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Pixels, Styled, Window, canvas, div, fill, point, px, rgb, size,
};
use vizdaw_app::mixer::MidiStripMath;
use vizdaw_app::mixer::layout::{RowHeights, StripLayout, StripRowKind};

use crate::controls::{self, KnobSize, fader};
use crate::mixer_view::{BoxKind, MidiParts, MixerView, Place, at, edge};
use crate::registry::RegistryScope;
use crate::strip_palette::{KnobFunction, tokens};

/// MIDI strip `x`, placed in the scrolled area.
pub(crate) fn midi_strip(
    view: &Entity<MixerView>,
    x: MidiParts,
    place: &Place<'_>,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = tokens();
    let mixer = view.read(cx).mixer.clone();
    let g = place.grid;
    let side = StripLayout::SIDE_PADDING;
    let inner = StripLayout::inner();
    let (dest, key) = (x.id, x.key);
    let dim = |set: bool| {
        if set {
            1.
        } else {
            MidiStripMath::DIMMED as f32
        }
    };
    let mut out = place
        .panel(x.index, false)
        .id(gpui::ElementId::Name(format!("mixer-midi:{key:?}").into()));
    if let (Some(y), Some(_)) = place.row(StripRowKind::Header) {
        out = out.child(at(side, y, inner, RowHeights::HEADER).child(header(&x, inner, scope)));
    }
    // The pan.
    if let (Some(y), Some(h)) = place.row(StripRowKind::Pan) {
        let nb = view.update(cx, |v, cx| v.number_box((key, BoxKind::Pan), window, cx));
        let centre = MidiStripMath::pan_position(Some(MidiStripMath::PAN_CENTRE));
        let knob = controls::knob(
            &x.pan,
            KnobSize::Medium,
            t.knob(KnobFunction::Pan),
            centre,
            scope,
            window,
            cx,
        );
        let pan_box = controls::number_box(&nb, x.pan_box.clone(), scope, cx);
        out = out.child(
            at(side, y, inner, h)
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(div().opacity(dim(x.pan_set)).child(knob))
                .child(div().opacity(dim(x.pan_set)).child(pan_box)),
        );
    }
    // The fader and the note-activity meter, centred.
    let marks = MidiStripMath::fader_marks();
    let fader_el = controls::fader_on(&x.fader, g.throw, x.cap, &marks, scope, window, cx);
    out = out.child(
        at(side, g.fader_top, inner, g.throw + StripLayout::FADER_CAP)
            .flex()
            .flex_row()
            .justify_center()
            .items_start()
            .gap(px(4.))
            .child(div().opacity(dim(x.volume_set)).child(fader_el))
            .child(
                div()
                    .pt(px((fader::CAP.1 / 2. - StripLayout::METER_OVERHANG) as f32))
                    .child(activity_meter(
                        x.activity,
                        g.throw + 2. * StripLayout::METER_OVERHANG,
                    )),
            ),
    );
    // The volume.
    let nb = view.update(cx, |v, cx| v.number_box((key, BoxKind::Level), window, cx));
    let volume = controls::number_box(&nb, x.volume.clone(), scope, cx);
    out = out.child(
        at(side, g.level_top, inner, StripLayout::LEVEL_BOX)
            .flex()
            .justify_center()
            .child(div().opacity(dim(x.volume_set)).child(volume)),
    );
    // M and R (no solo).
    let (muting, arming, lane) = (mixer.clone(), mixer, x.lane);
    out = out.child(
        at(side, g.buttons_top, inner, StripLayout::TOGGLE_ROW)
            .flex()
            .flex_row()
            .justify_center()
            .gap(px(4.))
            .child(controls::toggle(x.mute.clone(), scope, move |_, cx| {
                if let Some(dest) = dest {
                    muting.with(cx, |m, d| m.toggle_midi_mute(d, dest));
                }
            }))
            .child(controls::toggle(x.arm.clone(), scope, move |_, cx| {
                if let Some(lane) = lane {
                    arming.with(cx, |m, d| m.toggle_midi_arm(d, lane));
                }
            })),
    );
    out.child(edge(x.selected)).into_any_element()
}

/// The colour bar, the name and the kind line.
fn header(x: &MidiParts, w: f64, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    let line = |top: f64, h: f64| {
        at(0., top, w, h)
            .relative()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
    };
    let name_top = StripLayout::COLOUR_BAR + 2.;
    let kind_top = name_top + 16. + 2.;
    at(0., 0., w, RowHeights::HEADER)
        .child(
            at(0., 0., w, StripLayout::COLOUR_BAR)
                .rounded(px(t.radius))
                .bg(x.cap),
        )
        .child(
            line(name_top, 16.)
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                // No destination: the name is dimmed (Swift's secondary).
                .text_color(rgb(if x.id.is_some() {
                    t.text_primary
                } else {
                    t.text_secondary
                }))
                .child(x.name.clone())
                .child(scope.control("text", &x.name, true)),
        )
        .child(
            line(kind_top, 14.)
                .text_size(px(10.))
                .text_color(rgb(t.text_secondary))
                .child(x.kind_label.clone())
                .child(scope.control("text", &x.kind_label, true)),
        )
        .into_any_element()
}

/// The note-activity meter (Swift `MidiActivityMeter`): 6 points wide, the
/// activity (0…1) filled from the bottom in the meter's low colour. Not
/// in the tree (Swift hides it).
fn activity_meter(activity: f64, height: f64) -> AnyElement {
    let a = if activity.is_finite() {
        activity.clamp(0., 1.)
    } else {
        0.
    };
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window, _| {
            let t = tokens();
            window.paint_quad(fill(bounds, rgb(t.meter_background)));
            let h = f32::from(bounds.size.height) * a as f32;
            if h > 0. {
                let o = bounds.origin;
                window.paint_quad(fill(
                    Bounds::new(
                        point(o.x, o.y + bounds.size.height - px(h)),
                        size(bounds.size.width, px(h)),
                    ),
                    rgb(t.meter_low),
                ));
            }
        },
    )
    .flex_none()
    .w(px(MidiStripMath::METER_WIDTH as f32))
    .h(px(height.max(0.) as f32))
    .into_any_element()
}
