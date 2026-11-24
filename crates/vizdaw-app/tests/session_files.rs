//! The file and job session calls (slice 7a Task 1): round trips through
//! the fake, its scripted jobs, the core-call table, and a few smokes on
//! a real session.

use std::sync::Arc;
use std::time::{Duration, Instant};

use vizdaw_app::testing::{FakeCore, FakeSession, JobRequest, JobScript, demo_fake};
use vizdaw_app::{CoreCalls, DocSession, DocumentStore, LiveCore};
use vizdaw_ffi::{
    AafDepthRow, AafExportRequestRow, AafFpsRow, AafImportRequestRow, AafInfoRow, AafStereoRow,
    BitDepthRow, BlockRule, BounceCheckRow, BounceRangeRow, BounceRequestRow, BounceResultRow,
    BounceSourceRow, BouncedFileRow, ClipExportRequestRow, ClipNamesRow, ContainerRow,
    DawprojectExportRequestRow, DawprojectImportRequestRow, DawprojectInfoRow,
    DawprojectMergeRequestRow, ForDawRequestRow, ImportTargetRow, JobKindRow, JobStateRow,
    MergePreviewRow, MergeStepRow, MidiFileInfoRow, MidiFileKindRow, MidiImportRequestRow,
    OpenOptions, ProjectFormatRow, RegionGainRow, ReportRow, SmfExportRequestRow, SongInfoRow,
    VizError,
};

fn report(summary: &str) -> ReportRow {
    ReportRow {
        summary: summary.into(),
        items: vec![],
    }
}

fn model(m: &str) -> VizError {
    VizError::Model { message: m.into() }
}

fn format(rate: u32, depth: BitDepthRow) -> ProjectFormatRow {
    ProjectFormatRow {
        sample_rate: rate,
        depth,
    }
}

fn bounce_req(path: Option<String>) -> BounceRequestRow {
    BounceRequestRow {
        seq: 3,
        source: BounceSourceRow::Mix,
        range: BounceRangeRow::Song,
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path,
        add_to_project: false,
        replace: true,
    }
}

fn smf(path: &str) -> SmfExportRequestRow {
    SmfExportRequestRow {
        seq: 3,
        lanes: vec![],
        path: path.into(),
        replace: false,
        type0: false,
        ppq: 960,
        mpe: false,
    }
}

fn clip(path: &str) -> ClipExportRequestRow {
    ClipExportRequestRow {
        seq: 3,
        lanes: vec![30],
        path: path.into(),
        replace: false,
        start: None,
        end: None,
    }
}

fn dawproject_export(path: &str) -> DawprojectExportRequestRow {
    DawprojectExportRequestRow {
        seq: 3,
        path: path.into(),
        replace: false,
        region_gain: RegionGainRow::Apply,
        blocks: BlockRule::FixedBars { bars: 1 },
    }
}

fn aaf_export(path: &str) -> AafExportRequestRow {
    AafExportRequestRow {
        seq: 3,
        path: path.into(),
        embed: true,
        depth: AafDepthRow::Int24,
        stereo: AafStereoRow::MonoPairs,
        hour_start: false,
        fps: AafFpsRow::Fps25,
        clip_gain: true,
        automation: true,
    }
}

fn merge_req(step: MergeStepRow) -> DawprojectMergeRequestRow {
    DawprojectMergeRequestRow {
        path: "/in/Song.dawproject".into(),
        snapshot: None,
        step,
        date: "2026-11-23".into(),
    }
}

fn preview() -> MergePreviewRow {
    MergePreviewRow {
        applied: 1,
        kept: 2,
        added: 0,
        removed: 0,
        conflicts: 0,
        conflict_lines: vec![],
        summary: "1 change".into(),
        export_id: "e1".into(),
        exported_at: 7,
    }
}

// ---- scripted jobs ----

