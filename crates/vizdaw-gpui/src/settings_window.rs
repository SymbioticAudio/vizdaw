//! The Settings window (Swift `Settings { TabView { … } }`,
//! `VizDAWApp.swift:267-287`): one per app, the nine tabs in Swift's
//! order. It belongs to no document, so [`AppState`] keeps its handle
//! outside `windows`, and it is not one of a document's key contexts:
//! Space in it does not play. A tab whose slice has not come shows
//! "`<Tab>` comes in a later version.".

use gpui::{
    AnyElement, App, AppContext, Bounds, Context, FocusHandle, InteractiveElement, IntoElement,
    ParentElement, Render, StatefulInteractiveElement, Styled, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, px, rgb, size,
};

use crate::app_state::AppState;
use crate::menus::{CloseWindow, SETTINGS_WINDOW_CONTEXT};
use crate::registry::{RegistryScope, SharedRegistry};
use crate::strip_palette::tokens;

/// The window's width (Swift's Form width).
pub const WIDTH: f32 = 500.;
/// Its content height when a tab has no height of its own.
pub const DEFAULT_HEIGHT: f32 = 340.;
/// The tab strip's room: one row, and a second where the labels wrap.
const STRIP_HEIGHT: f32 = 64.;

/// The Settings tabs, in Swift's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Appearance,
    Editing,
    Keys,
    Audio,
    Recording,
    Automation,
    Surfaces,
    Plugins,
    CoProducer,
}

impl SettingsTab {
    pub const ALL: [SettingsTab; 9] = [
        SettingsTab::Appearance,
        SettingsTab::Editing,
        SettingsTab::Keys,
        SettingsTab::Audio,
        SettingsTab::Recording,
        SettingsTab::Automation,
        SettingsTab::Surfaces,
        SettingsTab::Plugins,
        SettingsTab::CoProducer,
    ];

    /// The tab's label (`VizDAWApp.swift:269-285`).
    pub fn label(self) -> &'static str {
        match self {
            SettingsTab::Appearance => "Appearance",
            SettingsTab::Editing => "Editing",
            SettingsTab::Keys => "Keys",
            SettingsTab::Audio => "Audio",
            SettingsTab::Recording => "Recording",
            SettingsTab::Automation => "Automation",
            SettingsTab::Surfaces => "Surfaces",
            SettingsTab::Plugins => "Plug-ins",
            SettingsTab::CoProducer => "Co-Producer",
        }
    }

    /// The window's content height with this tab shown (Tasks 5-6 give
    /// their tabs their own).
    pub fn height(self) -> f32 {
        DEFAULT_HEIGHT
    }
}

/// The line a tab not built yet shows.
pub fn placeholder(tab: SettingsTab) -> String {
    format!("{} comes in a later version.", tab.label())
}

/// The Settings window's view.
pub struct SettingsWindow {
    tab: SettingsTab,
    registry: SharedRegistry,
    focus: FocusHandle,
}

impl SettingsWindow {
    fn new(tab: SettingsTab, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        // ⌘W reaches the window's root through its focus.
        window.focus(&focus, cx);
        cx.on_release(|_, cx| {
            let app = AppState::global(cx);
            app.update(cx, |s, _| s.settings_window = None);
        })
        .detach();
        SettingsWindow {
            tab,
            registry: SharedRegistry::default(),
            focus,
        }
    }

    /// The tab shown.
    pub fn tab(&self) -> SettingsTab {
        self.tab
    }

    /// The elements this window drew last.
    pub fn registry(&self) -> &SharedRegistry {
        &self.registry
    }

    fn select(&mut self, tab: SettingsTab, window: &mut Window, cx: &mut Context<Self>) {
        self.tab = tab;
        let width = window.viewport_size().width;
        window.resize(size(width, px(tab.height() + STRIP_HEIGHT)));
        cx.notify();
    }

    // One body per tab; Tasks 5-6 fill Audio, Recording and Automation.

