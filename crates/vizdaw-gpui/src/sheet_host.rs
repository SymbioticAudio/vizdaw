//! The sheet host (slice 7 inventory §7.2-4): one modal sheet per window,
//! drawn over a dimmed window as a centred card with Swift's sheet chrome
//! (the body, then the buttons at the bottom right, the default one
//! highlighted). Esc presses the cancel button, Return the default one,
//! Tab and Shift-Tab move between the sheet's fields, and no key reaches
//! the window while a sheet is up (the focus stays in the sheet; the
//! sheet is drawn outside the window's key context, so no window binding
//! or key handler sees what is typed into its fields). While a sheet's
//! text field edits, the document's bare transport keys are off in every
//! window of the document (Swift `beginTextEditing`). Clicks never reach
//! the window under the dimmed backdrop. The sheet and its buttons are
//! registry elements with Swift's labels. Under the control channel no
//! sheet shows ([`crate::control::channel_on`]).
//!
//! The same card shows an [`Ask`] (a question with buttons): in a sheet
//! ([`AskSheet`]), as the app-level dialog window when no window is open
//! ([`crate::alerts`]), and as GPUI's prompt off macOS
//! ([`use_sheet_prompts`]: GPUI's own Linux prompt has no keys and no
//! theme).

use futures::channel::oneshot;
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, AppContext, Context, Div, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, Keystroke, ParentElement, PromptButton,
    PromptHandle, PromptLevel, PromptResponse, Render, RenderablePromptHandle, Stateful,
    StatefulInteractiveElement, Styled, Window, div, px, rgb, rgba,
};

use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::transport_bar::colors;

/// The text-editing owner a sheet's field in window `token` uses (Swift
/// `beginTextEditing`): the document's bare transport keys wait. One per
/// window, so a document's two windows never end each other's.
pub fn sheet_owner(token: vizdaw_app::WindowToken) -> String {
    format!("sheet-{}", token.0)
}

/// The card's width (Swift's sheets are about this wide).
const CARD_WIDTH: f32 = 460.;
/// The default button's colour.
const DEFAULT_BUTTON: u32 = 0x3f5f8f;
/// A destructive button's colour.
const DESTRUCTIVE_BUTTON: u32 = 0x7a3434;
/// The card's colour.
const CARD: u32 = 0x2a2a32;

/// What a button does when a key presses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonRole {
    /// Return presses it (Swift `.defaultAction`); drawn highlighted.
    Default,
    /// Esc presses it (Swift `.cancelAction`, `role: .cancel`).
    Cancel,
    /// Drawn as destructive (Swift `role: .destructive`).
    Destructive,
    Plain,
}

/// One button of a sheet or an [`Ask`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetButton {
    pub label: String,
    pub role: ButtonRole,
    pub enabled: bool,
}

impl SheetButton {
    pub fn new(label: impl Into<String>, role: ButtonRole) -> Self {
        SheetButton {
            label: label.into(),
            role,
            enabled: true,
        }
    }

    /// The same button, shown disabled.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// The button Return presses: the enabled default one.
pub fn default_index(buttons: &[SheetButton]) -> Option<usize> {
    buttons
        .iter()
        .position(|b| b.role == ButtonRole::Default && b.enabled)
}

/// The button Esc presses: the enabled cancel one, or a lone button (an
/// alert's OK).
pub fn cancel_index(buttons: &[SheetButton]) -> Option<usize> {
    buttons
        .iter()
        .position(|b| b.role == ButtonRole::Cancel && b.enabled)
        .or_else(|| (buttons.len() == 1 && buttons[0].enabled).then_some(0))
}

/// A sheet a window shows in its [`SheetHost`] (Tasks 4–6 put Project
/// Settings, Relink and Bounce here). Its state lives in the sheet (or in
/// entities it holds); a press answers whether it closes.
pub trait Sheet: 'static {
    /// The sheet's accessibility label (Swift's sheet title): the
    /// registry's `sheet` element.
    fn label(&self) -> String;
    /// The body, above the buttons. Record its controls in `scope`.
    fn render(
        &mut self,
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;
    /// The buttons, in Swift's order (drawn left to right at the bottom
    /// right).
    fn buttons(&self, cx: &App) -> Vec<SheetButton>;
    /// The fields Tab moves between, in order (none: Tab does nothing).
    fn focus_order(&self, _cx: &App) -> Vec<FocusHandle> {
        vec![]
    }
    /// Button `index` was pressed (clicked, or by Return or Esc); true
    /// closes the sheet.
    fn press(&mut self, index: usize, window: &mut Window, cx: &mut App) -> bool;
}

/// A window's sheet, if one is up.
pub struct SheetHost {
    sheet: Option<Box<dyn Sheet>>,
    /// The card's focus: the sheet keeps the keyboard inside it.
    focus: FocusHandle,
    /// A field of the sheet has the keyboard (the document's transport
    /// keys are off).
    editing: bool,
}

impl SheetHost {
    pub fn new(cx: &mut App) -> Self {
        SheetHost {
            sheet: None,
            focus: cx.focus_handle(),
            editing: false,
        }
    }

