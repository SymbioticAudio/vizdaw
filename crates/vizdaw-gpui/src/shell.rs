//! The document window: one window per open document, with the menu bar
//! (off macOS), the alert banner, the transport bar, the arrangement's tool
//! bar, the track header column beside the arrangement canvas, the bottom
//! detail pane under them, and the status line.
//! The window holds the bar's shuttle (Rewind and Fast Forward held, and
//! their held keys) and its fields' edit.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::prelude::FluentBuilder;
use gpui::{
    App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, KeyUpEvent, Keystroke, MouseButton, ParentElement, PromptLevel,
    Render, StatefulInteractiveElement, Styled, Subscription, Task, Window, WindowBounds,
    WindowHandle, WindowOptions, canvas, div, px, rgb, size,
};
use vizdaw_app::alerts::{AlertItem, error_message};
use vizdaw_app::commands::{self, FieldFocus};
use vizdaw_app::transport::Shuttle;
use vizdaw_app::transport_fields::{self as fields_model, TransportField};
use vizdaw_app::{DocId, DocumentModel, SaveAction, WindowToken};
use vizdaw_ffi::snapshot::LaneRow;

use crate::app_state::AppState;
use crate::arrangement::{self, Arrangement, ArrangementState, ToolbarState};
use crate::detail_pane::{self, DetailPaneState};
use crate::list_editor::{ListEditor, ListState};
use crate::menu_bar::MenuBar;
use crate::menus::{self, CloseWindow, PlayStop, RunCommand, Save, SaveAs, StopAndReturn};
use crate::registry::{RegistryScope, SharedRegistry};
use crate::text_input::{TextInput, TextInputEvent};
use crate::track_headers::{HeadersState, Rename, TrackHeaders};
use crate::transport_bar::{self, BarState, Plan, colors};
use crate::transport_fields::TransportFields;

/// How long a status text shows (Swift `StatusLine`).
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(6);

