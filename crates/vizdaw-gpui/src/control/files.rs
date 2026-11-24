//! The File commands over the control channel (Swift `ControlCommands`
//! file half, `InterchangeCommands.run`, `BounceCommands`): nothing here
//! opens a panel, a sheet or a prompt.
//!
//! A command that starts a job answers when the job ends: the pump keeps
//! a [`PendingJob`] per call, polls `session.job` itself (a job the
//! channel started is not in the document's job list, as in Swift) and
//! replies exactly once — done, failed, cancelled, the document gone, or
//! [`JOB_WAIT_MS`] passed (the server's app timeout is extended to
//! 600 s + 5 s first). A file read or a dry run runs off the UI thread
//! on the background executor ([`PendingCall`]); its answer lands on the
//! main thread by its generation, and the pump replies with it (or
//! starts the job it leads to). A project a command made shows its
//! window only once its job succeeded, and the reply names that window;
//! a failed or cancelled one closes again while untouched.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::{Duration, Instant};

use core_control::registry::ArgValue;
use core_control::request::{CrashAnswer, RecoveryAnswers};
use core_control::{ControlError, ErrorCode};
use gpui::{App, Global};
use serde_json::Value;
use vizdaw_app::alerts::AlertItem;
use vizdaw_app::interchange_commands::{
    self as ic, DawprojectNext, DawprojectRead, FileCheck, InterchangeRequest, MidiIntoNewProject,
};
use vizdaw_app::jobs::{self, FileJob, JOB_WAIT_MS};
use vizdaw_app::mixer::commands::thrown;
use vizdaw_app::{DocId, DocSession, JobAnswer, bounce_commands, formats};
use vizdaw_ffi::control::{ControlResult, result_json};
use vizdaw_ffi::{
    AafImportRequestRow, AafInfoRow, DawprojectInfoRow, JobKindRow, MidiFileInfoRow, ReportRow,
    VizError,
};

use super::{CallSource, Handled, closed, doc_id, number, ran_json};
use crate::app_state::{AppState, WindowEntry};
use crate::shell::DocumentWindow;

/// How long past [`JOB_WAIT_MS`] the server waits for a job's reply, so
/// the app's own expiry answer is the one the client gets.
const EXTRA_MS: u64 = 5_000;

/// The document File commands this module runs (`command.run` with a
/// target document).
pub const DOC_IDS: [&str; 14] = [
    "media.import",
    "media.convert",
    "media.relink",
    "file.project_settings",
    bounce_commands::ID,
    ic::IMPORT_MIDI,
    ic::EXPORT_MIDI,
    ic::EXPORT_CLIP,
    ic::EXPORT_FOR_DAW,
    ic::IMPORT_DAWPROJECT,
    ic::EXPORT_DAWPROJECT,
    ic::IMPORT_AAF,
    ic::EXPORT_AAF,
    ic::REPORT,
];

/// The File commands that need no document: the new projects, the demos
/// and the opens.
pub const APP_IDS: [&str; 6] = [
    "file.new",
    "file.create_demo",
    "file.create_demo_audio",
    "file.open",
    "file.open_copy",
    "app.new_demo",
];

/// A job a call waits for.
struct PendingJob {
    token: u64,
    command: String,
    doc: DocId,
    job: u64,
    deadline: Instant,
}

/// What a background call read.
enum ReadOut {
    Report(Result<ReportRow, VizError>),
    Midi(Result<MidiFileInfoRow, VizError>),
    Dawproject(Result<DawprojectInfoRow, VizError>),
    Aaf(Result<AafInfoRow, VizError>),
}

/// What a background call's answer leads to.
enum Then {
    /// A dry run of `doc`: its report answers.
    Check { command: String, doc: DocId },
    /// `file.import_midi` with `new_project`.
    Midi(MidiIntoNewProject),
    /// `file.import_dawproject`.
    Dawproject(DawprojectRead),
    /// `file.import_aaf` with `new_project`.
    Aaf(AafImportRequestRow),
}

/// A call whose answer is read off the UI thread.
struct PendingCall {
    generation: u64,
    /// Set by the pump (a call made outside the pump has none and is
    /// dropped).
    token: Option<u64>,
    deadline: Option<Instant>,
    then: Then,
    /// The read, once it landed: `None` inside when its work panicked.
    landed: Option<Option<ReadOut>>,
}

