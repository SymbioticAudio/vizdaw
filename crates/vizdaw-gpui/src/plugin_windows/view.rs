//! What the plug-in and Built-in Synth windows draw (Swift
//! `PluginWindowContent`, `ParameterList`, `BuiltinSynthContent`).
//!
//! - **The header** (37 points with its divider): the "Bypass" toggle
//!   (`checkbox`, selected while bypassed; one undo step, a refusal the
//!   alert "Bypass Failed"), "Learn" (enabled once a parameter was
//!   touched, it learns that parameter; its help, the tree value, names
//!   what it would learn), then, while a learn waits, the banner's text
//!   and "Cancel", else the placeholder's reason in secondary text.
//! - **Under it** the plug-in's own editor, or its parameters: a row per
//!   parameter a user sees (the name 110 wide, a slider labelled by the
//!   name and valued with its text, ", read-only" when the user cannot
//!   set it, the value 48 wide in a monospaced face). A slider's drag is
//!   touch, values and release (one undo step "Change Plugin Parameter");
//!   its events follow the pointer anywhere in the window. An empty list
//!   says why there is no editor, else "No parameters.".
//! - **The Built-in Synth window:** the "Sound" radio group (Piano, Bass,
//!   Pad, Lead, Drums) and its caption; it follows every document step.
//! - ⌘W (Ctrl-W) closes the window as its close button does.

use crate::appearance::ThemedCorners as _;
use std::cell::Cell;
use std::rc::Rc;
use std::time::Instant;

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Entity, FocusHandle, InteractiveElement,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Render, StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window, canvas, div, px,
    relative, rgb,
};
use vizdaw_app::builtin_synth_window::{CAPTION, SOUND, SOUNDS, sound_title};
use vizdaw_app::plugin_window::{BYPASS, HEADER_HEIGHT, LEARN, PluginWindowModel};
use vizdaw_app::{DocId, WindowToken};
use vizdaw_ffi::BuiltinSound;
use vizdaw_ffi::snapshot::ParamRow;

use super::{AppState, EditorHost, Model, Subject, close, later, user_resized};
use crate::menus::{CloseWindow, PLUGIN_WINDOW_CONTEXT};
use crate::registry::{RegistryScope, SharedRegistry};

/// The name column's width (Swift `ParameterList`).
pub const NAME_WIDTH: f64 = 110.;
/// The value column's width.
pub const VALUE_WIDTH: f64 = 48.;

/// A monospaced face for the values (the platform's own, else a fallback).
#[cfg(target_os = "macos")]
const MONO: &str = "Menlo";
#[cfg(not(target_os = "macos"))]
const MONO: &str = "DejaVu Sans Mono";

/// The window's colours, read from the current theme each time they are
/// asked (the Swift window was system-drawn: the nearest `surface.*`,
/// `text.*` and `accent.*` tokens).
mod colors {
    use crate::appearance::current;
    use crate::transport_bar::colors as bar;

    /// The window (`surface.window`).
    pub fn background() -> u32 {
        current().surface.window.rgb
    }
    /// The header (`surface.panel`).
    pub fn header() -> u32 {
        current().surface.panel.rgb
    }
    pub fn divider() -> u32 {
        current().surface.separator.rgb
    }
    pub fn text() -> u32 {
        current().text.primary.rgb
    }
    pub fn dim() -> u32 {
        current().text.secondary.rgb
    }
    pub fn button() -> u32 {
        current().surface.raised.rgb
    }
    /// A toggle that is on: the transport's lit face.
    pub fn on() -> u32 {
        bar::on()
    }
    /// A slider's track and its filled part.
    pub fn track() -> u32 {
        current().surface.separator.rgb
    }
    pub fn fill() -> u32 {
        current().accent.accent.rgb
    }
    pub fn thumb() -> u32 {
        current().text.primary.rgb
    }
}

/// The plug-in window's colours (the test seam of what render draws).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowColours {
    pub background: u32,
    pub header: u32,
    pub divider: u32,
    pub text: u32,
    pub dim: u32,
    pub button: u32,
    pub on: u32,
    pub track: u32,
    pub fill: u32,
    pub thumb: u32,
}

