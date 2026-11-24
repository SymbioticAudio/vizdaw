//! The record popover (Swift `RecordPopover`, `RecordPopoverController`;
//! spec §6.9): the record options under the bar's Record Options chevron
//! (or », folded), one per window, in this window's host. An in-window
//! overlay, 340 wide with no title, disabled while a pass records:
//! Replace/Overdub, the overdub note, Punch with its In and Out fields,
//! Loop, Count-in and Pre-roll, MIDI passes as takes, and the time left.
//!
//! The punch fields keep drafts in the model while they are typed in
//! (`set_punch_text` per keystroke): Return commits, Tab and focus moving
//! to the other field commit the one left, focus leaving the pair drops
//! it, and Esc drops it and closes the popover. It closes on Esc, a press
//! outside it or its chevron, and closing drops every draft; a press on
//! Record with a draft open is the one press that keeps it open, with the
//! field holding the keyboard, so Record refuses and says why
//! (`RecordModel::close_allowed`).

use std::cell::Cell;
use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, AppContext, Bounds, Context, Div, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Point,
    StatefulInteractiveElement, Styled, Subscription, Window, anchored, deferred, div, px, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::record::{MAX_COUNT_IN, MAX_PRE_ROLL, MIDI_TAKES_HELP, PopoverHost, RecordModel};
use vizdaw_app::transport_fields::TransportField;

use crate::app_state::WindowKind;
use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::text_input::{TextInput, TextInputEvent};
use crate::transport_bar::{Piece, Plan, colors, face};

/// The chevron's label and the popover's name in `ui.tree`.
pub const RECORD_OPTIONS: &str = "Record Options";
/// The popover's width (Swift `.frame(width: 340)`).
pub const WIDTH: f32 = 340.;
const PUNCH: [TransportField; 2] = [TransportField::PunchIn, TransportField::PunchOut];

/// `n` bars as the steppers say it (Swift `RecordPopover.bars`).
pub fn bars(n: u8) -> String {
    match n {
        0 => "off".into(),
        1 => "1 bar".into(),
        _ => format!("{n} bars"),
    }
}

/// What the popover shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopoverState {
    pub replace: bool,
    /// The overdub note (overdub on and an armed audio track).
    pub note: Option<&'static str>,
    pub punch: bool,
    /// Punch In's and Out's texts: the draft, else the value in use.
    pub punch_texts: [String; 2],
    pub looping: bool,
    pub count_in: u8,
    pub pre_roll: u8,
    pub midi_takes: bool,
    /// The time left on the project's disk.
    pub disk: Option<String>,
    /// A pass records: everything is disabled.
    pub recording: bool,
}

impl PopoverState {
    pub fn of(d: &DocumentModel) -> PopoverState {
        let r = d.record();
        PopoverState {
            replace: d.record_replaces(),
            note: d.record_overdub_note(),
            punch: d.record_punches(),
            punch_texts: PUNCH.map(|f| d.punch_text(f)),
            looping: d.looping(),
            count_in: r.count_in_bars(),
            pre_roll: r.pre_roll_bars(),
            midi_takes: r.midi_passes_as_takes(),
            disk: d.record_remaining_text(),
            recording: r.is_recording(),
        }
    }

    /// The punch fields take typing.
    fn punch_editable(&self) -> bool {
        self.punch && !self.recording
    }
}

/// The window's popover fields and the punch field holding the keyboard.
pub struct RecordPopover {
    inputs: [Entity<TextInput>; 2],
    /// The popover's own focus: a press on it away from the fields
    /// takes the keyboard from them (leaving the pair).
    focus: FocusHandle,
    /// The punch field with the keyboard, as last told to the model.
    focused: Option<TransportField>,
    /// Where the chevron was drawn last frame: a press on it is its own
    /// toggle, not a press outside.
    pub(crate) chevron: Rc<Cell<Option<Bounds<Pixels>>>>,
    _subscriptions: Vec<Subscription>,
}

fn index(f: TransportField) -> usize {
    usize::from(f == TransportField::PunchOut)
}