/// The jobs and background calls the pump checks every tick.
#[derive(Default)]
struct Deferred {
    jobs: Vec<PendingJob>,
    calls: Vec<PendingCall>,
    next_generation: u64,
}

impl Global for Deferred {}

/// Pending replies of either kind (the pump's `Tick::deferred`).
pub(super) fn deferred_count(cx: &App) -> usize {
    cx.try_global::<Deferred>()
        .map_or(0, |d| d.jobs.len() + d.calls.len())
}

/// Takes what [`run`] or [`run_app`] answered later: a job joins the
/// list (its call may now wait 600 s + 5 s); a background call learns
/// its token.
pub(super) fn defer(calls: &impl CallSource, token: u64, handled: &Handled, cx: &mut App) {
    let now = cx.background_executor().now();
    let deadline = now + Duration::from_millis(JOB_WAIT_MS);
    match handled {
        Handled::Job { command, doc, job } => {
            calls.extend(token, Duration::from_millis(JOB_WAIT_MS + EXTRA_MS));
            cx.default_global::<Deferred>().jobs.push(PendingJob {
                token,
                command: command.clone(),
                doc: *doc,
                job: *job,
                deadline,
            });
        }
        Handled::Later(generation) => {
            calls.extend(token, Duration::from_millis(JOB_WAIT_MS + EXTRA_MS));
            if let Some(c) = cx
                .default_global::<Deferred>()
                .calls
                .iter_mut()
                .find(|c| c.generation == *generation)
            {
                c.token = Some(token);
                c.deadline = Some(deadline);
            }
        }
        Handled::Reply(_) | Handled::Quit => {}
    }
}

/// Answers every background call that landed or expired, then every job
/// that ended or expired: each exactly once, and dropped then.
pub(super) fn check(calls: &impl CallSource, cx: &mut App) {
    if !cx.has_global::<Deferred>() {
        return;
    }
    check_calls(calls, cx);
    check_jobs(calls, cx);
}

fn check_calls(calls: &impl CallSource, cx: &mut App) {
    let now = cx.background_executor().now();
    let pending = std::mem::take(&mut cx.global_mut::<Deferred>().calls);
    let mut kept = Vec::new();
    for c in pending {
        let Some(token) = c.token else { continue };
        if !calls.is_waiting(token) {
            continue;
        }
        match c.landed {
            Some(out) => {
                let handled = match out {
                    Some(out) => finish(c.then, out, cx),
                    None => Handled::Reply(Err(ControlError::new(
                        ErrorCode::Failed,
                        "the call failed while it ran",
                    ))),
                };
                match handled {
                    Handled::Reply(r) => {
                        calls.reply(token, r);
                    }
                    other => defer(calls, token, &other, cx),
                }
            }
            None if c.deadline.is_some_and(|d| now >= d) => {
                calls.reply(
                    token,
                    Err(ControlError::new(
                        ErrorCode::Timeout,
                        "the call did not end in time",
                    )),
                );
            }
            None => kept.push(c),
        }
    }
    // A call deferred while answering (none yet) is kept too.
    let d = cx.global_mut::<Deferred>();
    kept.append(&mut d.calls);
    d.calls = kept;
}

fn check_jobs(calls: &impl CallSource, cx: &mut App) {
    let now = cx.background_executor().now();
    let pending = std::mem::take(&mut cx.global_mut::<Deferred>().jobs);
    let mut kept = Vec::new();
    for j in pending {
        if !calls.is_waiting(j.token) {
            continue;
        }
        let expired = now >= j.deadline;
        match job_step(&j, expired, cx) {
            Some(r) => {
                calls.reply(j.token, r);
            }
            None => kept.push(j),
        }
    }
    let d = cx.global_mut::<Deferred>();
    kept.append(&mut d.jobs);
    d.jobs = kept;
}

