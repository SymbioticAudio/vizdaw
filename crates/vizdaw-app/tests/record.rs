//! The record model (Swift `RecordModelTests`, the non-recovery rows, and
//! the model rows of `RecordOptionsTests`): what Record asks the core for,
//! Record again stopping, keep and discard, the poll following a pass,
//! the punch drafts that refuse Record, the popover's host, and the
//! readouts (time left, loop counters, overdub note, mode switch).

mod support;

use std::sync::Arc;
use std::time::Duration;

use core_control::ErrorCode;
use support::{FakeSession, demo_fake, lane_row};
use vizdaw_app::record::{
    self, MIDI_TAKES_HELP, MODE_HELP, OPEN_DRAFT, OVERDUB_NOTE, PopoverHost, RecordDefaults,
    RecordModel, STEP_ENTRY_ON, STOP_BYTES,
};
use vizdaw_app::transport::TransportModel;
use vizdaw_app::transport_fields::{TEXT_OWNER, TransportField, field_text};
use vizdaw_app::{DocumentModel, DocumentStore};
use vizdaw_ffi::VizError;
use vizdaw_ffi::edit::LaneInputRow;
use vizdaw_ffi::record::RecordSpecRow;
use vizdaw_ffi::selection::TickRange;
use vizdaw_ffi::{BitDepthRow, ProjectFormatRow, ProjectInfoRow};

fn model(f: &Arc<FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

fn audio(id: u64, name: &str, stereo: bool) -> vizdaw_ffi::snapshot::LaneRow {
    let mut l = lane_row(id, name, false);
    l.arm = true;
    l.input = Some(LaneInputRow::Audio {
        first: 0,
        stereo,
        return_of: None,
    });
    l
}

fn midi(id: u64, name: &str) -> vizdaw_ffi::snapshot::LaneRow {
    let mut l = lane_row(id, name, false);
    l.arm = true;
    l.input = Some(LaneInputRow::Midi {
        port: None,
        channel: None,
    });
    l
}

/// Song (3) with a stereo audio track and a MIDI track, both armed.
fn armed() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(demo_fake());
    f.set_lanes(3, vec![audio(30, "Vocal", true), midi(31, "Keys")]);
    let mut m = model(&f);
    m.select(Some(3));
    m.refresh_project_info();
    (f, m)
}

fn store() -> DocumentStore {
    DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(demo_fake()))),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    )
}

fn code(r: Result<(), core_control::ControlError>) -> Option<(ErrorCode, String)> {
    r.err().map(|e| (e.code, e.message))
}

// ---- RecordModelTests ----

/// Plan 5c-2 takes Rulings 1–2: a new document overdubs, and its MIDI
/// loop passes layer into the playing take.
#[test]
fn a_new_document_overdubs_and_layers_midi_passes() {
    let (_f, m) = armed();
    let spec = m.record_spec();
    assert!(!spec.replace && !spec.passes_as_takes);
    assert_eq!(spec.punch, None);
    assert!(!spec.looped);
}

#[test]
fn record_asks_with_the_transports_modes_and_stops_on_the_second_press() {
    let (f, mut m) = armed();
    m.update_layout(|l| {
        l.transport.overdub = true;
        l.transport.punch = true;
        l.transport.punch_in = 15_360;
        l.transport.punch_out = 61_440;
    });
    m.toggle_loop();
    m.record_mut().set_pre_roll(2);
    assert_eq!(m.toggle_record(), Ok(()));
    assert_eq!(f.count("record(3)"), 1);
    let spec = m.record().state().and_then(|s| s.spec.clone());
    assert_eq!(
        spec,
        Some(RecordSpecRow {
            replace: false,
            punch: Some(TickRange {
                start: 15_360,
                end: 61_440
            }),
            looped: true,
            count_in_bars: 0,
            pre_roll_bars: 2,
            passes_as_takes: false,
        })
    );
    assert!(m.record().is_recording() && m.record().is_loop_recording());
    // Keep and discard while loop recording.
    assert_eq!(m.press_pass(true), Ok(()));
    assert_eq!(m.press_pass(false), Ok(()));
    assert_eq!((f.count("keep_pass"), f.count("discard_pass")), (1, 1));
    // Record again stops (and finishes the pass): one undo step "Record".
    assert_eq!(m.toggle_record(), Ok(()));
    assert!(!m.record().is_recording());
    assert!(!m.is_playing());
    assert_eq!(m.undo_title(), "Undo Record");
    assert_eq!(
        code(m.press_pass(true)),
        Some((
            ErrorCode::Unavailable,
            "Keep and discard act on loop passes while recording.".into()
        ))
    );
    assert_eq!(f.count("record(3)"), 1);
}

#[test]
fn record_needs_a_sequence_and_no_step_entry() {
    let (f, mut m) = armed();
    m.select(None);
    assert_eq!(
        code(m.toggle_record()),
        Some((ErrorCode::Unavailable, "Select a sequence first.".into()))
    );
    m.select(Some(3));
    m.select_track(30, false);
    m.start_step_entry().unwrap();
    assert_eq!(
        code(m.toggle_record()),
        Some((ErrorCode::Failed, STEP_ENTRY_ON.into()))
    );
    m.stop_step_entry();
    f.set_lanes(3, vec![lane_row(30, "Groove", false)]);
    assert_eq!(
        code(m.toggle_record()),
        Some((
            ErrorCode::Failed,
            "arm a track that has an input to record".into()
        ))
    );
    f.fail_next(
        "record",
        VizError::Busy {
            message: "Stop recording first.".into(),
        },
    );
    assert_eq!(
        code(m.toggle_record()),
        Some((ErrorCode::Busy, "Stop recording first.".into()))
    );
    assert_eq!(f.count("record(3)"), 0);
}

