//! The Plugin Manager window (Swift `PluginManagerView`,
//! `S/PluginManagerView.swift`; `Window("Plugin Manager", id: "plugins")`,
//! `S/VizDAWApp.swift:258-261`): one per app, at least 760×360. The
//! toolbar has the format picker, the scan's progress, Rescan Selected,
//! Rescan All, Clear Blocklist and Reveal, and the search field; then the
//! table (On, Name, Vendor, Format, Version, Type, Status; each row's
//! tooltip is its path; click, ⇧-click and ⌘-click select) and the
//! footer. The rows are [`AppState::app_plugins`]' as the 4 Hz poll last
//! read them: the window observes the app state, which the poll notifies
//! when the scan moved ([`crate::plugins_tick`]). While it is open the
//! poll runs ([`AppState::plugin_manager_open`]).

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Bounds, ClickEvent, Context, Entity, FocusHandle, Focusable,
    FontWeight, InteractiveElement, IntoElement, ParentElement, Render, StatefulInteractiveElement,
    Styled, Subscription, TitlebarOptions, Window, WindowBounds, WindowOptions, div, px, rgb, size,
};
use vizdaw_app::plugin_browser::SEARCH;
use vizdaw_app::plugin_manager::{
    CLEAR_BLOCKLIST, COLUMNS, Click, FOOTER, FormatFilter, ManagerRow, PluginManagerModel,
    RESCAN_ALL, RESCAN_SELECTED, REVEAL, TITLE,
};

use crate::app_state::AppState;
use crate::menus::{CloseWindow, PLUGIN_MANAGER_CONTEXT};
use crate::registry::{RegistryScope, SharedRegistry};
use crate::strip_palette::tokens;
use crate::text_input::TextInput;

/// The window's least size (`S/PluginManagerView.swift:61`).
pub const MIN_WIDTH: f32 = 760.;
pub const MIN_HEIGHT: f32 = 360.;
/// The size it opens at.
const OPEN_HEIGHT: f32 = 440.;
/// The fixed columns' widths (`S/PluginManagerView.swift:20-26`): On,
/// Format, Version, Type. Name, Vendor and Status share the rest.
pub const ON_WIDTH: f32 = 30.;
pub const FORMAT_WIDTH: f32 = 50.;
pub const VERSION_WIDTH: f32 = 70.;
pub const TYPE_WIDTH: f32 = 80.;
/// The search field's width.
const SEARCH_WIDTH: f32 = 180.;

/// The Plugin Manager's view.
pub struct PluginManagerWindow {
    model: PluginManagerModel,
    search: Entity<TextInput>,
    registry: SharedRegistry,
    focus: FocusHandle,
    /// How many times the app state's notify reached the window.
    refreshes: u64,
    _subscriptions: Vec<Subscription>,
}