    /// A field of the sheet has the keyboard (its window's owner holds the
    /// document's keys off).
    pub fn editing(&self) -> bool {
        self.editing
    }

    pub fn is_up(&self) -> bool {
        self.sheet.is_some()
    }

    /// The label of the sheet showing.
    pub fn label(&self) -> Option<String> {
        self.sheet.as_ref().map(|s| s.label())
    }

    /// The card's focus handle (a sheet with no field keeps it).
    pub fn focus(&self) -> &FocusHandle {
        &self.focus
    }
}

impl DocumentWindow {
    /// Shows `sheet` over this window. Refused (handed back) while another
    /// sheet is up (one per window) or while the control channel is on (no
    /// sheet ever shows then).
    pub fn show_sheet(
        &mut self,
        sheet: Box<dyn Sheet>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), Box<dyn Sheet>> {
        if self.sheet.is_up() || crate::control::channel_on(cx) {
            return Err(sheet);
        }
        // What the keyboard was doing in the window ends here.
        self.cancel_field(window, cx);
        self.sheet_opened(cx);
        self.sheet.sheet = Some(sheet);
        window.focus(&self.sheet.focus, cx);
        cx.notify();
        Ok(())
    }

    /// The label of the sheet showing in this window.
    pub fn sheet_label(&self) -> Option<String> {
        self.sheet.label()
    }

    /// Presses the sheet's button `index` (an enabled one): the sheet
    /// answers, and closes if it says so.
    pub fn press_sheet_button(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut sheet) = self.sheet.sheet.take() else {
            return;
        };
        let enabled = sheet.buttons(cx).get(index).is_some_and(|b| b.enabled);
        let close = enabled && sheet.press(index, window, cx);
        if close {
            self.set_sheet_editing(false, cx);
            window.focus(&self.focus, cx);
        } else {
            self.sheet.sheet = Some(sheet);
        }
        cx.notify();
    }

    /// A key while a sheet is up (the keystroke interceptor's call): Esc
    /// and Return press the cancel and default buttons, Tab moves between
    /// the sheet's fields; any other key goes on only when the sheet has
    /// the keyboard (to its field). True: the key stops here.
    pub(crate) fn sheet_key(
        &mut self,
        key: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(sheet) = &self.sheet.sheet else {
            return false;
        };
        // A prompt drawn over the sheet has the keyboard.
        if window.has_active_prompt() {
            return false;
        }
        let m = &key.modifiers;
        let bare = !(m.control || m.alt || m.platform || m.function);
        match key.key.as_str() {
            "escape" if bare && !m.shift => {
                if let Some(i) = cancel_index(&sheet.buttons(cx)) {
                    self.press_sheet_button(i, window, cx);
                }
                true
            }
            "enter" if bare && !m.shift => {
                if let Some(i) = default_index(&sheet.buttons(cx)) {
                    self.press_sheet_button(i, window, cx);
                }
                true
            }
            "tab" if bare => {
                let order = sheet.focus_order(cx);
                if !order.is_empty() {
                    let at = order.iter().position(|f| f.is_focused(window));
                    let n = order.len();
                    let next = match (at, m.shift) {
                        (Some(i), false) => (i + 1) % n,
                        (Some(i), true) => (i + n - 1) % n,
                        (None, false) => 0,
                        (None, true) => n - 1,
                    };
                    window.focus(&order[next], cx);
                    cx.notify();
                }
                true
            }
            _ => {
                if self.sheet.focus.contains_focused(window, cx) {
                    return false;
                }
                // The keyboard got out of the sheet: back in, and the key
                // does nothing.
                window.focus(&self.sheet.focus, cx);
                true
            }
        }
    }