#[test]
fn a_scripted_job_advances_one_step_per_poll_then_ends_done() {
    let f = demo_fake();
    f.script_job(
        JobScript::done()
            .steps(&[0.25, 0.5])
            .written(&["/out/Song.mid"])
            .report(report("1 track")),
    );
    let id = f.export_midi(smf("/out/Song.mid")).unwrap();
    let a = f.job(id).unwrap();
    assert_eq!((a.state, a.progress), (JobStateRow::Running, 0.25));
    assert_eq!(a.kind, JobKindRow::ExportMidi);
    let b = f.job(id).unwrap();
    assert_eq!((b.state, b.progress), (JobStateRow::Running, 0.5));
    let c = f.job(id).unwrap();
    assert_eq!((c.state, c.progress), (JobStateRow::Done, 1.0));
    assert_eq!(c.written, vec!["/out/Song.mid".to_string()]);
    assert_eq!(c.report, Some(report("1 track")));
    // An ended job stays as it ended.
    assert_eq!(f.job(id).unwrap(), c);
    assert_eq!(f.reads("job"), 4);
}

#[test]
fn a_scripted_job_can_fail_with_its_message_and_problems() {
    let f = demo_fake();
    f.script_job(
        JobScript::failed("No room on the disk.")
            .steps(&[0.5])
            .problems(&["a.wav: unreadable"]),
    );
    let id = f.import_audio(vec!["/x/a.wav".into()], None).unwrap();
    assert_eq!(f.job(id).unwrap().state, JobStateRow::Running);
    let end = f.job(id).unwrap();
    assert_eq!(end.state, JobStateRow::Failed);
    assert_eq!(end.message.as_deref(), Some("No room on the disk."));
    assert_eq!(end.problems, vec!["a.wav: unreadable".to_string()]);
}

#[test]
fn a_scripted_job_can_end_cancelled() {
    let f = demo_fake();
    f.script_job(JobScript::cancelled());
    let id = f.convert_media().unwrap();
    assert_eq!(f.job(id).unwrap().state, JobStateRow::Cancelled);
}

#[test]
fn cancel_job_ends_a_running_job_cancelled_but_not_an_ended_one() {
    let f = demo_fake();
    f.script_job(JobScript::done().steps(&[0.1, 0.2, 0.3]));
    f.script_job(JobScript::done());
    let running = f.bounce(bounce_req(Some("/out/B.wav".into()))).unwrap();
    let ended = f.convert_media().unwrap();
    assert_eq!(f.job(running).unwrap().progress, 0.1);
    assert_eq!(f.job(ended).unwrap().state, JobStateRow::Done);
    f.cancel_job(running).unwrap();
    f.cancel_job(ended).unwrap();
    assert_eq!(f.job(running).unwrap().state, JobStateRow::Cancelled);
    assert_eq!(f.job(ended).unwrap().state, JobStateRow::Done);
    assert_eq!(f.cancelled_jobs(), vec![running, ended]);
    assert!(f.calls().contains(&format!("cancel_job({running})")));
    // A cancelled bounce wrote nothing to report.
    assert_eq!(f.bounce_result(running).unwrap(), None);
}

#[test]
fn an_unscripted_job_is_done_at_its_first_poll() {
    let f = demo_fake();
    let id = f.convert_media().unwrap();
    let row = f.job(id).unwrap();
    assert_eq!(row.state, JobStateRow::Done);
    assert_eq!(row.kind, JobKindRow::Convert);
}

#[test]
fn an_unknown_job_is_refused_as_the_core_refuses_it() {
    let f = demo_fake();
    assert_eq!(f.job(9).unwrap_err(), model("unknown job 9"));
    let real = LiveCore.new_demo().unwrap();
    assert_eq!(real.job(9).unwrap_err(), model("unknown job 9"));
}

