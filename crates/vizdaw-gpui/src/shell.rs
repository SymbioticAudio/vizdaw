//! The document window: one window per open document, with the menu bar
//! (off macOS), the alert banner, the transport bar and (for now) an empty
//! body.

use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, PromptLevel, Render, StatefulInteractiveElement,
    Styled, Subscription, Task, Window, WindowBounds, WindowHandle, WindowOptions, div, px, rgb,
    size,
};
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::commands;
use vizdaw_app::{DocId, SaveAction, WindowToken};

use crate::app_state::AppState;
use crate::menu_bar::MenuBar;
use crate::menus::{self, CloseWindow, PlayStop, RunCommand, Save, SaveAs, StopAndReturn};
use crate::registry::{RegistryScope, SharedRegistry};
use crate::text_input::TextInput;
use crate::transport_bar::{self, BarState, colors};

pub struct DocumentWindow {
    app: Entity<AppState>,
    doc: DocId,
    token: WindowToken,
    focus: FocusHandle,
    registry: SharedRegistry,
    /// The title last given to the platform window.
    title: String,
    /// Text fields in the body (none yet; slice 2 panes add theirs).
    fields: Vec<Entity<TextInput>>,
    /// A save-changes prompt is showing.
    prompting: bool,
    /// The in-window menu bar, when this window draws one (off macOS).
    menu_bar: Option<MenuBar>,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl DocumentWindow {
    /// Opens a window for `doc`, an open document of the store, and builds
    /// its transport. A failure is an alert and the document closes. Call
    /// it outside `AppState::update`: the window draws (and reads the
    /// state) before this returns.
    pub fn open(
        app: &Entity<AppState>,
        doc: DocId,
        cx: &mut App,
    ) -> Result<WindowHandle<DocumentWindow>, String> {
        let attached = app.update(cx, |s, cx| {
            cx.notify();
            s.attach(doc)
        });
        if !attached {
            return Err("the document's transport could not be read".into());
        }
        let token = app.update(cx, |s, _| s.new_token());
        let bounds = Bounds::centered(None, size(px(1000.), px(640.)), cx);
        let state = app.clone();
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| cx.new(|cx| DocumentWindow::new(state, doc, token, window, cx)),
        );
        let handle = match opened {
            Ok(h) => h,
            Err(e) => {
                app.update(cx, |s, cx| {
                    s.store
                        .post_alert(AlertItem::new("Window Failed", e.to_string()));
                    s.close_document(doc);
                    cx.notify();
                });
                return Err(e.to_string());
            }
        };
        app.update(cx, |s, _| {
            s.add_window(doc, token, handle);
            s.store.focus_document(doc, token);
        });
        handle
            .update(cx, |v, window, cx| {
                window.focus(&v.focus, cx);
                window.activate_window();
            })
            .ok();
        Ok(handle)
    }