impl PluginManagerWindow {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        // ⌘W reaches the window's root through its focus.
        window.focus(&focus, cx);
        cx.on_release(|_, cx| {
            AppState::global(cx).update(cx, |s, _| {
                s.plugin_manager_window = None;
                // The poll stops unless a scan runs.
                s.plugin_manager_open = false;
            });
        })
        .detach();
        let search = cx.new(|cx| TextInput::new(SEARCH, cx));
        let app = AppState::global(cx);
        let subscriptions = vec![
            // The poll's news (scan progress, new rows) redraws the table.
            cx.observe(&app, |this, _, cx| {
                this.refreshes += 1;
                cx.notify();
            }),
            // The table follows the search.
            cx.observe(&search, |this, field, cx| {
                this.model.search = field.read(cx).text().to_owned();
                cx.notify();
            }),
        ];
        PluginManagerWindow {
            model: PluginManagerModel::new(),
            search,
            registry: SharedRegistry::default(),
            focus,
            refreshes: 0,
            _subscriptions: subscriptions,
        }
    }

    /// The elements this window drew last.
    pub fn registry(&self) -> &SharedRegistry {
        &self.registry
    }

    /// How many times the app state's notify reached the window (each
    /// redraws it).
    pub fn refreshes(&self) -> u64 {
        self.refreshes
    }

    /// The search, filter and selection.
    pub fn model(&self) -> &PluginManagerModel {
        &self.model
    }

    /// Runs `body` on the app's catalog and store, then redraws every
    /// window (a toggled plug-in's slots change in the mixers).
    fn act(
        &mut self,
        cx: &mut Context<Self>,
        body: impl FnOnce(&PluginManagerModel, &mut AppState),
    ) {
        let model = &self.model;
        AppState::global(cx).update(cx, |s, cx| {
            body(model, s);
            cx.notify();
        });
        cx.notify();
    }

    fn toggle(&mut self, row: ManagerRow, cx: &mut Context<Self>) {
        let on = !row.is_enabled();
        self.act(cx, |m, s| {
            m.set_enabled(&mut s.app_plugins, &mut s.store, &row, on);
        });
    }

    fn click_row(&mut self, id: String, e: &ClickEvent, cx: &mut Context<Self>) {
        let mods = e.modifiers();
        let how = if mods.shift {
            Click::Extend
        } else if mods.secondary(cx) {
            Click::Toggle
        } else {
            Click::Plain
        };
        let app = AppState::global(cx);
        self.model.click(&id, how, &app.read(cx).app_plugins);
        cx.notify();
    }

    fn press(&mut self, label: &'static str, cx: &mut Context<Self>) {
        self.act(cx, |m, s| match label {
            RESCAN_SELECTED => m.rescan_selected(&mut s.app_plugins),
            RESCAN_ALL => PluginManagerModel::rescan_all(&mut s.app_plugins),
            CLEAR_BLOCKLIST => m.clear_blocklist(&mut s.app_plugins),
            REVEAL => {
                let revealer = s.plugin_revealer.clone();
                m.reveal(&s.app_plugins, &*revealer);
            }
            _ => {}
        });
    }

    fn toolbar(
        &self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = tokens();
        let app = AppState::global(cx);
        let plugins = &app.read(cx).app_plugins;
        let enabled = [
            (RESCAN_SELECTED, self.model.can_rescan_selected(plugins)),
            (RESCAN_ALL, PluginManagerModel::can_rescan_all(plugins)),
            (CLEAR_BLOCKLIST, self.model.can_clear_blocklist(plugins)),
            (REVEAL, self.model.can_reveal(plugins)),
        ];
        let progress = PluginManagerModel::progress_text(plugins);
        let mut scope = scope.container();
        // The format picker: one segment per format offered here.
        let mut picker = div()
            .flex()
            .flex_row()
            .flex_none()
            .rounded(px(t.radius))
            .border_1()
            .border_color(rgb(t.surface_separator))
            .overflow_hidden();
        for &f in FormatFilter::available() {
            let on = self.model.format == f;
            picker = picker.child(
                div()
                    .id(("pm-format", f as usize))
                    .relative()
                    .px_2()
                    .py(px(2.))
                    .cursor_pointer()
                    .map(|d| {
                        if on {
                            d.bg(rgb(t.accent)).text_color(rgb(t.on_accent))
                        } else {
                            d.bg(rgb(t.surface_inset)).text_color(rgb(t.text_primary))
                        }
                    })
                    .child(f.label())
                    .child(scope.control_selected("radio", f.label(), true, None, on))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.model.format = f;
                        cx.notify();
                    })),
            );
        }
        let mut bar = div()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap_2()
            .p_2()
            .bg(rgb(t.surface_panel))
            .border_b_1()
            .border_color(rgb(t.surface_separator))
            .child(picker);
        // The progress, in the tree only while a scan runs.
        let mut line = div()
            .relative()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(11.))
            .text_color(rgb(t.text_secondary));
        if !progress.is_empty() {
            line = line
                .child(progress.clone())
                .child(scope.control("text", &progress, true));
        }
        bar = bar.child(line);
        for (label, on) in enabled {
            bar = bar.child(button(label, on, &mut scope, cx));
        }
        let text = self.search.read(cx).text().to_owned();
        let focused = self.search.focus_handle(cx).is_focused(window);
        bar.child(
            div()
                .relative()
                .flex_none()
                .w(px(SEARCH_WIDTH))
                .child(self.search.clone())
                .child(scope.control_with("textfield", SEARCH, true, Some(&text), focused)),
        )
        .into_any_element()
    }

    fn table(&self, scope: &mut RegistryScope, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens();
        let app = AppState::global(cx);
        let rows = self.model.rows(&app.read(cx).app_plugins);
        let mut scope = scope.container();
        let mut header = div()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .px_2()
            .py_1()
            .gap_2()
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(11.))
            .text_color(rgb(t.text_secondary))
            .border_b_1()
            .border_color(rgb(t.surface_separator));
        for (i, title) in COLUMNS.iter().enumerate() {
            header = header.child(
                column(i, div().relative())
                    .child(*title)
                    .child(scope.control("header", title, true)),
            );
        }
        let mut list_scope = scope.container();
        let mut list = div()
            .id("pm-rows")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(list_scope.clip());
        for (n, r) in rows.into_iter().enumerate() {
            list = list.child(self.row(n, r, &mut list_scope, cx));
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(header)
            .child(list)
            .into_any_element()
    }

    fn row(
        &self,
        n: usize,
        r: ManagerRow,
        scope: &mut RegistryScope,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = tokens();
        let id = r.id();
        let selected = self.model.selection.contains(&id);
        let mut scope = scope.container();
        let status_colour = if r.is_blocklisted() {
            t.state_error
        } else if r.is_ok() {
            t.text_primary
        } else {
            t.state_warning
        };
        let status = r.status_text();
        let can_toggle = r.has_plugin();
        let on = r.is_enabled();
        // The On toggle: a press turns the plug-in on or off (and does not
        // select the row).
        let toggle_row = r.clone();
        let mut check = div()
            .id(("pm-on", n))
            .relative()
            .size(px(14.))
            .rounded(px(3.))
            .border_1()
            .border_color(rgb(t.surface_separator))
            .bg(rgb(if on { t.accent } else { t.surface_inset }))
            .opacity(if can_toggle { 1. } else { 0.45 })
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(10.))
            .text_color(rgb(t.on_accent))
            .when(on, |d| d.child("✓"));
        if can_toggle {
            check = check
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.toggle(toggle_row.clone(), cx);
                }));
        }
        let row_marker = scope
            .control_selected("row", r.name(), true, Some(r.path()), selected)
            .into_any_element();
        let check = check.child(scope.control_with(
            "checkbox",
            r.name(),
            can_toggle,
            Some(if on { "on" } else { "off" }),
            false,
        ));
        let cells: [AnyElement; 6] = [
            div().child(r.name().to_owned()).into_any_element(),
            div().child(r.vendor().to_owned()).into_any_element(),
            div().child(r.format_text()).into_any_element(),
            div().child(r.version().to_owned()).into_any_element(),
            div().child(r.kind_text()).into_any_element(),
            div()
                .relative()
                .text_color(rgb(status_colour))
                .child(status.clone())
                .child(scope.control("text", &status, true))
                .into_any_element(),
        ];
        let mut line = div()
            .id(("pm-row", n))
            .relative()
            .flex()
            .flex_row()
            .flex_none()
            .items_center()
            .gap_2()
            .px_2()
            .py(px(3.))
            .text_size(px(12.))
            .map(|d| {
                if selected {
                    d.bg(rgb(t.accent)).text_color(rgb(t.on_accent))
                } else {
                    d.text_color(rgb(t.text_primary))
                }
            })
            .child(row_marker)
            .child(column(0, div().flex().items_center()).child(check));
        for (i, cell) in cells.into_iter().enumerate() {
            line = line.child(column(i + 1, div().relative()).child(cell));
        }
        let line = line.on_click(cx.listener(move |this, e: &ClickEvent, _, cx| {
            this.click_row(id.clone(), e, cx);
        }));
        crate::sheet_host::with_help(line, r.path().to_owned()).into_any_element()
    }
}

