//! The right pane's Plug-ins view (Swift `PluginsPane`,
//! `S/PaneViews.swift:236-257`): the effect browser at the pane's width.
//! Its list is the catalog's usable effects as they are now (Swift read
//! them at each draw), filtered by its own search. A pick goes on the end
//! of the inserts of the strip the view points at
//! ([`vizdaw_app::view::pane_insert_plugin`]); with no strip, or no mixer,
//! the status line says why. No window opens.

use std::rc::Rc;

use gpui::{AnyElement, AppContext, Context, Entity, Focusable, Subscription, WeakEntity, Window};
use vizdaw_app::plugin_browser::{PluginBrowserModel, SEARCH};
use vizdaw_app::view;
use vizdaw_ffi::PluginKindTag;
use vizdaw_ffi::snapshot::PluginRefRow;

use crate::plugin_browser::{self, Fit};
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::text_input::{TextInput, TextInputEvent};

/// The bare transport keys wait while the search has the keyboard.
const TEXT_OWNER: &str = "plug-ins pane search";

/// The view's search field, kept while the window lives.
pub struct PluginsPane {
    field: Entity<TextInput>,
    _subscriptions: Vec<Subscription>,
}

impl PluginsPane {
    pub fn new(window: &mut Window, cx: &mut Context<DocumentWindow>) -> Self {
        let field = cx.new(|cx| TextInput::new(SEARCH, cx));
        let focus = field.focus_handle(cx);
        let subscriptions = vec![
            cx.on_focus(&focus, window, |this, _, cx| {
                this.with_model(cx, |d| d.begin_text_editing(TEXT_OWNER));
            }),
            cx.on_blur(&focus, window, |this, _, cx| {
                this.with_model(cx, |d| d.end_text_editing(TEXT_OWNER));
            }),
            cx.subscribe_in(&field, window, |this, field, e: &TextInputEvent, _, cx| {
                if matches!(e, TextInputEvent::Confirm) {
                    // Return picks the one plug-in the search leaves.
                    let text = field.read(cx).text().to_owned();
                    let model = effects(this, cx);
                    if let Some(plugin) = plugin_browser::only_match(&model, &text) {
                        pick(this, &plugin, cx);
                    }
                }
            }),
            // The list follows the search.
            cx.observe(&field, |_, _, cx| cx.notify()),
        ];
        PluginsPane {
            field,
            _subscriptions: subscriptions,
        }
    }

    /// The search has the keyboard.
    pub fn focused(&self, window: &Window, cx: &gpui::App) -> bool {
        self.field.focus_handle(cx).is_focused(window)
    }
}

/// The catalog's usable effects now.
fn effects(w: &DocumentWindow, cx: &gpui::App) -> PluginBrowserModel {
    let rows = w.app().read(cx).app_plugins.browser(PluginKindTag::Effect);
    PluginBrowserModel::new(PluginKindTag::Effect, rows)
}

fn pick(w: &mut DocumentWindow, plugin: &PluginRefRow, cx: &mut Context<DocumentWindow>) {
    w.with_model(cx, |d| view::pane_insert_plugin(d, plugin));
}

/// The view, filling the pane. `scope` is its own container.
pub(crate) fn render(
    w: &DocumentWindow,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let model = effects(w, cx);
    let weak: WeakEntity<DocumentWindow> = cx.entity().downgrade();
    let on_pick: plugin_browser::OnPick = Rc::new(move |plugin, _, cx| {
        weak.update(cx, |w, cx| pick(w, plugin, cx)).ok();
    });
    let on_outside: plugin_browser::OnOutside = Rc::new(|_, _, _| {});
    plugin_browser::view(
        Fit::Pane,
        &model,
        &w.plugins_pane.field,
        on_pick,
        on_outside,
        scope,
        window,
        cx,
    )
}
