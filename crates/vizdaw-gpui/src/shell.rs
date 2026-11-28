//! The document window: one window per open document, with the menu bar
//! (off macOS), the transport bar, the pane host's
//! regions (the centre: the arrangement's tool bar and the track header
//! column beside the arrangement canvas, or the Mix view's mixer; the
//! left, bottom and right panes), and the status line.
//! The window holds the bar's shuttle (Rewind and Fast Forward held, and
//! their held keys) and its fields' edit.
//!
//! A document's Mixer window (Swift `MixerWindow`, Window › Mixer) is a
//! `DocumentWindow` too, of kind [`WindowKind::Mixer`]: titled "Mixer —
//! <title>", it draws the menu bar (off macOS), its own
//! transport bar without the document-only items (Main View, the pane
//! toggles), and the mixer's Window placement. It has its own focus, bar
//! fields, shuttle and keyboard token; it closes without asking, and it
//! closes itself when its document goes. Its transport is the document's
//! (the document window polls it).
//!
//! Either kind shows one modal sheet at a time over itself
//! ([`crate::sheet_host`]); alerts are prompts ([`crate::alerts`]).

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use core_control::bindings::Keymap;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent, Keystroke, MouseButton,
    ParentElement, PromptLevel, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, WindowBounds, WindowHandle, WindowOptions, canvas, div, px, rgb,
    size,
};
use vizdaw_app::alerts::{AlertItem, error_message};
use vizdaw_app::commands;
use vizdaw_app::editor::{MainView, PaneSide, PaneView, ViewLayouts};
use vizdaw_app::marker_list::{MarkerList, MarkerListRow};
use vizdaw_app::mixer::MixerPlacement;
use vizdaw_app::record::PopoverHost;
use vizdaw_app::strip_chart::ChartSlot;
use vizdaw_app::surface_relay::RelayStep;
use vizdaw_app::transport::Shuttle;
use vizdaw_app::transport_bar::TransportItem;
use vizdaw_app::transport_fields::{self as fields_model, TransportField};
use vizdaw_app::{DocId, DocumentModel, SaveAction, SaveChoice, WindowToken};
use vizdaw_ffi::snapshot::LaneRow;

use crate::app_state::{AppState, WindowKind};
use crate::arrangement::{self, Arrangement, ArrangementState, ToolbarState};
use crate::banners;
use crate::channel_strip_pane;
use crate::controls::MixerRef;
use crate::detail_pane::{self, DetailPaneState};
use crate::frame_stats::{self, FrameStats};
use crate::list_editor::{ListEditor, ListState};
use crate::menu_bar::MenuBar;
use crate::menus::{
    self, CloseWindow, FileCommand, MixerMenuItem, PlayStop, RunCommand, RunModifyTemplate, Save,
    SaveAs, StepKey, StopAndReturn,
};
use crate::mixer_view::{self, MixerView};
use crate::panes::{self, HostState, PaneDrag};
use crate::piano_roll::{PianoRoll, RollState};
use crate::record_popover::{PopoverState, RecordPopover};
use crate::registry::{RegistryScope, SharedRegistry};
use crate::strip_chart::{StripChart, StripChartState};
use crate::text_input::{TextInput, TextInputEvent};
use crate::track_headers::{self, HeaderInput, HeadersState, Rename, TrackHeaders};
use crate::transport_bar::{self, BarState, FaultsState, Plan, RecordBar, colors};
use crate::transport_fields::TransportFields;

/// How long a status text shows (Swift `StatusLine`).
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(6);
/// How often DSP Load reads the engine's load (Swift: 0.25 s).
pub const DSP_POLL: Duration = Duration::from_millis(250);
/// How many frames [`DocumentWindow::fit_log`] keeps.
const FIT_LOG_KEPT: usize = 32;
/// One frame's pane host area and the centre's bounds in it.
pub type FitFrame = (Bounds<gpui::Pixels>, Option<Bounds<gpui::Pixels>>);

pub struct DocumentWindow {
    app: Entity<AppState>,
    doc: DocId,
    token: WindowToken,
    /// A document's own window, or its Mixer window.
    kind: WindowKind,
    pub(crate) focus: FocusHandle,
    registry: SharedRegistry,
    /// The title last given to the platform window.
    title: String,
    /// A save-changes prompt is showing.
    prompting: bool,
    /// The sheet over the window, if one is up.
    pub(crate) sheet: crate::sheet_host::SheetHost,
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
    /// The record popover's punch fields (this window's host).
    pub(crate) record_popover: RecordPopover,
    /// The right pane's Plug-ins view's search.
    pub(crate) plugins_pane: crate::plugins_pane::PluginsPane,
    /// Rewind and Fast Forward held, and their held keys.
    shuttle: Shuttle,
    /// Winds a held button every `Shuttle::TICK_EVERY` until it ends.
    _hold: Option<Task<()>>,
    /// The transport bar's » menu is open.
    more_open: bool,
    /// Click's own menu, while open: the row of its open submenu at each
    /// depth.
    click_menu: Option<Vec<usize>>,
    /// Click's submenu in », while open: likewise.
    more_click: Option<Vec<usize>>,
    /// Reads the DSP load every [`DSP_POLL`] while DSP Load is on the bar.
    _dsp_poll: Option<Task<()>>,
    /// The step key last run whose key has not come up: a key-down for it
    /// before then is the key repeating.
    step_key_down: Option<SharedString>,
    /// The pane whose picker menu is open.
    pub(crate) pane_menu: Option<PaneSide>,
    /// A pane divider being dragged.
    pub(crate) pane_drag: Option<PaneDrag>,
    /// The main view and panes last seen (the frame left out): a change
    /// ends the track header's rename.
    views_seen: Option<ViewLayouts>,
    /// The List editor's table (its selection, scroll and cell edit).
    pub(crate) list_editor: ListEditor,
    /// The Piano Roll (its axes, scroll and bar menus).
    pub(crate) piano_roll: PianoRoll,
    /// The bottom pane's Strip Chart, and the right pane's Controllers
    /// chart.
    pub(crate) strip_chart: StripChart,
    pub(crate) controllers_chart: StripChart,
    /// The size of the pane host's area (under the transport bar, above
    /// the status line), at the last draw (zero before the first).
    regions: Rc<Cell<gpui::Size<gpui::Pixels>>>,
    /// Each frame's pane host area and the centre drawn in it, the last
    /// [`FIT_LOG_KEPT`] frames.
    fit_log: Rc<std::cell::RefCell<Vec<FitFrame>>>,
    /// The mixer's placements here: the Mix view's centre and the bottom
    /// pane's Mixer view, each while it shows.
    mix_view: Option<Placed>,
    pane_mixer: Option<Placed>,
    /// The left pane's Channel Strip, while it shows and a track is
    /// focused.
    strip_mixer: Option<Placed>,
    /// The Mixer window's placement.
    window_mixer: Option<Placed>,
    /// The frames' timing (`VIZDAW_FRAME_STATS`; `None` without it).
    frames: Option<Rc<RefCell<FrameStats>>>,
    /// The poll runs fast: a note-off or a drag's final edit waits on
    /// Busy.
    fast_poll: bool,
    _poll: Task<()>,
    _subscriptions: Vec<Subscription>,
}