#[test]
fn every_job_call_starts_a_job_of_its_kind_with_its_request() {
    let f = demo_fake();
    let target = ImportTargetRow {
        seq: 3,
        lane: Some(30),
        tick: 0,
    };
    let fmt = format(44_100, BitDepthRow::Int16);
    let for_daw = ForDawRequestRow {
        seq: 3,
        strips: None,
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path: "/out".into(),
        add_to_project: false,
    };
    let midi_in = MidiImportRequestRow {
        path: "/in/a.mid".into(),
        new_project: false,
        seq: 3,
        lane: None,
        tick: 0,
        channel_map: vec![],
        split_channels: None,
        mpe: false,
        use_tempo: true,
    };
    let aaf_in = AafImportRequestRow {
        path: "/in/a.aaf".into(),
        new_project: false,
        midi_path: None,
        media_folder: None,
        join_pairs: true,
    };
    let dp_in = DawprojectImportRequestRow {
        path: "/in/a.dawproject".into(),
    };
    let ids = [
        f.import_audio(vec!["/a.wav".into()], Some(target)).unwrap(),
        f.convert_media().unwrap(),
        f.set_project_format(fmt).unwrap(),
        f.relink_media(5, "/a.wav".into()).unwrap(),
        f.search_media("/media".into()).unwrap(),
        f.bounce(bounce_req(None)).unwrap(),
        f.import_midi(midi_in.clone()).unwrap(),
        f.export_midi(smf("/o.mid")).unwrap(),
        f.export_midi_clip(clip("/o.midi2")).unwrap(),
        f.export_for_daw(for_daw.clone()).unwrap(),
        f.export_dawproject(dawproject_export("/o.dawproject"))
            .unwrap(),
        f.import_dawproject(dp_in.clone()).unwrap(),
        f.merge_dawproject(merge_req(MergeStepRow::Apply)).unwrap(),
        f.export_aaf(aaf_export("/o")).unwrap(),
        f.import_aaf(aaf_in.clone()).unwrap(),
    ];
    assert_eq!(ids.to_vec(), (1..=15).collect::<Vec<u64>>());
    let started = f.jobs_started();
    let kinds: Vec<JobKindRow> = started.iter().map(|j| j.kind).collect();
    use JobKindRow::*;
    assert_eq!(
        kinds,
        vec![
            Import,
            Convert,
            Format,
            Relink,
            Relink,
            Bounce,
            ImportMidi,
            ExportMidi,
            ExportMidi,
            ExportProject,
            ExportDawproject,
            ImportProject,
            MergeDawproject,
            ExportAaf,
            ImportAaf
        ]
    );
    let requests: Vec<JobRequest> = started.into_iter().map(|j| j.request).collect();
    assert_eq!(
        requests,
        vec![
            JobRequest::Import {
                paths: vec!["/a.wav".into()],
                target: Some(target)
            },
            JobRequest::Convert,
            JobRequest::Format(fmt),
            JobRequest::Relink {
                media: 5,
                path: "/a.wav".into()
            },
            JobRequest::Search("/media".into()),
            JobRequest::Bounce(bounce_req(None)),
            JobRequest::ImportMidi(midi_in),
            JobRequest::ExportMidi(smf("/o.mid")),
            JobRequest::ExportMidiClip(clip("/o.midi2")),
            JobRequest::ForDaw(for_daw),
            JobRequest::ExportDawproject(dawproject_export("/o.dawproject")),
            JobRequest::ImportDawproject(dp_in),
            JobRequest::MergeDawproject(merge_req(MergeStepRow::Apply)),
            JobRequest::ExportAaf(aaf_export("/o")),
            JobRequest::ImportAaf(aaf_in),
        ]
    );
    for id in ids {
        assert_eq!(f.job(id).unwrap().kind, kinds[id as usize - 1]);
    }
}

#[test]
fn scripted_errors_refuse_a_job_before_it_starts() {
    let f = demo_fake();
    f.set_call_error("bounce", Some(model("Choose a file ending in .wav.")));
    assert_eq!(
        f.bounce(bounce_req(None)).unwrap_err(),
        model("Choose a file ending in .wav.")
    );
    assert!(f.jobs_started().is_empty());
    let id = f.convert_media().unwrap();
    f.fail_next("job", model("busy"));
    assert_eq!(f.job(id).unwrap_err(), model("busy"));
    assert_eq!(f.job(id).unwrap().state, JobStateRow::Done);
    f.fail_next("cancel_job", model("no"));
    assert!(f.cancel_job(id).is_err());
}