    fn new(
        app: Entity<AppState>,
        doc: DocId,
        token: WindowToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = vec![
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    let (doc, token) = (this.doc, this.token);
                    this.app
                        .update(cx, |s, _| s.store.focus_document(doc, token));
                }
            }),
        ];
        let state = app.clone();
        subscriptions.push(cx.on_release(move |_, cx| {
            state.update(cx, |s, cx| {
                s.close_document(doc);
                cx.notify();
            });
        }));
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.should_close(window, cx))
                .unwrap_or(true)
        });
        DocumentWindow {
            _poll: Self::start_polling(doc, window, cx),
            app,
            doc,
            token,
            focus: cx.focus_handle(),
            registry: SharedRegistry::default(),
            title: String::new(),
            fields: Vec::new(),
            prompting: false,
            menu_bar: cfg!(not(target_os = "macos"))
                .then(|| MenuBar::new(menus::menu_model(None, &Default::default()))),
            _subscriptions: subscriptions,
        }
    }

    /// Polls the transport every `poll_interval()` (read again each tick:
    /// fast while playing), until the document closes.
    fn start_polling(doc: DocId, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let interval = this
                    .update(cx, |this, cx| this.app.read(cx).poll_interval(doc))
                    .ok()
                    .flatten();
                let Some(interval) = interval else { break };
                cx.background_executor().timer(interval).await;
                let polled = this.update(cx, |this, cx| {
                    this.app.update(cx, |s, cx| {
                        if s.poll(doc) {
                            cx.notify();
                        }
                    })
                });
                if polled.is_err() {
                    break;
                }
            }
        })
    }

    pub fn doc(&self) -> DocId {
        self.doc
    }

    pub fn token(&self) -> WindowToken {
        self.token
    }

    pub fn app(&self) -> &Entity<AppState> {
        &self.app
    }

    /// The title last drawn (the document's title, marked when edited).
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The elements drawn last frame, for `ui.tree` and `ui.click`.
    pub fn registry(&self) -> &SharedRegistry {
        &self.registry
    }

    /// Adds a text field to the window body. Slice 1 has no panes with
    /// fields of their own; the shortcut tests use this one.
    pub fn add_field(&mut self, placeholder: &str, cx: &mut Context<Self>) -> Entity<TextInput> {
        let field = cx.new(|cx| TextInput::new(placeholder, cx));
        self.fields.push(field.clone());
        cx.notify();
        field
    }

    /// The in-window menu bar, if this window draws one.
    pub fn menu_bar(&self) -> Option<&MenuBar> {
        self.menu_bar.as_ref()
    }

    pub fn menu_bar_mut(&mut self) -> Option<&mut MenuBar> {
        self.menu_bar.as_mut()
    }

    /// Shows or hides the in-window menu bar. Off macOS it is shown from
    /// the start; tests turn it on to drive it on macOS.
    pub fn set_menu_bar_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if shown != self.menu_bar.is_some() {
            self.menu_bar =
                shown.then(|| MenuBar::new(menus::menu_model(None, &Default::default())));
            cx.notify();
        }
    }

    /// A document command from its menu item or key, on this window's
    /// document. A refusal shows in the status line; Rename starts the
    /// track header's rename field.
    pub fn run_command(
        &mut self,
        action: &RunCommand,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let doc = self.doc;
        let (result, lane) = self.app.update(cx, |s, cx| {
            cx.notify();
            s.store
                .with_document(doc, |d, _| {
                    (
                        commands::perform_from_menu(&action.id, d),
                        d.selected_lane(),
                    )
                })
                .unzip()
        });
        if let (Some(Err(e)), Some(Some(lane))) = (result, lane)
            && e.dialog.as_deref() == Some("Rename Track")
        {
            self.begin_rename(lane, window, cx);
        }
        cx.notify();
    }

    /// Starts the inline rename of track `lane` in its header. Task 5
    /// (track headers) fills this in; until then Rename does nothing.
    pub fn begin_rename(&mut self, lane: u64, _window: &mut Window, _cx: &mut Context<Self>) {
        let _ = lane;
    }

    /// Escape closes an open menu list (the key goes no further).
    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" && self.menu_bar.as_mut().is_some_and(MenuBar::close) {
            cx.stop_propagation();
            cx.notify();
        }
    }

    pub fn play_stop(&mut self, _: &PlayStop, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc;
        self.app.update(cx, |s, cx| {
            s.play_stop(doc);
            cx.notify();
        });
        self.restart_polling(window, cx);
    }

    pub fn stop_and_return(
        &mut self,
        _: &StopAndReturn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let doc = self.doc;
        self.app.update(cx, |s, cx| {
            s.stop_and_return(doc);
            cx.notify();
        });
        self.restart_polling(window, cx);
    }

    /// Starting or stopping changes the interval: wait the new one, not
    /// the rest of the old (a stopped wait would hold the readout still
    /// for up to 250 ms after Play).
    fn restart_polling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._poll = Self::start_polling(self.doc, window, cx);
    }

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        self.save_then_close(false, window, cx);
    }

    fn save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        self.prompt_save_as(false, window, cx);
    }

    fn close_window(&mut self, _: &CloseWindow, window: &mut Window, cx: &mut Context<Self>) {
        if self.should_close(window, cx) {
            window.remove_window();
        }
    }

    /// Saves (asking for a location when untitled), then closes the window
    /// if `close` and the save went through.
    fn save_then_close(&mut self, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc;
        let result = self.app.update(cx, |s, cx| {
            let r = s.save(doc);
            cx.notify();
            r
        });
        match result {
            SaveAction::Saved if close => window.remove_window(),
            SaveAction::NeedsSaveAs => self.prompt_save_as(close, window, cx),
            _ => {}
        }
    }

    fn prompt_save_as(&mut self, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc;
        let (dir, name) = {
            let s = self.app.read(cx);
            (s.save_directory(doc), s.suggested_name(doc))
        };
        let chosen = cx.prompt_for_new_path(&dir, Some(&name));
        cx.spawn_in(window, async move |this, cx| {
            let answer = chosen.await;
            this.update_in(cx, |this, window, cx| {
                let path = match answer {
                    Ok(Ok(Some(path))) => Some(path),
                    Ok(Err(e)) => {
                        this.app.update(cx, |s, cx| {
                            cx.notify();
                            s.store
                                .post_alert(AlertItem::new("Save Failed", e.to_string()));
                        });
                        None
                    }
                    _ => None,
                };
                let Some(path) = path else { return };
                match crate::app_state::silent_replacement(&path) {
                    Some(existing) => this.confirm_replace(doc, path, existing, close, window, cx),
                    None => this.finish_save_as(doc, &path, close, window, cx),
                }
            })
            .ok();
        })
        .detach();
    }

    fn finish_save_as(
        &mut self,
        doc: DocId,
        path: &Path,
        close: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let saved = self.app.update(cx, |s, cx| {
            cx.notify();
            s.save_as(doc, path).is_ok()
        });
        if saved && close {
            window.remove_window();
        }
    }

    /// The panel did not ask (the name had no extension), so ask before
    /// saving over the existing project.
    fn confirm_replace(
        &mut self,
        doc: DocId,
        path: PathBuf,
        existing: PathBuf,
        close: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = existing
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("\u{201c}{name}\u{201d} already exists. Do you want to replace it?"),
            Some("Replacing it overwrites the project that is there."),
            &["Replace", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(0) = answer.await {
                this.update_in(cx, |this, window, cx| {
                    this.finish_save_as(doc, &path, close, window, cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// True when the window may close now; with unsaved changes it asks
    /// first (Save, Don't Save, Cancel) and answers false.
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let doc = self.doc;
        let (dirty, title) = {
            let s = self.app.read(cx);
            (
                s.has_unsaved_changes(doc),
                s.store.document(doc).map(|d| d.snapshot().title.clone()),
            )
        };
        if !dirty {
            return true;
        }
        if self.prompting {
            return false;
        }
        self.prompting = true;
        let message = format!(
            "Save changes to \u{201c}{}\u{201d}?",
            title.unwrap_or_default()
        );
        let answer = window.prompt(
            PromptLevel::Warning,
            &message,
            Some("Your changes are lost if you don't save them."),
            &["Save", "Don't Save", "Cancel"],
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await;
            this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                match answer {
                    Ok(0) => this.save_then_close(true, window, cx),
                    Ok(1) => window.remove_window(),
                    _ => {}
                }
            })
            .ok();
        })
        .detach();
        false
    }

    fn alert_banner(
        alert: &AlertItem,
        scope: &mut RegistryScope,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .py_2()
            .bg(rgb(0x5a2a2a))
            .child(
                div()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(alert.title.clone()),
            )
            .child(div().flex_1().child(alert.message.clone()))
            .child(
                transport_bar::button("dismiss-alert", "Dismiss", true, colors::BUTTON, scope)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.update(cx, |s, cx| {
                            s.dismiss_alert();
                            cx.notify();
                        })
                    })),
            )
    }
}