/// The colours the plug-in window draws now.
pub fn colours() -> WindowColours {
    WindowColours {
        background: colors::background(),
        header: colors::header(),
        divider: colors::divider(),
        text: colors::text(),
        dim: colors::dim(),
        button: colors::button(),
        on: colors::on(),
        track: colors::track(),
        fill: colors::fill(),
        thumb: colors::thumb(),
    }
}

/// A slider's drag under way: its parameter and its track as drawn.
#[derive(Debug, Clone, Copy)]
struct ParamDrag {
    id: u64,
    track: Bounds<Pixels>,
}

/// A plug-in or Built-in Synth window's view. Its model and editor host
/// live in the registry ([`super::PluginWindows`]); the view draws them.
pub struct PluginWindowView {
    app: Entity<AppState>,
    token: WindowToken,
    subject: Subject,
    registry: SharedRegistry,
    focus: FocusHandle,
    drag: Option<ParamDrag>,
    _subscriptions: Vec<Subscription>,
}

impl PluginWindowView {
    pub(super) fn new(
        app: Entity<AppState>,
        _doc: DocId,
        subject: Subject,
        token: WindowToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![
            cx.observe_in(&app, window, move |_, app, window, cx| {
                // Its entry went (its document closed, quit, its slot
                // went): the window goes; its editor is already closed.
                if app.read(cx).plugin_windows.entry(token).is_none() {
                    window.remove_window();
                    return;
                }
                cx.notify();
            }),
            cx.observe_window_bounds(window, move |_, window, cx| {
                let v = window.viewport_size();
                let content = (
                    f64::from(f32::from(v.width)),
                    f64::from(f32::from(v.height)),
                );
                // Later, on its own turn of the main loop: the editor is
                // asked outside this window's update and the platform's
                // resize callback, and its answer is not applied from
                // inside the resize handler.
                later(cx, move |cx| user_resized(token, content, cx));
            }),
            cx.on_release(move |_, cx| {
                // Closed some other way (the platform tearing it down):
                // the editor closes now, if it has not.
                if AppState::global(cx)
                    .read(cx)
                    .plugin_windows
                    .entry(token)
                    .is_some()
                {
                    close(token, cx);
                }
            }),
        ];
        window.on_window_should_close(cx, move |_, cx| {
            // The close runs later, outside this window's update and the
            // platform's close callback; the window goes once its editor
            // and container have.
            later(cx, move |cx| close(token, cx));
            false
        });
        let focus = cx.focus_handle();
        // ⌘W reaches the window's root through its focus.
        window.focus(&focus, cx);
        PluginWindowView {
            app,
            token,
            subject,
            registry: SharedRegistry::default(),
            focus,
            drag: None,
            _subscriptions: subscriptions,
        }
    }

    /// What this window shows.
    pub fn subject(&self) -> Subject {
        self.subject
    }

    /// The elements this window drew last.
    pub fn registry(&self) -> &SharedRegistry {
        &self.registry
    }

    /// Runs `f` with this window's plug-in model and its document; every
    /// view of the app redraws.
    fn with_plugin(
        &self,
        cx: &mut App,
        f: impl FnOnce(&mut PluginWindowModel, &mut vizdaw_app::DocumentModel),
    ) {
        let token = self.token;
        self.app.update(cx, |s, cx| {
            s.plugin_windows.with_plugin(token, &mut s.store, f);
            cx.notify();
        });
    }

    /// A slider's mouse-down: its editing span begins and it takes the
    /// value under the pointer.
    fn param_down(&mut self, id: u64, track: Bounds<Pixels>, x: Pixels, cx: &mut Context<Self>) {
        self.drag = Some(ParamDrag { id, track });
        self.with_plugin(cx, |m, d| {
            m.begin_param(id);
            if let Some(v) = value_at(m, id, track, x) {
                m.set_param(d, id, v, Instant::now());
            }
        });
    }

    /// The pointer moved: a drag under way follows it; one whose button
    /// went up unseen ends.
    fn param_moved(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = self.drag else {
            return;
        };
        if e.pressed_button != Some(MouseButton::Left) {
            self.param_up(cx);
            return;
        }
        let x = e.position.x;
        self.with_plugin(cx, |m, d| {
            if let Some(v) = value_at(m, drag.id, drag.track, x) {
                m.set_param(d, drag.id, v, Instant::now());
            }
        });
    }