#[test]
fn a_format_job_done_changes_the_project_format() {
    let f = demo_fake();
    assert_eq!(
        f.project_info().unwrap().format,
        format(48_000, BitDepthRow::Int24)
    );
    f.script_job(JobScript::done().steps(&[0.5]));
    let id = f
        .set_project_format(format(96_000, BitDepthRow::Float32))
        .unwrap();
    f.job(id).unwrap();
    assert_eq!(
        f.project_info().unwrap().format,
        format(48_000, BitDepthRow::Int24)
    );
    f.job(id).unwrap();
    assert_eq!(
        f.project_info().unwrap().format,
        format(96_000, BitDepthRow::Float32)
    );
    assert!(
        f.calls()
            .contains(&"set_project_format(96000,Float32)".to_string())
    );
}

#[test]
fn bounce_result_answers_once_the_bounce_is_done() {
    let f = demo_fake();
    let wrote = BounceResultRow {
        files: vec![BouncedFileRow {
            path: "/out/Bounce.wav".into(),
            frames: 48_000,
            rate: 48_000,
            bits: "24-bit".into(),
            channels: 2,
            clipped: 0,
        }],
        media: vec![],
    };
    f.script_job(JobScript::done().steps(&[0.5]).bounce(wrote.clone()));
    let id = f
        .bounce(bounce_req(Some("/out/Bounce.wav".into())))
        .unwrap();
    assert_eq!(f.bounce_result(id).unwrap(), None);
    f.job(id).unwrap();
    assert_eq!(f.bounce_result(id).unwrap(), None);
    f.job(id).unwrap();
    assert_eq!(f.bounce_result(id).unwrap(), Some(wrote));
}

#[test]
fn a_held_merge_is_confirmed_or_cancelled_once() {
    let f = demo_fake();
    f.set_merge_report(report("merged"));
    f.script_job(JobScript::done().merge(preview()));
    let held = f.merge_dawproject(merge_req(MergeStepRow::Hold)).unwrap();
    let other = f.convert_media().unwrap();
    let no = model("no merge is waiting");
    // Not ended yet.
    assert_eq!(f.confirm_merge(held).unwrap_err(), no);
    assert_eq!(f.job(held).unwrap().merge, Some(preview()));
    assert_eq!(f.confirm_merge(other).unwrap_err(), no);
    let version = f.snap().version;
    assert_eq!(f.confirm_merge(held).unwrap(), report("merged"));
    assert_eq!(f.snap().version, version + 1);
    assert_eq!(f.interchange_report().unwrap(), Some(report("merged")));
    assert_eq!(f.confirm_merge(held).unwrap_err(), no);
    assert_eq!(f.cancel_merge(held).unwrap_err(), no);

    f.script_job(JobScript::done().merge(preview()));
    let dropped = f.merge_dawproject(merge_req(MergeStepRow::Hold)).unwrap();
    f.job(dropped).unwrap();
    f.cancel_merge(dropped).unwrap();
    assert!(f.calls().contains(&format!("cancel_merge({dropped})")));
    assert_eq!(f.confirm_merge(dropped).unwrap_err(), no);
}

// ---- scripted reads ----

