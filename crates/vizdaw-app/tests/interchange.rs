//! The Import MIDI File sheet's model (slice 7c Task 1; Swift
//! `InterchangeTests`' import rows): the read's defaults, the tempo lock
//! (Ruling T7-a), the choices Import sends, a refused file, the report a
//! finished import keeps to show, and Import into a new project with the
//! sheet's choices.

mod support;

use std::sync::Arc;

use support::{FakeSession, demo_fake, lane_row};
use vizdaw_app::interchange::{
    self as mi, ChannelChoice, MidiImport, TEMPO_HELP, TEMPO_LOCKED_HELP, TempoBox,
};
use vizdaw_app::testing::{FakeCore, JobRequest, JobScript, seq};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::selection::SelectionRow;
use vizdaw_ffi::{
    ChannelMapRow, ChannelUseRow, MidiFileInfoRow, MidiFileKindRow, MidiImportRequestRow,
    ReportItemRow, ReportKindRow, ReportRow,
};

/// The demo with events in its shown sequence's tracks (Swift `busy`).
fn busy() -> FakeSession {
    let f = demo_fake();
    let mut a = lane_row(30, "Groove", false);
    let mut b = lane_row(31, "Chords", false);
    a.midi_event_count = 12;
    b.midi_event_count = 3;
    f.set_lanes(3, vec![a, b]);
    f
}

fn info(kind: MidiFileKindRow, mpe: bool) -> MidiFileInfoRow {
    let ch = |channel, notes, suggested| ChannelUseRow {
        group: 0,
        channel,
        notes,
        suggested,
    };
    MidiFileInfoRow {
        kind,
        tracks: 2,
        channels: vec![ch(0, 10, Some(5)), ch(9, 4, None), ch(2, 7, Some(6))],
        has_tempo: true,
        mpe_found: mpe,
    }
}

/// A store whose front document is `fake`.
fn store_with(fake: &Arc<FakeSession>) -> (DocumentStore, Arc<FakeCore>, DocId) {
    let core = Arc::new(FakeCore::new());
    let mut s = DocumentStore::with_core(core.clone());
    core.hand_out("new_demo", fake.clone());
    let id = s.new_demo().unwrap();
    (s, core, id)
}

/// Reads `path` for document `id`'s sheet (run here, as the UI's
/// background executor would).
fn read(s: &mut DocumentStore, id: DocId, path: &str) {
    s.begin_midi_import(id, path);
    s.run_reads();
}

fn sheet(s: &DocumentStore, id: DocId) -> MidiImport {
    s.document(id).unwrap().midi_import().unwrap().clone()
}

fn imports(f: &FakeSession) -> Vec<MidiImportRequestRow> {
    f.jobs_started()
        .into_iter()
        .filter_map(|j| match j.request {
            JobRequest::ImportMidi(r) => Some(r),
            _ => None,
        })
        .collect()
}

#[test]
fn import_sheet_defaults() {
    let f = Arc::new(busy());
    f.set_midi_file_info("/tmp/a.mid", info(MidiFileKindRow::Smf0, true));
    f.set_midi_file_info("/tmp/b.mid", info(MidiFileKindRow::Smf1, false));
    let (mut s, _, id) = store_with(&f);
    assert!(s.document(id).unwrap().midi_import().is_none());
    read(&mut s, id, "/tmp/a.mid");
    let i = sheet(&s, id);
    assert!(i.shown());
    assert_eq!(i.path(), "/tmp/a.mid");
    assert_eq!(i.info(), &info(MidiFileKindRow::Smf0, true));
    assert!(i.split_channels && i.mpe && !i.into_new_project);
    let d = s.document(id).unwrap();
    assert!(!d.midi_import_use_tempo() && !d.midi_import_tempo_locked());
    let c = |channel, dest, notes| ChannelChoice {
        group: 0,
        channel,
        dest,
        notes,
    };
    assert_eq!(
        i.choices,
        [c(0, Some(5), 10), c(9, None, 4), c(2, Some(6), 7)]
    );
    // A second read starts afresh: a type 1 file is not split.
    s.with_document(id, |d, _| {
        d.edit_midi_import(|m| m.into_new_project = true);
        d.set_midi_import_use_tempo(true);
    });
    read(&mut s, id, "/tmp/b.mid");
    let i = sheet(&s, id);
    assert!(!i.split_channels && !i.mpe && !i.into_new_project);
    assert!(!s.document(id).unwrap().midi_import_use_tempo());
}