/// Column `i`'s width: fixed for On, Format, Version and Type; Name,
/// Vendor and Status share the rest.
fn column(i: usize, d: gpui::Div) -> gpui::Div {
    let d = d.min_w_0().overflow_hidden().whitespace_nowrap();
    match i {
        0 => d.flex_none().w(px(ON_WIDTH)),
        3 => d.flex_none().w(px(FORMAT_WIDTH)),
        4 => d.flex_none().w(px(VERSION_WIDTH)),
        5 => d.flex_none().w(px(TYPE_WIDTH)),
        _ => d.flex_1(),
    }
}

/// A toolbar button: `button` in the tree; a disabled one takes no
/// clicks.
fn button(
    label: &'static str,
    enabled: bool,
    scope: &mut RegistryScope,
    cx: &mut Context<PluginManagerWindow>,
) -> AnyElement {
    let t = tokens();
    let mut face = div()
        .id(label)
        .relative()
        .flex_none()
        .px_2()
        .py(px(2.))
        .rounded(px(t.radius))
        .border_1()
        .border_color(rgb(t.surface_separator))
        .bg(rgb(t.surface_inset))
        .text_color(rgb(t.text_primary))
        .opacity(if enabled { 1. } else { 0.45 })
        .child(label);
    if enabled {
        face = face
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.press(label, cx)));
    }
    face.child(scope.control("button", label, enabled))
        .into_any_element()
}