impl RecordPopover {
    pub fn new(window: &mut Window, cx: &mut Context<DocumentWindow>) -> Self {
        let inputs = PUNCH.map(|_| cx.new(|cx| TextInput::new("", cx).compact()));
        let mut subscriptions = Vec::new();
        for (f, input) in PUNCH.into_iter().zip(&inputs) {
            let focus = input.focus_handle(cx);
            subscriptions.push(cx.on_focus(&focus, window, |this, window, cx| {
                this.punch_focus_sync(window, cx)
            }));
            subscriptions.push(cx.on_blur(&focus, window, |this, window, cx| {
                this.punch_focus_sync(window, cx)
            }));
            subscriptions.push(cx.subscribe_in(
                input,
                window,
                move |this, _, event, window, cx| this.punch_event(f, *event, window, cx),
            ));
            // Each keystroke is the model's draft.
            subscriptions.push(cx.observe_in(input, window, move |this, input, _, cx| {
                if this.record_popover.focused != Some(f) {
                    return;
                }
                let text = input.read(cx).text().to_owned();
                this.with_model(cx, |d| {
                    if d.punch_text(f) != text {
                        d.set_punch_text(f, &text);
                    }
                });
            }));
        }
        RecordPopover {
            inputs,
            focus: cx.focus_handle(),
            focused: None,
            chevron: Rc::default(),
            _subscriptions: subscriptions,
        }
    }

    fn input(&self, f: TransportField) -> &Entity<TextInput> {
        &self.inputs[index(f)]
    }

    /// The punch field holding the keyboard.
    pub fn focused(&self) -> Option<TransportField> {
        self.focused
    }
}

impl DocumentWindow {
    /// The record popover's host in this window.
    pub(crate) fn popover_host(&self) -> PopoverHost {
        match self.kind() {
            WindowKind::Document => PopoverHost::Document,
            WindowKind::Mixer => PopoverHost::MixerWindow,
        }
    }