#[test]
fn the_reads_answer_what_was_scripted() {
    let f = demo_fake();
    assert!(f.media().unwrap().is_empty());
    assert!(!f.recheck_media().unwrap());
    f.set_recheck_media(true);
    assert!(f.recheck_media().unwrap());
    assert_eq!(f.bounce_check(bounce_req(None)).unwrap().frames, 0);
    let check = BounceCheckRow {
        external_lanes: 1,
        frames: 96_000,
        files: vec!["/out/B.wav".into()],
        warning: Some("1 track plays on an external instrument".into()),
    };
    f.set_bounce_check(check.clone());
    assert_eq!(f.bounce_check(bounce_req(None)).unwrap(), check);

    assert_eq!(f.export_midi_check(smf("/o.mid")).unwrap(), report(""));
    f.set_check_report("export_midi_check", report("2 tracks"));
    f.set_check_report("export_midi_clip_check", report("1 clip"));
    f.set_check_report("export_dawproject_check", report("dp"));
    f.set_check_report("export_aaf_check", report("aaf"));
    assert_eq!(
        f.export_midi_check(smf("/o.mid")).unwrap(),
        report("2 tracks")
    );
    assert_eq!(
        f.export_midi_clip_check(clip("/o")).unwrap(),
        report("1 clip")
    );
    assert_eq!(
        f.export_dawproject_check(dawproject_export("/o")).unwrap(),
        report("dp")
    );
    assert_eq!(f.export_aaf_check(aaf_export("/o")).unwrap(), report("aaf"));
    assert_eq!(f.reads("export_midi_check"), 2);

    let names = ClipNamesRow {
        names: vec!["Groove.midi2".into()],
        taken: vec!["Groove.midi2".into()],
    };
    f.set_clip_names(names.clone());
    assert_eq!(f.export_midi_clip_names(clip("/o")).unwrap(), names);

    let info = MidiFileInfoRow {
        kind: MidiFileKindRow::Smf1,
        tracks: 2,
        channels: vec![],
        has_tempo: true,
        mpe_found: false,
    };
    assert!(matches!(
        f.midi_file_info("/in/a.mid".into()),
        Err(VizError::Io { .. })
    ));
    f.set_midi_file_info("/in/a.mid", info.clone());
    assert_eq!(f.midi_file_info("/in/a.mid".into()).unwrap(), info);

    assert!(f.export_for_daw_check(for_daw_req()).is_err());
    assert_eq!(f.interchange_report().unwrap(), None);
    assert!(f.dawproject_snapshots().unwrap().is_empty());
    assert!(f.recovered_files().unwrap().is_empty());
    assert_eq!(f.co_producer_copies_size().unwrap().count, 0);
}

fn for_daw_req() -> ForDawRequestRow {
    ForDawRequestRow {
        seq: 3,
        strips: None,
        container: ContainerRow::Wav,
        depth: BitDepthRow::Int24,
        rate: 48_000,
        dither: false,
        path: "/out".into(),
        add_to_project: false,
    }
}

#[test]
fn export_names_follow_the_song_title_else_the_sequence() {
    let f = demo_fake();
    assert_eq!(f.dawproject_name(3).unwrap(), "Song.dawproject");
    assert_eq!(f.aaf_folder_name(3).unwrap(), "Song AAF");
    assert!(f.dawproject_name(99).is_err());
    let info = SongInfoRow {
        title: Some("Tune".into()),
        ..SongInfoRow::default()
    };
    let version = f.snap().version;
    f.set_song_info(info.clone()).unwrap();
    assert_eq!(f.snap().version, version + 1);
    assert_eq!(f.song_info().unwrap(), info);
    assert_eq!(f.dawproject_name(3).unwrap(), "Tune.dawproject");
    assert_eq!(f.aaf_folder_name(3).unwrap(), "Tune AAF");
}

#[test]
fn recovering_recordings_answers_the_offer_once() {
    let f = demo_fake();
    assert_eq!(
        f.recover_recordings(true).unwrap_err(),
        model("there are no recovered recordings")
    );
    f.set_recovered_files(vec![vizdaw_ffi::record::RecoveredFileRow {
        path: "Media/Recorded/a.caf".into(),
        frames: 10,
        size_known: false,
        readable: true,
    }]);
    assert_eq!(f.recovered_files().unwrap().len(), 1);
    f.recover_recordings(true).unwrap();
    assert!(f.recovered_files().unwrap().is_empty());
    assert!(f.calls().contains(&"recover_recordings(true)".to_string()));
}