#[test]
fn an_open_drag_refuses_record() {
    let (f, mut m) = armed();
    assert!(m.begin_gesture("Move Block", vec![]));
    assert_eq!(
        code(m.toggle_record()),
        Some((ErrorCode::Unavailable, "Finish the drag first.".into()))
    );
    assert_eq!(f.count("record(3)"), 0);
}

#[test]
fn the_poll_follows_the_recording() {
    let (f, mut m) = armed();
    m.toggle_record().unwrap();
    assert!(m.record().is_recording());
    // The core ended it by itself (a full disk): the poll notices.
    f.end_pass_by_itself();
    m.poll_transport().unwrap();
    assert!(m.poll_record());
    assert!(!m.record().is_recording());
    assert_eq!(m.undo_title(), "Undo Record", "the take's snapshot is in");
}

/// Swift polls the record state only while playing or recording; a
/// stopped, idle transport reads nothing.
#[test]
fn the_poll_reads_the_record_state_only_while_playing_recording_or_counting_in() {
    let (f, mut m) = armed();
    let idle = f.reads("record_state");
    m.poll_transport().unwrap();
    m.poll_record();
    assert_eq!(f.reads("record_state"), idle, "stopped and idle: no read");
    m.record_mut().set_count_in(1);
    m.toggle_record().unwrap();
    assert!(m.counting_in() && !m.is_playing());
    let before = f.reads("record_state");
    m.poll_record();
    assert_eq!(f.reads("record_state"), before + 1, "counting in reads it");
}

#[test]
fn the_transport_polls_fast_while_counting_in_and_slow_when_idle() {
    let (f, mut m) = armed();
    let mut s = store();
    let mut t = TransportModel::new(&m).unwrap();
    assert_eq!(t.poll_interval_for(&m), Duration::from_millis(250));
    m.record_mut().set_count_in(2);
    m.toggle_record().unwrap();
    t.poll(&mut s);
    assert!(!t.is_playing() && t.state().counting_in);
    assert_eq!(t.poll_interval_for(&m), Duration::from_millis(33));
    f.count_in_done();
    t.poll(&mut s);
    assert_eq!(t.poll_interval_for(&m), Duration::from_millis(33));
    m.toggle_record().unwrap();
    t.poll(&mut s);
    assert_eq!(t.poll_interval_for(&m), Duration::from_millis(250));
}

/// A recording that reports stopped at a moment still polls fast: the
/// record state says it records.
#[test]
fn recording_polls_fast_even_when_the_transport_reads_stopped() {
    let (f, mut m) = armed();
    let t = TransportModel::new(&m).unwrap();
    m.toggle_record().unwrap();
    f.set_transport(|t| t.playing = false);
    assert!(m.record_polls_fast());
    assert_eq!(t.poll_interval_for(&m), Duration::from_millis(33));
}

#[test]
fn the_poll_through_the_store_refreshes_the_record_state() {
    let f = Arc::new(demo_fake());
    f.set_lanes(3, vec![audio(30, "Vocal", true)]);
    let fc = f.clone();
    let mut s = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(fc.clone())),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let id = s.new_demo().unwrap();
    s.with_document(id, |d, _| d.toggle_record().unwrap());
    let mut t = TransportModel::new(s.document(id).unwrap()).unwrap();
    f.end_pass_by_itself();
    t.poll_document(&mut s, id);
    assert!(!s.document(id).unwrap().record().is_recording());
}

#[test]
fn a_failing_record_state_is_surfaced() {
    let (f, mut m) = armed();
    f.set_call_error(
        "record_state",
        Some(VizError::Engine {
            message: "no state".into(),
        }),
    );
    m.refresh_record(false);
    assert_eq!(m.status(), Some("no state"));
    f.set_call_error(
        "record_state",
        Some(VizError::Busy {
            message: "busy".into(),
        }),
    );
    m.set_status(None);
    m.refresh_record(false);
    assert_eq!(m.status(), None);
}

/// Task 15: Record alone reads the core first, so a stale "recording"
/// starts a new pass.
#[test]
fn record_after_a_pass_ended_by_itself_starts_a_new_one() {
    let (f, mut m) = armed();
    m.toggle_record().unwrap();
    f.end_pass_by_itself();
    assert!(m.record().is_recording(), "no poll since");
    m.toggle_record().unwrap();
    assert_eq!(f.count("record(3)"), 2);
    assert!(m.record().is_recording() && m.is_playing());
}

/// Keep and discard read the core first too.
#[test]
fn keep_after_a_pass_ended_by_itself_is_unavailable() {
    let (f, mut m) = armed();
    m.toggle_loop();
    m.toggle_record().unwrap();
    f.end_pass_by_itself();
    assert_eq!(
        code(m.press_pass(true)).map(|c| c.0),
        Some(ErrorCode::Unavailable)
    );
    assert_eq!(f.count("keep_pass"), 0);
}

/// Task 15 review Minor 5: the pre-reads leave the status line alone.
#[test]
fn the_pre_reads_fail_quietly() {
    let (f, mut m) = armed();
    f.fail_next(
        "record_state",
        VizError::Engine {
            message: "no state".into(),
        },
    );
    assert_eq!(m.toggle_record(), Ok(()));
    assert_eq!(m.status(), None);
    assert!(m.record().is_recording());
}

// ---- RecordOptionsTests (the model rows) ----