    fn set_sheet_editing(&mut self, editing: bool, cx: &mut Context<Self>) {
        if self.sheet.editing == editing {
            return;
        }
        self.sheet.editing = editing;
        let doc = self.doc();
        let owner = sheet_owner(self.token());
        crate::app_state::AppState::global(cx).update(cx, |s, cx| {
            if let Some(d) = s.store.document_mut(doc) {
                if editing {
                    d.begin_text_editing(&owner);
                } else {
                    d.end_text_editing(&owner);
                }
            }
            cx.notify();
        });
    }

    /// The sheet over the window, if one is up: a dimmed backdrop that
    /// takes every click, and the card. Recorded in `scope` (the root's
    /// last container, so the window's own paths stay put).
    pub(crate) fn render_sheet(
        &mut self,
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let mut sheet = self.sheet.sheet.take()?;
        // A field inside the card has the keyboard: the document's keys
        // are off.
        let editing =
            self.sheet.focus.contains_focused(window, cx) && !self.sheet.focus.is_focused(window);
        self.set_sheet_editing(editing, cx);
        let label = sheet.label();
        let buttons = sheet.buttons(cx);
        let mut card_scope = scope.container();
        let body = sheet.render(&mut card_scope.container(), window, cx);
        let mut row_scope = card_scope.container();
        let row = buttons_row(&buttons, &mut row_scope, |i, b| {
            b.on_click(cx.listener(move |this, _, window, cx| {
                this.press_sheet_button(i, window, cx);
                cx.stop_propagation();
            }))
        });
        self.sheet.sheet = Some(sheet);
        let card = card()
            .id("sheet-card")
            .track_focus(&self.sheet.focus)
            .child(body)
            .child(row)
            .child(card_scope.this("sheet", &label, true));
        Some(backdrop().child(card).into_any_element())
    }
}

/// The dimmed backdrop over a window, taking every click.
fn backdrop() -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .bg(rgba(0x0000_0099))
        .flex()
        .items_center()
        .justify_center()
}

/// The card's look.
fn card() -> Div {
    div()
        .relative()
        .w(px(CARD_WIDTH))
        .max_w_full()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded_lg()
        .border_1()
        .border_color(rgb(colors::BORDER))
        .bg(rgb(CARD))
        .text_color(rgb(colors::TEXT))
        .text_sm()
        .cursor_default()
}

/// The buttons at the card's bottom right, each recorded in `scope` and
/// wired by `wire` (an enabled one only).
fn buttons_row(
    buttons: &[SheetButton],
    scope: &mut RegistryScope,
    mut wire: impl FnMut(usize, Stateful<Div>) -> Stateful<Div>,
) -> Div {
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .justify_end()
        .gap_2()
        .children(buttons.iter().enumerate().map(|(i, b)| {
            let face = button_face(i, b).child(scope.control("button", &b.label, b.enabled));
            if b.enabled { wire(i, face) } else { face }
        }))
}

fn button_face(i: usize, b: &SheetButton) -> Stateful<Div> {
    let background = match b.role {
        ButtonRole::Default if b.enabled => DEFAULT_BUTTON,
        ButtonRole::Destructive if b.enabled => DESTRUCTIVE_BUTTON,
        _ => colors::BUTTON,
    };
    div()
        .id(("sheet-button", i))
        .relative()
        .h(px(28.))
        .min_w(px(80.))
        .px_3()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(rgb(background))
        .text_color(rgb(if b.enabled {
            colors::TEXT
        } else {
            colors::DISABLED
        }))
        .when(b.enabled, |d| d.cursor_pointer())
        .child(b.label.clone())
}

// ---------------------------------------------------------------------
// Asks: a question with buttons

/// A question with buttons (Swift's `NSAlert`s and SwiftUI `.alert`s):
/// the bold message, the informative text, the buttons in order.
#[derive(Debug, Clone, PartialEq)]
pub struct Ask {
    pub level: PromptLevel,
    pub message: String,
    pub detail: Option<String>,
    pub buttons: Vec<SheetButton>,
}

impl Ask {
    /// An alert: its title, its message, and OK.
    pub fn alert(level: PromptLevel, title: &str, message: &str) -> Ask {
        Ask {
            level,
            message: title.to_owned(),
            detail: (!message.is_empty()).then(|| message.to_owned()),
            buttons: vec![SheetButton::new("OK", ButtonRole::Default)],
        }
    }