/// Job `j`'s reply now, or `None` while it runs (Swift
/// `ControlChannel.whenJobEnds`).
fn job_step(j: &PendingJob, expired: bool, cx: &mut App) -> Option<Result<Value, ControlError>> {
    let app = AppState::global(cx);
    let doc = j.doc;
    let read = {
        let s = app.read(cx);
        let Some(d) = s.store.document(doc) else {
            return Some(Err(ControlError::new(
                ErrorCode::NoDocument,
                "the document closed",
            )));
        };
        d.session().job(j.job).map(|row| {
            let answer = d.job_answer(&row);
            (row, answer)
        })
    };
    let (row, answer) = match read {
        Ok(r) => r,
        // Busy: read again next tick.
        Err(VizError::Busy { .. }) if !expired => return None,
        Err(VizError::Busy { .. }) => {
            return Some(Err(ControlError::new(
                ErrorCode::Timeout,
                still_running(j.job),
            )));
        }
        Err(_) => {
            return Some(Err(ControlError::new(
                ErrorCode::Failed,
                "the job is unknown",
            )));
        }
    };
    match answer {
        JobAnswer::Wait if expired => Some(Err(ControlError::new(
            ErrorCode::Timeout,
            jobs::expired_message(&row),
        ))),
        JobAnswer::Wait => None,
        JobAnswer::Ok(status) => {
            app.update(cx, |s, cx| {
                s.store.with_document(doc, |d, _| d.refresh_after_job());
                cx.notify();
            });
            if !interchange(row.kind) {
                let window = window_number(doc, cx);
                let undo = undo_label(doc, cx);
                return Some(Ok(ran_value(&j.command, doc, window, undo, status)));
            }
            // A project the command made shows its window now, and the
            // reply names it; a window that fails to open is the call's
            // failure (the project closed with it).
            let made = app.read(cx).store.made_project(doc);
            let shown = if made {
                show(doc, cx).map(|_| ())
            } else {
                Ok(())
            };
            app.update(cx, |s, _| s.store.job_ended(doc, false, |_, _| {}));
            if let Err(e) = shown {
                return Some(Err(e));
            }
            let window = window_number(doc, cx);
            let undo = undo_label(doc, cx);
            Some(Ok(result_json(&ControlResult::Interchange {
                command: j.command.clone(),
                doc: doc_id(doc),
                window,
                undo_label: undo,
                report: row.report.clone(),
                written: row.written.clone(),
            })))
        }
        JobAnswer::Failed(message) => {
            // A failed import may still have changed nothing; a refresh
            // is harmless.
            let mut gone = None;
            app.update(cx, |s, cx| {
                s.store.with_document(doc, |d, _| d.refresh_after_job());
                s.store.job_ended(doc, true, |store, id| {
                    store.close(id);
                    gone = Some(id);
                });
                cx.notify();
            });
            if let Some(id) = gone {
                close_windows(id, cx);
            }
            Some(Err(ControlError::new(ErrorCode::Failed, message)))
        }
    }
}

/// The expiry text for a job whose row could not be read (Busy).
fn still_running(job: u64) -> String {
    format!(
        "job {job} is still running after {} minutes; it was not cancelled",
        JOB_WAIT_MS / 60_000
    )
}

/// Whether a job of `kind` answers as `interchange` (Swift's list).
fn interchange(kind: JobKindRow) -> bool {
    use JobKindRow as K;
    matches!(
        kind,
        K::ImportMidi
            | K::ExportMidi
            | K::ExportProject
            | K::ExportDawproject
            | K::ImportProject
            | K::MergeDawproject
            | K::ExportAaf
            | K::ImportAaf
    )
}

fn window_number(doc: DocId, cx: &App) -> Option<i64> {
    AppState::global(cx)
        .read(cx)
        .window_of(doc)
        .map(|w| number(w.handle))
}

fn undo_label(doc: DocId, cx: &App) -> Option<String> {
    AppState::global(cx)
        .read(cx)
        .store
        .document(doc)
        .and_then(|d| d.snapshot().undo_label.clone())
}

fn ran_value(
    command: &str,
    doc: DocId,
    window: Option<i64>,
    undo: Option<String>,
    status: Option<String>,
) -> Value {
    serde_json::json!({
        "command": command, "doc": doc_id(doc), "window": window,
        "undo": undo, "status": status,
    })
}

/// Closes `doc`'s windows (if it has any: a project the channel made has
/// none until its job succeeds) and forgets it.
pub(crate) fn close_windows(doc: DocId, cx: &mut App) {
    let app = AppState::global(cx);
    let handles: Vec<_> = app
        .read(cx)
        .windows()
        .iter()
        // Its plug-in windows close with it (before its session drops).
        .filter(|w| w.doc == doc && w.kind.is_document_window())
        .map(|w| w.handle)
        .collect();
    for h in handles {
        h.update(cx, |_, window, _| window.remove_window()).ok();
    }
    app.update(cx, |s, cx| {
        s.close_document(doc);
        cx.notify();
    });
}