#[test]
fn the_popover_sets_the_transports_modes() {
    let (_f, mut m) = armed();
    assert!(!m.record_replaces() && !m.record_punches());
    m.set_record_replace(true);
    assert!(m.record_replaces() && m.record_spec().replace);
    m.set_record_replace(false);
    m.set_record_punch(true);
    assert!(m.layout().transport.overdub && m.layout().transport.punch);
    assert!(!m.record_spec().replace && m.record_spec().punch.is_some());
    // The punch fields: typed text is a draft until Return.
    m.set_punch_text(TransportField::PunchIn, "2");
    assert_eq!(m.punch_text(TransportField::PunchIn), "2");
    assert_eq!(m.layout().transport.punch_in, 0);
    m.commit_punch(TransportField::PunchIn);
    assert_eq!(m.layout().transport.punch_in, 61_440);
    assert_eq!(
        m.punch_text(TransportField::PunchIn),
        field_text(&m, TransportField::PunchIn)
    );
    // Closing the popover drops a draft: nothing is said.
    m.set_record_popover(true);
    m.set_punch_text(TransportField::PunchOut, "x");
    m.set_record_popover(false);
    assert_eq!(
        m.punch_text(TransportField::PunchOut),
        field_text(&m, TransportField::PunchOut)
    );
    assert_eq!(m.status(), None);
    // Only the punch fields keep drafts.
    m.set_punch_text(TransportField::Position, "9");
    assert_eq!(
        m.punch_text(TransportField::Position),
        field_text(&m, TransportField::Position)
    );
}

#[test]
fn the_bar_switch_is_the_transports_mode() {
    let (_f, mut m) = armed();
    assert_eq!(m.record_mode_title(), "Overdub");
    assert_eq!(m.record_mode_label(), "Record mode: Overdub");
    assert_eq!(m.toggle_record_mode(), Ok(()));
    assert_eq!(m.record_mode_title(), "Replace");
    assert_eq!(m.record_mode_label(), "Record mode: Replace");
    assert!(m.record_replaces() && m.record_spec().replace);
    m.begin_text_editing("rename");
    assert_eq!(
        code(m.toggle_record_mode()),
        Some((
            ErrorCode::Unavailable,
            "A text field has the keyboard.".into()
        ))
    );
    m.press_record_mode();
    assert_eq!(m.status(), Some("A text field has the keyboard."));
    assert_eq!(m.record_mode_title(), "Replace");
    m.end_text_editing("rename");
    assert_eq!(
        MODE_HELP,
        "Replace removes the notes in the recorded range; Overdub keeps them (transport.overdub)"
    );
    assert_eq!(
        MIDI_TAKES_HELP,
        "Loop recording: each MIDI pass becomes a new take. Off: passes layer into the playing take. Audio passes always become takes."
    );
}

/// Plan 7a final review I2: the popover opens under the chevron that was
/// clicked; the other window's moves it there, its own closes it.
#[test]
fn opening_in_the_mixer_window_does_not_show_in_the_document_window() {
    let (_f, mut m) = armed();
    m.toggle_record_popover(PopoverHost::MixerWindow);
    assert!(m.record().shows_popover_in(PopoverHost::MixerWindow));
    assert!(!m.record().shows_popover_in(PopoverHost::Document));
    m.toggle_record_popover(PopoverHost::Document);
    assert!(m.record().shows_popover_in(PopoverHost::Document));
    assert!(!m.record().shows_popover_in(PopoverHost::MixerWindow));
    m.toggle_record_popover(PopoverHost::Document);
    assert!(!m.record().shows_popover());
}

/// Moving the popover to the other window drops the drafts (it closed).
#[test]
fn moving_the_popover_drops_its_drafts() {
    let (_f, mut m) = armed();
    m.toggle_record_popover(PopoverHost::Document);
    m.set_punch_text(TransportField::PunchIn, "3");
    m.toggle_record_popover(PopoverHost::MixerWindow);
    assert!(!m.has_open_punch_draft());
}

#[test]
fn closing_the_popover_drops_every_punch_draft() {
    let (_f, mut m) = armed();
    m.set_record_punch(true);
    let before = m.layout().transport.clone();
    m.set_record_popover(true);
    m.set_punch_text(TransportField::PunchIn, "3");
    m.set_punch_text(TransportField::PunchOut, "6");
    m.set_record_popover(false);
    assert_eq!(m.layout().transport.punch_in, before.punch_in);
    assert_eq!(m.layout().transport.punch_out, before.punch_out);
    assert!(!m.has_open_punch_draft());
    assert_eq!(m.status(), None);
}

/// Controller ruling I5: Record waits for an open punch draft, so a pass
/// never punches at the old range with a typed time on screen.
#[test]
fn record_refuses_while_a_punch_draft_is_open() {
    let (f, mut m) = armed();
    m.set_record_punch(true);
    m.set_record_popover(true);
    m.set_punch_text(TransportField::PunchIn, "5");
    assert!(m.has_open_punch_draft());
    assert_eq!(
        code(m.toggle_record()),
        Some((ErrorCode::Failed, OPEN_DRAFT.into()))
    );
    assert_eq!(f.count("record(3)"), 0);
    assert!(!m.record().is_recording());
    assert_eq!(
        m.punch_text(TransportField::PunchIn),
        "5",
        "the draft stays"
    );
    m.commit_punch(TransportField::PunchIn);
    assert_eq!(m.toggle_record(), Ok(()));
    assert!(m.record().is_recording());
    let punch = |m: &DocumentModel| {
        m.record()
            .state()
            .and_then(|s| s.spec.clone())
            .and_then(|s| s.punch)
            .map(|p| p.start)
    };
    assert_eq!(punch(&m), Some(245_760));
    m.toggle_record().unwrap();
    // A draft equal to the field's text is not open.
    let shown = field_text(&m, TransportField::PunchIn);
    m.set_punch_text(TransportField::PunchIn, &shown);
    assert!(!m.has_open_punch_draft());
    assert_eq!(m.toggle_record(), Ok(()));
    assert!(m.record().is_recording());
}