pub struct DocumentWindow {
    app: Entity<AppState>,
    doc: DocId,
    token: WindowToken,
    focus: FocusHandle,
    registry: SharedRegistry,
    /// The title last given to the platform window.
    title: String,
    /// A save-changes prompt is showing.
    prompting: bool,
    /// The in-window menu bar, when this window draws one (off macOS).
    menu_bar: Option<MenuBar>,
    /// The track header column (its rename field, its Add Track menu).
    track_headers: TrackHeaders,
    /// The arrangement canvas (its axis, scroll and ruler press).
    pub(crate) arrangement: Arrangement,
    /// The status text last drawn, and the timer that clears it.
    status_shown: Option<String>,
    _status_timer: Option<Task<()>>,
    /// The transport bar's Position and Edit In/Out fields.
    pub(crate) transport_fields: TransportFields,
    /// Rewind and Fast Forward held, and their held keys.
    shuttle: Shuttle,
    /// Winds a held button every `Shuttle::TICK_EVERY` until it ends.
    _hold: Option<Task<()>>,
    /// The transport bar's » menu is open.
    more_open: bool,
    /// The bottom pane's picker menu is open.
    pub(crate) detail_menu_open: bool,
    /// The List editor's table (its selection, scroll and cell edit).
    pub(crate) list_editor: ListEditor,
    /// The size of the area the arrangement and the bottom pane share, at
    /// the last draw (zero before the first).
    regions: Rc<Cell<gpui::Size<gpui::Pixels>>>,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

/// The text-editing owner of the track header's rename field.
const RENAME_OWNER: &str = "track-rename";

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
            cx.observe_in(&app, window, |this, _, window, cx| {
                this.end_stale_rename(window, cx);
                cx.notify();
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    let (doc, token) = (this.doc, this.token);
                    this.app
                        .update(cx, |s, _| s.store.focus_document(doc, token));
                } else {
                    // The window lost the keyboard: a field edit cancels, a
                    // hold ends where it got to, a held key lands.
                    this.cancel_field(window, cx);
                    this.end_shuttle(cx);
                    this.cancel_ruler_drag(cx);
                    // A press whose release may never come is dropped; a
                    // released drag whose final edit waits on Busy is left
                    // to the timer's tick (Swift never cancels here).
                    if this.arrangement.pressing() {
                        this.cancel_block_drag(cx);
                    }
                }
            }),
        ];
        let state = app.clone();
        subscriptions.push(cx.on_release(move |this: &mut Self, cx| {
            this._hold = None;
            let shuttle = &mut this.shuttle;
            state.update(cx, |s, cx| {
                s.store.with_document(doc, |d, _| {
                    shuttle.cancel(d);
                    shuttle.key_up(d);
                });
                s.close_document(doc);
                cx.notify();
            });
        }));
        let list_editor = ListEditor::new(cx);
        subscriptions.extend(Self::watch_list_focus(
            list_editor.focus_handle(),
            window,
            cx,
        ));
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
            prompting: false,
            menu_bar: cfg!(not(target_os = "macos"))
                .then(|| MenuBar::new(menus::menu_model(None, &Default::default()))),
            track_headers: TrackHeaders::default(),
            arrangement: Arrangement::new(cx),
            status_shown: None,
            _status_timer: None,
            transport_fields: TransportFields::new(cx),
            shuttle: Shuttle::new(),
            _hold: None,
            more_open: false,
            detail_menu_open: false,
            list_editor,
            regions: Rc::default(),
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
        if action.id.starts_with("transport.") {
            self.after_transport(window, cx);
        }
        match (result, lane) {
            (Some(Err(e)), Some(Some(lane))) if e.dialog.as_deref() == Some("Rename Track") => {
                self.begin_rename(lane, window, cx);
            }
            (Some(Ok(ran)), _) => match ran.focus {
                Some(FieldFocus::Position) => {
                    self.focus_field(TransportField::Position, window, cx)
                }
                Some(FieldFocus::EditIn) => self.focus_field(TransportField::EditIn, window, cx),
                None => {}
            },
            _ => {}
        }
        cx.notify();
    }

    /// A transport command ran: the readouts read it now, and the poll
    /// takes the interval playing or stopping set.
    pub(crate) fn after_transport(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc;
        self.app.update(cx, |s, cx| {
            s.poll(doc);
            cx.notify();
        });
        self.restart_polling(window, cx);
    }

    /// Return to Start, the bar's button: a refusal shows in the status
    /// line.
    pub(crate) fn return_to_start(&mut self, cx: &mut Context<Self>) {
        let doc = self.doc;
        self.with_model(cx, |d| {
            if let Err(e) = d.move_to(0) {
                d.set_status(Some(error_message(&e)));
            }
        });
        self.app.update(cx, |s, _| s.poll(doc));
    }

    /// The bar's Loop button; a refusal is the alert "Loop Failed".
    pub(crate) fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        self.with_model(cx, DocumentModel::toggle_loop);
    }

    pub(crate) fn toggle_more(&mut self, cx: &mut Context<Self>) {
        self.more_open = !self.more_open;
        cx.notify();
    }

    pub(crate) fn close_more(&mut self, cx: &mut Context<Self>) {
        self.more_open = false;
        cx.notify();
    }

    /// Runs `f` on the shuttle and the window's document.
    fn with_shuttle<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Shuttle, &mut DocumentModel) -> R,
    ) -> Option<R> {
        let doc = self.doc;
        let shuttle = &mut self.shuttle;
        self.app.update(cx, |s, cx| {
            cx.notify();
            s.store.with_document(doc, |d, _| f(shuttle, d))
        })
    }

    /// Rewind (`forward` false) or Fast Forward pressed: held, it winds
    /// every `Shuttle::TICK_EVERY` until the release or a cancel.
    pub(crate) fn shuttle_press(
        &mut self,
        forward: bool,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = cx.background_executor().now();
        let holding = self
            .with_shuttle(cx, |sh, d| {
                sh.press(forward, shift, now, d);
                sh.holding()
            })
            .unwrap_or(false);
        self._hold = holding.then(|| {
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor().timer(Shuttle::TICK_EVERY).await;
                    let going = this.update(cx, |this, cx| {
                        let now = cx.background_executor().now();
                        let going = this
                            .with_shuttle(cx, |sh, d| {
                                sh.tick(now, d);
                                sh.holding()
                            })
                            .unwrap_or(false);
                        let doc = this.doc;
                        this.app.update(cx, |s, _| s.poll(doc));
                        going
                    });
                    if !matches!(going, Ok(true)) {
                        break;
                    }
                }
            })
        });
    }

    /// The held button is released, over it (`inside`) or not.
    pub(crate) fn shuttle_release(&mut self, inside: bool, cx: &mut Context<Self>) {
        if !self.shuttle.holding() {
            return;
        }
        self._hold = None;
        let now = cx.background_executor().now();
        self.with_shuttle(cx, |sh, d| sh.release(inside, now, d));
        let doc = self.doc;
        self.app.update(cx, |s, _| s.poll(doc));
    }

    /// Ends a hold where it got to and lands a held key.
    fn end_shuttle(&mut self, cx: &mut Context<Self>) {
        self._hold = None;
        self.with_shuttle(cx, |sh, d| {
            sh.cancel(d);
            sh.key_up(d);
        });
    }

    /// The track header column's state.
    pub fn track_headers(&self) -> &TrackHeaders {
        &self.track_headers
    }

    pub(crate) fn track_headers_mut(&mut self) -> &mut TrackHeaders {
        &mut self.track_headers
    }

    /// The track whose name is being edited in its header.
    pub fn renaming(&self) -> Option<u64> {
        self.track_headers.renaming()
    }

    pub(crate) fn with_model<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut DocumentModel) -> R,
    ) -> Option<R> {
        let doc = self.doc;
        self.app.update(cx, |s, cx| {
            cx.notify();
            s.store.with_document(doc, |d, _| f(d))
        })
    }

    /// Track `lane` of the shown sequence, as last fetched.
    pub(crate) fn lane(&self, lane: u64, cx: &App) -> Option<LaneRow> {
        let s = self.app.read(cx);
        let d = s.store.document(self.doc)?;
        d.lanes().iter().find(|l| l.id == lane).cloned()
    }

    pub(crate) fn select_track(&mut self, lane: u64, extend: bool, cx: &mut Context<Self>) {
        self.with_model(cx, |d| d.select_track(lane, extend));
    }

    /// Sets a header's flags; a track gone since it was drawn (`None`
    /// everywhere) sends nothing.
    pub(crate) fn set_flags(
        &mut self,
        lane: u64,
        mute: Option<bool>,
        solo: Option<bool>,
        arm: Option<bool>,
        cx: &mut Context<Self>,
    ) {
        if mute.is_none() && solo.is_none() && arm.is_none() {
            return;
        }
        self.with_model(cx, |d| d.set_flags(lane, mute, solo, arm));
    }

    pub(crate) fn set_status(&mut self, text: &str, cx: &mut Context<Self>) {
        self.with_model(cx, |d| d.set_status(Some(text.to_owned())));
    }

    /// A status text newly drawn clears itself `STATUS_TIMEOUT` later
    /// unless another has replaced it (Swift `StatusLine`: the timer is
    /// keyed on the text, so a new message starts it again).
    fn follow_status(&mut self, status: &Option<String>, window: &Window, cx: &mut Context<Self>) {
        if *status == self.status_shown {
            return;
        }
        self.status_shown = status.clone();
        self._status_timer = status.clone().map(|text| {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(STATUS_TIMEOUT).await;
                this.update(cx, |this, cx| {
                    this.with_model(cx, |d| {
                        if d.status() == Some(text.as_str()) {
                            d.set_status(None);
                        }
                    })
                })
                .ok();
            })
        });
    }

    fn status_line(
        status: String,
        scope: &mut RegistryScope,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .px_3()
            .py_1()
            .bg(rgb(colors::BAR))
            .text_xs()
            .text_color(rgb(colors::DIM))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(status),
            )
            .child(
                transport_bar::button("dismiss-status", "Dismiss", true, colors::BAR, scope)
                    .h(px(20.))
                    .min_w(px(0.))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.with_model(cx, |d| d.set_status(None));
                    })),
            )
    }

    /// Starts the inline rename of track `lane` in its header: a field
    /// holding the name, focused, the name selected. Enter commits (the
    /// name trimmed; blank or unchanged sends nothing), Escape cancels,
    /// focus loss commits. A rename already open is committed first (as
    /// its field losing the focus would); then a track not shown does
    /// nothing.
    pub fn begin_rename(&mut self, lane: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.end_rename(true, window, cx);
        let Some(row) = self.lane(lane, cx) else {
            return;
        };
        let field = cx.new(|cx| TextInput::new("Name", cx).with_text(&row.name));
        let focus = field.focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(&field, window, |this, _, event, window, cx| match event {
                TextInputEvent::Confirm => this.end_rename(true, window, cx),
                TextInputEvent::Cancel => this.end_rename(false, window, cx),
                TextInputEvent::Tab | TextInputEvent::BackTab => {}
            }),
            cx.on_blur(&focus, window, |this, window, cx| {
                this.end_rename(true, window, cx)
            }),
        ];
        window.focus(&focus, cx);
        // The bare transport keys wait while the name is typed.
        self.with_model(cx, |d| d.begin_text_editing(RENAME_OWNER));
        self.track_headers.rename = Some(Rename {
            lane,
            field,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// Ends the open rename, renaming the track if `commit` and it is still
    /// there. The field goes, and the keyboard returns to the window (so
    /// the single-key shortcuts work again) unless focus moved elsewhere.
    fn end_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rename) = self.track_headers.rename.take() else {
            return;
        };
        let focus = rename.field.focus_handle(cx);
        let name = rename.field.read(cx).text().to_owned();
        self.with_model(cx, |d| d.end_text_editing(RENAME_OWNER));
        if commit && self.lane(rename.lane, cx).is_some() {
            let lane = rename.lane;
            self.with_model(cx, |d| d.rename_track(lane, &name));
        }
        if window.focused(cx).is_none_or(|f| f == focus) {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// Commits an open rename, as its field losing the focus would: Close,
    /// Save, Save As and Quit do this first, so the typed name is in the
    /// song they check or save.
    pub fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_rename(true, window, cx);
    }

    /// A rename whose track is no longer shown (Undo removed it, the
    /// lanes were fetched again without it) ends without a command.
    fn end_stale_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(lane) = self.renaming()
            && self.lane(lane, cx).is_none()
        {
            self.end_rename(false, window, cx);
        }
    }

    /// An invisible overlay that records the size of the area the
    /// arrangement and the bottom pane share; a new size draws again, so
    /// the pane fits the area it is in.
    fn measure_regions(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let regions = self.regions.clone();
        let weak = cx.entity().downgrade();
        canvas(
            move |bounds, _, cx| {
                if regions.get() != bounds.size {
                    regions.set(bounds.size);
                    let weak = weak.clone();
                    cx.defer(move |cx| {
                        weak.update(cx, |_, cx| cx.notify()).ok();
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
    }

    /// Escape closes an open menu list (the key goes no further). ⇧, and
    /// ⇧. (Rewind and Fast Forward) step a bar per key-down and
    /// auto-repeat while the window has the keyboard.
    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(forward) = held_key(&event.keystroke)
            && window
                .focused(cx)
                .is_none_or(|f| f == self.focus || &f == self.arrangement.focus_handle())
        {
            cx.stop_propagation();
            self.with_shuttle(cx, |sh, d| {
                let _ = sh.key_down(forward, d);
            });
            let doc = self.doc;
            self.app.update(cx, |s, _| s.poll(doc));
            return;
        }
        if event.keystroke.key != "escape" {
            return;
        }
        let headers = std::mem::take(&mut self.track_headers.add_menu_open);
        let more = std::mem::take(&mut self.more_open);
        let arrangement = self.arrangement.close_menus();
        let detail = std::mem::take(&mut self.detail_menu_open);
        let list = self.list_editor.close_menu();
        if self.menu_bar.as_mut().is_some_and(MenuBar::close)
            || headers
            || more
            || arrangement
            || detail
            || list
        {
            cx.stop_propagation();
            cx.notify();
        }
    }

    /// The held key is up: one landing if it stepped.
    fn key_up(&mut self, event: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let k = event.keystroke.key.as_str();
        if matches!(k, "," | "." | "<" | ">") {
            self.with_shuttle(cx, |sh, d| sh.key_up(d));
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

    /// `transport.stop` (S): stops and returns to where playing started;
    /// while stopped the status line says "Nothing is playing.".
    pub fn stop_and_return(
        &mut self,
        _: &StopAndReturn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_command(&RunCommand::new("transport.stop"), window, cx);
    }

    /// Starting or stopping changes the interval: wait the new one, not
    /// the rest of the old (a stopped wait would hold the readout still
    /// for up to 250 ms after Play).
    fn restart_polling(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self._poll = Self::start_polling(self.doc, window, cx);
    }

    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_rename(window, cx);
        self.save_then_close(false, window, cx);
    }

    fn save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_rename(window, cx);
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
    /// first (Save, Don't Save, Cancel) and answers false. An open rename
    /// is committed first.
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.commit_rename(window, cx);
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
        // First, so the rows built below are the ones in view.
        let rows = self.shown_rows(cx);
        self.clamp_arrangement(rows);
        let fallback = f32::from(window.viewport_size().height);
        let (
            title,
            dirty,
            alert,
            bar,
            model,
            front,
            lanes,
            selected,
            status,
            layout,
            arranged,
            tools,
            pane,
            bottom,
            list_closed,
            listed,
        ) = {
            let s = self.app.read(cx);
            let d = s.store.document(doc);
            (
                s.title(doc).unwrap_or_default(),
                s.is_dirty(doc),
                s.store.alert().cloned(),
                s.readout(doc).zip(d).map(|(readout, d)| BarState {
                    readout,
                    can_play: s.can_play(doc),
                    has_sequence: d.selected_seq().is_some(),
                    looping: d.looping(),
                    field_texts: TransportField::ALL
                        .into_iter()
                        .map(|f| (f, fields_model::field_text(d, f)))
                        .collect(),
                }),
                menus::menu_model(s.store.document(doc), &s.keymap),
                s.store.front().is_none_or(|f| f == doc),
                d.map(|d| d.lanes().to_vec()).unwrap_or_default(),
                d.map(DocumentModel::selected_lanes).unwrap_or_default(),
                d.and_then(|d| d.status().map(String::from)),
                s.transport_layout(),
                d.map(|d| {
                    let visible = self
                        .arrangement
                        .visible_rows(d.lanes().len(), f32::from(window.viewport_size().height));
                    ArrangementState::of(d, s.transport(doc), Instant::now(), visible)
                }),
                d.map(ToolbarState::of).unwrap_or_default(),
                d.map(DetailPaneState::of).unwrap_or_default(),
                d.map(|d| d.layout().views.current().bottom),
                // The selection and scroll first: the rows drawn follow them.
                d.map(|d| self.list_editor.prepare(d, fallback)),
                d.filter(|d| d.list_open())
                    .map(|d| ListState::of(d, &self.list_editor, fallback)),
            )
        };
        if let Some((editing, focused)) = list_closed {
            self.list_closed(editing, focused, window, cx);
        }
        let bottom_height = bottom.and_then(|b| detail_pane::bottom_height(self.regions.get(), &b));
        if bottom_height.is_none() {
            self.detail_menu_open = false;
        }
        let plan = Plan::new(&layout, f32::from(window.viewport_size().width));
        self.set_shown_fields(
            if bar.is_some() {
                plan.shown_fields()
            } else {
                vec![]
            },
            window,
            cx,
        );
        if plan.folded().is_empty() {
            self.more_open = false;
        }
        self.follow_status(&status, window, cx);
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

        let arranged = arranged.unwrap_or_else(ArrangementState::empty);
        self.arrangement.follow(&arranged);
        // The playhead moves every frame while the shown sequence plays;
        // stopped, it moves when a poll reads a locate.
        if arranged.playing {
            window.request_animation_frame();
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
            .on_key_up(cx.listener(Self::key_up))
            // A held Rewind or Fast Forward released anywhere else (the
            // button's own release comes first and ends the hold).
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.shuttle_release(false, cx)),
            )
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
                d.child(transport_bar::render(
                    &bar,
                    &plan,
                    &self.transport_fields,
                    self.more_open,
                    &mut root.container(),
                    window,
                    cx,
                ))
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(self.measure_regions(cx))
                    .child(arrangement::toolbar(
                        tools,
                        self.arrangement.division_open(),
                        &mut root.container(),
                        window,
                        cx,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .flex_row()
                            .on_scroll(cx.listener(|this, e, _, cx| this.arrangement_wheel(e, cx)))
                            .child(self.track_headers.render(
                                &HeadersState {
                                    lanes: &lanes,
                                    selected: &selected,
                                    vertical_offset: self.arrangement.vertical_offset(),
                                },
                                &mut root.container(),
                                window,
                                cx,
                            ))
                            .child(self.arrangement.render(
                                arranged,
                                &mut root.container(),
                                window,
                                cx,
                            )),
                    )
                    .when(bottom_height.is_some(), |d| d.child(detail_pane::divider()))
                    .child({
                        // A hidden pane stays mounted, recorded nowhere.
                        let scratch = SharedRegistry::default();
                        let mut hidden = RegistryScope::root(&scratch);
                        let scope = &mut if bottom_height.is_some() {
                            root.container()
                        } else {
                            hidden.container()
                        };
                        detail_pane::render(
                            &pane,
                            bottom_height,
                            self.detail_menu_open,
                            listed.as_ref().map(|l| (&self.list_editor, l)),
                            scope,
                            window,
                            cx,
                        )
                    }),
            )
            .when_some(status, |d, status| {
                d.child(Self::status_line(status, &mut root.container(), cx))
            })
    }
}

/// Rewind (false) or Fast Forward (true) for a held-key keystroke: ⇧, and
/// ⇧. with no other modifier (a layout may report the shifted character).
fn held_key(k: &Keystroke) -> Option<bool> {
    let m = &k.modifiers;
    if !m.shift || m.control || m.alt || m.platform || m.function {
        return None;
    }
    match k.key.as_str() {
        "," | "<" => Some(false),
        "." | ">" => Some(true),
        _ => None,
    }
}