impl Focusable for PluginManagerWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PluginManagerWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens();
        let mut scope = RegistryScope::root(&self.registry);
        let toolbar = self.toolbar(&mut scope, window, cx);
        let table = self.table(&mut scope, cx);
        let footer = div()
            .relative()
            .flex_none()
            .p_2()
            .text_size(px(11.))
            .text_color(rgb(t.text_secondary))
            .border_t_1()
            .border_color(rgb(t.surface_separator))
            .child(FOOTER)
            .child(scope.control("text", FOOTER, true));
        div()
            .id("plugin-manager")
            .track_focus(&self.focus)
            .key_context(PLUGIN_MANAGER_CONTEXT)
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(crate::settings_window::background()))
            .text_color(rgb(t.text_primary))
            .text_size(px(13.))
            .child(toolbar)
            .child(table)
            .child(footer)
    }
}

/// Opens the Plugin Manager (Window › Plugin Manager, ⌥⌘P; the control
/// verb `window.plugin_manager`), or brings the open one to the front.
/// While it is open the catalog's progress is polled. Call it outside
/// `AppState::update`.
pub fn open_plugin_manager(cx: &mut App) {
    let app = AppState::global(cx);
    if let Some(h) = app.read(cx).plugin_manager_window {
        if h.update(cx, |_, window, _| window.activate_window())
            .is_ok()
        {
            return;
        }
        // The window is gone and its handle stale: forget it, open anew.
        app.update(cx, |s, _| s.plugin_manager_window = None);
    }
    let bounds = Bounds::centered(None, size(px(MIN_WIDTH), px(OPEN_HEIGHT)), cx);
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            titlebar: Some(TitlebarOptions {
                title: Some(TITLE.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        |window, cx| cx.new(|cx| PluginManagerWindow::new(window, cx)),
    );
    match opened {
        Ok(h) => app.update(cx, |s, cx| {
            s.plugin_manager_window = Some(h);
            s.plugin_manager_open = true;
            cx.notify();
        }),
        Err(e) => {
            app.update(cx, |s, cx| {
                s.store.post_alert(vizdaw_app::alerts::AlertItem::new(
                    "Window Failed",
                    e.to_string(),
                ));
                cx.notify();
            });
            crate::alerts::present(cx);
        }
    }
}