/// 5d-2 final re-review N1: a draft that is not a position refuses Record;
/// the old punch range is kept.
#[test]
fn a_refused_punch_draft_refuses_record() {
    let (f, mut m) = armed();
    m.set_record_punch(true);
    m.set_record_popover(true);
    let before = field_text(&m, TransportField::PunchIn);
    m.set_punch_text(TransportField::PunchIn, "abc");
    m.press_record();
    assert!(!m.record().is_recording());
    assert_eq!(f.count("record(3)"), 0);
    assert_eq!(m.status(), Some(OPEN_DRAFT));
    assert_eq!(field_text(&m, TransportField::PunchIn), before);
    m.set_punch_text(TransportField::PunchIn, "2");
    m.commit_punch(TransportField::PunchIn);
    m.set_status(None);
    m.press_record();
    assert!(m.record().is_recording());
    assert_eq!(f.count("record(3)"), 1);
    assert_eq!(m.layout().transport.punch_in, 61_440);
}

/// A refused commit drops the draft: the field shows the old value.
#[test]
fn a_refused_commit_drops_the_draft() {
    let (_f, mut m) = armed();
    m.set_punch_text(TransportField::PunchIn, "abc");
    assert!(!m.commit_punch(TransportField::PunchIn));
    assert!(m.status().is_some());
    assert!(!m.has_open_punch_draft());
    assert!(m.commit_punch(TransportField::PunchOut), "no draft: taken");
}

/// Ruling 7: focus moving to the other punch field commits the one it
/// left; leaving the pair drops the draft.
#[test]
fn the_popovers_punch_fields_cancel_when_focus_leaves_them() {
    let (_f, mut m) = armed();
    m.set_record_popover(true);
    let before = m.layout().transport.punch_in;
    m.punch_focus_changed(None, Some(TransportField::PunchIn));
    m.set_punch_text(TransportField::PunchIn, "3");
    m.punch_focus_changed(Some(TransportField::PunchIn), None);
    assert_eq!(m.layout().transport.punch_in, before);
    assert_eq!(
        m.punch_text(TransportField::PunchIn),
        field_text(&m, TransportField::PunchIn)
    );
    m.punch_focus_changed(None, Some(TransportField::PunchIn));
    m.set_punch_text(TransportField::PunchIn, "3");
    m.punch_focus_changed(
        Some(TransportField::PunchIn),
        Some(TransportField::PunchOut),
    );
    assert_eq!(
        m.layout().transport.punch_in,
        122_880,
        "moving within the pair commits"
    );
}

#[test]
fn the_popover_closes_for_every_press_but_record_with_a_draft() {
    assert!(!RecordModel::close_allowed(true, true));
    assert!(RecordModel::close_allowed(true, false));
    assert!(RecordModel::close_allowed(false, true));
    assert!(RecordModel::close_allowed(false, false));
}

/// Ruling 7's bar Record rule: a press on Record with a draft open keeps
/// the popover, the draft and its keyboard; Record refuses; Return then
/// sets the punch and Record records there.
#[test]
fn a_record_press_with_an_open_draft_refuses_and_keeps_it() {
    let (f, mut m) = armed();
    m.set_record_punch(true);
    m.toggle_record_popover(PopoverHost::Document);
    m.punch_focus_changed(None, Some(TransportField::PunchIn));
    m.set_punch_text(TransportField::PunchIn, "5");
    let draft = m.has_open_punch_draft();
    if RecordModel::close_allowed(true, draft) {
        m.set_record_popover(false);
    }
    m.press_record();
    assert_eq!(m.status(), Some(OPEN_DRAFT));
    assert_eq!(f.count("record(3)"), 0);
    assert!(m.record().shows_popover());
    assert_eq!(m.punch_text(TransportField::PunchIn), "5");
    assert!(!m.transport_keys_enabled(), "the field keeps the keyboard");
    m.commit_punch(TransportField::PunchIn);
    assert!(RecordModel::close_allowed(true, m.has_open_punch_draft()));
    m.set_status(None);
    m.press_record();
    assert!(m.record().is_recording());
    assert_eq!(m.layout().transport.punch_in, 245_760);
}

/// Fix round 2 N2: the bar's Record button stays enabled while a text
/// edit holds the keyboard; a click with a draft open refuses and says
/// why.
#[test]
fn the_record_button_is_enabled_whatever() {
    let (f, mut m) = armed();
    m.set_record_punch(true);
    assert!(m.record_button_enabled());
    m.set_record_popover(true);
    m.punch_focus_changed(None, Some(TransportField::PunchIn));
    m.set_punch_text(TransportField::PunchIn, "5");
    assert!(!m.transport_keys_enabled() && m.has_open_punch_draft());
    assert!(m.record_button_enabled());
    m.press_record();
    assert_eq!(m.status(), Some(OPEN_DRAFT));
    assert_eq!(f.count("record(3)"), 0);
    m.set_record_popover(false);
    m.begin_text_editing(TEXT_OWNER);
    m.bar_punch_typed(TransportField::PunchOut, "9");
    assert!(m.record_button_enabled() && m.has_open_punch_draft());
    m.bar_punch_ended();
    m.end_text_editing(TEXT_OWNER);
    assert!(m.record_button_enabled() && !m.has_open_punch_draft());
    // No sequence and not recording: off.
    m.select(None);
    assert!(!m.record_button_enabled());
}