/// Ruling T7-a: the core takes the file's tempo into a sequence without
/// events, so the sheet never shows a choice that is ignored.
#[test]
fn an_empty_sequence_forces_the_tempo_on() {
    let empty = Arc::new(demo_fake());
    empty.set_midi_file_info("/tmp/a.mid", info(MidiFileKindRow::Smf1, false));
    let (mut s, _, id) = store_with(&empty);
    read(&mut s, id, "/tmp/a.mid");
    let d = s.document_mut(id).unwrap();
    assert!(d.midi_import_use_tempo() && d.midi_import_tempo_locked());
    d.set_midi_import_use_tempo(false);
    assert!(d.midi_import_use_tempo());
    assert_eq!(
        d.midi_import_tempo_box(),
        TempoBox {
            on: true,
            enabled: false,
            help: TEMPO_LOCKED_HELP
        }
    );

    let f = Arc::new(busy());
    f.set_midi_file_info("/tmp/a.mid", info(MidiFileKindRow::Smf1, false));
    let (mut s, _, id) = store_with(&f);
    read(&mut s, id, "/tmp/a.mid");
    let d = s.document_mut(id).unwrap();
    assert!(!d.midi_import_use_tempo() && !d.midi_import_tempo_locked());
    assert_eq!(
        d.midi_import_tempo_box(),
        TempoBox {
            on: false,
            enabled: true,
            help: TEMPO_HELP
        }
    );
    d.set_midi_import_use_tempo(true);
    assert!(d.midi_import_use_tempo());
    // A new project always takes the file's tempo: shown on, locked.
    d.edit_midi_import(|m| m.into_new_project = true);
    d.set_midi_import_use_tempo(false);
    assert_eq!(
        d.midi_import_tempo_box(),
        TempoBox {
            on: true,
            enabled: false,
            help: TEMPO_LOCKED_HELP
        }
    );
    d.edit_midi_import(|m| m.into_new_project = false);
    d.set_midi_import_use_tempo(true);
    d.start_midi_import();
    assert!(imports(&f).last().unwrap().use_tempo);
}

#[test]
fn start_import_sends_the_choices() {
    let f = Arc::new(busy());
    f.set_midi_file_info("/tmp/a.mid", info(MidiFileKindRow::Smf1, false));
    f.set_transport(|t| t.tick = 1920);
    let (mut s, _, id) = store_with(&f);
    s.with_document(id, |d, _| {
        d.set_selection(SelectionRow {
            time_range: None,
            lanes: vec![LaneRef { seq: 3, lane: 31 }],
            events: vec![],
        })
    });
    read(&mut s, id, "/tmp/a.mid");
    let d = s.document_mut(id).unwrap();
    d.edit_midi_import(|m| {
        m.choices[1].dest = None;
        m.choices[2].dest = Some(9);
        m.mpe = true;
    });
    d.start_midi_import();
    let map = |channel, dest| ChannelMapRow {
        group: 0,
        channel,
        dest,
    };
    assert_eq!(
        imports(&f),
        [MidiImportRequestRow {
            path: "/tmp/a.mid".into(),
            new_project: false,
            seq: 3,
            lane: Some(31),
            tick: 1920,
            channel_map: vec![map(0, Some(5)), map(9, None), map(2, Some(9))],
            split_channels: Some(false),
            mpe: true,
            use_tempo: false,
        }]
    );
    let d = s.document(id).unwrap();
    assert!(!d.midi_import().unwrap().shown());
    assert_eq!(d.jobs().len(), 0, "done at its first poll");
}

