//! The app's alerts and questions as prompts (slice 7b Task 2; Swift
//! `DocumentAlerts`, `StoreAlerts`, `RecoveryAlerts`, `ReturnPrompts`):
//!
//! - a document's alert: a modal prompt with OK on its own window (the
//!   front window once it is closed); a store alert (New, Open, a file
//!   that landed) on the front window. One at a time per window, in the
//!   order posted; one whose window closes before it is answered shows
//!   again on the front window.
//! - the recovery questions (Open with Plugins Off?, Restore Autosave)
//!   before a project opens, on the front window; the answer opens it.
//! - the DAWproject return prompt (New Project, Merge With Existing
//!   Project… shown disabled until slice 7c, Cancel), as a sheet on the
//!   front window (a platform prompt cannot disable a button).
//! - Recovered Recordings on a project's window once it opens.
//!
//! With no window open at all, each shows in a small dialog window of its
//! own ([`AskDialog`]). Under the control channel nothing shows: alerts
//! stay for the channel to report, and it answers the questions
//! ([`crate::control`]).
//!
//! [`present`] runs whenever the app state notifies.

use std::collections::{HashMap, HashSet};

use futures::channel::oneshot;
use gpui::{AnyWindowHandle, App, Global, PromptLevel};
use vizdaw_app::DocId;
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::document::{RECOVERY_IMPORT, RECOVERY_LATER, RECOVERY_TITLE, RECOVERY_TRASH};
use vizdaw_app::store::{CrashChoice, OpenQuestion, ReturnChoice, ReturnPrompt};

use crate::app_state::AppState;
use crate::sheet_host::{Ask, AskDialog, AskSheet, ButtonRole, SheetButton};
use crate::shell::DocumentWindow;

/// What a window shows of ours.
#[derive(Debug, Clone, PartialEq)]
pub enum Showing {
    /// An alert (title and message), with the document it came from.
    Alert(Option<DocId>, AlertItem),
    /// A recovery question.
    Question(OpenQuestion),
    /// The DAWproject return prompt.
    Return,
    /// Recovered Recordings for this document.
    Recovery(DocId),
    /// A question the app asks ([`ask`]: save changes, quit, replace),
    /// by its message.
    Asked(String),
}

#[derive(Default)]
struct Presenter {
    /// The windows showing one of ours (the dialog window too).
    busy: HashMap<AnyWindowHandle, Showing>,
    /// The windows that are dialog windows ([`AskDialog`]).
    dialogs: HashSet<AnyWindowHandle>,
    /// The app's own questions waiting for their window, oldest first.
    asks: Vec<(AnyWindowHandle, Ask, oneshot::Sender<usize>)>,
}

impl Global for Presenter {}

impl Presenter {
    fn asking(&self, what: impl Fn(&Showing) -> bool) -> bool {
        self.busy.values().any(what)
    }
}

/// One of our prompts is up, or an app question waits for its window (a
/// window losing the keyboard to it is not the app leaving). Only open
/// windows count: a question for a window that closed never shows.
pub(crate) fn prompting(cx: &App) -> bool {
    let Some(p) = cx.try_global::<Presenter>() else {
        return false;
    };
    let open = cx.windows();
    p.busy.keys().any(|w| open.contains(w)) || p.asks.iter().any(|(w, _, _)| open.contains(w))
}

/// What each window shows of ours now.
pub fn showing(cx: &App) -> Vec<(AnyWindowHandle, Showing)> {
    cx.try_global::<Presenter>()
        .map(|p| p.busy.iter().map(|(w, s)| (*w, s.clone())).collect())
        .unwrap_or_default()
}

/// The dialog windows open now (shown while no other window is open).
pub fn dialogs(cx: &App) -> Vec<AnyWindowHandle> {
    cx.try_global::<Presenter>()
        .map(|p| p.dialogs.iter().copied().collect())
        .unwrap_or_default()
}

/// Shows whatever waits each time the app state notifies.
pub fn install(state: &gpui::Entity<AppState>, cx: &mut App) {
    cx.default_global::<Presenter>();
    cx.observe(state, |_, cx| present(cx)).detach();
}

/// Where a prompt shows.
#[derive(Clone, Copy)]
enum Target {
    Window(AnyWindowHandle),
    /// No window is open: a dialog window of its own.
    Dialog,
}

