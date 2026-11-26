//! The transport bar's Position, Edit In/Out and Punch In/Out fields as
//! the window edits them (Swift: `TransportFieldsView`). The inputs are
//! made once per window and kept while folded (never remade). One click
//! starts an edit with all the text selected; a click inside the editing
//! field places the caret. Return, Tab and Shift-Tab commit (only a
//! changed text); Tab moves to the next field. Escape, a click elsewhere,
//! the window losing the keyboard (or closing) or the field folding away
//! cancels: the field shows the value in use again. While a field edits
//! it is the document's "transport" text-editing owner, so the bare
//! transport keys wait.
//!
//! A punch field reports each change (`bar_punch_typed`) and every end of
//! its edit (`bar_punch_ended`): a draft left behind would refuse Record
//! with nothing on screen. A press on Record with a draft open keeps the
//! edit (`RecordModel::close_allowed`).
//!
//! Folded into », a field item is a popover of its live fields (Swift's
//! `.popover` entry); the field keys (P, E, U) open it at their field.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{AppContext, Bounds, Context, Entity, Focusable, Pixels, Point, Subscription, Window};
use vizdaw_app::record::RecordModel;
use vizdaw_app::transport_bar::TransportItem;
use vizdaw_app::transport_fields::{self, FieldKeyTarget, TEXT_OWNER, TransportField};

use crate::shell::DocumentWindow;
use crate::text_input::{TextInput, TextInputEvent};
use crate::transport_bar::{Piece, Plan};

/// Why a field key does nothing when its field is not drawn.
pub const NO_FIELDS: &str = "The window shows no transport fields.";

/// The window's fields and the edit in progress.
pub struct TransportFields {
    inputs: Vec<(TransportField, Entity<TextInput>)>,
    editing: Option<Editing>,
    /// The fields drawn on the row last frame, in bar order.
    shown: Vec<TransportField>,
    /// The bar's items last frame, and those folded into ».
    items: Vec<TransportItem>,
    folded: Vec<TransportItem>,
    /// The folded field item whose popover is open at ».
    popover: Option<Piece>,
    /// Where the bar drew Record last frame (Swift `RecordButtons`): a
    /// press on it with a punch draft open keeps the edit.
    pub(crate) record_button: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Where the bar drew » last frame: a press on it is a field
    /// popover's opener, not a press outside.
    pub(crate) more_button: Rc<Cell<Option<Bounds<Pixels>>>>,
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
            items: Vec::new(),
            folded: Vec::new(),
            popover: None,
            record_button: Rc::default(),
            more_button: Rc::default(),
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

    /// The fields the bar draws on its row now, in bar order.
    pub fn shown(&self) -> &[TransportField] {
        &self.shown
    }

    /// The folded field item whose popover is open.
    pub fn popover(&self) -> Option<Piece> {
        self.popover
    }

    /// The fields the popover draws.
    fn popover_fields(&self) -> &'static [TransportField] {
        self.popover.map_or(&[], Piece::fields)
    }

    /// The fields Tab goes round from `f`: the popover's, or the row's.
    fn tab_ring(&self, f: TransportField) -> Vec<TransportField> {
        let popover = self.popover_fields();
        if popover.contains(&f) {
            popover.to_vec()
        } else {
            self.shown.clone()
        }
    }
}