    /// A question with `labels` as its buttons: the first the default, a
    /// "Cancel" the cancel one (as GPUI's prompt takes them).
    pub fn with_labels(
        level: PromptLevel,
        message: &str,
        detail: Option<&str>,
        labels: &[&str],
    ) -> Ask {
        let buttons: Vec<PromptButton> = labels.iter().map(|l| PromptButton::from(*l)).collect();
        Ask::from_prompt(level, message, detail, &buttons)
    }

    /// A disabled button cannot be shown by the platform's prompt: the
    /// ask shows as a sheet.
    pub fn needs_sheet(&self) -> bool {
        self.buttons.iter().any(|b| !b.enabled)
    }

    /// The buttons as GPUI's prompt takes them (the cancel one is Esc's on
    /// macOS; the first is Return's).
    pub fn prompt_buttons(&self) -> Vec<PromptButton> {
        self.buttons
            .iter()
            .map(|b| match b.role {
                ButtonRole::Cancel => PromptButton::cancel(b.label.clone()),
                _ => PromptButton::new(b.label.clone()),
            })
            .collect()
    }

    /// The ask GPUI's prompt builder is handed (the first button is the
    /// default, a Cancel one the cancel).
    pub fn from_prompt(
        level: PromptLevel,
        message: &str,
        detail: Option<&str>,
        buttons: &[PromptButton],
    ) -> Ask {
        Ask {
            level,
            message: message.to_owned(),
            detail: detail.map(str::to_owned),
            buttons: buttons
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let role = if b.is_cancel() {
                        ButtonRole::Cancel
                    } else if i == 0 {
                        ButtonRole::Default
                    } else {
                        ButtonRole::Plain
                    };
                    SheetButton::new(b.label().to_string(), role)
                })
                .collect(),
        }
    }

    /// The message and informative text, recorded as texts.
    fn body(&self, scope: &mut RegistryScope) -> Div {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .relative()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(self.message.clone())
                    .child(scope.control("text", &self.message, true)),
            )
            .children(self.detail.as_ref().map(|d| {
                div()
                    .relative()
                    .text_color(rgb(colors::DIM))
                    .child(d.clone())
                    .child(scope.control("text", d, true))
            }))
    }
}

/// An [`Ask`] in a window's sheet: a press answers its index and closes
/// it. Dropped unanswered (its window closed), the answer is cancelled.
pub struct AskSheet {
    ask: Ask,
    answer: Option<oneshot::Sender<usize>>,
}

impl AskSheet {
    pub fn new(ask: Ask) -> (AskSheet, oneshot::Receiver<usize>) {
        let (tx, rx) = oneshot::channel();
        (
            AskSheet {
                ask,
                answer: Some(tx),
            },
            rx,
        )
    }
}

impl Sheet for AskSheet {
    fn label(&self) -> String {
        self.ask.message.clone()
    }

    fn render(&mut self, scope: &mut RegistryScope, _: &mut Window, _: &mut App) -> AnyElement {
        self.ask.body(scope).into_any_element()
    }

    fn buttons(&self, _: &App) -> Vec<SheetButton> {
        self.ask.buttons.clone()
    }

    fn press(&mut self, index: usize, _: &mut Window, _: &mut App) -> bool {
        if let Some(tx) = self.answer.take() {
            tx.send(index).ok();
        }
        true
    }
}

/// The key an [`Ask`] card answers to: Return its default button, Esc
/// its cancel one.
fn ask_key(ask: &Ask, key: &Keystroke) -> Option<usize> {
    let m = &key.modifiers;
    if m.control || m.alt || m.platform || m.function || m.shift {
        return None;
    }
    match key.key.as_str() {
        "enter" => default_index(&ask.buttons),
        "escape" => cancel_index(&ask.buttons),
        _ => None,
    }
}

// ---------------------------------------------------------------------
// The app-level dialog window (no window is open)

/// A small window showing one [`Ask`] when no window is open (GPUI has no
/// window-less alert; Swift's store alerts were app-modal `NSAlert`s).
/// A press answers and closes it; closing it presses the cancel button
/// (an alert's OK), or answers nothing.
pub struct AskDialog {
    ask: Ask,
    answer: Option<oneshot::Sender<usize>>,
    focus: FocusHandle,
}

/// The dialog window's size.
pub const DIALOG_SIZE: (f32, f32) = (480., 220.);