/// Fix round 1 M4: the bar's own Punch In or Out, typed in, refuses Record
/// with the same message from the button and from the model call every
/// other path uses; other fields are ignored.
#[test]
fn a_typed_bar_punch_time_refuses_record() {
    let (f, mut m) = armed();
    m.set_record_punch(true);
    m.begin_text_editing(TEXT_OWNER);
    m.bar_punch_typed(TransportField::Position, "5");
    assert!(!m.has_open_punch_draft(), "only the punch fields count");
    m.bar_punch_typed(TransportField::PunchIn, "5");
    assert!(m.has_open_punch_draft());
    m.press_record();
    assert_eq!(m.status(), Some(OPEN_DRAFT));
    assert_eq!(
        code(m.toggle_record()),
        Some((ErrorCode::Failed, OPEN_DRAFT.into()))
    );
    assert_eq!(f.count("record(3)"), 0);
    // Return sets it; then Record records there.
    assert!(vizdaw_app::transport_fields::commit_field(
        &mut m,
        TransportField::PunchIn,
        "5"
    ));
    m.bar_punch_ended();
    m.end_text_editing(TEXT_OWNER);
    m.set_status(None);
    m.press_record();
    assert!(m.record().is_recording());
    assert_eq!(m.layout().transport.punch_in, 245_760);
}

#[test]
fn the_time_left_counts_the_armed_audio_channels() {
    let (f, mut m) = armed();
    assert_eq!(m.armed_channels(), 2);
    // 48 kHz, 24-bit, two channels: 288 000 bytes a second.
    let ten_minutes: u64 = 288_000 * 600;
    f.set_free_bytes(Some(ten_minutes + STOP_BYTES));
    m.refresh_record(false);
    assert_eq!(m.record_remaining_text().as_deref(), Some("10 min left"));
    f.set_free_bytes(Some(288_000 * 3_900 + STOP_BYTES));
    m.refresh_record(false);
    assert_eq!(m.record_remaining_text().as_deref(), Some("1 h 5 min left"));
    f.set_free_bytes(Some(1_000));
    m.refresh_record(false);
    assert_eq!(
        m.record_remaining_text().as_deref(),
        Some("Less than a minute left")
    );
    // While recording, the core's reading counts.
    f.set_free_bytes(Some(288_000 * 7_200 + STOP_BYTES));
    m.toggle_record().unwrap();
    assert!(m.record().is_recording());
    assert_eq!(m.record_remaining_text().as_deref(), Some("2 h left"));
    m.toggle_record().unwrap();
    // MIDI only: no readout.
    f.set_lanes(3, vec![midi(31, "Keys")]);
    m.select(None);
    m.select(Some(3));
    assert_eq!(m.armed_channels(), 0);
    assert_eq!(m.record_remaining_text(), None);
}

/// 5d-2 T2 M4: the time left counts the depth and mono tracks, and says
/// nothing without a reading.
#[test]
fn the_time_left_counts_every_depth_and_mono_tracks() {
    let (f, mut m) = armed();
    f.set_lanes(3, vec![audio(30, "Vocal", false), midi(31, "Keys")]);
    m.select(None);
    m.select(Some(3));
    let format = |rate, depth| ProjectInfoRow {
        format: ProjectFormatRow {
            sample_rate: rate,
            depth,
        },
        mismatched: 0,
        rate_mismatch: false,
        device_rate: 0.0,
    };
    f.set_project_info(format(44_100, BitDepthRow::Int16));
    m.refresh_project_info();
    // 44.1 kHz, 16-bit, one channel: 88 200 bytes a second.
    f.set_free_bytes(Some(88_200 * 600 + STOP_BYTES));
    m.refresh_record(false);
    assert_eq!(m.record_remaining_text().as_deref(), Some("10 min left"));
    f.set_project_info(format(48_000, BitDepthRow::Float32));
    m.refresh_project_info();
    f.set_free_bytes(Some(192_000 * 3_600 + STOP_BYTES));
    m.refresh_record(false);
    assert_eq!(m.record_remaining_text().as_deref(), Some("1 h left"));
    f.set_free_bytes(None);
    m.refresh_record(false);
    assert_eq!(m.record_remaining_text(), None, "no reading, no readout");
    f.set_free_bytes(Some(0));
    m.refresh_record(false);
    assert_eq!(
        m.record_remaining_text().as_deref(),
        Some("Less than a minute left")
    );
}

#[test]
fn the_remaining_texts() {
    assert_eq!(record::remaining(59.9), "Less than a minute left");
    assert_eq!(record::remaining(60.0), "1 min left");
    assert_eq!(record::remaining(3_599.0), "59 min left");
    assert_eq!(record::remaining(3_600.0), "1 h left");
    assert_eq!(record::remaining(3_660.0), "1 h 1 min left");
}

#[test]
fn loop_counters_and_the_overdub_note() {
    let (f, mut m) = armed();
    m.set_record_replace(true);
    assert_eq!(m.record_overdub_note(), None);
    m.set_record_replace(false);
    assert_eq!(m.record_overdub_note(), Some(OVERDUB_NOTE));
    assert_eq!(m.record_pass_text(), None);
    m.toggle_loop();
    m.toggle_record().unwrap();
    assert_eq!(m.record_pass_text().as_deref(), Some("Pass 1"));
    f.next_pass();
    m.press_pass(true).unwrap();
    f.next_pass();
    m.press_pass(false).unwrap();
    assert_eq!(
        m.record_pass_text().as_deref(),
        Some("Pass 3 \u{b7} 1 kept \u{b7} 1 discarded")
    );
    m.toggle_record().unwrap();
    assert_eq!(m.record_pass_text(), None);
    // MIDI only armed: no overdub note.
    f.set_lanes(3, vec![midi(31, "Keys")]);
    m.select(None);
    m.select(Some(3));
    assert_eq!(m.record_overdub_note(), None);
    // Counting in follows the transport.
    m.record_mut().set_count_in(1);
    m.toggle_record().unwrap();
    assert!(m.counting_in());
}

