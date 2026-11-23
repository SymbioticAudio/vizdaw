//! A typed-value box (Swift `NumberBox`, `NumberBoxModel`): the level or
//! pan as text; a click (or Return or Space when it has the keyboard)
//! edits it in a [`TextInput`] with all the text selected. Return commits
//! through the strict grammar (`StripEntry`): refused text sends nothing,
//! the status line shows the hint and the box's edge turns the error
//! colour until the next edit. Escape, or the field losing the keyboard,
//! cancels and shows the value in use again. After Return or Escape the
//! box takes the keyboard back; after losing it, it does not. While it
//! edits, the bare transport keys wait.

use std::time::Instant;

use gpui::{
    AnyElement, App, AppContext, Context, Entity, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, ParentElement, Render, Styled, Subscription, Window,
    div, px, rgb,
};

use super::MixerRef;
use crate::registry::RegistryScope;
use crate::strip_palette::tokens;
use crate::text_input::{TextInput, TextInputEvent};
use vizdaw_app::mixer::{MixerModel, StripEntry};

/// The box's drawn size (Swift `ControlMetrics.numberBox`).
pub const SIZE: (f64, f64) = (56., 22.);
/// Its hit height (the 24-point minimum).
pub const HIT_HEIGHT: f64 = 24.;

/// What a box types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Level {
        strip: u64,
    },
    Pan {
        strip: u64,
    },
    /// Send `index` (0-based).
    Send {
        strip: u64,
        index: usize,
    },
}

/// What a box shows and where its text goes.
#[derive(Clone)]
pub struct NumberBinding {
    pub mixer: MixerRef,
    pub entry: Entry,
    /// "Level of Synth", "Pan of Synth", "Send 1 of Synth".
    pub label: String,
    /// The value in use: "−4.2 dB", "L 24".
    pub value: String,
    pub enabled: bool,
}

impl NumberBinding {
    pub fn level(mixer: &MixerRef, m: &MixerModel, strip: u64) -> Option<Self> {
        let s = m.strip(strip)?;
        Some(NumberBinding {
            mixer: mixer.clone(),
            entry: Entry::Level { strip },
            label: format!("Level of {}", s.name),
            value: StripEntry::level_text(m.gain_db(s)),
            enabled: true,
        })
    }

    pub fn pan(mixer: &MixerRef, m: &MixerModel, strip: u64) -> Option<Self> {
        let s = m.strip(strip)?;
        Some(NumberBinding {
            mixer: mixer.clone(),
            entry: Entry::Pan { strip },
            label: format!("Pan of {}", s.name),
            value: StripEntry::pan_text(m.pan(s)),
            enabled: true,
        })
    }

    /// Send `index` (0-based): "Send <index + 1> of <name>".
    pub fn send(mixer: &MixerRef, m: &MixerModel, strip: u64, index: usize) -> Option<Self> {
        let s = m.strip(strip)?;
        if index >= s.sends.len() {
            return None;
        }
        Some(NumberBinding {
            mixer: mixer.clone(),
            entry: Entry::Send { strip, index },
            label: format!("Send {} of {}", index + 1, s.name),
            value: StripEntry::level_text(m.send_level_db(s, index)),
            enabled: true,
        })
    }
}

/// How an edit ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum End {
    Submit,
    Escape,
    FocusLost,
}

/// One box's editing state, kept by the view that lays it out (one per
/// box, made once and kept).
pub struct NumberBox {
    input: Entity<TextInput>,
    /// The box's own keyboard focus (Return or Space edits).
    focus: FocusHandle,
    binding: Option<NumberBinding>,
    editing: bool,
    refused: bool,
    /// Its name as a text-editing owner of the document.
    owner: String,
    _subscriptions: Vec<Subscription>,
}

