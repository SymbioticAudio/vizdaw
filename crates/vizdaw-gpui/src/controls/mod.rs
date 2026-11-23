//! The mixer's controls (Swift `Fader`, `Knob`, `NumberBox`,
//! `StripToggle`, `StripMenu`, `StripMeter`): standalone elements a strip
//! lays out (slice 6a Task 5), each driven by the document's
//! [`MixerModel`] through a [`ControlBinding`]. A fader's or knob's drag
//! lives in the model (press, drag, lift, double-click, cancel): the
//! element only forwards the mouse, so a bare press sends nothing and a
//! double-click never leaves a touch open. Window deactivation (see
//! [`watch_activation`]) and a control's unmount cancel the placement's
//! drag, which releases a touch once.
//!
//! Every control records its role, label and value in the registry while
//! drawing: faders and knobs `slider`, number boxes `textfield`, toggles
//! `button` with `on` / `off`, menu faces `menu`.

pub mod fader;
pub mod knob;
pub mod menu_face;
pub mod meter;
pub mod number_box;
pub mod toggle;

use std::time::Instant;

use gpui::{
    App, Context, DispatchPhase, Div, ElementId, Entity, FocusHandle, InteractiveElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Styled, Subscription, WeakEntity, Window, canvas,
};
use vizdaw_app::mixer::{MixerModel, MixerPlacement};
use vizdaw_app::{DocId, DocumentModel};
use vizdaw_ffi::events::AutomationTargetRow as Target;

use crate::AppState;

pub use fader::fader;
pub use knob::{KnobSize, knob};
pub use menu_face::menu_face;
pub use meter::meter;
pub use number_box::{Entry, NumberBinding, NumberBox, number_box};
pub use toggle::{Toggle, toggle};

/// One placement of one document's mixer: what a control calls into.
#[derive(Clone)]
pub struct MixerRef {
    pub app: Entity<AppState>,
    pub doc: DocId,
    pub placement: MixerPlacement,
}

impl MixerRef {
    pub fn new(app: Entity<AppState>, doc: DocId, placement: MixerPlacement) -> Self {
        MixerRef {
            app,
            doc,
            placement,
        }
    }

    /// Runs `f` with the mixer (made first if need be) and its document;
    /// everything observing the app redraws. `None` without a mixer.
    /// Never call it from inside another `with` (the mixer is lent out).
    pub fn with<R>(
        &self,
        cx: &mut App,
        f: impl FnOnce(&mut MixerModel, &mut DocumentModel) -> R,
    ) -> Option<R> {
        let doc = self.doc;
        self.app.update(cx, |s, cx| {
            cx.notify();
            s.store.with_document(doc, |d, _| d.with_mixer(f)).flatten()
        })
    }

    /// Reads the mixer, if it was made.
    pub fn read<R>(&self, cx: &App, f: impl FnOnce(&MixerModel, &DocumentModel) -> R) -> Option<R> {
        let d = self.app.read(cx).store.document(self.doc)?;
        d.mixer().map(|m| f(m, d))
    }

    /// The placement's drag ends (released once, when one was under way).
    pub fn cancel_drags(&self, cx: &mut App) {
        let p = self.placement;
        self.with(cx, |m, d| m.cancel_drags(d, p, Instant::now()));
    }
}

/// What a fader or knob shows and moves: its target, its record in the
/// tree, and its position (0…1).
#[derive(Clone)]
pub struct ControlBinding {
    pub mixer: MixerRef,
    pub target: Target,
    /// "Fader of Synth", "Pan knob of Synth", "Send knob 1 of Synth".
    pub label: String,
    /// "−4.2 dB", "L 24".
    pub value: String,
    /// The control's position, 0…1.
    pub position: f64,
    pub enabled: bool,
    /// Touched here or in the core: the 2-point ring.
    pub touched: bool,
}

