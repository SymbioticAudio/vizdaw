//! The plug-in browser popover (Swift `PluginBrowserView`,
//! `S/PluginBrowserView.swift:7-51`): 320 wide, hung from the button that
//! opened it as the record popover hangs from its chevron. The "Search"
//! field (focused when it opens), then one section per vendor, each row
//! the plug-in's name on the left and its format ("AU", "CLAP", "VST3")
//! on the right; the list is at least 240 high. An empty list says "No
//! plugins found. …", or "No match." with a search, at least 80 high. A
//! click on a row picks it; Return picks the one row a search leaves; Esc
//! (the field's Cancel) or a press outside closes it. The list is the
//! mixer's [`PluginBrowserModel`], read once per presentation. The right
//! pane's Plug-ins view shows the same browser at the pane's width
//! ([`Fit::Pane`], `S/PluginBrowserView.swift:48-49`).

use crate::appearance::ThemedCorners as _;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Entity, Focusable, FontWeight, InteractiveElement, IntoElement,
    MouseDownEvent, ParentElement, StatefulInteractiveElement, Styled, Window, anchored, deferred,
    div, px, rgb,
};
use vizdaw_app::plugin_browser::{PluginBrowserModel, SEARCH};
use vizdaw_ffi::snapshot::PluginRefRow;

use crate::registry::RegistryScope;
use crate::text_input::TextInput;
use crate::transport_bar::colors;

/// The popover's name in `ui.tree`.
pub const PLUGIN_BROWSER: &str = "Plugin Browser";
/// The popover's width (`S/PluginBrowserView.swift:11`).
pub const WIDTH: f32 = 320.;
/// The list's least height (`:44`).
pub const LIST_MIN_HEIGHT: f32 = 240.;
/// The empty line's least height (`:19-22`).
pub const EMPTY_MIN_HEIGHT: f32 = 80.;
/// Past this the list scrolls.
const LIST_MAX_HEIGHT: f32 = 400.;

/// A row was picked.
pub type OnPick = Rc<dyn Fn(&PluginRefRow, &mut Window, &mut App)>;
/// A press outside the popover.
pub type OnOutside = Rc<dyn Fn(&MouseDownEvent, &mut Window, &mut App)>;

/// Where the browser shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// A popover, [`WIDTH`] wide, its list scrolling past a height.
    Popover,
    /// The right pane's view: the pane's width and height.
    Pane,
}

/// The plug-in `search` leaves when it leaves exactly one (Return picks
/// it).
pub fn only_match(model: &PluginBrowserModel, search: &str) -> Option<PluginRefRow> {
    let mut rows = model.groups(search).into_iter().flat_map(|(_, rows)| rows);
    let first = rows.next()?;
    rows.next().is_none().then_some(first.plugin)
}

/// The browser's panel for `model`, filtered by `search`'s text, as `fit`
/// shows it (`on_outside` is the popover's only).
#[allow(clippy::too_many_arguments)]
pub fn view(
    fit: Fit,
    model: &PluginBrowserModel,
    search: &Entity<TextInput>,
    on_pick: OnPick,
    on_outside: OnOutside,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &App,
) -> AnyElement {
    let mut scope = scope.container();
    let text = search.read(cx).text().to_owned();
    let focused = search.focus_handle(cx).is_focused(window);
    let field = div()
        .relative()
        .child(search.clone())
        .child(scope.control_with("textfield", SEARCH, true, Some(&text), focused));
    let groups = model.groups(&text);
    let list = if groups.is_empty() {
        let empty = model.empty_text(&text);
        div()
            .relative()
            .min_h(px(EMPTY_MIN_HEIGHT))
            .flex()
            .items_center()
            .justify_center()
            .text_xs()
            .text_color(rgb(colors::dim()))
            .child(empty)
            .child(scope.control("text", empty, true))
            .into_any_element()
    } else {
        let mut list = div()
            .id("plugin-browser-list")
            .relative()
            .min_h(px(LIST_MIN_HEIGHT))
            .map(|d| match fit {
                Fit::Popover => d.max_h(px(LIST_MAX_HEIGHT)),
                Fit::Pane => d.flex_1(),
            })
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let mut n = 0usize;
        for (vendor, rows) in groups {
            list = list.child(
                div()
                    .relative()
                    .flex_none()
                    .pt_1()
                    .px_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(colors::dim()))
                    .child(vendor.clone())
                    .child(scope.control("text", &vendor, true)),
            );
            for r in rows {
                let format = PluginBrowserModel::format(&r);
                let pick = on_pick.clone();
                let plugin = r.plugin;
                list = list.child(
                    div()
                        .id(("plugin-browser-row", n))
                        .relative()
                        .flex_none()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .text_xs()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(colors::button())))
                        .child(div().flex_1().min_w_0().child(plugin.name.clone()))
                        .child(div().text_color(rgb(colors::dim())).child(format))
                        .child(scope.control_with(
                            "button",
                            &plugin.name,
                            true,
                            Some(format),
                            false,
                        ))
                        .on_click(move |_, window, cx| pick(&plugin, window, cx)),
                );
                n += 1;
            }
        }
        list.into_any_element()
    };
    let panel = div()
        .id("plugin-browser")
        .relative()
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .text_color(rgb(colors::text()))
        .child(scope.this("group", PLUGIN_BROWSER, true));
    let panel = match fit {
        Fit::Popover => panel
            .occlude()
            .w(px(WIDTH))
            .bg(rgb(colors::list()))
            .border_1()
            .border_color(rgb(colors::border()))
            .round_md()
            .shadow_md()
            .on_mouse_down_out(move |e: &MouseDownEvent, window, cx| on_outside(e, window, cx)),
        Fit::Pane => panel.size_full().min_h_0(),
    };
    panel.child(field).child(list).into_any_element()
}

/// `panel` hung from the bottom edge of the opener it is laid over (the
/// opener must be `.relative()`), kept inside the window.
pub fn hang_below(panel: AnyElement) -> AnyElement {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(
            div()
                .absolute()
                .top_full()
                .left_0()
                .child(deferred(anchored().snap_to_window().child(panel)).with_priority(2)),
        )
        .into_any_element()
}