    /// The slider is let go: the drag is one undo step.
    fn param_up(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        self.with_plugin(cx, |m, d| m.commit_param(d, drag.id, Instant::now()));
    }
}

/// The plain value of parameter `id` at `x` on `track`: its share of the
/// track mapped onto the range (the range's least for an empty one).
fn value_at(m: &PluginWindowModel, id: u64, track: Bounds<Pixels>, x: Pixels) -> Option<f64> {
    let p = m.params().iter().find(|p| p.id == id)?;
    let width = f64::from(f32::from(track.size.width));
    let at = f64::from(f32::from(x - track.origin.x));
    let share = if width > 0. {
        (at / width).clamp(0., 1.)
    } else {
        0.
    };
    if p.max.partial_cmp(&p.min) != Some(std::cmp::Ordering::Greater) {
        return Some(p.min);
    }
    Some(p.min + share * (p.max - p.min))
}

/// What the header shows.
struct Header {
    /// The slot's bypass, when it has a row.
    bypass: Option<bool>,
    learn_help: String,
    /// A parameter was touched: Learn learns it.
    can_learn: bool,
    reason: Option<String>,
    /// The document's waiting learn: its banner text.
    banner: Option<(DocId, String)>,
}

/// One parameter row.
struct Line {
    id: u64,
    name: String,
    value: String,
    accessibility: String,
    editable: bool,
    share: f64,
}

impl Line {
    fn of(p: &ParamRow) -> Line {
        Line {
            id: p.id,
            name: p.name.clone(),
            value: PluginWindowModel::value_text(p),
            accessibility: PluginWindowModel::accessibility_value(p),
            editable: PluginWindowModel::is_editable(p),
            share: PluginWindowModel::normalised(p.value, p),
        }
    }
}

/// What a plug-in window's body shows.
enum Body {
    Editor {
        size: (f64, f64),
        view: Option<gpui::AnyElement>,
    },
    Params(Vec<Line>),
    Empty(String),
}

impl Render for PluginWindowView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut scope = RegistryScope::root(&self.registry);
        let token = self.token;
        let weak = cx.entity().downgrade();
        let root = div()
            .id("plugin-window")
            .track_focus(&self.focus)
            .key_context(PLUGIN_WINDOW_CONTEXT)
            .on_action(move |_: &CloseWindow, _, cx| later(cx, move |cx| close(token, cx)))
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(rgb(colors::background()))
            .text_color(rgb(colors::text()))
            .text_size(px(12.));
        let s = self.app.read(cx);
        let Some(e) = s.plugin_windows.entry(token) else {
            return root.into_any_element();
        };
        let doc = e.doc;
        let banner = crate::learn_banner::banner_text(doc, cx).map(|t| (doc, t));
        let (header, body) = match &e.model {
            Model::Synth(m) => {
                let sound = m.sound();
                return root
                    .child(synth_body(&self.app, token, sound, &mut scope))
                    .into_any_element();
            }
            Model::Plugin(m) => {
                let header = Header {
                    bypass: m.row().map(|r| r.bypass),
                    learn_help: m.learn_help(),
                    can_learn: m.can_learn(),
                    reason: m.placeholder_reason().map(str::to_owned),
                    banner,
                };
                let body = match m.editor_size() {
                    Some(ed) => Body::Editor {
                        size: (f64::from(ed.width), f64::from(ed.height)),
                        view: e
                            .host
                            .as_ref()
                            .and_then(EditorHost::native_view)
                            .map(|v| v.clone().into_any_element()),
                    },
                    None => {
                        let lines: Vec<Line> =
                            m.visible_params().into_iter().map(Line::of).collect();
                        if lines.is_empty() {
                            Body::Empty(m.empty_text().to_owned())
                        } else {
                            Body::Params(lines)
                        }
                    }
                };
                (header, body)
            }
        };
        let header = header_view(&self.app, token, header, &mut scope);
        let body = match body {
            Body::Editor { size, view } => {
                let area = div().flex_none().w(px(size.0 as f32)).h(px(size.1 as f32));
                match view {
                    Some(v) => area.child(v),
                    None => area,
                }
                .into_any_element()
            }
            Body::Params(lines) => params_view(&weak, lines, &mut scope),
            Body::Empty(text) => div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .relative()
                        .text_color(rgb(colors::dim()))
                        .child(text.clone())
                        .child(scope.control("text", &text, true)),
                )
                .into_any_element(),
        };
        root.child(header)
            .child(body)
            .child(drag_listeners(weak))
            .into_any_element()
    }
}