#[test]
fn a_refused_file_is_an_alert() {
    let f = Arc::new(busy());
    let (mut s, _, id) = store_with(&f);
    read(&mut s, id, "/tmp/missing.mid");
    assert!(s.document(id).unwrap().midi_import().is_none());
    let a = s.take_alert().expect("an alert");
    assert_eq!(a.title, "Import Failed");
    assert_eq!(a.message, "/tmp/missing.mid could not be read");
}

#[test]
fn a_refused_start_is_titled_midi_import_failed() {
    let f = Arc::new(busy());
    f.set_midi_file_info("/tmp/a.mid", info(MidiFileKindRow::Smf1, false));
    f.set_call_error(
        "import_midi",
        Some(vizdaw_ffi::VizError::Model {
            message: "busy".into(),
        }),
    );
    let (mut s, _, id) = store_with(&f);
    read(&mut s, id, "/tmp/a.mid");
    let d = s.document_mut(id).unwrap();
    d.start_midi_import();
    let a = d.alert.clone().expect("an alert");
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("MIDI Import Failed", "busy")
    );
    assert!(!d.midi_import().unwrap().shown());
}

/// The report of an import this document's UI ran is kept and waits to
/// be shown; one the control channel started never shows here.
#[test]
fn a_finished_ui_job_shows_the_report() {
    let report = ReportRow {
        summary: "Imported 2 tracks".into(),
        items: vec![ReportItemRow {
            kind: ReportKindRow::Note,
            what: "x".into(),
            count: 1,
            details: vec![],
        }],
    };
    let f = Arc::new(busy());
    f.set_midi_file_info("/tmp/a.mid", info(MidiFileKindRow::Smf1, false));
    f.script_job(JobScript::done().steps(&[0.5]).report(report.clone()));
    let (mut s, _, id) = store_with(&f);
    read(&mut s, id, "/tmp/a.mid");
    let d = s.document_mut(id).unwrap();
    d.start_midi_import();
    assert!(!d.report_shown(), "still running");
    d.poll_jobs();
    assert!(d.report_shown());
    assert_eq!(d.last_report(), Some(&report));
    d.hide_report();
    assert!(!d.report_shown() && d.last_report() == Some(&report));
    // A job the channel started is not the UI's: nothing is kept.
    f.script_job(JobScript::done().report(report.clone()));
    let g = Arc::new(busy());
    g.script_job(JobScript::done().report(report));
    let (mut s, _, id) = store_with(&g);
    let job = vizdaw_app::jobs::FileJob::ImportMidi(MidiImportRequestRow {
        path: "/tmp/a.mid".into(),
        new_project: false,
        seq: 3,
        lane: None,
        tick: 0,
        channel_map: vec![],
        split_channels: None,
        mpe: false,
        use_tempo: false,
    });
    let d = s.document_mut(id).unwrap();
    job.start(d.session().as_ref()).unwrap();
    d.poll_jobs();
    assert!(!d.report_shown() && d.last_report().is_none());
}

#[test]
fn the_sheets_new_project_imports_with_its_choices() {
    let f = Arc::new(busy());
    f.set_midi_file_info("/tmp/a.mid", info(MidiFileKindRow::Smf1, false));
    let (mut s, core, id) = store_with(&f);
    read(&mut s, id, "/tmp/a.mid");
    assert!(!sheet(&s, id).into_new_project);
    s.with_document(id, |d, _| {
        d.edit_midi_import(|m| {
            m.into_new_project = true;
            m.split_channels = true;
            m.mpe = true;
        })
    });
    let made = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    core.hand_out("new_project", made.clone());
    let new = s.import_midi_as_new_project(id).expect("a new project");
    assert!(new != id && s.document(new).is_some());
    assert!(!sheet(&s, id).shown() && imports(&f).is_empty());
    let r = imports(&made).pop().unwrap();
    assert!(r.path == "/tmp/a.mid" && r.new_project && r.use_tempo);
    assert!(r.seq == 9 && r.lane.is_none() && r.tick == 0);
    assert!(r.split_channels == Some(true) && r.mpe);
    assert!(r.channel_map.len() == 3 && r.channel_map.iter().all(|c| c.dest.is_none()));
    assert!(
        core.calls()
            .iter()
            .any(|c| c.starts_with("new_project(48000,")),
        "{:?}",
        core.calls()
    );
    // Read again: the choice starts as this project.
    read(&mut s, id, "/tmp/a.mid");
    assert!(!sheet(&s, id).into_new_project);
}