// ---------------------------------------------------------------------
// The commands

/// Runs document File command `id` (one of [`DOC_IDS`]) on `entry`'s
/// document.
pub(super) fn run(
    id: &str,
    args: &[(String, ArgValue)],
    entry: &WindowEntry,
    cx: &mut App,
) -> Handled {
    match request(id, args, entry.doc, cx) {
        Ok(Ask::Job(job)) => start(id, entry.doc, &job, cx),
        Ok(Ask::Report) => Handled::Reply(report(id, entry.doc, cx)),
        Ok(Ask::Later(work, then)) => later(work, then, cx),
        Err(e) => Handled::Reply(Err(e)),
    }
}

/// Runs document File command `id` for a key (`ui.key`), where no caller
/// waits for a later reply: a command that would start a job or a
/// background read is refused before anything starts (it answers when it
/// ends: run it with `command.run`); one answered now (a dialog asked for,
/// the report) answers.
pub(super) fn run_now(
    id: &str,
    args: &[(String, ArgValue)],
    entry: &WindowEntry,
    cx: &mut App,
) -> Result<Value, ControlError> {
    match request(id, args, entry.doc, cx)? {
        Ask::Report => report(id, entry.doc, cx),
        Ask::Job(_) | Ask::Later(..) => Err(ControlError::new(
            ErrorCode::Unavailable,
            format!("{id} answers when it ends: run it with command.run"),
        )),
    }
}

/// What a document File command asks for once its arguments are read.
enum Ask {
    Job(FileJob),
    Report,
    Later(Box<dyn FnOnce() -> ReadOut + Send>, Then),
}

fn request(
    id: &str,
    args: &[(String, ArgValue)],
    doc: DocId,
    cx: &mut App,
) -> Result<Ask, ControlError> {
    let app = AppState::global(cx);
    let default = app.read(cx).store.default_format();
    let core = app.read(cx).store.core();
    app.update(cx, |s, _| {
        let d = s.store.document_mut(doc).ok_or_else(closed)?;
        // A result's `status` is this command's own.
        d.set_status(None);
        let d = s.store.document(doc).ok_or_else(closed)?;
        let session: Arc<dyn DocSession> = d.session().clone();
        Ok(match id {
            "media.import" => Ask::Job(jobs::media_import_request(args)?),
            "media.convert" => Ask::Job(FileJob::Convert),
            "media.relink" => Ask::Job(jobs::relink_request(args, d)?),
            "file.project_settings" => {
                let base = session.project_info().map_or(default, |i| i.format);
                Ask::Job(FileJob::Format(formats::project_settings_format(
                    args, base,
                )?))
            }
            bounce_commands::ID => Ask::Job(bounce_commands::job(args, d)?),
            _ => match ic::request(id, args, d)? {
                InterchangeRequest::Report => Ask::Report,
                InterchangeRequest::Job(job) => Ask::Job(job),
                InterchangeRequest::Check(check) => Ask::Later(
                    Box::new(move || ReadOut::Report(check_run(&check, session.as_ref()))),
                    Then::Check {
                        command: id.to_owned(),
                        doc,
                    },
                ),
                InterchangeRequest::MidiIntoNewProject(m) => {
                    let path = m.request.path.clone();
                    Ask::Later(
                        Box::new(move || ReadOut::Midi(session.midi_file_info(path))),
                        Then::Midi(m),
                    )
                }
                InterchangeRequest::ReadDawproject(read) => {
                    let path = read.path.clone();
                    Ask::Later(
                        Box::new(move || ReadOut::Dawproject(core.dawproject_info(&path))),
                        Then::Dawproject(read),
                    )
                }
                InterchangeRequest::AafIntoNewProject(req) => {
                    let path = req.path.clone();
                    Ask::Later(
                        Box::new(move || ReadOut::Aaf(core.aaf_info(&path))),
                        Then::Aaf(req),
                    )
                }
            },
        })
    })
}

fn check_run(check: &FileCheck, s: &dyn DocSession) -> Result<ReportRow, VizError> {
    check.run(s)
}

