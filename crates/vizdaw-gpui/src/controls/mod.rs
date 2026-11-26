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
pub mod stepper;
pub mod toggle;

use std::time::Instant;

use gpui::{
    App, Context, DispatchPhase, Div, ElementId, Entity, FocusHandle, InteractiveElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels,
    Point, Styled, Subscription, WeakEntity, Window, canvas,
};
use vizdaw_app::mixer::{MidiControl, MidiStripMath, MixerModel, MixerPlacement};
use vizdaw_app::{DocId, DocumentModel};
use vizdaw_ffi::events::AutomationTargetRow as Target;

use crate::AppState;

pub use fader::{fader, fader_on};
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

/// What a fader or knob drags: an automation target (a strip's fader,
/// pan or send, a VCA's fader: a gesture on the core), or an external MIDI
/// destination's volume or pan (previews, then one commit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grip {
    Gesture(Target),
    Midi { dest: u64, control: MidiControl },
}

impl Grip {
    /// A key step: 0.01 of the position (a MIDI value, 1/127), ×10 with ⇧.
    pub fn step(self) -> f64 {
        match self {
            Grip::Gesture(_) => STEP,
            Grip::Midi { .. } => MidiStripMath::STEP,
        }
    }

    /// The mouse went down on it in `p` (⌥ held: `option`).
    fn press(
        self,
        m: &mut MixerModel,
        d: &mut DocumentModel,
        p: MixerPlacement,
        travel: f64,
        option: bool,
        now: Instant,
    ) {
        match self {
            Grip::Gesture(t) => m.press(d, p, t, travel, option, now),
            Grip::Midi { dest, control } => m.midi_press(d, p, dest, control, travel, now),
        }
    }

    /// The press in `p` moved by (`dx`, `dy`).
    #[allow(clippy::too_many_arguments)]
    fn drag(
        self,
        m: &mut MixerModel,
        d: &mut DocumentModel,
        p: MixerPlacement,
        dx: f64,
        dy: f64,
        fine: bool,
        now: Instant,
    ) {
        match self {
            Grip::Gesture(_) => m.drag(d, p, dx, dy, fine, now),
            Grip::Midi { .. } => m.midi_drag(d, p, dx, dy, fine, now),
        }
    }

    /// The press in `p` lifts; a second click that did not drag is a
    /// double-click.
    fn lift(
        self,
        m: &mut MixerModel,
        d: &mut DocumentModel,
        p: MixerPlacement,
        clicks: usize,
        now: Instant,
    ) {
        match self {
            Grip::Gesture(t) => {
                let dragged = m.is_dragging(p);
                m.lift(d, p, now);
                if !dragged && clicks >= 2 {
                    m.double_click(d, p, t, now);
                }
            }
            Grip::Midi { dest, control } => {
                let dragged = m.is_midi_dragging(p);
                m.midi_lift(d, p, now);
                if !dragged && clicks >= 2 {
                    m.midi_double_click(d, p, dest, control, now);
                }
            }
        }
    }

    /// A key step of `delta`.
    fn nudge(self, m: &mut MixerModel, d: &mut DocumentModel, delta: f64, now: Instant) {
        match self {
            Grip::Gesture(t) => m.nudge(d, t, delta, now),
            Grip::Midi { dest, control } => m.midi_nudge(d, dest, control, delta, now),
        }
    }

    /// The press in `p` is on it.
    pub fn held(self, m: &MixerModel, p: MixerPlacement) -> bool {
        match self {
            Grip::Gesture(t) => m.pressed(p) == Some(t),
            Grip::Midi { dest, control } => m.midi_pressed(p) == Some((dest, control)),
        }
    }
}

/// What a fader or knob shows and moves: what it drags, its record in the
/// tree, and its position (0…1).
#[derive(Clone)]
pub struct ControlBinding {
    pub mixer: MixerRef,
    pub target: Grip,
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
            target: Grip::Gesture(t),
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

    /// VCA `vca`'s fader: "Fader of VCA <name>", its level text.
    pub fn vca(mixer: &MixerRef, m: &MixerModel, vca: u64) -> Option<Self> {
        let v = m.vca(vca)?;
        Some(Self::of(
            mixer,
            m,
            Target::VcaFader { vca },
            vizdaw_app::mixer::VcaStripLabels::fader(&v.name),
            vizdaw_app::mixer::StripEntry::level_text(m.vca_gain_db(v)),
        ))
    }