/// Shows what waits and can show now: the alerts in order, then a
/// recovery question, then the return prompt, then each project's
/// Recovered Recordings, then the sheets that show by themselves (Import
/// MIDI File once its read landed, an export MIDI sheet its failure
/// brought back, Relink). Call it outside any update.
pub fn present(cx: &mut App) {
    // The app's own questions (asked by a key or menu) show under the
    // channel too.
    present_asks(cx);
    if crate::control::channel_on(cx) {
        // Nothing shows; the Relink offer still forgets items back online.
        crate::sheets::relink::present(cx, |_, _| false);
        crate::sheets::import_midi::present(cx, |_, _| false);
        crate::sheets::export_midi::present(cx, |_, _| false);
        crate::sheets::for_daw::present(cx, |_, _| false);
        crate::sheets::dawproject::present(cx, |_, _| false);
        crate::sheets::report::present(cx, |_, _| false);
        return;
    }
    let app = AppState::global(cx);
    app.update(cx, |s, _| s.store.collect_document_alerts());
    let waiting: Vec<(Option<DocId>, AlertItem)> = app
        .read(cx)
        .store
        .alerts()
        .map(|(d, a)| (d, a.clone()))
        .collect();
    for (doc, item) in waiting {
        let target = target(doc, cx);
        if busy(target, cx) {
            continue;
        }
        let level = if doc.is_some() {
            PromptLevel::Info
        } else {
            PromptLevel::Warning
        };
        let ask = Ask::alert(level, &item.title, &item.message);
        let Some(taken) = app.update(cx, |s, _| s.store.take_alert_id(item.id)) else {
            continue;
        };
        let shown = show(target, &ask, Showing::Alert(doc, taken.clone()), cx);
        match shown {
            Some(answer) => cx
                .spawn(async move |cx| {
                    let answered = answer.await.is_ok();
                    cx.update(|cx| {
                        if !answered {
                            // Its window closed first: it shows again.
                            AppState::global(cx)
                                .update(cx, |s, _| s.store.return_alert(doc, taken));
                        }
                        present(cx);
                    });
                })
                .detach(),
            None => app.update(cx, |s, _| s.store.return_alert(doc, taken)),
        }
    }
    present_question(cx);
    present_return(cx);
    present_recovery(cx);
    // Last: the sheet would hold back what waits for its window.
    crate::sheets::import_midi::present(cx, |w, cx| !busy(Target::Window(w), cx));
    crate::sheets::export_midi::present(cx, |w, cx| !busy(Target::Window(w), cx));
    crate::sheets::for_daw::present(cx, |w, cx| !busy(Target::Window(w), cx));
    crate::sheets::dawproject::present(cx, |w, cx| !busy(Target::Window(w), cx));
    crate::sheets::report::present(cx, |w, cx| !busy(Target::Window(w), cx));
    crate::sheets::relink::present(cx, |w, cx| !busy(Target::Window(w), cx));
}

/// Asks `ask` in `window` once nothing else of ours (or a GPUI prompt)
/// shows there: the save-changes, quit and replace questions go through
/// here, so no prompt ever replaces another (GPUI's in-window prompt off
/// macOS replaces the one up). The answer is its button; a window that
/// closes first drops it. Can be called inside a window's update.
pub fn ask(window: AnyWindowHandle, ask: Ask, cx: &mut App) -> oneshot::Receiver<usize> {
    let (tx, rx) = oneshot::channel();
    presenter(cx).asks.push((window, ask, tx));
    cx.defer(present);
    rx
}

fn present_asks(cx: &mut App) {
    let asks = std::mem::take(&mut presenter(cx).asks);
    let open = cx.windows();
    let mut left = vec![];
    for (window, ask, tx) in asks {
        // Its asker went, or its window closed: dropped (the asker reads
        // Cancel).
        if tx.is_canceled() || !open.contains(&window) {
            continue;
        }
        // One at a time per window, in order.
        if left.iter().any(|(w, _, _)| *w == window) || prompt_up(window, false, cx) {
            left.push((window, ask, tx));
            continue;
        }
        let message = ask.message.clone();
        // A window that went drops the answer.
        if let Some(answer) = show(Target::Window(window), &ask, Showing::Asked(message), cx) {
            cx.spawn(async move |cx| {
                if let Ok(i) = answer.await {
                    tx.send(i).ok();
                }
                // The window is free: what waits for it shows (after the
                // asker's own answer has run).
                cx.update(|cx| cx.defer(present));
            })
            .detach();
        }
    }
    let p = presenter(cx);
    left.append(&mut p.asks);
    p.asks = left;
}

fn present_question(cx: &mut App) {
    let Some(q) = AppState::global(cx).read(cx).store.open_question().cloned() else {
        return;
    };
    if presenter(cx).asking(|s| matches!(s, Showing::Question(_))) {
        return;
    }
    let target = target(None, cx);
    if busy(target, cx) {
        return;
    }
    let ask = question_ask(&q);
    let Some(answer) = show(target, &ask, Showing::Question(q.clone()), cx) else {
        return;
    };
    cx.spawn(async move |cx| {
        let answer = answer.await;
        cx.update(|cx| {
            if let Ok(i) = answer {
                answer_question(&q, i, cx);
            }
            present(cx);
        });
    })
    .detach();
}