/// Starts `job` on `doc`: the reply waits for its end; a refusal is the
/// call's failure.
fn start(command: &str, doc: DocId, job: &FileJob, cx: &mut App) -> Handled {
    let app = AppState::global(cx);
    let started = {
        let s = app.read(cx);
        let Some(d) = s.store.document(doc) else {
            return Handled::Reply(Err(closed()));
        };
        job.start(d.session().as_ref())
    };
    match started {
        Ok(id) => Handled::Job {
            command: command.to_owned(),
            doc,
            job: id,
        },
        Err(e) => Handled::Reply(Err(thrown(&e, job.refusal_title()))),
    }
}

/// `file.interchange_report`: the session's last report, read once;
/// never a sheet.
fn report(command: &str, doc: DocId, cx: &App) -> Result<Value, ControlError> {
    let s = AppState::global(cx);
    let s = s.read(cx);
    let d = s.store.document(doc).ok_or_else(closed)?;
    let report = d
        .session()
        .interchange_report()
        .map_err(|e| thrown(&e, ic::REPORT_UNAVAILABLE))?;
    Ok(result_json(&ControlResult::Interchange {
        command: command.to_owned(),
        doc: doc_id(doc),
        window: None,
        undo_label: d.snapshot().undo_label.clone(),
        report,
        written: vec![],
    }))
}

/// Runs `work` on the background executor; its answer lands on the main
/// thread by its generation (an answer nobody waits for any more is
/// dropped) and the pump replies with it.
fn later(work: Box<dyn FnOnce() -> ReadOut + Send>, then: Then, cx: &mut App) -> Handled {
    let generation = {
        let d = cx.default_global::<Deferred>();
        d.next_generation += 1;
        d.next_generation
    };
    cx.global_mut::<Deferred>().calls.push(PendingCall {
        generation,
        token: None,
        deadline: None,
        then,
        landed: None,
    });
    let task = cx
        .background_executor()
        .spawn(async move { std::panic::catch_unwind(AssertUnwindSafe(work)).ok() });
    cx.spawn(async move |cx| {
        let out = task.await;
        cx.update(|cx| {
            if !cx.has_global::<Deferred>() {
                return;
            }
            if let Some(c) = cx
                .global_mut::<Deferred>()
                .calls
                .iter_mut()
                .find(|c| c.generation == generation)
            {
                c.landed = Some(out);
            }
        });
    })
    .detach();
    Handled::Later(generation)
}

/// What a background call's answer `out` leads to.
fn finish(then: Then, out: ReadOut, cx: &mut App) -> Handled {
    let app = AppState::global(cx);
    let made = |r: Result<(DocId, u64), ControlError>, command: &str| match r {
        Ok((doc, job)) => Handled::Job {
            command: command.to_owned(),
            doc,
            job,
        },
        Err(e) => Handled::Reply(Err(e)),
    };
    match (then, out) {
        (Then::Check { command, doc }, ReadOut::Report(r)) => {
            let s = app.read(cx);
            // The document closed while it ran: the answer is dropped.
            let Some(d) = s.store.document(doc) else {
                return Handled::Reply(Err(ControlError::new(
                    ErrorCode::NoDocument,
                    "the document closed",
                )));
            };
            Handled::Reply(match r {
                Ok(report) => Ok(result_json(&ControlResult::Interchange {
                    command,
                    doc: doc_id(doc),
                    window: None,
                    undo_label: d.snapshot().undo_label.clone(),
                    report: Some(report),
                    written: vec![],
                })),
                Err(e) => Err(thrown(&e, ic::CHECK_FAILED)),
            })
        }
        (Then::Midi(m), ReadOut::Midi(r)) => match r {
            Err(e) => Handled::Reply(Err(thrown(&e, ic::IMPORT_FAILED))),
            Ok(_) => made(
                app.update(cx, |s, _| s.store.import_midi_into_new_project(&m)),
                ic::IMPORT_MIDI,
            ),
        },
        (Then::Dawproject(read), ReadOut::Dawproject(r)) => {
            let info = match r {
                Err(e) => return Handled::Reply(Err(thrown(&e, ic::IMPORT_FAILED))),
                Ok(info) => info,
            };
            let next = match read.after_read(&info) {
                Err(e) => return Handled::Reply(Err(e)),
                Ok(n) => n,
            };
            let started = app.update(cx, |s, _| match next {
                DawprojectNext::NewProject { path, rate } => {
                    s.store.import_dawproject_into_new_project(&path, rate)
                }
                DawprojectNext::Merge { .. } => {
                    s.store
                        .merge_dawproject(&read, &info, ic::merge_date_today())
                }
            });
            made(started, ic::IMPORT_DAWPROJECT)
        }
        (Then::Aaf(req), ReadOut::Aaf(r)) => match r {
            Err(e) => Handled::Reply(Err(thrown(&e, ic::IMPORT_FAILED))),
            Ok(row) => made(
                app.update(cx, |s, _| {
                    s.store.import_aaf_into_new_project(req, row.sample_rate)
                }),
                ic::IMPORT_AAF,
            ),
        },
        _ => Handled::Reply(Err(ControlError::new(
            ErrorCode::Failed,
            "the call read the wrong thing",
        ))),
    }
}