fn is_punch(f: TransportField) -> bool {
    matches!(f, TransportField::PunchIn | TransportField::PunchOut)
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
        let mut subscriptions = vec![
            cx.subscribe_in(&input, window, move |this, _, event, window, cx| {
                this.field_event(f, *event, window, cx)
            }),
            // Focus going anywhere else ends the edit as a click elsewhere.
            cx.on_blur(&focus, window, move |this, window, cx| {
                this.end_field(f, false, window, cx)
            }),
        ];
        if is_punch(f) {
            // Each change is the document's bar punch draft (Swift
            // `barPunchTyped`): Record waits for Return or Esc.
            let mut reported = text.clone();
            subscriptions.push(cx.observe_in(&input, window, move |this, input, _, cx| {
                if this.editing_field() != Some(f) {
                    return;
                }
                let now = input.read(cx).text();
                if now != reported {
                    reported = now.to_owned();
                    let text = reported.clone();
                    this.with_model(cx, |d| d.bar_punch_typed(f, &text));
                }
            }));
        }
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
            // Every end of an edit, committed or not (Swift `barPunchEnded`).
            if is_punch(f) {
                d.bar_punch_ended();
            }
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

    /// A press at `at` outside the field being edited: it ends the edit
    /// (cancelling), unless it is on Record with a punch draft open, which
    /// keeps it so Record refuses and Return still sets the punch (Swift
    /// `handleMouseDown`, `RecordModel.closeAllowed`).
    pub(crate) fn press_outside_field(
        &mut self,
        f: TransportField,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on_record = self
            .transport_fields
            .record_button
            .get()
            .is_some_and(|b| b.contains(&at));
        let draft = self
            .read_model(cx, |d| d.has_open_punch_draft())
            .unwrap_or(false);
        if RecordModel::close_allowed(on_record, draft) {
            self.end_field(f, false, window, cx);
        }
    }

    /// The field key (P, E, U; Swift `openField`): `f` on the row takes
    /// the keyboard with its text selected; folded, » opens at its item
    /// with `f` taking it; with no item holding it, the status says so.
    pub fn focus_field(&mut self, f: TransportField, window: &mut Window, cx: &mut Context<Self>) {
        let fields = &self.transport_fields;
        match transport_fields::field_key(f, &fields.items, &fields.folded) {
            Some(FieldKeyTarget::OnRow) if fields.shown.contains(&f) => {
                self.begin_field(f, window, cx)
            }
            Some(FieldKeyTarget::Folded(_)) => {
                self.open_field_popover(Piece::holding(f), window, cx);
                self.begin_field(f, window, cx);
            }
            _ => self.set_status(NO_FIELDS, cx),
        }
    }

    /// Opens `piece`'s popover at » (closing the » menu and the window's
    /// other popovers; an edit elsewhere is cancelled).
    pub(crate) fn open_field_popover(
        &mut self,
        piece: Piece,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_more(cx);
        self.escape_record_popover(window, cx);
        self.escape_faults(cx);
        if let Some(f) = self.editing_field()
            && !piece.fields().contains(&f)
        {
            self.end_field(f, false, window, cx);
        }
        self.transport_fields.popover = Some(piece);
        cx.notify();
    }

    /// Closes the field popover, cancelling an edit in it; true when one
    /// was open.
    pub(crate) fn close_field_popover(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(piece) = self.transport_fields.popover.take() else {
            return false;
        };
        if let Some(f) = self.editing_field()
            && piece.fields().contains(&f)
        {
            self.end_field(f, false, window, cx);
        }
        cx.notify();
        true
    }

    /// A press at `at` outside the field popover: it closes, unless the
    /// press is on » (its opener, which closes it itself) or on Record
    /// with a punch draft open.
    pub(crate) fn press_outside_field_popover(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on = |b: &Rc<Cell<Option<Bounds<Pixels>>>>| b.get().is_some_and(|b| b.contains(&at));
        if on(&self.transport_fields.more_button) {
            return;
        }
        let on_record = on(&self.transport_fields.record_button);
        let draft = self
            .read_model(cx, |d| d.has_open_punch_draft())
            .unwrap_or(false);
        if RecordModel::close_allowed(on_record, draft) {
            self.close_field_popover(window, cx);
        }
    }

    /// The bar drew `plan` (`None`: no bar). An edit of a field that is
    /// no longer drawn is cancelled, and the field popover closes when
    /// its item is no longer folded or another popover opened here
    /// (`other_popover`) (next turn: not from inside a draw).
    pub(crate) fn follow_bar_fields(
        &mut self,
        plan: Option<&Plan>,
        other_popover: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let fields = &mut self.transport_fields;
        fields.shown = plan.map(Plan::shown_fields).unwrap_or_default();
        fields.items = plan.map(|p| p.items().to_vec()).unwrap_or_default();
        fields.folded = plan.map(Plan::folded_items).unwrap_or_default();
        let folded = plan.map(Plan::folded).unwrap_or_default();
        if let Some(p) = fields.popover
            && (other_popover || !folded.contains(&p))
        {
            cx.defer_in(window, move |this, window, cx| {
                if this.transport_fields.popover == Some(p) {
                    this.close_field_popover(window, cx);
                }
            });
        }
        if let Some(f) = self.editing_field()
            && !self.field_drawn(f)
        {
            cx.defer_in(window, move |this, window, cx| {
                if !this.field_drawn(f) {
                    this.end_field(f, false, window, cx);
                }
            });
        }
    }

    /// `f` is drawn, on the row or in the field popover.
    fn field_drawn(&self, f: TransportField) -> bool {
        let fields = &self.transport_fields;
        fields.shown.contains(&f) || fields.popover_fields().contains(&f)
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
            TextInputEvent::Cancel => {
                self.end_field(f, false, window, cx);
                // Esc in a field popover closes it too.
                if self.transport_fields.popover_fields().contains(&f) {
                    self.close_field_popover(window, cx);
                }
            }
            TextInputEvent::Tab | TextInputEvent::BackTab => {
                let next = transport_fields::tab_target(
                    &self.transport_fields.tab_ring(f),
                    f,
                    event == TextInputEvent::BackTab,
                );
                self.end_field(f, true, window, cx);
                if let Some(next) = next {
                    self.begin_field(next, window, cx);
                }
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