    /// The chevron or its » entry: the popover opens here (closing it in
    /// the other window, which drops its drafts) or, open here, closes.
    pub(crate) fn toggle_record_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let host = self.popover_host();
        if self.read_model(cx, |d| d.record().shows_popover_in(host)) == Some(true) {
            self.close_record_popover(window, cx);
        } else {
            self.with_model(cx, |d| d.toggle_record_popover(host));
        }
    }

    /// Closes the popover (dropping the drafts and ending its text
    /// editing); the keyboard goes back to the window from its fields.
    pub(crate) fn close_record_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.record_popover.focused = None;
        self.with_model(cx, |d| d.set_record_popover(false));
        self.focus_window_from_popover(window, cx);
    }

    /// The keyboard leaves the popover (its fields or itself) for the
    /// window.
    fn focus_window_from_popover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let p = &self.record_popover;
        let held = p.focus.is_focused(window)
            || p.inputs
                .iter()
                .any(|i| i.focus_handle(cx).is_focused(window));
        if held {
            window.focus(&self.focus_handle(cx), cx);
        }
    }

    /// Esc: the popover open here closes; true when it was.
    pub(crate) fn escape_record_popover(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let host = self.popover_host();
        let open = self.read_model(cx, |d| d.record().shows_popover_in(host)) == Some(true);
        if open {
            self.close_record_popover(window, cx);
        }
        open
    }

    /// A press at `at` outside the popover: it closes, unless the press
    /// is on its chevron (which toggles it itself) or on Record with a
    /// punch draft open (Record refuses with the draft on screen and its
    /// field keeping the keyboard).
    fn press_outside_record_popover(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .record_popover
            .chevron
            .get()
            .is_some_and(|b| b.contains(&at))
        {
            return;
        }
        let on_record = self
            .transport_fields
            .record_button
            .get()
            .is_some_and(|b| b.contains(&at));
        let draft = self
            .read_model(cx, DocumentModel::has_open_punch_draft)
            .unwrap_or(false);
        if RecordModel::close_allowed(on_record, draft) {
            self.close_record_popover(window, cx);
        }
    }

    /// The keyboard moved: tells the model which punch field has it
    /// (moving within the pair commits the one left, leaving the pair
    /// drops it), and a field taking it shows its text, all selected.
    fn punch_focus_sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = PUNCH.into_iter().find(|f| {
            self.record_popover
                .input(*f)
                .focus_handle(cx)
                .is_focused(window)
        });
        let old = self.record_popover.focused;
        if now == old {
            return;
        }
        self.record_popover.focused = now;
        self.with_model(cx, |d| d.punch_focus_changed(old, now));
        if let Some(f) = now {
            self.show_punch_text(f, cx);
        }
    }

    /// A click on `f`: it takes the keyboard with its text all selected
    /// (from the other field, committing that one).
    fn begin_punch(&mut self, f: TransportField, window: &mut Window, cx: &mut Context<Self>) {
        let old = self.record_popover.focused;
        if old != Some(f) {
            self.record_popover.focused = Some(f);
            self.with_model(cx, |d| d.punch_focus_changed(old, Some(f)));
        }
        self.show_punch_text(f, cx);
        let focus = self.record_popover.input(f).focus_handle(cx);
        window.focus(&focus, cx);
        cx.notify();
    }

    /// `f`'s input holds the model's text for it, all selected.
    fn show_punch_text(&mut self, f: TransportField, cx: &mut Context<Self>) {
        let text = self.read_model(cx, |d| d.punch_text(f)).unwrap_or_default();
        let input = self.record_popover.input(f).clone();
        input.update(cx, |i, cx| i.set_text(&text, cx));
    }

    fn punch_event(
        &mut self,
        f: TransportField,
        event: TextInputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            // Return sets it (a refusal says why and keeps the old value);
            // the field keeps the keyboard and shows the value in use.
            TextInputEvent::Confirm => {
                self.with_model(cx, |d| d.commit_punch(f));
                self.show_punch_text(f, cx);
            }
            // Esc drops it, and the popover closes.
            TextInputEvent::Cancel => {
                self.with_model(cx, |d| d.drop_punch(f));
                self.close_record_popover(window, cx);
            }
            // Tab and ⇧Tab go to the other field, committing this one.
            TextInputEvent::Tab | TextInputEvent::BackTab => {
                self.begin_punch(PUNCH[1 - index(f)], window, cx);
            }
        }
        cx.notify();
    }

    /// After the bar's plan: the popover open here closes when its item
    /// is gone from the bar (neither shown nor folded), and a punch field
    /// that is not drawn (closed, Punch off, recording) gives up the
    /// keyboard (next turn: not from inside a draw).
    pub(crate) fn follow_record_popover(
        &mut self,
        plan: &Plan,
        state: Option<&PopoverState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let on_bar = plan.shown().contains(&Piece::RecordOptions)
            || plan.folded().contains(&Piece::RecordOptions);
        if state.is_some() && !on_bar {
            cx.defer_in(window, |this, window, cx| {
                this.close_record_popover(window, cx);
            });
            return;
        }
        let editable = state.is_some_and(PopoverState::punch_editable);
        if self.record_popover.focused.is_some() && !editable {
            let closed = state.is_none();
            cx.defer_in(window, move |this, window, cx| {
                if closed {
                    // The model closed it (the other window took it): its
                    // drafts and text editing are gone already.
                    this.record_popover.focused = None;
                    this.focus_window_from_popover(window, cx);
                } else {
                    // Leaving the pair: the draft drops.
                    let focus = this.record_popover.focus.clone();
                    window.focus(&focus, cx);
                }
            });
        }
    }
}

/// The Record Options chevron (Swift `RecordOptionsButton`), and the
/// popover under it while it is open here.
pub(crate) fn chevron(
    state: Option<&PopoverState>,
    popover: &RecordPopover,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let bounds = popover.chevron.clone();
    let button = face("record-options", "\u{2304}", true, colors::BUTTON)
        .w(px(crate::transport_bar::CHEVRON))
        .when(state.is_some(), |b| b.bg(rgb(colors::ON)))
        .child(scope.control_with(
            "button",
            RECORD_OPTIONS,
            true,
            Some(crate::transport_bar::RECORD_OPTIONS_HELP),
            false,
        ))
        .child(
            gpui::canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {})
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
        )
        .on_click(cx.listener(|this, _, window, cx| this.toggle_record_popover(window, cx)));
    let button =
        crate::sheet_host::with_help(button, crate::transport_bar::RECORD_OPTIONS_HELP.to_owned());
    let mut item = div().relative().flex_none().child(button);
    if let Some(s) = state {
        item = item.child(overlay(s, popover, false, scope, window, cx));
    }
    item.into_any_element()
}