// ---------------------------------------------------------------------
// The commands that need no document

/// Runs app-wide File command `id` (one of [`APP_IDS`] but `app.new_demo`):
/// a project made or opened shows its window, which the reply names.
pub(super) fn run_app(
    id: &str,
    args: &[(String, ArgValue)],
    cx: &mut App,
) -> Result<Value, ControlError> {
    let app = AppState::global(cx);
    match id {
        "file.new" => {
            let f = formats::new_project_format(args, app.read(cx).store.default_format())?;
            let doc = app.update(cx, |s, cx| {
                cx.notify();
                s.store.new_project(f)
            });
            made(id, doc, cx)
        }
        "file.create_demo" => {
            let doc = app.update(cx, |s, cx| {
                cx.notify();
                s.store.new_demo()
            });
            made(id, doc, cx)
        }
        "file.create_demo_audio" => {
            let doc = app.update(cx, |s, cx| {
                cx.notify();
                s.store.new_demo_audio()
            });
            made(id, doc, cx)
        }
        "file.open" => {
            let Some(path) = formats::text(id, args, "path")? else {
                return Err(ControlError::dialog("Open", &["path"]));
            };
            let answers = recovery_answers(id, args)?;
            let doc = open_answered(std::path::Path::new(path), &answers, cx)?;
            shown(id, doc, cx)
        }
        "file.open_copy" => {
            let Some(path) = formats::text(id, args, "path")? else {
                return Err(ControlError::dialog("Open", &["path"]));
            };
            let before = super::alert_id(cx);
            let opened = app.update(cx, |s, cx| {
                cx.notify();
                s.store.open_copy(std::path::Path::new(path))
            });
            match opened {
                Ok(doc) => shown(id, doc, cx),
                Err(m) => {
                    let raised = take_new_alert(before, cx);
                    Err(ControlError::new(
                        ErrorCode::Failed,
                        raised.map_or(m, |a| a.message),
                    ))
                }
            }
        }
        _ => Err(ControlError::new(
            ErrorCode::UnknownId,
            format!("VizDAW has no handler for {id}."),
        )),
    }
}

/// `app.open` and `command.run file.open`: no question is asked (the
/// call's answers settle them).
pub(super) fn open_answered(
    path: &std::path::Path,
    answers: &RecoveryAnswers,
    cx: &mut App,
) -> Result<DocId, ControlError> {
    AppState::global(cx).update(cx, |s, cx| {
        cx.notify();
        s.store.open_answered(path, answers)
    })
}

/// `app.new_demo {audio: true}`: the demo with audio, in a window.
pub(super) fn new_demo_audio(cx: &mut App) -> Result<Value, ControlError> {
    let doc = AppState::global(cx).update(cx, |s, cx| {
        cx.notify();
        s.store.new_demo_audio()
    });
    let Some(doc) = doc else {
        return Err(made_nothing("app.new_demo", cx));
    };
    let h = show(doc, cx)?;
    super::opened(h, cx)
}

/// The recovery answers in `command.run file.open`'s arguments.
fn recovery_answers(
    id: &str,
    args: &[(String, ArgValue)],
) -> Result<RecoveryAnswers, ControlError> {
    let plugins = match formats::text(id, args, "plugins")? {
        None => None,
        Some("normal") => Some(CrashAnswer::Normal),
        Some("off") => Some(CrashAnswer::Off),
        Some("disable") => Some(CrashAnswer::Disable),
        Some(_) => {
            return Err(ControlError::params(
                id,
                "`plugins` is normal, off or disable",
            ));
        }
    };
    Ok(RecoveryAnswers {
        plugins,
        restore_autosave: formats::flag(id, args, "restore_autosave")?,
    })
}