    fn appearance_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Appearance, scope)
    }

    fn editing_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Editing, scope)
    }

    fn keys_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Keys, scope)
    }

    fn audio_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Audio, scope)
    }

    fn recording_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Recording, scope)
    }

    fn automation_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Automation, scope)
    }

    fn surfaces_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Surfaces, scope)
    }

    fn plugins_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::Plugins, scope)
    }

    fn co_producer_tab(&mut self, scope: &mut RegistryScope) -> AnyElement {
        placeholder_body(SettingsTab::CoProducer, scope)
    }

    fn body(&mut self, scope: &mut RegistryScope) -> AnyElement {
        match self.tab {
            SettingsTab::Appearance => self.appearance_tab(scope),
            SettingsTab::Editing => self.editing_tab(scope),
            SettingsTab::Keys => self.keys_tab(scope),
            SettingsTab::Audio => self.audio_tab(scope),
            SettingsTab::Recording => self.recording_tab(scope),
            SettingsTab::Automation => self.automation_tab(scope),
            SettingsTab::Surfaces => self.surfaces_tab(scope),
            SettingsTab::Plugins => self.plugins_tab(scope),
            SettingsTab::CoProducer => self.co_producer_tab(scope),
        }
    }
}

fn placeholder_body(tab: SettingsTab, scope: &mut RegistryScope) -> AnyElement {
    let t = tokens();
    let line = placeholder(tab);
    div()
        .relative()
        .p_4()
        .text_color(rgb(t.text_secondary))
        .child(line.clone())
        .child(scope.control("text", &line, true))
        .into_any_element()
}

impl Render for SettingsWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens();
        let mut scope = RegistryScope::root(&self.registry);
        let mut strip = scope.container();
        let mut tabs = div()
            .flex()
            .flex_row()
            // The nine labels wrap to a second row where a face is wide.
            .flex_wrap()
            .items_center()
            .gap_1()
            .p_1()
            .min_h(px(STRIP_HEIGHT))
            .bg(rgb(t.surface_panel))
            .border_b_1()
            .border_color(rgb(t.surface_separator));
        for tab in SettingsTab::ALL {
            let on = tab == self.tab;
            tabs = tabs.child(
                div()
                    .id(tab.label())
                    .relative()
                    .px_2()
                    .py_1()
                    .rounded(px(t.radius))
                    .when_selected(on, t.accent, t.text_inverse, t.text_primary)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.select(tab, window, cx);
                    }))
                    .child(tab.label())
                    .child(strip.control_selected("tab", tab.label(), true, None, on)),
            );
        }
        let body = self.body(&mut scope);
        div()
            .id("settings-window")
            .track_focus(&self.focus)
            .key_context(SETTINGS_WINDOW_CONTEXT)
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(t.surface_raised))
            .text_color(rgb(t.text_primary))
            .text_size(px(13.))
            .child(tabs)
            .child(body)
    }
}

trait Selected: Styled + Sized {
    /// The selected tab's fill and text, else the plain text colour.
    fn when_selected(self, on: bool, fill: u32, on_text: u32, text: u32) -> Self {
        if on {
            self.bg(rgb(fill)).text_color(rgb(on_text))
        } else {
            self.text_color(rgb(text))
        }
    }
}

impl<T: Styled + Sized> Selected for T {}

/// Opens the Settings window on `tab` (`None`: the tab it was on, else
/// Appearance), or brings the open one to the front. Call it outside
/// `AppState::update`.
pub fn open_settings(tab: Option<SettingsTab>, cx: &mut App) {
    let app = AppState::global(cx);
    if let Some(h) = app.read(cx).settings_window {
        h.update(cx, |this, window, cx| {
            if let Some(tab) = tab {
                this.select(tab, window, cx);
            }
            window.activate_window();
        })
        .ok();
        return;
    }
    let tab = tab.unwrap_or(SettingsTab::Appearance);
    let bounds = Bounds::centered(None, size(px(WIDTH), px(tab.height() + STRIP_HEIGHT)), cx);
    let opened = cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Settings".into()),
                ..Default::default()
            }),
            is_resizable: false,
            ..Default::default()
        },
        move |window, cx| cx.new(|cx| SettingsWindow::new(tab, window, cx)),
    );
    match opened {
        Ok(h) => {
            app.update(cx, |s, cx| {
                s.settings_window = Some(h);
                cx.notify();
            });
        }
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