/// 5d-2 T3 M1 (Ruling 12): text editing is held only while a punch field
/// has focus.
#[test]
fn text_editing_is_held_only_while_a_punch_field_has_focus() {
    let (_f, mut m) = armed();
    m.set_record_popover(true);
    assert!(m.transport_keys_enabled());
    m.punch_focus_changed(None, Some(TransportField::PunchIn));
    assert!(!m.transport_keys_enabled());
    m.set_punch_text(TransportField::PunchIn, "3");
    m.punch_focus_changed(
        Some(TransportField::PunchIn),
        Some(TransportField::PunchOut),
    );
    assert!(!m.transport_keys_enabled());
    assert_eq!(
        m.layout().transport.punch_in,
        122_880,
        "the old field's draft is in"
    );
    m.punch_focus_changed(Some(TransportField::PunchOut), None);
    assert!(m.transport_keys_enabled());
    m.punch_focus_changed(None, Some(TransportField::PunchOut));
    m.set_record_popover(false);
    assert!(m.transport_keys_enabled(), "closing ends it");
}

#[test]
fn the_record_button_shows_why_it_was_refused() {
    let (f, mut m) = armed();
    f.fail_next(
        "record",
        VizError::Model {
            message: "arm a track that has an input to record".into(),
        },
    );
    m.press_record();
    assert_eq!(m.status(), Some("arm a track that has an input to record"));
    m.set_status(None);
    m.press_record();
    assert!(m.record().is_recording());
    assert_eq!(m.status(), None);
    m.press_record();
    assert!(!m.record().is_recording());
}

/// Plan 5f-4 Ruling 16: the Count-in toggle is off ↔ the last non-zero
/// bar count.
#[test]
fn count_in_toggles_off_and_back_to_its_last_bars() {
    let (_f, mut m) = armed();
    let r = m.record_mut();
    assert_eq!(r.count_in_bars(), 0);
    r.toggle_count_in();
    assert_eq!(r.count_in_bars(), 1, "the first time: one bar");
    r.set_count_in(3);
    r.toggle_count_in();
    assert_eq!(r.count_in_bars(), 0);
    r.toggle_count_in();
    assert_eq!(r.count_in_bars(), 3);
    r.set_count_in(9);
    assert_eq!(r.count_in_bars(), 4, "0 to 4");
    r.set_pre_roll(12);
    assert_eq!(r.pre_roll_bars(), 8, "0 to 8");
}

/// Plan 5d-2 Ruling 3: a new document's options start from Settings ›
/// Recording's defaults.
#[test]
fn the_options_start_from_the_defaults() {
    let r = RecordModel::new(RecordDefaults {
        count_in_bars: 2,
        pre_roll_bars: 1,
        midi_passes_as_takes: true,
    });
    assert_eq!(
        (
            r.count_in_bars(),
            r.pre_roll_bars(),
            r.midi_passes_as_takes()
        ),
        (2, 1, true)
    );
    let mut r = RecordModel::new(RecordDefaults::default());
    assert_eq!(
        (
            r.count_in_bars(),
            r.pre_roll_bars(),
            r.midi_passes_as_takes()
        ),
        (0, 0, false)
    );
    r.toggle_count_in();
    assert_eq!(r.count_in_bars(), 1);
    let (_f, mut m) = armed();
    m.apply_record_defaults(RecordDefaults {
        count_in_bars: 1,
        pre_roll_bars: 2,
        midi_passes_as_takes: true,
    });
    let spec = m.record_spec();
    assert_eq!(
        (spec.count_in_bars, spec.pre_roll_bars, spec.passes_as_takes),
        (1, 2, true)
    );
}

// ---- The record commands (slice 8a Task 4; Swift `RecordControlTests`) ----

use core_control::ControlError;
use core_control::registry::ArgValue;
use vizdaw_app::commands::{self, RECORD_COMMANDS, Ran};

fn run(id: &str, pairs: &[(&str, ArgValue)], m: &mut DocumentModel) -> Result<Ran, ControlError> {
    let args: Vec<(String, ArgValue)> = pairs
        .iter()
        .map(|(n, v)| ((*n).to_string(), v.clone()))
        .collect();
    commands::run(id, &args, m)
}

fn fails(r: Result<Ran, ControlError>) -> (ErrorCode, String) {
    let e = r.expect_err("expected a refusal");
    (e.code, e.message)
}

fn flag(v: bool) -> ArgValue {
    ArgValue::Flag(v)
}

fn int(v: i64) -> ArgValue {
    ArgValue::Int(v)
}

fn text(v: &str) -> ArgValue {
    ArgValue::Text(v.into())
}

#[test]
fn every_record_command_is_in_the_registry() {
    for id in RECORD_COMMANDS {
        assert!(core_control::registry::find(id).is_some(), "{id}");
    }
}

