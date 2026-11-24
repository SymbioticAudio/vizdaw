//! A VCA's column (Swift `VcaStripCells`, `VcaStripHeader`,
//! `VcaRenameField`, `VcaMenuItems`): the colour bar in the VCA's colour,
//! its name (a double-click renames it in a popover under the name; its
//! menu holds Rename VCA… and Delete VCA) and "VCA · n members"; the
//! fader (a gesture on its automation target, its cap in the VCA's
//! colour, the strips' dB scale; no meter: it carries no audio), the
//! level box, M and S (the VCA's own flags) and the automation mode menu
//! over its four modes. The rows above the fader it has no part in stay
//! empty, so its fader lines up with the strips'.

use gpui::{
    AnyElement, App, ClickEvent, Entity, Focusable, FontWeight, InteractiveElement, IntoElement,
    MouseButton, ParentElement, StatefulInteractiveElement, Styled, Window, anchored, deferred,
    div, point, px, rgb,
};
use vizdaw_app::mixer::layout::{RowHeights, StripLayout, StripRowKind};
use vizdaw_app::mixer::{AutomationModes, VcaStripLabels};

use crate::controls;
use crate::mixer_view::{
    BoxKind, ColumnKey, MenuEntry, MixerView, Place, RenameTarget, StripMenu, VcaParts, at, edge,
    popup,
};
use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// The rename popover's field width (Swift's 180).
const FIELD_WIDTH: f64 = 180.;

/// VCA column `v`, placed in the scrolled area.
pub(crate) fn vca_strip(
    view: &Entity<MixerView>,
    v: VcaParts,
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
    let id = v.id;
    let mut out = place
        .panel(v.index, false)
        .id(gpui::ElementId::Name(format!("mixer-vca:{id}").into()));
    if let (Some(y), Some(h)) = place.row(StripRowKind::Header) {
        out = out.child(at(side, y, inner, h).child(header(view, &v, inner, scope, window, cx)));
    }
    // The fader, centred (no meter).
    let fader = controls::fader(&v.fader, g.throw, v.colour, scope, window, cx);
    out = out.child(
        at(side, g.fader_top, inner, g.throw + StripLayout::FADER_CAP)
            .flex()
            .justify_center()
            .child(fader),
    );
    let nb = view.update(cx, |w, cx| {
        w.number_box((ColumnKey::Vca(id), BoxKind::Level), window, cx)
    });
    let level = controls::number_box(&nb, v.level.clone(), scope, cx);
    out = out.child(
        at(side, g.level_top, inner, StripLayout::LEVEL_BOX)
            .flex()
            .justify_center()
            .child(level),
    );
    let (muting, soloing) = (mixer.clone(), mixer.clone());
    out = out.child(
        at(side, g.buttons_top, inner, StripLayout::TOGGLE_ROW)
            .flex()
            .flex_row()
            .justify_center()
            .gap(px(4.))
            .child(controls::toggle(v.mute.clone(), scope, move |_, cx| {
                muting.with(cx, |m, d| m.toggle_vca_mute(d, id));
            }))
            .child(controls::toggle(v.solo.clone(), scope, move |_, cx| {
                soloing.with(cx, |m, d| m.toggle_vca_solo(d, id));
            })),
    );
    // The automation mode: Off, Read, Touch, Latch.
    let opening = view.downgrade();
    let face = controls::menu_face(
        VcaStripLabels::MODE,
        AutomationModes::title(v.mode),
        Some(t.automation(v.mode)),
        true,
        scope,
        move |_, cx| {
            opening
                .update(cx, |w, cx| w.toggle_menu(id, StripMenu::VcaMode, cx))
                .ok();
        },
    );
    let mut mode = at(side, g.mode_top, inner, StripLayout::MODE_MENU).child(face);
    if view.read(cx).menu == Some((id, StripMenu::VcaMode)) {
        let items = AutomationModes::VCA
            .iter()
            .map(|&m| {
                let mx = mixer.clone();
                MenuEntry::item(AutomationModes::title(m), move |_, cx| {
                    mx.with(cx, |vm, d| vm.set_vca_mode(d, id, m));
                })
                .checked(m == v.mode)
            })
            .collect();
        mode = mode.children(popup(view, "mixer-vca-mode-menu", items, scope, window, cx));
    }
    out.child(mode).child(edge(false)).into_any_element()
}