impl ControlBinding {
    /// Target `t` as `m` shows it.
    pub fn of(mixer: &MixerRef, m: &MixerModel, t: Target, label: String, value: String) -> Self {
        ControlBinding {
            mixer: mixer.clone(),
            target: t,
            label,
            value,
            position: m.position(t),
            enabled: true,
            touched: m.is_touched(t),
        }
    }

    /// Strip `strip`'s fader: "Fader of <name>", its level text.
    pub fn fader(mixer: &MixerRef, m: &MixerModel, strip: u64) -> Option<Self> {
        let s = m.strip(strip)?;
        Some(Self::of(
            mixer,
            m,
            Target::StripFader { strip },
            format!("Fader of {}", s.name),
            vizdaw_app::mixer::StripEntry::level_text(m.gain_db(s)),
        ))
    }

    /// Strip `strip`'s pan knob: "Pan knob of <name>", its pan text.
    pub fn pan(mixer: &MixerRef, m: &MixerModel, strip: u64) -> Option<Self> {
        let s = m.strip(strip)?;
        Some(Self::of(
            mixer,
            m,
            Target::StripPan { strip },
            format!("Pan knob of {}", s.name),
            vizdaw_app::mixer::StripEntry::pan_text(m.pan(s)),
        ))
    }

    /// Send `index`'s (0-based) knob: "Send knob <index + 1> of <name>".
    pub fn send(mixer: &MixerRef, m: &MixerModel, strip: u64, index: usize) -> Option<Self> {
        let s = m.strip(strip)?;
        if index >= s.sends.len() {
            return None;
        }
        Some(Self::of(
            mixer,
            m,
            Target::SendLevel {
                strip,
                send: vizdaw_app::mixer::send_index(index),
            },
            format!("Send knob {} of {}", index + 1, s.name),
            vizdaw_app::mixer::StripEntry::level_text(m.send_level_db(s, index)),
        ))
    }
}

/// Cancels `mixer`'s drag when the window stops being the active one (the
/// mouse-up may never come). Keep the subscription for the placement's
/// life.
pub fn watch_activation<V: 'static>(
    mixer: MixerRef,
    window: &mut Window,
    cx: &mut Context<V>,
) -> Subscription {
    cx.observe_window_activation(window, move |_, window, cx| {
        if !window.is_window_active() {
            mixer.cancel_drags(cx);
        }
    })
}

/// A drag control's state across frames: where its press went down, and
/// its keyboard focus. Dropped when the control is not drawn: a press it
/// still holds is cancelled then (the control went mid-drag).
pub(crate) struct DragState {
    origin: Option<Point<Pixels>>,
    pub(crate) focus: FocusHandle,
}

/// The keys a drag control steps with when focused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepKeys {
    /// Up and Down (a fader).
    Vertical,
    /// Up, Down, Left and Right (a knob).
    All,
}

/// A key step: 0.01 of the position, ×10 with ⇧.
pub const STEP: f64 = 0.01;

fn state_id(b: &ControlBinding) -> ElementId {
    ElementId::Name(format!("mixer-control:{:?}:{:?}", b.mixer.placement, b.target).into())
}

/// The state of `b`'s control, kept while it is drawn frame after frame.
pub(crate) fn drag_state(
    b: &ControlBinding,
    window: &mut Window,
    cx: &mut App,
) -> Entity<DragState> {
    let mixer = b.mixer.clone();
    let target = b.target;
    window.use_keyed_state(state_id(b), cx, move |_, cx| {
        let weak: WeakEntity<AppState> = mixer.app.downgrade();
        let (doc, p) = (mixer.doc, mixer.placement);
        cx.on_release(move |state: &mut DragState, cx| {
            if state.origin.is_none() {
                return;
            }
            let Some(app) = weak.upgrade() else { return };
            MixerRef::new(app, doc, p).with(cx, |m, d| {
                if m.pressed(p) == Some(target) {
                    m.cancel_drags(d, p, Instant::now());
                }
            });
        })
        .detach();
        DragState {
            origin: None,
            focus: cx.focus_handle(),
        }
    })
}