/// The recovery question as an ask (Swift `RecoveryAlerts`): the crash
/// question in the warning style, its Cancel Esc's.
pub fn question_ask(q: &OpenQuestion) -> Ask {
    let buttons = q
        .buttons()
        .into_iter()
        .enumerate()
        .map(|(i, label)| {
            let role = if i == 0 {
                ButtonRole::Default
            } else if label == "Cancel" {
                ButtonRole::Cancel
            } else {
                ButtonRole::Plain
            };
            SheetButton::new(label, role)
        })
        .collect();
    Ask {
        level: match q {
            OpenQuestion::AfterCrash { .. } => PromptLevel::Warning,
            OpenQuestion::RestoreAutosave { .. } => PromptLevel::Info,
        },
        message: q.message(),
        detail: Some(q.informative().to_owned()),
        buttons,
    }
}

/// Answers the question with its button `i`; the project, if it opens,
/// shows its window.
fn answer_question(q: &OpenQuestion, i: usize, cx: &mut App) {
    let app = AppState::global(cx);
    let opened = app.update(cx, |s, cx| {
        cx.notify();
        match q {
            OpenQuestion::AfterCrash { .. } => {
                let choice = match i {
                    0 => CrashChoice::PluginsOff,
                    1 => CrashChoice::Normally,
                    3 => CrashChoice::Disable,
                    _ => CrashChoice::Cancel,
                };
                s.store.answer_crash(choice)
            }
            OpenQuestion::RestoreAutosave { .. } => s.store.answer_autosave(i == 0),
        }
    });
    if let Some(doc) = opened {
        crate::show_or_front(doc, cx);
    }
}

fn present_return(cx: &mut App) {
    let Some(prompt) = AppState::global(cx).read(cx).store.return_prompt().cloned() else {
        return;
    };
    if presenter(cx).asking(|s| *s == Showing::Return) {
        return;
    }
    let target = target(None, cx);
    if busy(target, cx) {
        return;
    }
    let ask = return_ask(&prompt);
    let Some(answer) = show(target, &ask, Showing::Return, cx) else {
        return;
    };
    cx.spawn(async move |cx| {
        let answer = answer.await;
        cx.update(|cx| {
            if let Ok(i) = answer {
                let choice = if i == 0 {
                    ReturnChoice::NewProject
                } else {
                    ReturnChoice::Cancel
                };
                AppState::global(cx).update(cx, |s, cx| {
                    s.store.answer_return(choice);
                    cx.notify();
                });
                // A new project's window opens.
                crate::control::files::settle(cx);
            }
            present(cx);
        });
    })
    .detach();
}

/// The return prompt as an ask (Swift `ReturnPrompts`): Merge With
/// Existing Project… is shown, disabled, until slice 7c.
pub fn return_ask(p: &ReturnPrompt) -> Ask {
    Ask {
        level: PromptLevel::Info,
        message: ReturnPrompt::MESSAGE.to_owned(),
        detail: Some(p.informative()),
        buttons: vec![
            SheetButton::new(ReturnPrompt::NEW_PROJECT, ButtonRole::Default),
            SheetButton::new(ReturnPrompt::MERGE, ButtonRole::Plain).disabled(),
            SheetButton::new(ReturnPrompt::CANCEL, ButtonRole::Cancel),
        ],
    }
}

fn present_recovery(cx: &mut App) {
    let app = AppState::global(cx);
    let offers: Vec<(DocId, AnyWindowHandle, String)> = {
        let s = app.read(cx);
        s.document_windows()
            .filter_map(|w| {
                let message = s.store.document(w.doc)?.recovery_offer()?;
                Some((w.doc, w.handle.into(), message))
            })
            .collect()
    };
    for (doc, window, message) in offers {
        if presenter(cx).asking(|s| *s == Showing::Recovery(doc)) {
            continue;
        }
        let target = Target::Window(window);
        if busy(target, cx) {
            continue;
        }
        let ask = Ask {
            level: PromptLevel::Info,
            message: RECOVERY_TITLE.to_owned(),
            detail: Some(message),
            buttons: vec![
                SheetButton::new(RECOVERY_IMPORT, ButtonRole::Default),
                SheetButton::new(RECOVERY_TRASH, ButtonRole::Destructive),
                SheetButton::new(RECOVERY_LATER, ButtonRole::Cancel),
            ],
        };
        let Some(answer) = show(target, &ask, Showing::Recovery(doc), cx) else {
            continue;
        };
        cx.spawn(async move |cx| {
            let answer = answer.await;
            cx.update(|cx| {
                if let Ok(i) = answer {
                    let import = match i {
                        0 => Some(true),
                        1 => Some(false),
                        _ => None,
                    };
                    AppState::global(cx).update(cx, |s, cx| {
                        s.store.with_document(doc, |d, _| d.answer_recovery(import));
                        cx.notify();
                    });
                }
                present(cx);
            });
        })
        .detach();
    }
}