impl Focusable for DocumentWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DocumentWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let doc = self.doc;
        let (title, dirty, alert, bar, model, front) = {
            let s = self.app.read(cx);
            (
                s.title(doc).unwrap_or_default(),
                s.is_dirty(doc),
                s.store.alert().cloned(),
                s.readout(doc).map(|readout| BarState {
                    readout,
                    can_play: s.can_play(doc),
                }),
                menus::menu_model(s.store.document(doc), &s.keymap),
                s.store.front().is_none_or(|f| f == doc),
            )
        };
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        window.set_window_edited(dirty);
        // The app's menus follow the front document (set again only when
        // they change). Deferred: not from inside a draw.
        if front {
            let shown = model.clone();
            cx.defer(move |cx| menus::show(shown, cx));
        }
        if let Some(bar) = &mut self.menu_bar {
            bar.set_model(model);
        }

        let mut root = RegistryScope::root(&self.registry);
        let menu_bar = self
            .menu_bar
            .as_ref()
            .map(|b| b.render(&mut root.container(), window, cx));
        div()
            .key_context(menus::WINDOW_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::run_command))
            .on_action(cx.listener(Self::play_stop))
            .on_action(cx.listener(Self::stop_and_return))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::close_window))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x1c1c22))
            .text_color(rgb(colors::TEXT))
            .text_sm()
            .children(menu_bar)
            .when_some(alert, |d, alert| {
                d.child(Self::alert_banner(&alert, &mut root.container(), cx))
            })
            .when_some(bar, |d, bar| {
                d.child(transport_bar::render(&bar, &mut root.container(), cx))
            })
            .child(
                div().flex_1().p_3().flex().flex_col().gap_2().children(
                    self.fields
                        .iter()
                        .map(|f| div().w(px(240.)).text_color(rgb(0x000000)).child(f.clone())),
                ),
            )
    }
}