impl AskDialog {
    /// Opens the dialog window for `ask`: its handle and the answer.
    pub fn open(
        ask: Ask,
        cx: &mut App,
    ) -> Option<(gpui::WindowHandle<AskDialog>, oneshot::Receiver<usize>)> {
        let (tx, rx) = oneshot::channel();
        let bounds =
            gpui::Bounds::centered(None, gpui::size(px(DIALOG_SIZE.0), px(DIALOG_SIZE.1)), cx);
        let title = ask.message.clone();
        let handle = cx
            .open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some(title.into()),
                        ..Default::default()
                    }),
                    is_resizable: false,
                    ..Default::default()
                },
                move |window, cx| {
                    let focus = cx.focus_handle();
                    window.focus(&focus, cx);
                    let weak_ok = cx.new(|_| AskDialog {
                        ask,
                        answer: Some(tx),
                        focus,
                    });
                    let weak = weak_ok.downgrade();
                    window.on_window_should_close(cx, move |_, cx| {
                        weak.update(cx, |d, _| {
                            if let Some(i) = cancel_index(&d.ask.buttons)
                                && let Some(tx) = d.answer.take()
                            {
                                tx.send(i).ok();
                            }
                        })
                        .ok();
                        true
                    });
                    weak_ok
                },
            )
            .ok()?;
        handle
            .update(cx, |_, window, _| window.activate_window())
            .ok();
        Some((handle, rx))
    }

    /// The ask it shows.
    pub fn ask(&self) -> &Ask {
        &self.ask
    }

    /// Presses button `index` (an enabled one): answers, and the window
    /// closes.
    pub fn press(&mut self, index: usize, window: &mut Window) {
        if !self.ask.buttons.get(index).is_some_and(|b| b.enabled) {
            return;
        }
        if let Some(tx) = self.answer.take() {
            tx.send(index).ok();
        }
        window.remove_window();
    }

    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = ask_key(&self.ask, &e.keystroke) {
            self.press(i, window);
            cx.stop_propagation();
        }
    }
}

impl Render for AskDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let registry = crate::registry::SharedRegistry::default();
        let mut scope = RegistryScope::root(&registry);
        let row = buttons_row(&self.ask.buttons, &mut scope.container(), |i, b| {
            b.on_click(cx.listener(move |this, _, window, _| this.press(i, window)))
        });
        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .gap_3()
            .p_4()
            .bg(rgb(CARD))
            .text_color(rgb(colors::TEXT))
            .text_sm()
            .child(self.ask.body(&mut scope.container()))
            .child(row)
    }
}

// ---------------------------------------------------------------------
// A sheet in a dialog window of its own (no window is open)

/// A [`Sheet`] in a small window of its own, for when no document window
/// is open (the New Project format panel, shown before its window
/// exists): the sheet's body and buttons, Return its default button and
/// Esc its cancel one. A press that closes the sheet closes the window;
/// closing the window presses the cancel button.
pub struct SheetDialog {
    sheet: Option<Box<dyn Sheet>>,
    focus: FocusHandle,
    registry: crate::registry::SharedRegistry,
}

impl SheetDialog {
    /// Opens the dialog window showing `sheet`.
    pub fn open(sheet: Box<dyn Sheet>, cx: &mut App) -> Option<gpui::WindowHandle<SheetDialog>> {
        let bounds =
            gpui::Bounds::centered(None, gpui::size(px(DIALOG_SIZE.0), px(DIALOG_SIZE.1)), cx);
        let title = sheet.label();
        let handle = cx
            .open_window(
                gpui::WindowOptions {
                    window_bounds: Some(gpui::WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some(title.into()),
                        ..Default::default()
                    }),
                    is_resizable: false,
                    ..Default::default()
                },
                move |window, cx| {
                    let focus = cx.focus_handle();
                    window.focus(&focus, cx);
                    let dialog = cx.new(|_| SheetDialog {
                        sheet: Some(sheet),
                        focus,
                        registry: Default::default(),
                    });
                    let weak = dialog.downgrade();
                    window.on_window_should_close(cx, move |window, cx| {
                        weak.update(cx, |d, cx| {
                            if let Some(mut sheet) = d.sheet.take()
                                && let Some(i) = cancel_index(&sheet.buttons(cx))
                            {
                                sheet.press(i, window, cx);
                            }
                        })
                        .ok();
                        true
                    });
                    dialog
                },
            )
            .ok()?;
        handle
            .update(cx, |_, window, _| window.activate_window())
            .ok();
        Some(handle)
    }

    /// The label of the sheet it shows.
    pub fn label(&self) -> Option<String> {
        self.sheet.as_ref().map(|s| s.label())
    }

    /// What it drew last frame.
    pub fn registry(&self) -> &crate::registry::SharedRegistry {
        &self.registry
    }

    /// Presses the sheet's button `index` (an enabled one); the window
    /// closes when the sheet does.
    pub fn press(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut sheet) = self.sheet.take() else {
            return;
        };
        let enabled = sheet.buttons(cx).get(index).is_some_and(|b| b.enabled);
        if enabled && sheet.press(index, window, cx) {
            window.remove_window();
        } else {
            self.sheet = Some(sheet);
        }
        cx.notify();
    }

    fn key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sheet) = &self.sheet else { return };
        let m = &e.keystroke.modifiers;
        if m.control || m.alt || m.platform || m.function || m.shift {
            return;
        }
        let buttons = sheet.buttons(cx);
        let i = match e.keystroke.key.as_str() {
            "enter" => default_index(&buttons),
            "escape" => cancel_index(&buttons),
            _ => return,
        };
        if let Some(i) = i {
            self.press(i, window, cx);
        }
        cx.stop_propagation();
    }
}