/// The popover, hung from the bottom edge of the opener it is laid over
/// (the chevron, or » with `right` when folded).
pub(crate) fn overlay(
    state: &PopoverState,
    popover: &RecordPopover,
    right: bool,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let panel = panel(state, popover, &mut scope.container(), window, cx);
    let hang = div().absolute().top_full();
    let hang = if right { hang.right_0() } else { hang.left_0() };
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .child(hang.child(deferred(anchored().snap_to_window().child(panel)).with_priority(2)))
        .into_any_element()
}

fn panel(
    s: &PopoverState,
    popover: &RecordPopover,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let on = !s.recording;
    let mut panel = div()
        .id("record-popover")
        .track_focus(&popover.focus)
        .occlude()
        .relative()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .w(px(WIDTH))
        .bg(rgb(colors::LIST))
        .border_1()
        .border_color(rgb(colors::BORDER))
        .rounded_md()
        .shadow_md()
        .text_color(rgb(colors::TEXT))
        .child(scope.this("group", RECORD_OPTIONS, on))
        .on_mouse_down_out(cx.listener(|this, e: &MouseDownEvent, window, cx| {
            this.press_outside_record_popover(e.position, window, cx)
        }));
    // 1. Mode.
    panel = panel.child(mode(s.replace, on, &mut scope.container(), cx));
    // 2. The overdub note.
    if let Some(note) = s.note {
        panel = panel.child(
            div()
                .relative()
                .text_xs()
                .text_color(rgb(colors::DIM))
                .child(note)
                .child(scope.control("text", note, true)),
        );
    }
    // 3. Punch and its fields.
    panel = panel.child(toggle(
        "record-punch",
        "Punch",
        s.punch,
        on,
        None,
        scope,
        cx.listener(|this, _, _, cx| {
            this.with_model(cx, |d| {
                let on = !d.record_punches();
                d.set_record_punch(on);
            });
        }),
    ));
    let mut fields = div().flex().flex_row().gap_2();
    for f in PUNCH {
        fields = fields.child(punch_field(f, s, popover, scope, window, cx));
    }
    panel = panel.child(fields);
    // 4. Loop.
    panel = panel.child(toggle(
        "record-loop",
        "Loop",
        s.looping,
        on,
        None,
        scope,
        cx.listener(|this, _, _, cx| this.toggle_loop(cx)),
    ));
    // 5. Count-in and Pre-roll.
    panel = panel.child(stepper(
        "Count-in",
        s.count_in,
        MAX_COUNT_IN,
        on,
        &mut scope.container(),
        cx,
        |d, n| d.record_mut().set_count_in(n),
    ));
    panel = panel.child(stepper(
        "Pre-roll",
        s.pre_roll,
        MAX_PRE_ROLL,
        on,
        &mut scope.container(),
        cx,
        |d, n| d.record_mut().set_pre_roll(n),
    ));
    // 6. MIDI passes as takes.
    let takes = toggle(
        "record-takes",
        "MIDI passes as takes",
        s.midi_takes,
        on,
        Some(MIDI_TAKES_HELP),
        scope,
        cx.listener(|this, _, _, cx| {
            this.with_model(cx, |d| {
                let on = !d.record().midi_passes_as_takes();
                d.record_mut().set_midi_passes_as_takes(on);
            });
        }),
    );
    panel = panel.child(crate::sheet_host::with_help(takes, MIDI_TAKES_HELP));
    // 7. Disk.
    if let Some(t) = &s.disk {
        panel = panel.child(
            div()
                .relative()
                .flex()
                .flex_row()
                .justify_between()
                .child("Disk")
                .child(div().text_color(rgb(colors::DIM)).child(t.clone()))
                .child(scope.control_with("text", "Disk", true, Some(t), false)),
        );
    }
    panel.into_any_element()
}