    /// External MIDI destination `dest`'s fader ("Fader of <name>", the
    /// volume) or pan knob ("Pan knob of <name>", the pan).
    pub fn midi(mixer: &MixerRef, m: &MixerModel, dest: u64, control: MidiControl) -> Option<Self> {
        let d = m.destinations().iter().find(|d| d.id == dest)?;
        let mix = m.midi_mix(dest)?;
        let (label, value) = match control {
            MidiControl::Volume => (
                vizdaw_app::mixer::MidiStripLabels::fader(&d.name),
                MidiStripMath::volume_label(mix.volume),
            ),
            MidiControl::Pan => (
                vizdaw_app::mixer::MidiStripLabels::pan_knob(&d.name),
                MidiStripMath::pan_label(mix.pan),
            ),
        };
        Some(ControlBinding {
            mixer: mixer.clone(),
            target: Grip::Midi { dest, control },
            label,
            value,
            position: m.midi_position(dest, control),
            enabled: true,
            touched: false,
        })
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
pub struct DragState {
    origin: Option<Point<Pixels>>,
    pub(crate) focus: FocusHandle,
}

impl DragState {
    /// Where the press this control holds went down.
    pub fn origin(&self) -> Option<Point<Pixels>> {
        self.origin
    }
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

/// The state of `b`'s control, kept while it is drawn frame after frame
/// (the same one however often it is asked for in a frame).
pub fn drag_state(b: &ControlBinding, window: &mut Window, cx: &mut App) -> Entity<DragState> {
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
                if target.held(m, p) {
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
///
/// The window-wide move and up listeners are registered on every paint
/// and decide when an event comes whether this control holds the press:
/// GPUI dispatches mouse events against the last frame drawn, so a
/// mouse-up that comes before the frame after the press must still find
/// them. They listen in the capture phase, so no element that stops the
/// event keeps the mouse-up from them; and any press while this control
/// still holds one (its mouse-up was lost) lifts that one first.
pub(crate) fn drag_surface(
    surface: Div,
    b: &ControlBinding,
    travel: f64,
    keys: StepKeys,
    state: &Entity<DragState>,
    cx: &mut App,
) -> Div {
    let focus = state.read(cx).focus.clone();
    // A press that ended elsewhere (a cancel, another press in this
    // placement) leaves no origin behind.
    if state.read(cx).origin.is_some() && !holds(&b.mixer, b.target, cx) {
        state.update(cx, |s, _| s.origin = None);
    }
    let surface = surface.child(follow(b, state).absolute().top_0().left_0().size_full());
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
                target.press(m, d, p, travel, option, Instant::now())
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
            let step = if m.shift {
                target.step() * 10.
            } else {
                target.step()
            };
            let delta = if up { step } else { -step };
            stepped.with(cx, |m, d| target.nudge(m, d, delta, Instant::now()));
            cx.stop_propagation();
        })
}

/// The mixer's press in `mixer`'s placement is on `target`.
fn holds(mixer: &MixerRef, target: Grip, cx: &App) -> bool {
    mixer
        .read(cx, |m, _| target.held(m, mixer.placement))
        .unwrap_or(false)
}

/// Where this control's press went down, when it still holds the
/// placement's press.
fn held_origin(
    state: &WeakEntity<DragState>,
    mixer: &MixerRef,
    target: Grip,
    cx: &App,
) -> Option<Point<Pixels>> {
    let origin = state.read_with(cx, |s, _| s.origin).ok().flatten()?;
    holds(mixer, target, cx).then_some(origin)
}

/// Mouse-up (or a move without the button: the up went elsewhere): the
/// press lifts; a second click that did not drag is a double-click.
fn lift(
    state: &WeakEntity<DragState>,
    mixer: &MixerRef,
    target: Grip,
    clicks: usize,
    cx: &mut App,
) {
    state.update(cx, |s, _| s.origin = None).ok();
    let p = mixer.placement;
    mixer.with(cx, |m, d| target.lift(m, d, p, clicks, Instant::now()));
}

/// An invisible canvas whose paint registers the window-wide listeners
/// that make this control's press follow the pointer, until its mouse-up.
fn follow(b: &ControlBinding, state: &Entity<DragState>) -> gpui::Canvas<()> {
    let mixer = b.mixer.clone();
    let target = b.target;
    let state = state.downgrade();
    canvas(
        |_, _, _| {},
        move |_, _, window, _| {
            // A press anywhere while this control still holds one: its
            // mouse-up was lost, so it lifts before the new press lands.
            let (pressed, pressed_state) = (mixer.clone(), state.clone());
            window.on_mouse_event(move |_: &MouseDownEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture
                    || held_origin(&pressed_state, &pressed, target, cx).is_none()
                {
                    return;
                }
                lift(&pressed_state, &pressed, target, 1, cx);
            });
            let (moved, moved_state) = (mixer.clone(), state.clone());
            window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture {
                    return;
                }
                let Some(origin) = held_origin(&moved_state, &moved, target, cx) else {
                    return;
                };
                if e.pressed_button != Some(MouseButton::Left) {
                    lift(&moved_state, &moved, target, 1, cx);
                    return;
                }
                let p = moved.placement;
                let (dx, dy) = (
                    f64::from(e.position.x - origin.x),
                    f64::from(e.position.y - origin.y),
                );
                let fine = e.modifiers.alt;
                moved.with(cx, |m, d| {
                    target.drag(m, d, p, dx, dy, fine, Instant::now())
                });
            });
            let (released, released_state) = (mixer.clone(), state.clone());
            window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                if phase != DispatchPhase::Capture || e.button != MouseButton::Left {
                    return;
                }
                if held_origin(&released_state, &released, target, cx).is_none() {
                    return;
                }
                lift(&released_state, &released, target, e.click_count, cx);
            });
        },
    )
}

/// Whether `focus` has the keyboard (the control draws its ring).
pub(crate) fn has_focus(state: &Entity<DragState>, window: &Window, cx: &App) -> bool {
    state.read(cx).focus.is_focused(window)
}