/// The header: Bypass, Learn, the placeholder's reason.
fn header_view(
    app: &Entity<AppState>,
    token: WindowToken,
    h: Header,
    scope: &mut RegistryScope,
) -> AnyElement {
    let on = h.bypass.unwrap_or(false);
    let enabled = h.bypass.is_some();
    let toggling = app.clone();
    let mut bypass = div()
        .id("plugin-bypass")
        .relative()
        .flex_none()
        .h(px(22.))
        .px(px(10.))
        .flex()
        .items_center()
        .round_px(4.)
        .bg(rgb(if on { colors::on() } else { colors::button() }))
        .child(BYPASS)
        .child(scope.control_selected("checkbox", BYPASS, enabled, None, on));
    if enabled {
        bypass = bypass.cursor_pointer().on_click(move |_, _, cx| {
            toggling.update(cx, |s, cx| {
                s.plugin_windows
                    .with_plugin(token, &mut s.store, |m, d| m.toggle_bypass(d));
                cx.notify();
            });
        });
    }
    let can_learn = h.can_learn;
    let mut learn = div()
        .id("plugin-learn")
        .relative()
        .flex_none()
        .h(px(22.))
        .px(px(10.))
        .flex()
        .items_center()
        .round_px(4.)
        .bg(rgb(colors::button()))
        .opacity(if can_learn { 1. } else { 0.45 })
        .child(LEARN)
        .child(scope.control_with("button", LEARN, can_learn, Some(&h.learn_help), false));
    if can_learn {
        let learning = app.clone();
        learn = learn.cursor_pointer().on_click(move |_, _, cx| {
            learning.update(cx, |s, cx| {
                s.plugin_windows
                    .with_plugin(token, &mut s.store, |m, d| m.learn_last_touched(d));
                cx.notify();
            });
        });
    }
    let learn = crate::sheet_host::with_help(learn, h.learn_help.clone());
    // A waiting learn's banner takes the reason's place
    // (`S/PluginWindows.swift:271-275`).
    let text = |t: String, colour: u32, scope: &mut RegistryScope| {
        div()
            .relative()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_color(rgb(colour))
            .child(t.clone())
            .child(scope.control("text", &t, true))
            .into_any_element()
    };
    let trailing: Vec<AnyElement> = match (h.banner, h.reason) {
        (Some((doc, b)), _) => vec![
            text(b, colors::text(), scope),
            crate::learn_banner::cancel_button(doc, scope),
        ],
        (None, Some(r)) => vec![text(r, colors::dim(), scope)],
        (None, None) => vec![],
    };
    div()
        .flex_none()
        .w_full()
        .h(px(HEADER_HEIGHT as f32))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .px(px(8.))
        .bg(rgb(colors::header()))
        .border_b_1()
        .border_color(rgb(colors::divider()))
        .child(bypass)
        .child(learn)
        .children(trailing)
        .into_any_element()
}

/// The parameter rows, scrolling down when they do not fit.
fn params_view(
    view: &WeakEntity<PluginWindowView>,
    lines: Vec<Line>,
    scope: &mut RegistryScope,
) -> AnyElement {
    let mut list = scope.container();
    let rows: Vec<AnyElement> = lines
        .into_iter()
        .map(|l| param_row(view, l, &mut list))
        .collect();
    div()
        .id("plugin-params")
        .relative()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .child(
            div()
                .p(px(12.))
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(rows),
        )
        .child(list.clip())
        .into_any_element()
}