fn presenter(cx: &mut App) -> &mut Presenter {
    cx.default_global::<Presenter>()
}

/// The window an alert of `doc` (`None`: a store alert or question)
/// shows in: `doc`'s own window, else the key window, else the front
/// document's, else any; with none open, a dialog window.
fn target(doc: Option<DocId>, cx: &App) -> Target {
    let s = AppState::global(cx).read(cx);
    if let Some(w) = doc.and_then(|d| s.window_of(d)) {
        return Target::Window(w.handle.into());
    }
    // A window whose document the store closed is about to go.
    let open = |w: &&crate::app_state::WindowEntry| s.store.document(w.doc).is_some();
    let key = s
        .store
        .key_window()
        .and_then(|(_, token)| s.windows().iter().find(|w| w.token == token))
        .filter(open);
    let front = s.store.front().and_then(|d| s.window_of(d));
    match key.or(front).or_else(|| s.windows().iter().find(open)) {
        Some(w) => Target::Window(w.handle.into()),
        None => Target::Dialog,
    }
}

/// The front window a question of the app's shows on (the key window,
/// else the front document's, else any); `None` with no window open.
pub(crate) fn front_window(cx: &App) -> Option<AnyWindowHandle> {
    match target(None, cx) {
        Target::Window(w) => Some(w),
        Target::Dialog => None,
    }
}

/// Whether `target` already shows one of ours (with no window open: a
/// dialog window is up).
fn busy(target: Target, cx: &mut App) -> bool {
    match target {
        Target::Window(w) => prompt_up(w, true, cx),
        Target::Dialog => !presenter(cx).dialogs.is_empty(),
    }
}

/// Whether window `w` shows a prompt now: one of ours, an app question
/// waiting for it, a GPUI prompt drawn in it, or (`sheets`) a sheet.
fn prompt_up(w: AnyWindowHandle, sheets: bool, cx: &mut App) -> bool {
    let p = presenter(cx);
    if p.busy.contains_key(&w) || (sheets && p.asks.iter().any(|(a, _, _)| *a == w)) {
        return true;
    }
    if sheets
        && let Some(d) = w.downcast::<DocumentWindow>()
        && d.read(cx).is_ok_and(|v| v.sheet_label().is_some())
    {
        return true;
    }
    // A window being updated cannot be asked now.
    w.update(cx, |_, window, _| window.has_active_prompt())
        .unwrap_or(true)
}

/// Shows `ask` at `target` as `what`: the answer, or `None` when it
/// cannot show (its window went, or a sheet is up there).
fn show(
    target: Target,
    ask: &Ask,
    what: Showing,
    cx: &mut App,
) -> Option<oneshot::Receiver<usize>> {
    let (window, answer, dialog) = match target {
        Target::Window(w) => (w, ask_in(w, ask, cx)?, false),
        Target::Dialog => {
            let (h, answer) = AskDialog::open(ask.clone(), cx)?;
            (h.into(), answer, true)
        }
    };
    let p = presenter(cx);
    p.busy.insert(window, what);
    if dialog {
        p.dialogs.insert(window);
    }
    // The window is free again once answered (or gone).
    let (tx, rx) = oneshot::channel();
    cx.spawn(async move |cx| {
        let answered = answer.await;
        cx.update(|cx| {
            let p = presenter(cx);
            p.busy.remove(&window);
            p.dialogs.remove(&window);
        });
        if let Ok(i) = answered {
            tx.send(i).ok();
        }
    })
    .detach();
    Some(rx)
}

/// `ask` in window `w`: the platform's prompt (macOS: the window's alert
/// sheet), or a sheet when a button is disabled.
fn ask_in(w: AnyWindowHandle, ask: &Ask, cx: &mut App) -> Option<oneshot::Receiver<usize>> {
    if ask.needs_sheet() {
        let w = w.downcast::<DocumentWindow>()?;
        let (sheet, answer) = AskSheet::new(ask.clone());
        let shown = w
            .update(cx, |v, window, cx| {
                v.show_sheet(Box::new(sheet), window, cx).is_ok()
            })
            .ok()?;
        return shown.then_some(answer);
    }
    let buttons = ask.prompt_buttons(crate::sheet_host::native_prompts(cx));
    w.update(cx, |_, window, cx| {
        window.prompt(ask.level, &ask.message, ask.detail.as_deref(), &buttons, cx)
    })
    .ok()
}