#[test]
fn every_read_takes_a_scripted_error() {
    let f = demo_fake();
    for call in ["media", "project_info", "song_info", "interchange_report"] {
        f.fail_next(call, model(call));
    }
    assert_eq!(f.media().unwrap_err(), model("media"));
    assert_eq!(f.project_info().unwrap_err(), model("project_info"));
    assert_eq!(f.song_info().unwrap_err(), model("song_info"));
    assert_eq!(
        f.interchange_report().unwrap_err(),
        model("interchange_report")
    );
    assert!(f.media().is_ok());
}

// ---- the core-call table ----

#[test]
fn the_fake_core_makes_sessions_and_logs_each_call() {
    let core = FakeCore::new();
    core.new_empty().unwrap();
    core.new_demo_audio().unwrap();
    let made = core
        .new_project(format(44_100, BitDepthRow::Int16))
        .unwrap();
    assert_eq!(
        made.project_info().unwrap().format,
        format(44_100, BitDepthRow::Int16)
    );
    let opened = core.open("/p/Song.vizdaw").unwrap();
    assert_eq!(opened.path().unwrap().as_deref(), Some("/p/Song.vizdaw"));
    core.open_with(
        "/p/Song.vizdaw",
        OpenOptions {
            plugins_off: true,
            autosave: None,
        },
    )
    .unwrap();
    core.open_copy("/p/Song.vizdaw/Co-Producer/Copies/A.vizdaw")
        .unwrap();
    assert_eq!(
        core.calls(),
        vec![
            "new_empty",
            "new_demo_audio",
            "new_project(44100,Int16)",
            "open(/p/Song.vizdaw)",
            "open_with(/p/Song.vizdaw,plugins_off=true,autosave=None)",
            "open_copy(/p/Song.vizdaw/Co-Producer/Copies/A.vizdaw)",
        ]
    );
    let made = core.made();
    assert_eq!(made.len(), 6);
    // The demo with audio is the demo's three sequences.
    assert_eq!(made[1].1.snap().sequences.len(), 3);
}

#[test]
fn the_fake_core_hands_out_scripted_sessions_and_errors() {
    let core = FakeCore::new();
    let mine = Arc::new(FakeSession::new(vec![]));
    core.hand_out("open", mine.clone());
    core.open("/a.vizdaw").unwrap();
    assert!(Arc::ptr_eq(&core.last_made().unwrap(), &mine));
    core.fail_next("open_copy", model("That is not a Co-Producer copy."));
    assert_eq!(
        core.open_copy("/x").err().unwrap(),
        model("That is not a Co-Producer copy.")
    );
    core.set_call_error("new_demo_audio", Some(model("no demo")));
    assert!(core.new_demo_audio().is_err());
    core.set_call_error("new_demo_audio", None);
    assert!(core.new_demo_audio().is_ok());
}

#[test]
fn the_fake_core_reads_files_as_scripted() {
    let core = FakeCore::new();
    assert!(matches!(
        core.dawproject_info("/in/a.dawproject"),
        Err(VizError::Io { .. })
    ));
    let dp = DawprojectInfoRow {
        sample_rate: 44_100,
        tracks: 3,
        audio_files: 1,
        from_vizdaw: true,
        export_id: Some("e1".into()),
        project_id: None,
    };
    core.set_dawproject_info("/in/a.dawproject", dp.clone());
    assert_eq!(core.dawproject_info("/in/a.dawproject").unwrap(), dp);
    let aaf = AafInfoRow {
        sample_rate: 48_000,
        tracks: 2,
        audio_files: 2,
        offline: 0,
        from_vizdaw: false,
        midi_beside: None,
    };
    core.set_aaf_info("/in/a.aaf", aaf.clone());
    assert_eq!(core.aaf_info("/in/a.aaf").unwrap(), aaf);
    assert!(!core.recovery_check("/p/Song.vizdaw").unwrap().crashed);
    assert_eq!(
        core.format_label(format(44_100, BitDepthRow::Int16)),
        "44.1 kHz, 16-bit"
    );
    assert!(core.project_rates().contains(&48_000));
    assert!(
        core.calls()
            .contains(&"dawproject_info(/in/a.dawproject)".to_string())
    );
}