/// Swift `recordSetSetsTheModesAndOptions`.
#[test]
fn record_set_sets_the_modes_and_options() {
    let (_f, mut m) = armed();
    run(
        "record.set",
        &[
            ("replace", flag(false)),
            ("punch", flag(true)),
            ("loop", flag(true)),
            ("pre_roll", int(2)),
            ("count_in", int(1)),
            ("passes_as_takes", flag(true)),
        ],
        &mut m,
    )
    .unwrap();
    assert!(m.layout().transport.overdub && m.layout().transport.punch && m.looping());
    assert_eq!(m.record().pre_roll_bars(), 2);
    assert_eq!(m.record().count_in_bars(), 1);
    assert!(m.record().midi_passes_as_takes());
    let spec = m.record_spec();
    assert!(!spec.replace && spec.looped && spec.punch.is_some() && spec.passes_as_takes);
    // Loop already on stays on.
    run("record.set", &[("loop", flag(true))], &mut m).unwrap();
    assert!(m.looping());
    run("record.set", &[("replace", flag(true))], &mut m).unwrap();
    assert!(!m.layout().transport.overdub);
}

/// Swift `recordSetAppliesAllOrNothing`: validated before anything is
/// applied; a failing loop leaves the rest alone and is the call's
/// failure, not an alert; an open gesture refuses it.
#[test]
fn record_set_applies_all_or_nothing() {
    let (f, mut m) = armed();
    let overdub = m.layout().transport.overdub;
    assert_eq!(
        fails(run(
            "record.set",
            &[("replace", flag(overdub)), ("count_in", int(9))],
            &mut m
        )),
        (
            ErrorCode::InvalidParams,
            "record.set: `count_in` is 0 to 4".into()
        )
    );
    assert_eq!(
        fails(run(
            "record.set",
            &[("replace", flag(overdub)), ("pre_roll", int(-1))],
            &mut m
        )),
        (
            ErrorCode::InvalidParams,
            "record.set: `pre_roll` is 0 to 8".into()
        )
    );
    assert_eq!(m.layout().transport.overdub, overdub);
    assert_eq!(m.record().count_in_bars(), 0);
    f.set_loop_error(Some(VizError::Engine {
        message: "no loop".into(),
    }));
    assert_eq!(
        fails(run(
            "record.set",
            &[
                ("loop", flag(true)),
                ("replace", flag(overdub)),
                ("pre_roll", int(3)),
            ],
            &mut m
        )),
        (ErrorCode::Failed, "Loop Failed: no loop".into())
    );
    assert_eq!(m.layout().transport.overdub, overdub);
    assert_eq!(m.record().pre_roll_bars(), 0);
    assert!(!m.looping() && m.alert.is_none());
    f.set_loop_error(None);
    assert!(m.begin_gesture("Move", vec![]));
    assert_eq!(
        fails(run("record.set", &[("replace", flag(overdub))], &mut m)),
        (ErrorCode::Unavailable, "Finish the drag first.".into())
    );
    assert_eq!(m.layout().transport.overdub, overdub);
}

/// Swift `recordWhileRecordingStopsAndRecordSetIsRefusedThen`, and Task
/// 15: a pass that ended by itself is not recording for `record.set`.
#[test]
fn record_set_is_refused_while_recording() {
    let (f, mut m) = armed();
    run("transport.record", &[], &mut m).unwrap();
    assert!(m.record().is_recording());
    assert_eq!(
        fails(run("record.set", &[("loop", flag(true))], &mut m)),
        (ErrorCode::Unavailable, "Stop recording first.".into())
    );
    assert!(!m.looping());
    f.end_pass_by_itself();
    run("record.set", &[("loop", flag(true))], &mut m).unwrap();
    assert!(m.looping());
    // The pre-read fails quietly.
    f.set_call_error(
        "record_state",
        Some(VizError::Engine {
            message: "no state".into(),
        }),
    );
    run("record.set", &[("loop", flag(false))], &mut m).unwrap();
    assert_eq!(m.status(), None);
    assert!(!m.looping());
}

/// Swift `recoverAnswersTheOfferAndStateGetReadsTheCore`: nothing to
/// recover is unavailable and asks the core nothing; `trash` and `import`
/// answer; a failure is the call's, never an alert or the offer.
#[test]
fn record_recover_answers_without_side_effects() {
    let (f, mut m) = armed();
    assert_eq!(
        fails(run("record.recover", &[("action", text("import"))], &mut m)),
        (
            ErrorCode::Unavailable,
            "There are no recovered recordings.".into()
        )
    );
    assert_eq!(f.count("recover_recordings(true)"), 0);
    let file = vizdaw_ffi::record::RecoveredFileRow {
        path: "Media/Recorded/a.caf".into(),
        frames: 10,
        size_known: false,
        readable: true,
    };
    f.set_recovered_files(vec![file.clone()]);
    run("record.recover", &[("action", text("trash"))], &mut m).unwrap();
    assert_eq!(f.count("recover_recordings(false)"), 1);
    f.set_recovered_files(vec![file.clone()]);
    f.fail_next(
        "recover_recordings",
        VizError::Io {
            message: "disk".into(),
        },
    );
    assert_eq!(
        fails(run("record.recover", &[("action", text("import"))], &mut m)),
        (ErrorCode::Failed, "Recovery Failed: disk".into())
    );
    assert!(m.alert.is_none() && m.recovery_offer().is_none());
    f.fail_next(
        "recover_recordings",
        VizError::Busy {
            message: "busy".into(),
        },
    );
    assert_eq!(
        fails(run("record.recover", &[("action", text("import"))], &mut m)),
        (ErrorCode::Busy, "busy".into())
    );
    assert!(m.begin_gesture("Move", vec![]));
    assert_eq!(
        fails(run("record.recover", &[("action", text("import"))], &mut m)),
        (ErrorCode::Unavailable, "Finish the drag first.".into())
    );
    m.cancel_gesture();
    run("record.recover", &[("action", text("import"))], &mut m).unwrap();
    assert_eq!(f.count("recover_recordings(true)"), 1);
    assert!(m.alert.is_none() && m.recovery_offer().is_none());
}