impl NumberBox {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("", cx).compact());
        let input_focus = input.focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(
                &input,
                window,
                |this, _, e: &TextInputEvent, window, cx| match e {
                    TextInputEvent::Confirm => this.end(End::Submit, window, cx),
                    TextInputEvent::Cancel => this.end(End::Escape, window, cx),
                    TextInputEvent::Tab | TextInputEvent::BackTab => {}
                },
            ),
            cx.on_blur(&input_focus, window, |this, window, cx| {
                this.end(End::FocusLost, window, cx)
            }),
        ];
        // Dropped mid-edit (the strip went): the edit ends without a
        // commit, and the bare transport keys work again.
        cx.on_release(|this: &mut NumberBox, cx| {
            if !this.editing {
                return;
            }
            this.editing = false;
            if let Some(b) = &this.binding {
                let (doc, owner) = (b.mixer.doc, this.owner.clone());
                b.mixer.app.update(cx, |s, cx| {
                    s.store
                        .with_document(doc, |d, _| d.end_text_editing(&owner));
                    cx.notify();
                });
            }
        })
        .detach();
        NumberBox {
            input,
            focus: cx.focus_handle(),
            binding: None,
            editing: false,
            refused: false,
            owner: format!("mixer number box {}", cx.entity_id()),
            _subscriptions: subscriptions,
        }
    }

    /// An edit is open.
    pub fn editing(&self) -> bool {
        self.editing
    }

    /// The last commit was refused (the error edge shows).
    pub fn refused(&self) -> bool {
        self.refused
    }

    /// The text being typed.
    pub fn draft(&self, cx: &App) -> String {
        self.input.read(cx).text().to_owned()
    }

    /// Starts an edit: the value in use, all selected, with the keyboard.
    pub fn begin(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(b) = self.binding.clone() else {
            return;
        };
        if self.editing || !b.enabled {
            return;
        }
        self.input.update(cx, |i, cx| i.set_text(&b.value, cx));
        window.focus(&self.input.focus_handle(cx), cx);
        self.editing = true;
        self.refused = false;
        let (doc, owner) = (b.mixer.doc, self.owner.clone());
        b.mixer.app.update(cx, |s, cx| {
            s.store
                .with_document(doc, |d, _| d.begin_text_editing(&owner));
            cx.notify();
        });
        cx.notify();
    }

    fn end(&mut self, how: End, window: &mut Window, cx: &mut Context<Self>) {
        if !self.editing {
            return;
        }
        self.editing = false;
        let Some(b) = self.binding.clone() else {
            return;
        };
        let (doc, owner) = (b.mixer.doc, self.owner.clone());
        b.mixer.app.update(cx, |s, cx| {
            s.store
                .with_document(doc, |d, _| d.end_text_editing(&owner));
            cx.notify();
        });
        if how == End::Submit {
            let text = self.draft(cx);
            let entry = b.entry;
            let accepted = b
                .mixer
                .with(cx, |m, d| {
                    let now = Instant::now();
                    match entry {
                        Entry::Level { strip } => m.type_level(d, strip, &text, now),
                        Entry::Pan { strip } => m.type_pan(d, strip, &text, now),
                        Entry::Send { strip, index } => m.type_send(d, strip, index, &text, now),
                    }
                })
                .unwrap_or(false);
            self.refused = !accepted;
        }
        if how != End::FocusLost {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }
}

impl Render for NumberBox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = tokens();
        let (w, h) = SIZE;
        let shell = div()
            .w(px(w as f32))
            .h(px(h as f32))
            .flex_none()
            .rounded(px(k.radius))
            .overflow_hidden();
        if self.editing {
            return shell
                .bg(rgb(k.surface_raised))
                .child(self.input.clone())
                .into_any_element();
        }
        let enabled = self.binding.as_ref().is_some_and(|b| b.enabled);
        let text = self
            .binding
            .as_ref()
            .map(|b| b.value.clone())
            .unwrap_or_default();
        let focused = self.focus.is_focused(window);
        let (edge, width) = if self.refused {
            (k.state_error, 1.)
        } else if focused {
            (k.focus_ring, 2.)
        } else {
            (k.readout_text, 1.)
        };
        shell
            .track_focus(&self.focus)
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(k.readout_background))
            .border(px(width))
            .border_color(rgb(edge))
            .text_size(px(11.))
            .text_color(rgb(k.readout_text))
            .whitespace_nowrap()
            .opacity(if enabled { 1. } else { 0.45 })
            .child(text)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    this.begin(window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, window, cx| {
                let k = &e.keystroke;
                let m = &k.modifiers;
                if m.control || m.alt || m.platform || m.function || m.shift {
                    return;
                }
                if matches!(k.key.as_str(), "enter" | "space") {
                    this.begin(window, cx);
                    cx.stop_propagation();
                }
            }))
            .into_any_element()
    }
}

/// Box `nb` showing `b`, its record in the tree (`textfield`, the value
/// in use, or the text being typed while it edits).
pub fn number_box(
    nb: &Entity<NumberBox>,
    b: NumberBinding,
    scope: &mut RegistryScope,
    cx: &mut App,
) -> AnyElement {
    let (editing, draft) = {
        let n = nb.read(cx);
        (n.editing, n.draft(cx))
    };
    let value = if editing { draft } else { b.value.clone() };
    let (label, enabled) = (b.label.clone(), b.enabled);
    nb.update(cx, |n, _| n.binding = Some(b));
    div()
        .relative()
        .flex_none()
        .w(px(SIZE.0 as f32))
        .h(px(HIT_HEIGHT as f32))
        .flex()
        .items_center()
        .child(nb.clone())
        .child(scope.control_with("textfield", &label, enabled, Some(&value), editing))
        .into_any_element()
}