/// One parameter: its name, its slider and its value.
fn param_row(
    view: &WeakEntity<PluginWindowView>,
    l: Line,
    scope: &mut RegistryScope,
) -> AnyElement {
    let name = div()
        .relative()
        .flex_none()
        .w(px(NAME_WIDTH as f32))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .child(l.name.clone())
        .child(scope.control("text", &l.name, true));
    let slider = slider(view, &l, scope);
    let value = div()
        .relative()
        .flex_none()
        .w(px(VALUE_WIDTH as f32))
        .flex()
        .justify_end()
        .font_family(MONO)
        .text_size(px(11.))
        .child(l.value.clone())
        .child(scope.control("text", &l.value, true));
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .h(px(22.))
        .child(name)
        .child(slider)
        .child(value)
        .into_any_element()
}

/// A parameter's slider: the track, its filled part and the thumb; a
/// press takes the value under the pointer and drags (not while
/// read-only).
fn slider(view: &WeakEntity<PluginWindowView>, l: &Line, scope: &mut RegistryScope) -> AnyElement {
    let track = Rc::new(Cell::new(Bounds::default()));
    let drawn = track.clone();
    let share = l.share as f32;
    let id = l.id;
    let mut el = div()
        .id(gpui::ElementId::Name(format!("plugin-param:{id}").into()))
        .relative()
        .flex_1()
        .min_w(px(40.))
        .h(px(20.))
        .opacity(if l.editable { 1. } else { 0.45 })
        .child(
            canvas(move |b, _, _| drawn.set(b), |_, _, _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(9.))
                .h(px(2.))
                .rounded(px(1.))
                .bg(rgb(colors::track())),
        )
        .child(
            div()
                .absolute()
                .left_0()
                .top(px(9.))
                .h(px(2.))
                .w(relative(share))
                .rounded(px(1.))
                .bg(rgb(colors::fill())),
        )
        .child(
            div()
                .absolute()
                .top(px(3.))
                .left(relative(share))
                .ml(px(-7.))
                .size(px(14.))
                .rounded_full()
                .bg(rgb(colors::thumb())),
        )
        .child(scope.control_with("slider", &l.name, l.editable, Some(&l.accessibility), false));
    if l.editable {
        let view = view.clone();
        el = el.on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, _, cx| {
            let at = track.get();
            view.update(cx, |v, cx| v.param_down(id, at, e.position.x, cx))
                .ok();
            cx.stop_propagation();
        });
    }
    el.into_any_element()
}

/// The window's pointer listeners for a slider's drag, registered on every
/// paint and checked at the event: GPUI dispatches against the last frame
/// drawn, so a release before the frame after the press still finds them.
fn drag_listeners(view: WeakEntity<PluginWindowView>) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            let moved = view.clone();
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble {
                    moved.update(cx, |v, cx| v.param_moved(e, cx)).ok();
                }
            });
            let released = view.clone();
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                    released.update(cx, |v, cx| v.param_up(cx)).ok();
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
    .into_any_element()
}

/// The Built-in Synth window's content: the sounds, then the caption.
fn synth_body(
    app: &Entity<AppState>,
    token: WindowToken,
    sound: Option<BuiltinSound>,
    scope: &mut RegistryScope,
) -> AnyElement {
    let mut group = scope.container();
    let radios: Vec<AnyElement> = SOUNDS
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            let on = sound == Some(s);
            let title = sound_title(s);
            let app = app.clone();
            div()
                .id(("synth-sound", i))
                .relative()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .cursor_pointer()
                .child(if on { "\u{25C9}" } else { "\u{25CB}" })
                .child(title)
                .child(group.control_selected("radio", title, true, None, on))
                .on_click(move |_, _, cx| {
                    app.update(cx, |st, cx| {
                        st.plugin_windows
                            .with_synth(token, &mut st.store, |m, d| m.set_sound(d, s));
                        cx.notify();
                    });
                })
                .into_any_element()
        })
        .collect();
    let sounds = div()
        .relative()
        .flex()
        .flex_row()
        .gap(px(12.))
        .child(div().flex_none().child(format!("{SOUND}:")))
        .child(div().flex().flex_col().gap(px(4.)).children(radios))
        .child(group.this("radiogroup", SOUND, true));
    let caption = div()
        .relative()
        .text_size(px(11.))
        .text_color(rgb(colors::dim()))
        .child(CAPTION)
        .child(scope.control("text", CAPTION, true));
    div()
        .p(px(20.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(sounds)
        .child(caption)
        .into_any_element()
}