/// The new project goes again when its import fails while untouched,
/// with the store's alert "Import Failed"; refused at once, nothing is
/// left.
#[test]
fn a_failed_new_project_import_leaves_nothing() {
    let f = Arc::new(busy());
    f.set_midi_file_info("/tmp/a.mid", info(MidiFileKindRow::Smf1, false));
    let (mut s, core, id) = store_with(&f);
    read(&mut s, id, "/tmp/a.mid");
    let made = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    made.script_job(JobScript::failed("not a MIDI file").steps(&[0.5]));
    core.hand_out("new_project", made);
    let new = s.import_midi_as_new_project(id).unwrap();
    s.with_document(new, |d, _| d.poll_jobs());
    assert!(s.document(new).is_none() && s.count() == 1);
    let a = s.take_alert().unwrap();
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("Import Failed", "not a MIDI file")
    );
    // Refused at once.
    read(&mut s, id, "/tmp/a.mid");
    let made = Arc::new(FakeSession::new(vec![seq(9, "New", true, 0)]));
    made.script_job(JobScript::failed("bad file"));
    core.hand_out("new_project", made);
    assert_eq!(s.import_midi_as_new_project(id), None);
    assert_eq!(s.count(), 1);
    assert_eq!(s.take_alert().unwrap().title, "Import Failed");
}

#[test]
fn no_sequence_shown_is_an_alert() {
    let f = Arc::new(FakeSession::new(vec![]));
    f.set_midi_file_info("/tmp/a.mid", info(MidiFileKindRow::Smf1, false));
    let core = Arc::new(FakeCore::new());
    let mut s = DocumentStore::with_core(core.clone());
    core.hand_out("new_demo", f.clone());
    let id = s.new_demo().unwrap();
    read(&mut s, id, "/tmp/a.mid");
    let d = s.document_mut(id).unwrap();
    d.start_midi_import();
    let a = d.alert.clone().unwrap();
    assert_eq!(
        (a.title.as_str(), a.message.as_str()),
        ("Import Failed", "No sequence is shown.")
    );
    assert!(imports(&f).is_empty());
}

#[test]
fn the_sheets_texts() {
    let c = |group, channel| ChannelChoice {
        group,
        channel,
        dest: None,
        notes: 0,
    };
    assert_eq!(mi::channel_name(&c(0, 9)), "Channel 10");
    assert_eq!(mi::channel_name(&c(1, 9)), "Group 2, channel 10");
    assert_eq!(
        mi::destination_label(&c(0, 9)),
        "Destination for channel 10"
    );
    assert_eq!(
        mi::destination_label(&c(1, 0)),
        "Destination for group 2, channel 1"
    );
    assert_eq!(mi::notes_text(1), "1 note");
    assert_eq!(mi::notes_text(7), "7 notes");
    let mut i = info(MidiFileKindRow::Smf0, false);
    assert_eq!(
        mi::describe(&i, "/tmp/Song.mid"),
        "Song.mid: Standard MIDI File, type 0, 2 tracks"
    );
    i.kind = MidiFileKindRow::Clip;
    i.tracks = 1;
    assert_eq!(
        mi::describe(&i, "/x/a.midi2"),
        "a.midi2: MIDI Clip File, 1 track"
    );
    i.kind = MidiFileKindRow::Smf1;
    assert!(mi::describe(&i, "b.mid").starts_with("b.mid: Standard MIDI File, type 1"));
}