/// A mixer placement shown in the window; dropped, it goes.
struct Placed {
    view: Entity<MixerView>,
    _observe: Subscription,
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
        let visible = cx.primary_display().map(|d| d.visible_bounds());
        let bounds = Bounds::centered(None, default_window_size(visible), cx);
        let state = app.clone();
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                cx.new(|cx| {
                    DocumentWindow::new(state, doc, token, WindowKind::Document, window, cx)
                })
            },
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
        let offer = !crate::control::channel_on(cx);
        app.update(cx, |s, _| {
            s.add_window(doc, token, handle, WindowKind::Document);
            s.store.focus_document(doc, token);
            // Recordings a crash left in a saved project are offered
            // (never under the control channel).
            if offer
                && let Some(d) = s.store.document_mut(doc)
                && d.path().is_some()
            {
                d.offer_recovery();
            }
        });
        handle
            .update(cx, |v, window, cx| {
                window.focus(&v.focus, cx);
                window.activate_window();
            })
            .ok();
        // The plug-in windows open when the project was saved open again,
        // once this window is registered.
        cx.defer(move |cx| crate::plugin_windows::reopen_saved(doc, cx));
        Ok(handle)
    }

    /// Window › Mixer (`window.mixer`, ⌥⌘M): opens `doc`'s Mixer window,
    /// or brings the open one to the front (never a second). Call it
    /// outside `AppState::update` and outside the Mixer window's own
    /// update. A failure is the alert "Window Failed".
    pub fn open_mixer(
        app: &Entity<AppState>,
        doc: DocId,
        cx: &mut App,
    ) -> Result<WindowHandle<DocumentWindow>, String> {
        if let Some(handle) = app
            .read(cx)
            .mixer_window_of(doc)
            .and_then(|w| w.document_window())
        {
            handle
                .update(cx, |_, window, _| window.activate_window())
                .map_err(|e| e.to_string())?;
            return Ok(handle);
        }
        if app.read(cx).store.document(doc).is_none() {
            return Err("the document is closed".into());
        }
        let token = app.update(cx, |s, _| s.new_token());
        let visible = cx.primary_display().map(|d| d.visible_bounds());
        let bounds = Bounds::centered(None, mixer_window_size(visible), cx);
        let state = app.clone();
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            move |window, cx| {
                cx.new(|cx| DocumentWindow::new(state, doc, token, WindowKind::Mixer, window, cx))
            },
        );
        let handle = match opened {
            Ok(h) => h,
            Err(e) => {
                app.update(cx, |s, cx| {
                    s.store
                        .post_alert(AlertItem::new("Window Failed", e.to_string()));
                    cx.notify();
                });
                return Err(e.to_string());
            }
        };
        app.update(cx, |s, cx| {
            s.add_window(doc, token, handle, WindowKind::Mixer);
            s.store.focus_document(doc, token);
            cx.notify();
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
        kind: WindowKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = vec![
            cx.observe_in(&app, window, move |this, app, window, cx| {
                if kind == WindowKind::Mixer {
                    // Its document went (closed, quit): the Mixer window
                    // goes too.
                    if app.read(cx).store.document(this.doc).is_none() {
                        window.remove_window();
                        return;
                    }
                } else {
                    this.end_stale_rename(window, cx);
                    this.follow_views(window, cx);
                }
                cx.notify();
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    let (doc, token) = (this.doc, this.token);
                    this.app
                        .update(cx, |s, _| s.store.focus_document(doc, token));
                    crate::sheets::relink::window_activated(cx);
                } else {
                    crate::sheets::relink::window_deactivated(cx);
                    // The window lost the keyboard: a field edit cancels, a
                    // hold ends where it got to, a held key lands.
                    this.cancel_field(window, cx);
                    this.close_field_popover(window, cx);
                    // The record popover closes (Swift's transient
                    // popover), dropping its drafts.
                    this.escape_record_popover(window, cx);
                    this.end_shuttle(cx);
                    // A step key let go elsewhere still counts as up.
                    this.step_key_down = None;
                    this.cancel_ruler_drag(cx);
                    this.cancel_pane_drag(cx);
                    // A press whose release may never come is dropped; a
                    // released drag whose final edit waits on Busy is left
                    // to the timer's tick (Swift never cancels here).
                    if this.arrangement.pressing() {
                        this.cancel_block_drag(cx);
                    }
                    // A held note or key stops sounding.
                    this.roll_deactivated(cx);
                    // A Strip Chart stroke goes without an undo step.
                    this.charts_deactivated(cx);
                }
            }),
        ];
        // A keyboard layout change redraws the window, whose key context
        // says whether the layout is a US one.
        let me = cx.entity().downgrade();
        subscriptions.push(cx.on_keyboard_layout_change(move |cx| {
            if let Some(me) = me.upgrade() {
                me.update(cx, |_, cx| cx.notify());
            }
        }));
        let state = app.clone();
        subscriptions.push(cx.on_release(move |this: &mut Self, cx| {
            this._hold = None;
            // The MIDI Learn Maps sheet goes with it: its polling stops.
            let maps = this.sheet_label().as_deref() == Some(crate::sheets::learn_maps::TITLE);
            // A transport field's edit ends with its window: the keys come
            // back, and a bar punch draft does not stay to refuse Record.
            let field = this.transport_fields.editing();
            let shuttle = &mut this.shuttle;
            let roll = &mut this.piano_roll;
            let (pane_chart, controllers_chart) =
                (&mut this.strip_chart, &mut this.controllers_chart);
            // A sheet's field edit ends with its window: the document's
            // keys come back.
            let sheet_owner = this
                .sheet
                .editing()
                .then(|| crate::sheet_host::sheet_owner(token));
            state.update(cx, |s, cx| {
                s.store.with_document(doc, |d, _| {
                    if let Some(owner) = &sheet_owner {
                        d.end_text_editing(owner);
                    }
                    if maps {
                        d.learn.close_maps();
                    }
                    if field.is_some() {
                        d.end_text_editing(vizdaw_app::transport_fields::TEXT_OWNER);
                        d.bar_punch_ended();
                    }
                    shuttle.cancel(d);
                    shuttle.key_up(d);
                    // Its record popover goes with it: a draft left open
                    // would refuse Record with nothing on screen.
                    let host = match kind {
                        WindowKind::Document => Some(PopoverHost::Document),
                        WindowKind::Mixer => Some(PopoverHost::MixerWindow),
                        WindowKind::Plugin
                        | WindowKind::Other
                        | WindowKind::PluginManager
                        | WindowKind::ModifyPanel => None,
                    };
                    if host.is_some_and(|h| d.record().shows_popover_in(h)) {
                        d.set_record_popover(false);
                    }
                    if kind == WindowKind::Document {
                        DocumentWindow::roll_closing(roll, d);
                        DocumentWindow::charts_closing(pane_chart, controllers_chart, d);
                    }
                });
                match kind {
                    // Its Mixer window sees the document gone and closes.
                    WindowKind::Document => s.close_document(doc),
                    // Only the window goes; its placement released its
                    // own drags as it went.
                    WindowKind::Mixer
                    | WindowKind::Plugin
                    | WindowKind::Other
                    | WindowKind::PluginManager
                    | WindowKind::ModifyPanel => s.remove_window(token),
                }
                cx.notify();
            });
        }));
        let list_editor = ListEditor::new(cx);
        let record_popover = RecordPopover::new(window, cx);
        let plugins_pane = crate::plugins_pane::PluginsPane::new(window, cx);
        let views_seen = app.read(cx).store.document(doc).map(seen_views);
        let weak = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            weak.update(cx, |this, cx| this.should_close(window, cx))
                .unwrap_or(true)
        });
        DocumentWindow {
            // The Mixer window's transport is the document's: its own
            // window polls it.
            _poll: match kind {
                WindowKind::Document => {
                    Self::start_surface_tick(doc, window, cx);
                    Self::start_polling(doc, window, cx)
                }
                _ => Task::ready(()),
            },
            app,
            doc,
            token,
            kind,
            focus: cx.focus_handle(),
            registry: SharedRegistry::default(),
            title: String::new(),
            prompting: false,
            sheet: crate::sheet_host::SheetHost::new(cx),
            menu_bar: cfg!(not(target_os = "macos"))
                .then(|| MenuBar::new(menus::menu_model(None, &Default::default()))),
            track_headers: TrackHeaders::default(),
            arrangement: Arrangement::new(cx),
            status_shown: None,
            _status_timer: None,
            transport_fields: TransportFields::new(cx),
            record_popover,
            plugins_pane,
            shuttle: Shuttle::new(),
            _hold: None,
            more_open: false,
            click_menu: None,
            more_click: None,
            _dsp_poll: None,
            step_key_down: None,
            pane_menu: None,
            pane_drag: None,
            views_seen,
            list_editor,
            piano_roll: PianoRoll::new(cx),
            strip_chart: StripChart::new(ChartSlot::Pane, cx),
            controllers_chart: StripChart::new(ChartSlot::Controllers, cx),
            regions: Rc::default(),
            fit_log: Rc::default(),
            mix_view: None,
            pane_mixer: None,
            strip_mixer: None,
            window_mixer: None,
            frames: frame_stats::enabled().then(Rc::default),
            fast_poll: false,
            _subscriptions: subscriptions,
        }
    }

    /// The surface tick (slice 9d review I1): every [`FAST_POLL`] while a
    /// surface is set up, the document's surface events are taken and run
    /// in this window, as Swift's 60 Hz timer did, without speeding up the
    /// transport poll. Ends with the window.
    ///
    /// [`FAST_POLL`]: crate::app_state::FAST_POLL
    fn start_surface_tick(doc: DocId, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let interval = this
                    .update(cx, |this, cx| this.app.read(cx).surface_interval(doc))
                    .ok()
                    .flatten();
                let Some(interval) = interval else { break };
                cx.background_executor().timer(interval).await;
                let ticked = this.update_in(cx, |this, window, cx| {
                    let steps = this.app.update(cx, |s, cx| {
                        let (steps, idled) = s.surface_tick(doc);
                        if idled {
                            cx.notify();
                        }
                        steps
                    });
                    if !steps.is_empty() {
                        this.run_surface_steps(steps, window, cx);
                    }
                });
                if ticked.is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Polls the transport every `poll_interval()` (read again each tick:
    /// fast while playing, or while a note-off or a drag's final edit
    /// waits on Busy), until the document closes.
    fn start_polling(doc: DocId, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                let interval = this
                    .update(cx, |this, cx| this.app.read(cx).poll_interval(doc))
                    .ok()
                    .flatten();
                let Some(interval) = interval else { break };
                cx.background_executor().timer(interval).await;
                let polled = this.update_in(cx, |this, window, cx| {
                    let steps = this.app.update(cx, |s, cx| {
                        if s.poll(doc) {
                            cx.notify();
                        }
                        s.store
                            .document_mut(doc)
                            .map(DocumentModel::take_surface_steps)
                            .unwrap_or_default()
                    });
                    // What this document's session took from the surfaces
                    // runs here, in its own window, whichever is in front.
                    this.run_surface_steps(steps, window, cx);
                    // A drag's Exact Mode revert that met Busy succeeded on
                    // this tick: its panel opens now.
                    this.present_modify_if_wanted(cx);
                    // A job the poll ended may close a project the store
                    // made (or open one): outside this window's update.
                    if this.app.read(cx).store.settling() {
                        cx.defer(crate::control::files::settle);
                    }
                });
                if polled.is_err() {
                    break;
                }
            }
        })
    }

    /// Applies the surfaces' steps to this window's document, in order
    /// (slice 9d; `M/SurfaceRelay.swift:66-89`). A device's command runs
    /// as its menu item, through this window; with `bypass` the document
    /// takes it as a surface button, not a key (ruling T9-1), until the
    /// dispatched action ran. A programmatic one runs as `command.run`
    /// does and opens no dialog. The steps after a command wait for it.
    pub fn run_surface_steps(
        &mut self,
        steps: Vec<RelayStep>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let doc = self.doc;
        let mut steps = steps.into_iter();
        while let Some(step) = steps.next() {
            let command = self.app.update(cx, |s, cx| {
                cx.notify();
                s.store
                    .with_document(doc, |d, _| d.apply_surface_step(step))
                    .flatten()
            });
            let Some(c) = command else { continue };
            let rest: Vec<RelayStep> = steps.collect();
            if c.programmatic {
                // Outside this window's update, as the channel runs it
                // (the command may close or update the window); as a
                // surface button too (`M/SurfaceRelay.swift:70`).
                let app = self.app.clone();
                let own = window.window_handle().downcast::<DocumentWindow>();
                cx.defer(move |cx| {
                    set_surface_acting(&app, doc, true, cx);
                    crate::control::run_quietly(&c.id, doc, cx);
                    set_surface_acting(&app, doc, false, cx);
                    if let Some(own) = own {
                        let _ = own.update(cx, |this, window, cx| {
                            this.run_surface_steps(rest, window, cx);
                        });
                    }
                });
            } else {
                if c.bypass {
                    set_surface_acting(&self.app, doc, true, cx);
                }
                window.dispatch_action(menus::action_for(&c.id), cx);
                // Queued after the dispatch, which runs deferred too.
                cx.defer_in(window, move |this, window, cx| {
                    if c.bypass {
                        set_surface_acting(&this.app, doc, false, cx);
                    }
                    this.run_surface_steps(rest, window, cx);
                });
            }
            return;
        }
    }

    pub fn doc(&self) -> DocId {
        self.doc
    }

    pub fn token(&self) -> WindowToken {
        self.token
    }

    /// A document's own window, or its Mixer window.
    pub fn kind(&self) -> WindowKind {
        self.kind
    }

    /// The frames timed since the last call, as `state.get windows`
    /// reports them; `None` unless `VIZDAW_FRAME_STATS` is set.
    #[doc(hidden)]
    pub fn take_frame_stats(&self, now: Instant) -> Option<serde_json::Value> {
        self.frames.as_ref().map(|f| f.borrow_mut().take_json(now))
    }

    pub fn app(&self) -> &Entity<AppState> {
        &self.app
    }

    /// The mixer placement `p` (the Mix view's centre, the bottom pane's
    /// Mixer view, the left pane's Channel Strip, the Mixer window), while
    /// it shows.
    pub fn mixer_view(&self, p: MixerPlacement) -> Option<Entity<MixerView>> {
        match p {
            MixerPlacement::View => self.mix_view.as_ref(),
            MixerPlacement::Pane => self.pane_mixer.as_ref(),
            MixerPlacement::Window => self.window_mixer.as_ref(),
            MixerPlacement::Strip => self.strip_mixer.as_ref(),
        }
        .map(|p| p.view.clone())
    }

    /// Makes placement `p` when it shows (`wanted`), drops it when it does
    /// not (its drags end; the last placement ends every touch).
    fn place_mixer(
        &mut self,
        p: MixerPlacement,
        wanted: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<MixerView>> {
        let slot = match p {
            MixerPlacement::View => &mut self.mix_view,
            MixerPlacement::Window => &mut self.window_mixer,
            MixerPlacement::Pane => &mut self.pane_mixer,
            MixerPlacement::Strip => &mut self.strip_mixer,
        };
        if !wanted {
            *slot = None;
            return None;
        }
        if slot.is_none() {
            let mixer = MixerRef::new(self.app.clone(), self.doc, p);
            let view = cx.new(|cx| MixerView::new(mixer, window, cx));
            let observe = cx.observe(&view, |_, _, cx| cx.notify());
            *slot = Some(Placed {
                view,
                _observe: observe,
            });
        }
        slot.as_ref().map(|p| p.view.clone())
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
        // View › Customize Transport Bar…: the sheet on this window (a
        // document's or the mixer's); with no window open the menu has
        // nothing to run it on, so it does nothing.
        if action.id.as_ref() == "ui.customize_transport" {
            crate::sheets::customize_transport::show(self, window, cx);
            return;
        }
        let doc = self.doc;
        let (result, lane) = self.app.update(cx, |s, cx| {
            cx.notify();
            // The nudges read the app's Settings › Editing steps.
            let nudge = s.editing.nudge().clone();
            s.store
                .with_document(doc, |d, _| {
                    d.set_nudge_settings(nudge);
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
            // The Mixer window shows no track headers.
            (Some(Err(e)), Some(Some(lane)))
                if e.dialog.as_deref() == Some("Rename Track")
                    && self.kind == WindowKind::Document =>
            {
                self.begin_rename(lane, window, cx);
            }
            (Some(Ok(ran)), _) => {
                // Window › Mixer (⌥⌘M): after this update, as it may open
                // a window or bring this one to the front.
                if ran.open_mixer_window {
                    let app = self.app.clone();
                    cx.defer(move |cx| {
                        let _ = DocumentWindow::open_mixer(&app, doc, cx);
                    });
                }
                // A Modify panel task: the document's panel, after this
                // update, as the Mixer window.
                if ran.open_modify_panel {
                    cx.defer(move |cx| crate::modify_panel::open_modify_panel(doc, cx));
                }
                if let Some(focus) = ran.focus {
                    self.focus_field(focus.field(), window, cx);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /// A step entry key: runs its command, except that a held key repeats
    /// only rest, tie and Delete Last Step (Swift `KeyDispatch.repeating`).
    /// A Step menu item is not a key: it always runs and leaves the held
    /// key as it was.
    pub fn step_key(&mut self, action: &StepKey, window: &mut Window, cx: &mut Context<Self>) {
        if !action.from_menu {
            if self.step_key_down.as_ref() == Some(&action.id)
                && !vizdaw_app::step_entry::repeats(&action.id)
            {
                return;
            }
            self.step_key_down = Some(action.id.clone());
        }
        self.run_command(&RunCommand::new(action.id.clone()), window, cx);
        // A step moves the transport: the readouts read it now.
        self.after_transport(window, cx);
    }

    /// Modify › Templates › ‹name›: runs that template on this window's
    /// document; a refusal is the status line.
    pub fn run_modify_template(
        &mut self,
        action: &RunModifyTemplate,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let doc = self.doc;
        self.app.update(cx, |s, cx| {
            cx.notify();
            s.store.with_document(doc, |d, _| {
                if let Err(status) = d.run_modify_template(&action.name) {
                    d.set_status(Some(status));
                }
            })
        });
        cx.notify();
    }

    /// A Mixer menu item, on this window's document and its selected
    /// strips; a refusal is the mixer's alert.
    pub fn run_mixer_item(
        &mut self,
        action: &MixerMenuItem,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let doc = self.doc;
        self.app.update(cx, |s, cx| {
            cx.notify();
            s.store
                .with_document(doc, |d, _| menus::run_mixer_item(action.op, d));
        });
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

    /// A Markers row's click: the transport moves there (a refusal is the
    /// status line's) and the readouts read it now.
    pub(crate) fn go_to_marker(&mut self, row: &MarkerListRow, cx: &mut Context<Self>) {
        let doc = self.doc;
        self.with_model(cx, |d| MarkerList::go(row, d));
        self.app.update(cx, |s, _| s.poll(doc));
    }

    /// The bar's Loop button; a refusal is the alert "Loop Failed".
    pub(crate) fn toggle_loop(&mut self, cx: &mut Context<Self>) {
        self.with_model(cx, DocumentModel::toggle_loop);
    }

    /// The bar's Record (Swift `pressRecord`): the model's order, a
    /// refusal in the status line; the poll follows the pass.
    pub(crate) fn press_record(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.with_model(cx, DocumentModel::press_record);
        self.after_transport(window, cx);
    }

    /// Replace/Overdub, on the bar or in »: `transport.overdub`.
    pub(crate) fn press_record_mode(&mut self, cx: &mut Context<Self>) {
        self.with_model(cx, DocumentModel::press_record_mode);
    }

    /// Count-in, on the bar or in »: off, or back to its last bars.
    pub(crate) fn toggle_count_in(&mut self, cx: &mut Context<Self>) {
        self.with_model(cx, |d| d.record_mut().toggle_count_in());
    }

    pub(crate) fn toggle_more(&mut self, cx: &mut Context<Self>) {
        self.more_open = !self.more_open;
        self.more_click = None;
        self.click_menu = None;
        cx.notify();
    }

    pub(crate) fn close_more(&mut self, cx: &mut Context<Self>) {
        self.more_open = false;
        self.more_click = None;
        cx.notify();
    }

    /// Click's ⌄: its menu opens or closes. Under the channel it would
    /// track modally, so the status line names the commands instead.
    pub(crate) fn toggle_click_menu(&mut self, cx: &mut Context<Self>) {
        if self.click_menu.is_some() {
            self.close_bar_menus(cx);
        } else {
            self.open_click_menu(cx);
        }
    }

    /// A right-click on Click, or its ⌄: its menu opens (its entries
    /// disabled while the session has not given the settings).
    pub(crate) fn open_click_menu(&mut self, cx: &mut Context<Self>) {
        if crate::control::is_channel_click() {
            self.set_status(vizdaw_app::bar_items::CLICK_ON_CHANNEL, cx);
            return;
        }
        self.more_open = false;
        self.more_click = None;
        self.click_menu = Some(vec![]);
        cx.notify();
    }

    /// Click's menus and » close (an entry was chosen, or a press outside).
    pub(crate) fn close_bar_menus(&mut self, cx: &mut Context<Self>) {
        self.click_menu = None;
        self.close_more(cx);
    }

    /// Click's entry in »: its submenu opens or closes.
    pub(crate) fn toggle_more_click(&mut self, cx: &mut Context<Self>) {
        self.more_click = match self.more_click {
            Some(_) => None,
            None => Some(vec![]),
        };
        cx.notify();
    }

    /// A submenu row at `depth` of `owner`'s menu: it opens (or, open,
    /// closes), and whatever was open below it closes. In » the lists
    /// start at depth 1 (the » menu itself is depth 0).
    pub(crate) fn open_bar_submenu(
        &mut self,
        owner: transport_bar::MenuOwner,
        depth: usize,
        row: usize,
        cx: &mut Context<Self>,
    ) {
        let (path, depth) = match owner {
            transport_bar::MenuOwner::Click => (&mut self.click_menu, depth),
            transport_bar::MenuOwner::More => (&mut self.more_click, depth.saturating_sub(1)),
        };
        let Some(path) = path else { return };
        let was = path.get(depth).copied();
        path.truncate(depth);
        if was != Some(row) {
            path.push(row);
        }
        cx.notify();
    }

    /// A Click entry, on the bar or in »: a refusal in the status line.
    pub(crate) fn run_bar_action(
        &mut self,
        action: vizdaw_app::bar_items::BarAction,
        cx: &mut Context<Self>,
    ) {
        self.with_model(cx, |d| vizdaw_app::bar_items::run(action, d));
    }

    /// A command button, on the bar or in » (Swift `CommandButtonAction`):
    /// what its menu item runs, a dialog command's dialog too; when it
    /// cannot run, the status line says so.
    pub fn press_command(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let runs = self
            .with_model(cx, |d| vizdaw_app::bar_items::press_command(id, d))
            .unwrap_or(false);
        if runs {
            window.dispatch_action(menus::action_for(id), cx);
        }
    }

    /// DSP Load reads the engine's load through the mixer every
    /// [`DSP_POLL`] while it is on the bar (`shown`), and not otherwise.
    fn follow_dsp(&mut self, shown: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !shown {
            self._dsp_poll = None;
            return;
        }
        if self._dsp_poll.is_some() {
            return;
        }
        let doc = self.doc;
        self._dsp_poll = Some(cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(DSP_POLL).await;
                let now = cx.background_executor().now();
                let polled = this.update(cx, |this, cx| {
                    let load = |d: &DocumentModel| d.mixer().map(|m| m.meters().dsp_load);
                    let moved = this.app.update(cx, |s, _| {
                        s.store
                            .with_document(doc, |d, _| {
                                let before = load(d).map(vizdaw_app::bar_items::dsp_text);
                                d.with_mixer(|m, d| m.poll_levels(d, now));
                                load(d).map(vizdaw_app::bar_items::dsp_text) != before
                            })
                            .unwrap_or(false)
                    });
                    if moved {
                        cx.notify();
                    }
                });
                if polled.is_err() {
                    break;
                }
            }
        }));
    }

    /// The Faults button or its » entry: the popover opens or closes.
    pub(crate) fn toggle_faults(&mut self, cx: &mut Context<Self>) {
        self.with_model(cx, DocumentModel::toggle_faults);
    }

    pub(crate) fn close_faults(&mut self, cx: &mut Context<Self>) {
        self.with_model(cx, |d| d.set_shows_faults(false));
    }

    /// Esc: the Faults popover closes; true when it was open.
    pub(crate) fn escape_faults(&mut self, cx: &mut Context<Self>) -> bool {
        let open = self.read_model(cx, DocumentModel::shows_faults) == Some(true);
        if open {
            self.close_faults(cx);
        }
        open
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

    /// The document asked for its Select and Modify panel (an Exact Mode
    /// drag, at once or on the tick after a Busy revert): it opens after
    /// this update.
    pub(crate) fn present_modify_if_wanted(&mut self, cx: &mut Context<Self>) {
        let doc = self.doc;
        let wanted = self.app.update(cx, |s, _| {
            s.store
                .document_mut(doc)
                .is_some_and(DocumentModel::take_modify_present)
        });
        if wanted {
            cx.defer(move |cx| crate::modify_panel::open_modify_panel(doc, cx));
        }
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

    /// Escape: closes a mixer strip's open input picker, instrument menu
    /// or plug-in browser in this window.
    fn close_strip_input_pickers(&mut self, cx: &mut Context<Self>) -> bool {
        let views: Vec<_> = [
            &self.mix_view,
            &self.pane_mixer,
            &self.window_mixer,
            &self.strip_mixer,
        ]
        .into_iter()
        .flatten()
        .map(|p| p.view.clone())
        .collect();
        let mut closed = false;
        for v in views {
            closed |= v.update(cx, |v, cx| v.escape(cx));
        }
        closed
    }

    /// An input picker opens: the document reads its input again (the
    /// sources off the UI thread).
    pub(crate) fn refresh_inputs(&mut self, cx: &mut Context<Self>) {
        let doc = self.doc;
        self.app.update(cx, |s, cx| {
            s.store.refresh_inputs(doc);
            cx.notify();
        });
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
            .bg(rgb(colors::bar()))
            .text_xs()
            .text_color(rgb(colors::dim()))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .child(status),
            )
            .child(
                transport_bar::button("dismiss-status", "Dismiss", true, colors::bar(), scope)
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

    /// Commits an open rename (a track header's, a mixer strip's), as its
    /// field losing the focus would: Close,
    /// Save, Save As and Quit do this first, so the typed name is in the
    /// song they check or save.
    pub fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_rename(true, window, cx);
        // A strip's rename in the mixer too.
        for placed in [
            &self.mix_view,
            &self.pane_mixer,
            &self.strip_mixer,
            &self.window_mixer,
        ]
        .into_iter()
        .flatten()
        {
            placed.view.update(cx, |v, cx| v.end_rename(true, cx));
        }
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

    /// The main view or a pane changed (a menu item, a key, the channel,
    /// the transport bar, a picker): the track header's rename is
    /// committed (Swift ends the text edits of what the panes and the
    /// centre show first), an open picker closes, and an arrangement the
    /// Mix view now hides keeps no press, menu or keyboard.
    fn follow_views(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = self.read_model(cx, seen_views);
        if now == self.views_seen {
            return;
        }
        let was = std::mem::replace(&mut self.views_seen, now.clone());
        let (Some(was), Some(now)) = (was, now) else {
            return;
        };
        self.end_rename(true, window, cx);
        self.pane_menu = None;
        // The dragged pane changed under the drag (the main view went, or
        // its key, the channel or a picker set it): the drag was seeded
        // from what is no longer there, so it ends, setting nothing.
        if let Some(drag) = self.pane_drag
            && (now.main != was.main
                || now.current().pane(drag.side) != was.current().pane(drag.side))
        {
            self.cancel_pane_drag(cx);
        }
        if now.main == MainView::Mix && was.main != MainView::Mix {
            self.arrangement_hidden(window, cx);
        }
        cx.notify();
    }

    /// The Mix view hides the arrangement: its drags end, its menus and the
    /// track headers' close, and its keyboard returns to the window.
    fn arrangement_hidden(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_ruler_drag(cx);
        if self.arrangement.pressing() {
            self.cancel_block_drag(cx);
        }
        self.arrangement.close_menus();
        self.track_headers.add_menu_open = false;
        self.track_headers.input_picker = None;
        if self.arrangement.focus_handle().is_focused(window) {
            window.focus(&self.focus, cx);
        }
    }

    /// The last frames' pane host areas and the centre laid out in each
    /// (`None` when the centre was not drawn), oldest first.
    pub fn fit_log(&self) -> Vec<FitFrame> {
        self.fit_log.borrow().clone()
    }

    /// The pane host's area, as last measured: (width, height).
    pub(crate) fn regions_area(&self) -> (f64, f64) {
        let s = self.regions.get();
        (
            f64::from(f32::from(s.width)),
            f64::from(f32::from(s.height)),
        )
    }

    /// An invisible overlay that records the size of the pane host's area;
    /// a new size draws again, so the regions fit the area they are in.
    fn measure_regions(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let regions = self.regions.clone();
        let weak = cx.entity().downgrade();
        let log = self.fit_log.clone();
        canvas(
            move |bounds, window, cx| {
                let mut log = log.borrow_mut();
                log.push((bounds, None));
                if log.len() > FIT_LOG_KEPT {
                    log.remove(0);
                }
                drop(log);
                if regions.get() != bounds.size {
                    regions.set(bounds.size);
                    window.request_animation_frame();
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
        let held = held_key(&event.keystroke, self.app.read(cx).keys.keymap());
        if let Some(forward) = held
            && window.focused(cx).is_none_or(|f| {
                f == self.focus
                    || &f == self.arrangement.focus_handle()
                    || &f == self.piano_roll.focus_handle()
            })
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
        if self.escape_menus(window, cx) {
            cx.stop_propagation();
        }
    }

    /// Escape's first work: a key held on the roll's keyboard strip is
    /// let go, else every open menu list and popover closes. True when
    /// there was something to do (the key goes no further; a waiting MIDI
    /// learn is cancelled only by the next Esc).
    pub(crate) fn escape_menus(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.release_roll_key(cx) {
            return true;
        }
        let headers = std::mem::take(&mut self.track_headers.add_menu_open)
            | self.track_headers.input_picker.take().is_some()
            | self.close_strip_input_pickers(cx);
        let more = std::mem::take(&mut self.more_open);
        self.more_click = None;
        let click_menu = self.click_menu.take().is_some();
        let field_popover = self.close_field_popover(window, cx);
        let faults = self.escape_faults(cx);
        let record = self.escape_record_popover(window, cx);
        let arrangement = self.arrangement.close_menus();
        let detail = self.pane_menu.take().is_some();
        let list = self.list_editor.close_menu();
        let roll = self.piano_roll.close_menu();
        let charts = self.strip_chart.close_menu() | self.controllers_chart.close_menu();
        if self.menu_bar.as_mut().is_some_and(MenuBar::close)
            || headers
            || more
            || click_menu
            || field_popover
            || faults
            || record
            || arrangement
            || detail
            || list
            || roll
            || charts
        {
            cx.notify();
            return true;
        }
        false
    }

    /// The held key is up: one landing if it stepped.
    fn key_up(&mut self, _: &KeyUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.step_key_down = None;
        // Any key up ends a hold (a landing only if a key step happened):
        // the held keys are whatever the keymap says.
        self.with_shuttle(cx, |sh, d| sh.key_up(d));
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
        if self.kind == WindowKind::Mixer {
            // The document's own window polls the transport.
            let own = self
                .app
                .read(cx)
                .window_of(self.doc)
                .and_then(|w| w.document_window());
            if let Some(own) = own {
                own.update(cx, |v, window, cx| v.restart_polling(window, cx))
                    .ok();
            }
            return;
        }
        self._poll = Self::start_polling(self.doc, window, cx);
    }

    pub(crate) fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_rename(window, cx);
        match self.save_choice(false, cx) {
            SaveChoice::Save => self.save_then_close(false, window, cx),
            SaveChoice::Panel => self.prompt_save_as(false, window, cx),
            SaveChoice::Refused(_) => {}
        }
    }

    pub(crate) fn save_as(&mut self, _: &SaveAs, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_rename(window, cx);
        if self.save_choice(true, cx) == SaveChoice::Panel {
            self.prompt_save_as(false, window, cx);
        }
    }

    /// What Save (or Save As) does now; a refusal (recording, a held
    /// merge) shows in the status line and nothing opens.
    fn save_choice(&mut self, force_as: bool, cx: &mut Context<Self>) -> SaveChoice {
        let doc = self.doc;
        self.app.update(cx, |s, cx| {
            cx.notify();
            s.save_choice(doc, force_as)
        })
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

    /// Swift's save panel (row 4); a name typed without `.vizdaw` gets it,
    /// and the app asks before replacing a project there.
    fn prompt_save_as(&mut self, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        let doc = self.doc;
        let place = {
            let s = self.app.read(cx);
            crate::file_panels::PanelPlace {
                directory: Some(s.save_directory(doc)),
                name: s.save_title(doc),
                ext: String::new(),
            }
        };
        let chosen = crate::file_panels::choose_save(
            &vizdaw_app::panels::SAVE_PROJECT,
            place,
            window.window_handle(),
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = chosen.await;
            this.update_in(cx, |this, window, cx| match answer {
                Ok(Some(path)) => this.finish_save_as(doc, &path, close, window, cx),
                Ok(None) => {}
                Err(e) => this.app.update(cx, |s, cx| {
                    cx.notify();
                    s.store.post_alert(AlertItem::new("Save Failed", e));
                }),
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

    /// True when the window may close now; with unsaved changes it asks
    /// first (Save, Don't Save, Cancel) and answers false. An open rename
    /// is committed first.
    fn should_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        self.commit_rename(window, cx);
        // The Mixer window closes without asking: the document stays.
        if self.kind == WindowKind::Mixer {
            return true;
        }
        let doc = self.doc;
        let (dirty, title, pending) = {
            let s = self.app.read(cx);
            let d = s.store.document(doc);
            (
                s.has_unsaved_changes(doc),
                d.map(|d| d.snapshot().title.clone()),
                d.is_some_and(DocumentModel::save_after_pass_pending),
            )
        };
        if !dirty {
            return true;
        }
        if self.prompting {
            return false;
        }
        self.prompting = true;
        // A Save made while recording is owed: no Don't Save (it would
        // drop the save the user asked for).
        let prompt = vizdaw_app::close_prompts::close_prompt(&title.unwrap_or_default(), pending);
        let buttons = prompt.buttons.clone();
        let answer = crate::alerts::ask(
            window.window_handle(),
            crate::sheet_host::Ask::with_labels(
                PromptLevel::Warning,
                &prompt.message,
                Some(prompt.detail),
                &prompt.buttons,
            ),
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            let answer = answer.await;
            this.update_in(cx, |this, window, cx| {
                this.prompting = false;
                use vizdaw_app::close_prompts::{DONT_SAVE, SAVE};
                match answer.ok().and_then(|i| buttons.get(i).copied()) {
                    Some(SAVE) => this.save_then_close(true, window, cx),
                    Some(DONT_SAVE) => {
                        // A close never saves a pass nobody asked to save.
                        let doc = this.doc;
                        this.app.update(cx, |s, _| s.cancel_save_after_pass(doc));
                        window.remove_window();
                    }
                    _ => {}
                }
            })
            .ok();
        })
        .detach();
        false
    }

    /// A sheet opened: a held Rewind or Fast Forward ends (its key-up
    /// goes to the sheet).
    pub(crate) fn sheet_opened(&mut self, cx: &mut Context<Self>) {
        self.end_shuttle(cx);
    }

    /// The window's content with its sheet, if one is up, over it. The
    /// sheet is outside the content's key context: no window binding or
    /// key handler sees a key typed into it.
    fn with_sheet(
        &mut self,
        content: AnyElement,
        root: &mut RegistryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sheet = self.render_sheet(&mut root.container(), window, cx);
        div()
            .size_full()
            .relative()
            .child(content)
            .children(sheet)
            .into_any_element()
    }
}

/// What the regions draw of the document, read before drawing.
struct RegionParts<'a> {
    tools: ToolbarState,
    lanes: &'a [LaneRow],
    inputs: &'a [HeaderInput],
    bars: &'a [Option<crate::strip_palette::Bar>],
    picker: Option<(u64, Vec<crate::input_picker::PickerRow>)>,
    selected: &'a [u64],
    arranged: ArrangementState,
    pane: &'a DetailPaneState,
    listed: Option<&'a ListState>,
    rolled: Option<RollState>,
    /// The bottom pane's Strip Chart, and the Controllers chart.
    charted: Option<StripChartState>,
    controlled: Option<StripChartState>,
}

impl DocumentWindow {
    /// The pane host's area (Swift `DocumentShell.regions`): the centre,
    /// the left pane, the bottom pane (always mounted: hidden, it has no
    /// size and is out of the tree), the right pane, then the dividers
    /// over them.
    #[allow(clippy::too_many_arguments)]
    fn regions(
        &mut self,
        host: &HostState,
        frames: &panes::Frames,
        area: (f64, f64),
        editor_shown: bool,
        parts: RegionParts,
        root: &mut RegistryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        // The centre keeps its distance from the area's edges, so it fits
        // the area laid out this frame, not the one measured last frame.
        let fitted = self.fit_log.clone();
        let centre = panes::place_in(frames.centre, area, (true, true))
            .flex()
            .flex_col()
            .child(
                canvas(
                    move |bounds, _, _| {
                        let mut log = fitted.borrow_mut();
                        if let Some(last) = log.last_mut() {
                            last.1 = Some(bounds);
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        let mix = self.place_mixer(MixerPlacement::View, host.main == MainView::Mix, window, cx);
        let pane_mixer = self.place_mixer(
            MixerPlacement::Pane,
            frames.bottom.is_some()
                && !host.editor_chosen()
                && host.setup.bottom.view == PaneView::Mixer,
            window,
            cx,
        );
        let centre = if let Some(view) = mix {
            let mut scope = root.container();
            let content = mixer_view::render(&view, &mut scope, window, cx);
            centre.child(panes::mixer(content, &scope))
        } else {
            centre
                .child(arrangement::toolbar(
                    parts.tools,
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
                                lanes: parts.lanes,
                                selected: parts.selected,
                                inputs: parts.inputs,
                                bars: parts.bars,
                                picker: parts.picker,
                                vertical_offset: self.arrangement.vertical_offset(),
                            },
                            &mut root.container(),
                            window,
                            cx,
                        ))
                        .child(self.arrangement.render(
                            parts.arranged,
                            &mut root.container(),
                            window,
                            cx,
                        )),
                )
        };
        // The left pane's Channel Strip is a placement while the focused
        // track has a strip (or the mixer is still to be read); otherwise
        // the pane shows a note and the placement goes, ending its drag.
        let strip_mixer = self.place_mixer(
            MixerPlacement::Strip,
            frames.of(PaneSide::Left).is_some()
                && host.setup.left.view == PaneView::Strip
                && host.left_strip.placed(),
            window,
            cx,
        );
        let mut controlled = parts.controlled;
        let mut sides = vec![];
        for side in [PaneSide::Left, PaneSide::Right] {
            let Some(rect) = frames.of(side) else {
                continue;
            };
            let mut scope = root.container();
            let view = host.setup.pane(side).view;
            let content = if side == PaneSide::Left && view == PaneView::Strip {
                channel_strip_pane::render(
                    host.left_strip,
                    strip_mixer.as_ref(),
                    &mut scope.container(),
                    window,
                    cx,
                )
            } else if view == PaneView::Plugins {
                crate::plugins_pane::render(self, &mut scope.container(), window, cx)
            } else if side == PaneSide::Right
                && view == PaneView::Controllers
                && let Some(chart) = controlled.take()
            {
                // The focused track's chart (`S/PaneViews.swift:223-235`).
                crate::strip_chart::render(
                    &self.controllers_chart,
                    chart,
                    &mut scope.container(),
                    window,
                    cx,
                )
            } else {
                panes::content(view, host, &mut scope.container(), cx)
            };
            sides.push(panes::frame(
                side,
                Some(rect),
                area,
                host,
                self.pane_menu == Some(side),
                content,
                &mut scope,
                window,
                cx,
            ));
        }
        // The Plug-ins view's search went with its view: the keyboard
        // leaves it, so the transport keys work again.
        let plugins_shown = frames.of(PaneSide::Right).is_some()
            && host.setup.pane(PaneSide::Right).view == PaneView::Plugins;
        if !plugins_shown && self.plugins_pane.focused(window, cx) {
            window.blur(cx);
        }
        let bottom = {
            // A hidden pane, or an editor under another view, stays
            // mounted, recorded nowhere.
            let scratch = SharedRegistry::default();
            let mut hidden = RegistryScope::root(&scratch);
            let mut scope = if frames.bottom.is_some() {
                root.container()
            } else {
                hidden.container()
            };
            let editor = {
                let mut scope = if editor_shown {
                    scope.container()
                } else {
                    hidden.container()
                };
                detail_pane::render(
                    parts.pane,
                    parts.listed.map(|l| (&self.list_editor, l)),
                    parts.rolled.map(|r| (&self.piano_roll, r)),
                    parts.charted.map(|c| (&self.strip_chart, c)),
                    &mut scope,
                    window,
                    cx,
                )
            };
            let chosen = host.editor_chosen();
            let other = (!chosen).then(|| {
                let mut inner = scope.container();
                match &pane_mixer {
                    Some(view) => {
                        let content = mixer_view::render(view, &mut inner, window, cx);
                        panes::mixer(content, &inner)
                    }
                    None => panes::content(host.setup.bottom.view, host, &mut inner, cx),
                }
            });
            let content = div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .when(chosen, |d| d.flex_1().min_h_0())
                        .when(!chosen, |d| d.h(px(0.)).overflow_hidden().invisible())
                        .child(editor),
                )
                .children(other)
                .into_any_element();
            panes::frame(
                PaneSide::Bottom,
                frames.bottom,
                area,
                host,
                self.pane_menu == Some(PaneSide::Bottom),
                content,
                &mut scope,
                window,
                cx,
            )
        };
        let dividers = panes::dividers(
            frames,
            area,
            host,
            self.pane_drag,
            &mut root.container(),
            cx,
        );
        div()
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .child(self.measure_regions(cx))
            .child(centre)
            .child(bottom)
            .children(sides)
            .children(dividers)
    }
}

impl Focusable for DocumentWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DocumentWindow {
    /// The root's key context: step entry on adds [`menus::STEP_CONTEXT`],
    /// "reserve number row" [`menus::RESERVE_CONTEXT`], a US keyboard
    /// layout [`menus::US_LAYOUT_CONTEXT`] (read each render; a layout
    /// change redraws the window).
    fn key_context(&self, cx: &App) -> String {
        let s = self.app.read(cx);
        let step = s
            .store
            .document(self.doc)
            .is_some_and(DocumentModel::step_entry_on);
        let us = menus::us_layout(&s.keyboard_layout_id(cx));
        menus::window_context(step, s.keys.reserve_number_row(&s.prefs), us)
    }

    /// The window's root: its key context, focus, key handlers and
    /// actions, filling the window.
    fn root(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .key_context(self.key_context(cx).as_str())
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
            .on_action(cx.listener(Self::step_key))
            .on_action(cx.listener(Self::run_mixer_item))
            .on_action(cx.listener(Self::run_modify_template))
            .on_action(cx.listener(Self::play_stop))
            .on_action(cx.listener(Self::stop_and_return))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::save_as))
            .on_action(cx.listener(Self::close_window))
            .on_action(cx.listener(|this, a: &FileCommand, window, cx| {
                crate::file_menu::run_in_window(a, this, window, cx);
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(colors::window()))
            .text_color(rgb(colors::text()))
            .text_sm()
    }

    /// With `VIZDAW_FRAME_STATS`: the frame's render starts now, and the
    /// strips the placements drew last count.
    fn frame_started(&self, cx: &App) {
        let Some(f) = &self.frames else { return };
        let strips = [
            &self.mix_view,
            &self.pane_mixer,
            &self.strip_mixer,
            &self.window_mixer,
        ]
        .into_iter()
        .flatten()
        .map(|p| p.view.read(cx).drawn().len())
        .sum();
        let mut f = f.borrow_mut();
        f.strips = strips;
        f.render_started(Instant::now());
    }

    /// With `VIZDAW_FRAME_STATS`: an invisible last child whose paint
    /// marks the frame's end.
    fn frame_probe(&self) -> Option<impl IntoElement + use<>> {
        let frames = self.frames.clone()?;
        Some(
            canvas(
                |_, _, _| {},
                move |_, _, _, _| frames.borrow_mut().painted(Instant::now()),
            )
            .absolute()
            .top_0()
            .left_0()
            .size(px(1.)),
        )
    }

    /// What the bar shows now and then, with its labels measured.
    fn bar_shows(bar: Option<&BarState>, window: &Window) -> transport_bar::Shows {
        transport_bar::Shows::measured(bar, window)
    }

    /// Click's menu goes with Click off the row, and »'s Click submenu with
    /// »; DSP Load polls while it is on the row.
    fn follow_bar_menus(
        &mut self,
        plan: Option<&Plan>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let shown = plan.map(Plan::shown).unwrap_or_default();
        if !shown.contains(&transport_bar::Piece::Click) {
            self.click_menu = None;
        }
        if !self.more_open {
            self.more_click = None;
        }
        self.follow_dsp(shown.contains(&transport_bar::Piece::DspLoad), window, cx);
    }

    /// The Mixer window: the menu bar (off macOS), the alert, its
    /// transport bar without the document-only items, then the mixer's
    /// Window placement.
    fn render_mixer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let doc = self.doc;
        let (title, bar, model, front, layout) = {
            let s = self.app.read(cx);
            let d = s.store.document(doc);
            (
                d.map(crate::app_state::mixer_title).unwrap_or_default(),
                d.and_then(|d| bar_state(s, doc, d, false)),
                menus::menu_model(d, s.keys.keymap()),
                s.store.front().is_none_or(|f| f == doc),
                s.transport_layout(),
            )
        };
        let plan = Plan::for_window(
            &layout,
            f32::from(window.viewport_size().width),
            false,
            Self::bar_shows(bar.as_ref(), window),
        );
        let other_popover = bar.as_ref().is_some_and(|b| b.record_popover.is_some());
        self.follow_bar_fields(bar.as_ref().map(|_| &plan), other_popover, window, cx);
        if plan.folded().is_empty() {
            self.more_open = false;
        }
        self.follow_bar_menus(bar.as_ref().map(|_| &plan), window, cx);
        let popover = bar.as_ref().and_then(|b| b.record_popover.as_ref());
        self.follow_record_popover(&plan, popover, window, cx);
        if title != self.title {
            window.set_window_title(&title);
            self.title = title;
        }
        if front {
            let shown = model.clone();
            cx.defer(move |cx| menus::show(shown, cx));
        }
        if let Some(b) = &mut self.menu_bar {
            b.set_model(model);
        }
        let shows = bar.is_some();
        let mixer = self.place_mixer(MixerPlacement::Window, shows, window, cx);
        let mut root = RegistryScope::root(&self.registry);
        let menu_bar = self
            .menu_bar
            .as_ref()
            .map(|b| b.render(&mut root.container(), window, cx));
        let bar = bar.map(|bar| {
            transport_bar::render(
                &bar,
                &plan,
                transport_bar::BarParts {
                    fields: &self.transport_fields,
                    popover: &self.record_popover,
                    more_open: self.more_open,
                    click_menu: self.click_menu.as_deref(),
                    more_click: self.more_click.as_deref(),
                },
                &mut root.container(),
                window,
                cx,
            )
            .into_any_element()
        });
        let content = mixer.map(|view| {
            let mut scope = root.container();
            let content = mixer_view::render(&view, &mut scope, window, cx);
            panes::mixer(content, &scope)
        });
        let content = self
            .root(cx)
            .children(menu_bar)
            .children(bar)
            .child(div().h(px(1.)).flex_none().bg(rgb(colors::border())))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .overflow_hidden()
                    .children(content),
            )
            .children(self.frame_probe())
            .into_any_element();
        self.with_sheet(content, &mut root, window, cx)
    }
}

impl Render for DocumentWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frame_started(cx);
        if self.kind == WindowKind::Mixer {
            return self.render_mixer(window, cx);
        }
        let doc = self.doc;
        // First, so the rows built below are the ones in view.
        let rows = self.shown_rows(cx);
        self.clamp_arrangement(rows);
        let fallback = f32::from(window.viewport_size().height);
        let (
            title,
            dirty,
            bar,
            model,
            front,
            lanes,
            header_inputs,
            header_bars,
            picker,
            selected,
            status,
            layout,
            arranged,
            tools,
            pane,
            host,
            list_after,
            listed,
            rolled,
            charted,
            controlled,
            banners,
        ) = {
            let s = self.app.read(cx);
            let d = s.store.document(doc);
            (
                s.title(doc).unwrap_or_default(),
                s.is_dirty(doc),
                d.and_then(|d| bar_state(s, doc, d, true)),
                menus::menu_model(s.store.document(doc), s.keys.keymap()),
                s.store.front().is_none_or(|f| f == doc),
                d.map(|d| d.lanes().to_vec()).unwrap_or_default(),
                d.map(HeaderInput::all).unwrap_or_default(),
                d.map(|d| {
                    let bars = crate::appearance::colour_bars();
                    match d.mixer() {
                        Some(m) => track_headers::lane_bars(
                            d.lanes(),
                            m.destinations(),
                            &m.snapshot().strips,
                            bars,
                        ),
                        // No mixer: no strip colours (Swift `laneColour`).
                        None => vec![None; d.lanes().len()],
                    }
                })
                .unwrap_or_default(),
                d.and_then(|d| self.track_headers.picker_rows(d)),
                d.map(DocumentModel::selected_lanes).unwrap_or_default(),
                d.and_then(|d| d.status().map(String::from)),
                s.transport_layout(),
                d.map(|d| {
                    let visible = self
                        .arrangement
                        .visible_rows(d.lanes().len(), f32::from(window.viewport_size().height));
                    ArrangementState::of(
                        d,
                        s.transport(doc),
                        Instant::now(),
                        visible,
                        self.arrangement.axis(),
                        self.arrangement.layout_width(),
                    )
                }),
                d.map(ToolbarState::of).unwrap_or_default(),
                d.map(DetailPaneState::of).unwrap_or_default(),
                d.map(HostState::of),
                // The selection and scroll first: the rows drawn follow them.
                d.map(|d| self.list_editor.prepare(d, fallback)),
                d.filter(|d| d.list_open())
                    .map(|d| ListState::of(d, &self.list_editor, fallback)),
                // The roll's axes first: the notes drawn follow them.
                d.filter(|d| self.piano_roll.prepare(d))
                    .and_then(|d| RollState::of(d, s.transport(doc), Instant::now())),
                // The charts' axes first, likewise.
                d.filter(|d| self.strip_chart.prepare(d)).and_then(|d| {
                    StripChartState::of(
                        d,
                        ChartSlot::Pane,
                        s.transport(doc),
                        Instant::now(),
                        self.strip_chart.row_menu_open(),
                    )
                }),
                d.filter(|d| self.controllers_chart.prepare(d))
                    .and_then(|d| {
                        StripChartState::of(
                            d,
                            ChartSlot::Controllers,
                            s.transport(doc),
                            Instant::now(),
                            self.controllers_chart.row_menu_open(),
                        )
                    }),
                d.map(banners::bars).unwrap_or_default(),
            )
        };
        match &rolled {
            Some(r) => {
                self.piano_roll.follow(r);
                if r.playing() {
                    window.request_animation_frame();
                }
            }
            None => {
                self.roll_detach(cx);
                self.roll_unmounted(window, cx);
            }
        }
        let roll_mounted = rolled.is_some();
        for (slot, chart) in [
            (ChartSlot::Pane, &charted),
            (ChartSlot::Controllers, &controlled),
        ] {
            match chart {
                Some(c) => {
                    self.chart_mut(slot).follow(c);
                    if c.playing() {
                        window.request_animation_frame();
                    }
                }
                None => self.chart_detach(slot, window, cx),
            }
        }
        // The picker's track went: so does the picker.
        if picker.is_none() {
            self.track_headers.input_picker = None;
        }
        // A retry waits: poll at once, and often.
        let fast = self
            .read_model(cx, DocumentModel::wants_fast_poll)
            .unwrap_or(false);
        if fast != self.fast_poll {
            self.fast_poll = fast;
            if fast {
                self.restart_polling(window, cx);
            }
        }
        if let Some(after) = list_after {
            self.list_after(after, window, cx);
        }
        let host = host.unwrap_or_else(HostState::fallback);
        let area = self.regions_area();
        let frames = panes::frames(area.0, area.1, &host.setup);
        let bottom_shown = frames.bottom.is_some();
        let editor_shown = bottom_shown && host.editor_chosen();
        // A hidden editor's table and roll keep no keyboard.
        if !editor_shown {
            self.list_hidden(window, cx);
            if roll_mounted {
                self.roll_hidden(window, cx);
            }
        }
        self.sync_list_focus(listed.is_some() && editor_shown, window, cx);
        self.sync_roll_focus(roll_mounted && editor_shown, window, cx);
        if !editor_shown {
            self.piano_roll.menu = None;
            self.strip_chart.close_menu();
        }
        if frames.right.is_none() || controlled.is_none() {
            self.controllers_chart.close_menu();
        }
        // A chart mounted but out of sight (the bottom pane hidden or on
        // another view, a window too small for the right pane) keeps no
        // stroke (Review Focus 1).
        if charted.is_some() && !editor_shown {
            self.chart_hidden(ChartSlot::Pane, window, cx);
        }
        if controlled.is_some() && frames.right.is_none() {
            self.chart_hidden(ChartSlot::Controllers, window, cx);
        }
        if self.pane_menu.is_some_and(|side| frames.of(side).is_none()) {
            self.pane_menu = None;
        }
        if self
            .pane_drag
            .is_some_and(|drag| frames.of(drag.side).is_none())
        {
            self.pane_drag = None;
        }
        let faults = bar.as_ref().and_then(|b| b.faults.as_ref());
        let plan = Plan::new(
            &layout,
            f32::from(window.viewport_size().width),
            Self::bar_shows(bar.as_ref(), window),
        );
        // Faults went (its lines cleared): so does its popover, after the
        // draw (nothing shows the flag meanwhile).
        if faults.is_none() && self.read_model(cx, DocumentModel::shows_faults) == Some(true) {
            let (app, doc) = (self.app.clone(), self.doc);
            cx.defer(move |cx| {
                app.update(cx, |s, _| {
                    s.store.with_document(doc, |d, _| d.set_shows_faults(false));
                });
            });
        }
        let other_popover = bar.as_ref().is_some_and(|b| {
            b.record_popover.is_some() || b.faults.as_ref().is_some_and(|f| f.open)
        });
        self.follow_bar_fields(bar.as_ref().map(|_| &plan), other_popover, window, cx);
        if plan.folded().is_empty() {
            self.more_open = false;
        }
        self.follow_bar_menus(bar.as_ref().map(|_| &plan), window, cx);
        let popover = bar.as_ref().and_then(|b| b.record_popover.as_ref());
        self.follow_record_popover(&plan, popover, window, cx);
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
        let regions = self.regions(
            &host,
            &frames,
            area,
            editor_shown,
            RegionParts {
                tools,
                lanes: &lanes,
                inputs: &header_inputs,
                bars: &header_bars,
                picker,
                selected: &selected,
                arranged,
                pane: &pane,
                listed: listed.as_ref(),
                rolled,
                charted,
                controlled,
            },
            &mut root,
            window,
            cx,
        );
        // After the regions, so their paths stay put as bars come and go.
        let bars = banners::render(banners, &mut root, cx);
        let content = self
            .root(cx)
            .children(menu_bar)
            .when_some(bar, |d, bar| {
                d.child(transport_bar::render(
                    &bar,
                    &plan,
                    transport_bar::BarParts {
                        fields: &self.transport_fields,
                        popover: &self.record_popover,
                        more_open: self.more_open,
                        click_menu: self.click_menu.as_deref(),
                        more_click: self.more_click.as_deref(),
                    },
                    &mut root.container(),
                    window,
                    cx,
                ))
            })
            .children(bars)
            .child(regions)
            .when_some(status, |d, status| {
                d.child(Self::status_line(status, &mut root.container(), cx))
            })
            .children(self.frame_probe())
            .into_any_element();
        self.with_sheet(content, &mut root, window, cx)
    }
}

/// The size a new document window opens at (Swift
/// `PaneGeometry.defaultDocumentFrame`): 1440×900, or 90% of the screen's
/// visible area where that is smaller, each side on its own; never under
/// 980×620 where the screen holds that, and never under the window's
/// minimum, 700×450. With no display known, 1440×900.
pub fn default_window_size(visible: Option<Bounds<gpui::Pixels>>) -> gpui::Size<gpui::Pixels> {
    let Some(v) = visible else {
        return size(px(1440.), px(900.));
    };
    let side = |screen: f32, full: f32, old: f32, least: f32| {
        (full.min((screen * 0.9).max(old.min(screen)))).max(least)
    };
    size(
        px(side(f32::from(v.size.width), 1440., 980., 700.)),
        px(side(f32::from(v.size.height), 900., 620., 450.)),
    )
}

/// The size a new Mixer window opens at (Swift's `defaultSize`): 760×560,
/// each side kept inside the screen's visible area.
pub fn mixer_window_size(visible: Option<Bounds<gpui::Pixels>>) -> gpui::Size<gpui::Pixels> {
    let (w, h) = (760f32, 560f32);
    match visible {
        Some(v) => size(
            px(w.min(f32::from(v.size.width))),
            px(h.min(f32::from(v.size.height))),
        ),
        None => size(px(w), px(h)),
    }
}

/// The main view and panes, without the window frame (which a pane change
/// does not touch).
fn seen_views(d: &DocumentModel) -> ViewLayouts {
    let mut v = d.layout().views.clone();
    v.frame = None;
    v
}

/// Rewind (false) or Fast Forward (true) for a held-key keystroke: a key
/// the keymap gives `transport.rewind` or `transport.fast_forward` (⇧, and
/// ⇧. by default; a layout may report the shifted character, `<` or `>`).
fn held_key(k: &Keystroke, keymap: &Keymap) -> Option<bool> {
    let m = &k.modifiers;
    let pressed = (m.shift, m.control, m.alt, m.platform, m.function);
    [
        ("transport.rewind", false),
        ("transport.fast_forward", true),
    ]
    .into_iter()
    .find(|(id, _)| {
        keymap.chords(id).iter().any(|chord| {
            if chord.text().contains("Keypad") {
                return false;
            }
            let Some(want) = crate::menus::keystroke(chord)
                .and_then(|s| Keystroke::parse(crate::menus::interned(s)).ok())
            else {
                return false;
            };
            let w = &want.modifiers;
            (w.shift, w.control, w.alt, w.platform, w.function) == pressed
                && (want.key == k.key
                    || (w.shift
                        && matches!((want.key.as_str(), k.key.as_str()), (",", "<") | (".", ">"))))
        })
    })
    .map(|(_, forward)| forward)
}

/// What `doc`'s bar draws, in its document window (`document_window`) or
/// its Mixer window, which has no Faults.
fn bar_state(
    s: &AppState,
    doc: DocId,
    d: &DocumentModel,
    document_window: bool,
) -> Option<BarState> {
    Some(BarState {
        readout: s.readout(doc)?,
        can_play: s.can_play(doc),
        has_sequence: d.selected_seq().is_some(),
        looping: d.looping(),
        field_texts: TransportField::ALL
            .into_iter()
            .map(|f| (f, fields_model::field_text(d, f)))
            .collect(),
        main: d.layout().views.main,
        panes: PaneSide::ALL.map(|side| d.layout().views.current().pane(side).shown),
        faults: (document_window && (d.has_faults() || d.has_report())).then(|| FaultsState {
            lines: d.fault_lines(),
            error: d.has_faults(),
            open: d.shows_faults(),
        }),
        record: RecordBar::of(d),
        summary: d.step_summary(),
        record_popover: d
            .record()
            .shows_popover_in(if document_window {
                PopoverHost::Document
            } else {
                PopoverHost::MixerWindow
            })
            .then(|| PopoverState::of(d)),
        click: transport_bar::ClickState::of(d),
        dsp: vizdaw_app::bar_items::dsp_text(d.mixer().map_or(0., |m| m.meters().dsp_load)),
        device: vizdaw_app::bar_items::device_text(&d.last_transport()),
        commands: s
            .transport_layout()
            .items_for(document_window)
            .into_iter()
            .filter_map(|i| match i {
                TransportItem::Command { id, label } => Some((
                    id.clone(),
                    vizdaw_app::bar_items::CommandButtonFace::make(id, label.as_deref(), Some(d)),
                )),
                _ => None,
            })
            .collect(),
    })
}

/// Sets `doc`'s surface-acting flag (ruling T9-1).
fn set_surface_acting(app: &Entity<AppState>, doc: DocId, on: bool, cx: &mut App) {
    app.update(cx, |s, _| {
        if let Some(d) = s.store.document_mut(doc) {
            d.set_surface_acting(on);
        }
    });
}