/// `surface` taking `b`'s drag: a press (⌥ at the press moves a fader
/// alone), the drag followed anywhere in the window (⌥ fine), its
/// mouse-up, a double-click's reset, and the step keys when focused.
/// `travel` is the points for the whole range.
pub(crate) fn drag_surface(
    surface: Div,
    b: &ControlBinding,
    travel: f64,
    keys: StepKeys,
    state: &Entity<DragState>,
    cx: &mut App,
) -> Div {
    let focus = state.read(cx).focus.clone();
    let held = state.read(cx).origin.is_some()
        && b.mixer
            .read(cx, |m, _| m.pressed(b.mixer.placement) == Some(b.target))
            .unwrap_or(false);
    let surface = if held {
        surface.child(follow(b, state).absolute().top_0().left_0().size_full())
    } else {
        surface
    };
    if !b.enabled {
        return surface;
    }
    let pressed = b.mixer.clone();
    let target = b.target;
    let down_state = state.clone();
    let down_focus = focus.clone();
    let stepped = b.mixer.clone();
    surface
        .track_focus(&focus)
        .on_mouse_down(MouseButton::Left, move |e: &MouseDownEvent, window, cx| {
            window.focus(&down_focus, cx);
            down_state.update(cx, |s, cx| {
                s.origin = Some(e.position);
                cx.notify();
            });
            let p = pressed.placement;
            let option = e.modifiers.alt;
            pressed.with(cx, |m, d| {
                m.press(d, p, target, travel, option, Instant::now())
            });
            cx.stop_propagation();
        })
        .on_key_down(move |e: &KeyDownEvent, _, cx| {
            let k = &e.keystroke;
            let m = &k.modifiers;
            if m.control || m.alt || m.platform || m.function {
                return;
            }
            let up = match (k.key.as_str(), keys) {
                ("up", _) | ("right", StepKeys::All) => true,
                ("down", _) | ("left", StepKeys::All) => false,
                _ => return,
            };
            let step = if m.shift { STEP * 10. } else { STEP };
            let delta = if up { step } else { -step };
            stepped.with(cx, |m, d| m.nudge(d, target, delta, Instant::now()));
            cx.stop_propagation();
        })
}

/// An invisible canvas whose paint makes the held press follow the pointer
/// anywhere in the window, until its mouse-up.
fn follow(b: &ControlBinding, state: &Entity<DragState>) -> gpui::Canvas<()> {
    let moved = b.mixer.clone();
    let released = b.mixer.clone();
    let target = b.target;
    let state = state.downgrade();
    canvas(
        |_, _, _| {},
        move |_, _, window, cx| {
            let Some(origin) = state.read_with(cx, |s, _| s.origin).ok().flatten() else {
                return;
            };
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let p = moved.placement;
                let (dx, dy) = (
                    f64::from(e.position.x - origin.x),
                    f64::from(e.position.y - origin.y),
                );
                let fine = e.modifiers.alt;
                moved.with(cx, |m, d| m.drag(d, p, dx, dy, fine, Instant::now()));
            });
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase != DispatchPhase::Bubble || e.button != MouseButton::Left {
                    return;
                }
                state.update(cx, |s, _| s.origin = None).ok();
                let p = released.placement;
                let clicks = e.click_count;
                released.with(cx, |m, d| {
                    let now = Instant::now();
                    let dragged = m.is_dragging(p);
                    m.lift(d, p, now);
                    if !dragged && clicks >= 2 {
                        m.double_click(d, p, target, now);
                    }
                });
            });
        },
    )
}

/// Whether `focus` has the keyboard (the control draws its ring).
pub(crate) fn has_focus(state: &Entity<DragState>, window: &Window, cx: &App) -> bool {
    state.read(cx).focus.is_focused(window)
}