/// The Mode segments (Swift's segmented `Mode` picker).
fn mode(
    replace: bool,
    enabled: bool,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let mut row = div()
        .relative()
        .flex()
        .flex_row()
        .gap(px(1.))
        .child(scope.this("group", "Mode", enabled));
    for (title, is_replace) in [("Replace", true), ("Overdub", false)] {
        let on = replace == is_replace;
        row = row.child(
            face(
                ("record-mode", usize::from(is_replace)),
                title,
                enabled,
                if on { colors::ON } else { colors::BUTTON },
            )
            .flex_1()
            .child(scope.control_selected("button", title, enabled, None, on))
            .when(enabled, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    this.with_model(cx, |d| d.set_record_replace(is_replace));
                }))
            }),
        );
    }
    row.into_any_element()
}

/// A toggle row: ☑/☐ and its title.
fn toggle(
    id: &'static str,
    title: &'static str,
    on: bool,
    enabled: bool,
    value: Option<&str>,
    scope: &mut RegistryScope,
    flip: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .flex_row()
        .gap_2()
        .when(!enabled, |d| d.text_color(rgb(colors::DISABLED)))
        .child(if on { "\u{2611}" } else { "\u{2610}" })
        .child(title)
        .child(scope.control_selected("checkbox", title, enabled, value, on))
        .when(enabled, |d| d.cursor_pointer().on_click(flip))
}

/// A stepper (`<title>: <bars>`, − and +), from 0 to `max`.
fn stepper(
    title: &'static str,
    n: u8,
    max: u8,
    enabled: bool,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
    set: fn(&mut DocumentModel, u8),
) -> AnyElement {
    let label = format!("{title}: {}", bars(n));
    let mut row = div()
        .relative()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(scope.this("stepper", &label, enabled))
        .child(div().flex_1().child(label.clone()));
    for (glyph, verb, to, can) in [
        ("\u{2212}", "Decrement", n.saturating_sub(1), n > 0),
        ("+", "Increment", n.saturating_add(1).min(max), n < max),
    ] {
        let can = enabled && can;
        row = row.child(
            face(
                (title, usize::from(verb == "Increment")),
                glyph,
                can,
                colors::BUTTON,
            )
            .w(px(24.))
            .child(scope.control("button", &format!("{verb} {title}"), can))
            .when(can, |b| {
                b.on_click(cx.listener(move |this, _, _, cx| {
                    this.with_model(cx, |d| set(d, to));
                }))
            }),
        );
    }
    row.into_any_element()
}

/// Punch In or Out: the model's text, or the input while it has the
/// keyboard; disabled unless Punch is on (and while recording).
fn punch_field(
    f: TransportField,
    s: &PopoverState,
    popover: &RecordPopover,
    scope: &mut RegistryScope,
    window: &Window,
    cx: &mut Context<DocumentWindow>,
) -> AnyElement {
    let enabled = s.punch_editable();
    let input = popover.input(f).clone();
    let editing = enabled && popover.focused == Some(f);
    let focused = editing && input.focus_handle(cx).is_focused(window);
    let value = if editing {
        input.read(cx).text().to_owned()
    } else {
        s.punch_texts[index(f)].clone()
    };
    let boxed = div()
        .id(("record-punch-field", index(f)))
        .relative()
        .flex_1()
        .h(px(24.))
        .rounded_sm()
        .overflow_hidden()
        .text_color(rgb(0x000000));
    let boxed = if editing {
        boxed.child(input)
    } else {
        let shown = boxed
            .bg(rgb(if enabled {
                colors::FIELD
            } else {
                colors::DISABLED
            }))
            .px(px(4.))
            .flex()
            .items_center()
            .text_size(px(13.))
            .whitespace_nowrap()
            .child(value.clone());
        if enabled {
            shown.cursor_text().on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    // The popover must not take the keyboard back.
                    window.prevent_default();
                    this.begin_punch(f, window, cx);
                }),
            )
        } else {
            shown
        }
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .gap_1()
        .child(
            div()
                .text_xs()
                .text_color(rgb(colors::DIM))
                .child(f.title()),
        )
        .child(boxed.child(scope.control_with(
            "textfield",
            f.title(),
            enabled,
            Some(&value),
            focused,
        )))
        .into_any_element()
}