/// The colour bar, the name (its menu, its rename popover) and the
/// members line.
fn header(
    view: &Entity<MixerView>,
    v: &VcaParts,
    w: f64,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let t = tokens();
    let id = v.id;
    let (renaming, menu) = (view.downgrade(), view.downgrade());
    let name = div()
        .id(gpui::ElementId::Name(format!("mixer-vca-name:{id}").into()))
        .relative()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(t.text_primary))
        .child(v.name.clone())
        .child(scope.control("text", &v.title, true))
        .on_mouse_down(MouseButton::Right, move |_, _, cx| {
            menu.update(cx, |w, cx| w.open_menu(id, StripMenu::Vca, cx))
                .ok();
            cx.stop_propagation();
        })
        .on_click(move |e: &ClickEvent, window, cx| {
            if e.click_count() >= 2 {
                renaming
                    .update(cx, |w, cx| w.begin_vca_rename(id, window, cx))
                    .ok();
            }
        });
    let mut holder = at(0., NAME_TOP, w, NAME_HEIGHT).child(name);
    if view.read(cx).menu == Some((id, StripMenu::Vca)) {
        let mixer = view.read(cx).mixer.clone();
        let weak = view.downgrade();
        let items = vec![
            MenuEntry::item(VcaStripLabels::RENAME, move |window, cx| {
                weak.update(cx, |w, cx| w.begin_vca_rename(id, window, cx))
                    .ok();
            }),
            MenuEntry::item(VcaStripLabels::DELETE, move |_, cx| {
                mixer.with(cx, |m, d| m.delete_vca(d, id));
            }),
        ];
        holder = holder.children(popup(view, "mixer-vca-menu", items, scope, window, cx));
    }
    holder = holder.children(rename_popover(view, id, scope, window, cx));
    at(0., 0., w, RowHeights::HEADER)
        .child(
            at(0., 0., w, StripLayout::COLOUR_BAR)
                .rounded(px(t.radius))
                .bg(v.colour),
        )
        .child(holder)
        .child(
            at(0., KIND_TOP, w, KIND_HEIGHT)
                .relative()
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(10.))
                .text_color(rgb(t.text_secondary))
                .child(v.kind_line.clone())
                .child(scope.control("text", &v.kind_line, true)),
        )
        .into_any_element()
}

/// The rename popover under the name while VCA `id` is renamed here: the
/// field ("Name of the VCA"), and a backdrop whose press dismisses it
/// (cancelling, as Swift's popover does).
fn rename_popover(
    view: &Entity<MixerView>,
    id: u64,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> Vec<AnyElement> {
    let field = view
        .read(cx)
        .rename
        .as_ref()
        .filter(|r| r.target == RenameTarget::Vca(id))
        .map(|r| r.field.clone());
    let Some(field) = field else {
        return vec![];
    };
    let t = tokens();
    let text = field.read(cx).text().to_owned();
    let focused = field.focus_handle(cx).is_focused(window);
    let size = window.viewport_size();
    let dismissing = view.downgrade();
    let backdrop = deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("mixer-vca-rename-backdrop")
                .w(size.width)
                .h(size.height)
                .occlude()
                .on_any_mouse_down(move |_, _, cx| {
                    dismissing.update(cx, |w, cx| w.end_rename(false, cx)).ok();
                }),
        ),
    )
    .with_priority(1)
    .into_any_element();
    let panel = div()
        .relative()
        .w(px(FIELD_WIDTH as f32))
        .p(px(8.))
        .rounded(px(t.radius))
        .border_1()
        .border_color(rgb(t.surface_separator))
        .bg(rgb(t.surface_raised))
        .child(
            div()
                .relative()
                .h(px(22.))
                .child(field)
                .child(scope.control_with(
                    "textfield",
                    VcaStripLabels::RENAME_FIELD,
                    true,
                    Some(&text),
                    focused,
                )),
        );
    let list = div()
        .absolute()
        .top_full()
        .left_0()
        .child(deferred(anchored().snap_to_window().child(panel)).with_priority(2))
        .into_any_element();
    vec![backdrop, list]
}

/// The header's parts, as the strips' (the colour bar, the name and the
/// members line, 2 apart).
const NAME_TOP: f64 = StripLayout::COLOUR_BAR + 2.;
const NAME_HEIGHT: f64 = 16.;
const KIND_TOP: f64 = NAME_TOP + NAME_HEIGHT + 2.;
const KIND_HEIGHT: f64 = 14.;