/// Swift `ControlCommands.made`: the window of `doc`, or the store's
/// alert ("title: message"), or "<id> made no document.".
fn made(id: &str, doc: Option<DocId>, cx: &mut App) -> Result<Value, ControlError> {
    match doc {
        Some(doc) => shown(id, doc, cx),
        None => Err(made_nothing(id, cx)),
    }
}

fn made_nothing(id: &str, cx: &mut App) -> ControlError {
    let a = AppState::global(cx).update(cx, |s, cx| {
        cx.notify();
        s.store.take_alert()
    });
    match a {
        Some(a) => ControlError::new(ErrorCode::Failed, format!("{}: {}", a.title, a.message)),
        None => ControlError::new(ErrorCode::Failed, format!("{id} made no document.")),
    }
}

/// `doc`'s window, opened now; the `command.run` reply names it.
fn shown(id: &str, doc: DocId, cx: &mut App) -> Result<Value, ControlError> {
    let h = show(doc, cx)?;
    let entry = AppState::global(cx)
        .read(cx)
        .windows()
        .iter()
        .find(|w| w.handle == h.into())
        .cloned()
        .ok_or_else(super::gone)?;
    Ok(ran_json(id, doc, &entry, None, None))
}

/// Opens `doc`'s window (a failure is the alert it raised).
fn show(doc: DocId, cx: &mut App) -> Result<gpui::WindowHandle<DocumentWindow>, ControlError> {
    let app = AppState::global(cx);
    if let Some(h) = app
        .read(cx)
        .window_of(doc)
        .and_then(|w| w.document_window())
    {
        return Ok(h);
    }
    let before = super::alert_id(cx);
    DocumentWindow::open(&app, doc, cx).map_err(|m| {
        let a = take_new_alert(before, cx);
        ControlError::new(ErrorCode::Failed, a.map_or(m, |a| a.message))
    })
}

fn take_new_alert(before: Option<u64>, cx: &mut App) -> Option<AlertItem> {
    AppState::global(cx).update(cx, |s, cx| {
        if s.store.alert().map(|a| a.id) == before {
            return None;
        }
        cx.notify();
        s.store.take_alert()
    })
}

// ---------------------------------------------------------------------
// The store's own reads and windows

/// Where the store's reads run: on the background executor, landing on
/// the main thread. A read always lands or is cancelled (its work
/// panicked), so [`vizdaw_app::DocumentStore::reading`] never stays true;
/// the windows its answer opens or closes follow at once.
pub(crate) fn install_spawner(cx: &mut App) {
    let exec = cx.background_executor().clone();
    let async_cx = cx.to_async();
    let app = AppState::global(cx);
    app.update(cx, |s, _| {
        s.store.set_spawner(Box::new(move |read| {
            let ticket = read.ticket();
            let task = exec.spawn(async move {
                std::panic::catch_unwind(AssertUnwindSafe(|| read.run())).ok()
            });
            async_cx
                .spawn(async move |cx| {
                    let landed = task.await;
                    cx.update(|cx| {
                        let app = AppState::global(cx);
                        app.update(cx, |s, cx| {
                            match landed {
                                Some(l) => s.store.land(l),
                                None => s.store.cancel_read(ticket),
                            }
                            cx.notify();
                        });
                        settle(cx);
                    });
                })
                .detach();
        }));
    });
}

/// Opens the windows of projects the store made for a file that landed
/// ([`vizdaw_app::DocumentStore::take_pending`]) and closes those of
/// projects it closed itself (a failed import; an ID that never had a
/// window is only forgotten). Call it outside any update.
pub fn settle(cx: &mut App) {
    let app = AppState::global(cx);
    let (closed, pending) = app.update(cx, |s, _| {
        let closed = s.store.take_closed();
        let mut pending = vec![];
        while let Some(d) = s.store.take_pending() {
            pending.push(d);
        }
        (closed, pending)
    });
    for doc in closed {
        close_windows(doc, cx);
    }
    for doc in pending {
        let s = app.read(cx);
        if s.store.document(doc).is_some() && s.window_of(doc).is_none() {
            let _ = DocumentWindow::open(&app, doc, cx);
        }
    }
}
