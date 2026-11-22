//! The transport bar's Position and Edit In/Out fields as the window edits
//! them (Swift: `TransportFieldsView`). The inputs are made once per
//! window and kept while folded (never remade). One click starts an edit
//! with all the text selected; a click inside the editing field places
//! the caret. Return, Tab and Shift-Tab commit (only a changed text); Tab
//! moves to the next field. Escape, a click elsewhere, the window losing
//! the keyboard or the field folding away cancels: the field shows the
//! value in use again. While a field edits it is the document's
//! "transport" text-editing owner, so the bare transport keys wait.

use gpui::{AppContext, Context, Entity, Focusable, Subscription, Window};
use vizdaw_app::transport_fields::{self, TEXT_OWNER, TransportField};

use crate::shell::DocumentWindow;
use crate::text_input::{TextInput, TextInputEvent};

/// Why a field key does nothing when its field is not drawn.
pub const NO_FIELDS: &str = "The window shows no transport fields.";

/// The window's fields and the edit in progress.
pub struct TransportFields {
    inputs: Vec<(TransportField, Entity<TextInput>)>,
    editing: Option<Editing>,
    /// The fields drawn last frame, in bar order.
    shown: Vec<TransportField>,
}

struct Editing {
    field: TransportField,
    _subscriptions: Vec<Subscription>,
}

impl TransportFields {
    pub fn new(cx: &mut Context<DocumentWindow>) -> Self {
        let inputs = TransportField::ALL
            .into_iter()
            .map(|f| (f, cx.new(|cx| TextInput::new("", cx).compact())))
            .collect();
        TransportFields {
            inputs,
            editing: None,
            shown: TransportField::ALL.to_vec(),
        }
    }

    pub fn input(&self, f: TransportField) -> &Entity<TextInput> {
        &self
            .inputs
            .iter()
            .find(|(g, _)| *g == f)
            .expect("every field has an input")
            .1
    }

    /// The field being typed in.
    pub fn editing(&self) -> Option<TransportField> {
        self.editing.as_ref().map(|e| e.field)
    }

    /// The fields the bar draws now, in bar order.
    pub fn shown(&self) -> &[TransportField] {
        &self.shown
    }
}

impl DocumentWindow {
    /// The transport field being typed in.
    pub fn editing_field(&self) -> Option<TransportField> {
        self.transport_fields.editing()
    }

    /// Starts an edit of `f`: its text, all selected, with the keyboard.
    /// An edit of another field is cancelled first.
    pub fn begin_field(&mut self, f: TransportField, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(other) = self.editing_field() {
            if other == f {
                return;
            }
            self.end_field(other, false, window, cx);
        }
        let text = self.field_text(f, cx);
        let input = self.transport_fields.input(f).clone();
        input.update(cx, |i, cx| i.set_text(&text, cx));
        let focus = input.focus_handle(cx);
        let subscriptions = vec![
            cx.subscribe_in(&input, window, move |this, _, event, window, cx| {
                this.field_event(f, *event, window, cx)
            }),
            // Focus going anywhere else ends the edit as a click elsewhere.
            cx.on_blur(&focus, window, move |this, window, cx| {
                this.end_field(f, false, window, cx)
            }),
        ];
        window.focus(&focus, cx);
        self.with_model(cx, |d| d.begin_text_editing(TEXT_OWNER));
        self.transport_fields.editing = Some(Editing {
            field: f,
            _subscriptions: subscriptions,
        });
        cx.notify();
    }

    /// Ends the edit of `f` (nothing if `f` is not being edited). With
    /// `commit` a changed text is committed; either way the field shows
    /// the value in use, and the keyboard goes back to the window unless
    /// focus has moved elsewhere.
    pub fn end_field(
        &mut self,
        f: TransportField,
        commit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editing_field() != Some(f) {
            return;
        }
        self.transport_fields.editing = None;
        let input = self.transport_fields.input(f).clone();
        let text = input.read(cx).text().to_owned();
        let doc = self.doc();
        let changed = commit && text != self.field_text(f, cx);
        self.with_model(cx, |d| {
            d.end_text_editing(TEXT_OWNER);
            if changed {
                transport_fields::commit_field(d, f, &text);
            }
        });
        if changed {
            self.app().update(cx, |s, cx| {
                s.poll(doc);
                cx.notify();
            });
        }
        let focus = input.focus_handle(cx);
        if window.focused(cx).is_none_or(|h| h == focus) {
            window.focus(&self.focus_handle(cx), cx);
        }
        cx.notify();
    }

    /// The field key (P, E): `f` takes the keyboard with its text
    /// selected, unless it is folded into » or not in the bar.
    pub fn focus_field(&mut self, f: TransportField, window: &mut Window, cx: &mut Context<Self>) {
        if !self.transport_fields.shown().contains(&f) {
            self.set_status(NO_FIELDS, cx);
            return;
        }
        self.begin_field(f, window, cx);
    }

    /// The bar drew `shown`: an edit of a field that is no longer drawn
    /// is cancelled (next turn: not from inside a draw).
    pub(crate) fn set_shown_fields(
        &mut self,
        shown: Vec<TransportField>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(f) = self.editing_field()
            && !shown.contains(&f)
        {
            cx.defer_in(window, move |this, window, cx| {
                if !this.transport_fields.shown().contains(&f) {
                    this.end_field(f, false, window, cx);
                }
            });
        }
        self.transport_fields.shown = shown;
    }

    /// Ends whatever edit is open without committing it.
    pub(crate) fn cancel_field(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = self.editing_field() {
            self.end_field(f, false, window, cx);
        }
    }

    fn field_event(
        &mut self,
        f: TransportField,
        event: TextInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            TextInputEvent::Confirm => self.end_field(f, true, window, cx),
            TextInputEvent::Cancel => self.end_field(f, false, window, cx),
            TextInputEvent::Tab | TextInputEvent::BackTab => {
                let shown = self.transport_fields.shown().to_vec();
                self.end_field(f, true, window, cx);
                let Some(i) = shown.iter().position(|g| *g == f) else {
                    return;
                };
                if shown.len() < 2 {
                    return;
                }
                let n = shown.len();
                let next = if event == TextInputEvent::Tab {
                    shown[(i + 1) % n]
                } else {
                    shown[(i + n - 1) % n]
                };
                self.begin_field(next, window, cx);
            }
        }
    }

    /// What `f` shows when it is not being typed in.
    pub(crate) fn field_text(&self, f: TransportField, cx: &gpui::App) -> String {
        let s = self.app().read(cx);
        s.store
            .document(self.doc())
            .map(|d| transport_fields::field_text(d, f))
            .unwrap_or_default()
    }
}