#[test]
fn the_store_makes_and_opens_through_its_core() {
    let core = Arc::new(FakeCore::new());
    let mut store = DocumentStore::with_core(core.clone());
    store.new_empty().unwrap();
    store.new_demo().unwrap();
    store.open(std::path::Path::new("/p/Song.vizdaw")).unwrap();
    assert_eq!(
        core.calls(),
        vec![
            "new_empty",
            "new_demo",
            "recovery_check(/p/Song.vizdaw)",
            "open(/p/Song.vizdaw)"
        ]
    );
    assert_eq!(store.core().project_rates(), core.project_rates());
}

#[test]
fn a_store_built_from_makers_refuses_the_other_constructors() {
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(demo_fake()))),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    assert!(store.core().new_demo_audio().is_err());
    assert!(store.core().dawproject_info("/a.dawproject").is_err());
    assert_eq!(
        store
            .core()
            .format_label(format(48_000, BitDepthRow::Int24)),
        "48 kHz, 24-bit"
    );
}

// ---- real sessions ----

#[test]
fn a_real_session_answers_project_info_song_info_and_names() {
    let s = LiveCore.new_demo().unwrap();
    let info = s.project_info().unwrap();
    assert!(LiveCore.project_rates().contains(&info.format.sample_rate));
    assert_eq!(info.mismatched, 0);
    assert_eq!(s.interchange_report().unwrap(), None);
    assert!(s.recovered_files().unwrap().is_empty());
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .into_iter()
        .find(|q| q.top_level)
        .unwrap();
    assert!(s.dawproject_name(song.id).unwrap().ends_with(".dawproject"));
    assert!(s.aaf_folder_name(song.id).unwrap().ends_with(" AAF"));
    let before = s.snapshot().unwrap().version;
    let info = SongInfoRow {
        title: Some("Smoke".into()),
        ..s.song_info().unwrap()
    };
    let after = s.set_song_info(info.clone()).unwrap();
    assert!(after.version > before);
    assert_eq!(s.song_info().unwrap().title.as_deref(), Some("Smoke"));
    assert_eq!(s.dawproject_name(song.id).unwrap(), "Smoke.dawproject");
}

#[test]
fn a_real_demo_with_audio_lists_its_media() {
    let s = LiveCore.new_demo_audio().unwrap();
    assert!(!s.media().unwrap().is_empty());
}

#[test]
fn a_real_new_project_has_its_format() {
    let s = LiveCore
        .new_project(format(44_100, BitDepthRow::Int16))
        .unwrap();
    assert_eq!(
        s.project_info().unwrap().format,
        format(44_100, BitDepthRow::Int16)
    );
    assert!(
        LiveCore
            .dawproject_info("/nowhere/missing.dawproject")
            .is_err()
    );
}

#[test]
fn a_real_bounce_of_the_demo_runs_as_a_job_and_writes_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("Bounce.wav");
    let s = LiveCore.new_demo_audio().unwrap();
    let song = s
        .snapshot()
        .unwrap()
        .sequences
        .into_iter()
        .find(|q| q.top_level)
        .unwrap();
    let req = BounceRequestRow {
        seq: song.id,
        range: BounceRangeRow::Ticks {
            start: 0,
            end: 61_440,
        },
        ..bounce_req(Some(out.display().to_string()))
    };
    assert!(s.bounce_check(req.clone()).unwrap().frames > 0);
    let id = s.bounce(req).unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    let row = loop {
        let row = s.job(id).unwrap();
        if row.state != JobStateRow::Running {
            break row;
        }
        assert!(Instant::now() < deadline, "the bounce never ended");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(row.state, JobStateRow::Done, "{:?}", row.message);
    assert_eq!(row.kind, JobKindRow::Bounce);
    let result = s.bounce_result(id).unwrap().unwrap();
    assert_eq!(result.files[0].path, out.display().to_string());
    assert!(out.exists());
}