/// Swift `midiInjectSendsHexBytes`, `aPayloadTheCoreRefusesIsInvalidParams`
/// and `hexBytesWantPairs`.
#[test]
fn midi_inject_sends_hex_bytes() {
    let (f, mut m) = armed();
    let hex = "midi.inject: `bytes` are hex bytes such as \"90 3c 64\"";
    f.set_inject_taken(true);
    run(
        "midi.inject",
        &[("port", text("Keys")), ("bytes", text("90 3c 64"))],
        &mut m,
    )
    .unwrap();
    assert_eq!(
        f.injected().last().cloned(),
        Some(("Keys".to_string(), vec![0x90, 0x3C, 0x64]))
    );
    run(
        "midi.inject",
        &[("port", text("Keys")), ("bytes", text("f0 7E f7"))],
        &mut m,
    )
    .unwrap();
    assert_eq!(f.injected().last().unwrap().1, [0xF0, 0x7E, 0xF7]);
    let long = "90 ".repeat(1025);
    for bad in [
        "",
        "9g",
        "900",
        "90,3c",
        "9 3c 64",
        "90 3 64",
        long.as_str(),
    ] {
        assert_eq!(
            fails(run(
                "midi.inject",
                &[("port", text("Keys")), ("bytes", text(bad))],
                &mut m
            )),
            (ErrorCode::InvalidParams, hex.into()),
            "{}",
            &bad[..bad.len().min(10)]
        );
    }
    assert_eq!(
        fails(run("midi.inject", &[("bytes", text("90 3c 64"))], &mut m)),
        (ErrorCode::InvalidParams, hex.into()),
        "no port"
    );
    assert_eq!(f.injected().len(), 2);
    let send = |m: &mut DocumentModel| {
        run(
            "midi.inject",
            &[("port", text("Keys")), ("bytes", text("90 3c 64"))],
            m,
        )
    };
    f.set_inject_taken(false);
    assert_eq!(
        fails(send(&mut m)),
        (
            ErrorCode::Unavailable,
            "Play or record first: this document does not hold the audio engine.".into()
        )
    );
    f.fail_next(
        "inject_midi",
        VizError::Model {
            message: "byte 0xf0 does not start a channel message".into(),
        },
    );
    assert_eq!(
        fails(send(&mut m)),
        (
            ErrorCode::InvalidParams,
            "midi.inject: byte 0xf0 does not start a channel message".into()
        )
    );
    f.fail_next(
        "inject_midi",
        VizError::Engine {
            message: "no engine".into(),
        },
    );
    assert_eq!(
        fails(send(&mut m)),
        (ErrorCode::Failed, "MIDI Injection Failed: no engine".into())
    );
}

/// Plan 8c Ruling 4: the marker commands are chores, one undo step each
/// with the arguments as given; a refusal is `failed` (Busy `busy`) and
/// raises no alert; an open gesture refuses them.
#[test]
fn marker_commands_are_chores_with_their_undo_names() {
    let (f, mut m) = armed();
    let add = [
        ("seq", int(3)),
        ("tick", int(7680)),
        ("name", text("Bridge")),
    ];
    run("marker.add", &add, &mut m).unwrap();
    assert_eq!(m.undo_title(), "Undo Add Marker");
    let (id, args) = f.chores_run().last().cloned().unwrap();
    assert_eq!((id.as_str(), args.len()), ("marker.add", 3));
    run(
        "marker.rename",
        &[("seq", int(3)), ("index", int(1)), ("name", text("Coda"))],
        &mut m,
    )
    .unwrap();
    assert_eq!(m.undo_title(), "Undo Rename Marker");
    run(
        "marker.remove",
        &[("seq", int(3)), ("index", int(1))],
        &mut m,
    )
    .unwrap();
    assert_eq!(m.undo_title(), "Undo Remove Marker");
    f.fail_next(
        "run_chore",
        VizError::Model {
            message: "there is no marker 4".into(),
        },
    );
    assert_eq!(
        fails(run(
            "marker.remove",
            &[("seq", int(3)), ("index", int(4))],
            &mut m
        )),
        (ErrorCode::Failed, "there is no marker 4".into())
    );
    f.fail_next(
        "run_chore",
        VizError::Busy {
            message: "Stop recording first.".into(),
        },
    );
    assert_eq!(
        fails(run("marker.add", &add, &mut m)),
        (ErrorCode::Busy, "Stop recording first.".into())
    );
    assert!(m.alert.is_none());
    assert!(m.begin_gesture("Move", vec![]));
    let n = f.chores_run().len();
    assert_eq!(
        fails(run("marker.add", &add, &mut m)),
        (ErrorCode::Unavailable, "Finish the drag first.".into())
    );
    assert_eq!(f.chores_run().len(), n);
}

/// The record commands refuse parameters they do not take, and say when
/// they can run.
#[test]
fn record_commands_refuse_extra_params_and_say_when_they_run() {
    let (f, mut m) = armed();
    for id in RECORD_COMMANDS {
        assert_eq!(
            fails(run(id, &[("bars", int(2))], &mut m)).0,
            ErrorCode::InvalidParams,
            "{id}"
        );
        assert!(commands::enabled(id, &m), "{id}");
    }
    assert!(f.chores_run().is_empty() && f.injected().is_empty());
    m.toggle_record().unwrap();
    assert!(!commands::enabled("record.set", &m));
    m.toggle_record().unwrap();
    assert!(m.begin_gesture("Move", vec![]));
    for id in ["record.set", "record.recover", "marker.add"] {
        assert!(!commands::enabled(id, &m), "{id}");
    }
    assert!(commands::enabled("midi.inject", &m));
}