impl Render for SheetDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut root = RegistryScope::root(&self.registry);
        let mut card = root.container();
        let mut body = None;
        let mut buttons = vec![];
        let mut label = String::new();
        if let Some(mut sheet) = self.sheet.take() {
            label = sheet.label();
            buttons = sheet.buttons(cx);
            body = Some(sheet.render(&mut card.container(), window, cx));
            self.sheet = Some(sheet);
        }
        let row = buttons_row(&buttons, &mut card.container(), |i, b| {
            b.on_click(cx.listener(move |this, _, window, cx| this.press(i, window, cx)))
        });
        div()
            .relative()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .gap_3()
            .p_4()
            .bg(rgb(CARD))
            .text_color(rgb(colors::TEXT))
            .text_sm()
            .children(body)
            .child(row)
            .child(card.this("sheet", &label, true))
    }
}

// ---------------------------------------------------------------------
// GPUI's prompts off macOS

/// GPUI's prompts drawn as the sheet card (off macOS, where the platform
/// has no prompt and GPUI's own fallback has no keys): Return presses the
/// first button, Esc the Cancel one.
pub fn use_sheet_prompts(cx: &mut App) {
    cx.set_prompt_builder(prompt_card);
}

fn prompt_card(
    level: PromptLevel,
    message: &str,
    detail: Option<&str>,
    buttons: &[PromptButton],
    handle: PromptHandle,
    window: &mut Window,
    cx: &mut App,
) -> RenderablePromptHandle {
    let ask = Ask::from_prompt(level, message, detail, buttons);
    let view = cx.new(|cx| PromptCard {
        ask,
        focus: cx.focus_handle(),
    });
    handle.with_view(view, window, cx)
}

/// A GPUI prompt drawn as the sheet card.
pub struct PromptCard {
    ask: Ask,
    focus: FocusHandle,
}

impl EventEmitter<PromptResponse> for PromptCard {}

impl Focusable for PromptCard {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for PromptCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let registry = crate::registry::SharedRegistry::default();
        let mut scope = RegistryScope::root(&registry);
        let row = buttons_row(&self.ask.buttons, &mut scope.container(), |i, b| {
            b.on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(PromptResponse(i));
                cx.stop_propagation();
            }))
        });
        let body = self.ask.body(&mut scope.container());
        backdrop().child(
            card()
                .id("prompt-card")
                .track_focus(&self.focus)
                .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                    if let Some(i) = ask_key(&this.ask, &e.keystroke) {
                        cx.emit(PromptResponse(i));
                        cx.stop_propagation();
                    }
                }))
                .child(body)
                .child(row),
        )
    }
}

/// Keeps every key of a window with a sheet up inside the sheet
/// ([`DocumentWindow::sheet_key`]). Interceptors run before any binding
/// matches.
pub fn install(cx: &mut App) {
    cx.intercept_keystrokes(|e, window, cx| {
        let Some(Some(root)) = window.root::<DocumentWindow>() else {
            return;
        };
        if root.update(cx, |v, cx| v.sheet_key(&e.keystroke, window, cx)) {
            cx.stop_propagation();
        }
    })
    .detach();
}
