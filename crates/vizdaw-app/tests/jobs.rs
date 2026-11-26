//! Jobs and project banners (slice 7a Task 2; Swift `ProjectMediaTests`):
//! Import Audio as a job, polling from `idle`, the shared job answer,
//! failure alerts, the Problems, Convert, device-rate and disk banners,
//! Cancel, the default format; and real-session runs per job kind.

mod support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use support::{FakeSession, demo_fake};
use vizdaw_app::banners::khz;
use vizdaw_app::jobs::{
    answer, bounce_status, expired_message, failure_title, import_target, job_text,
};
use vizdaw_app::testing::{JobRequest, JobScript};
use vizdaw_app::transport::TransportModel;
use vizdaw_app::{
    BannerAction, DocSession, DocumentModel, DocumentStore, JobAnswer, Prefs, ProjectBanner,
};
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::{
    BitDepthRow, BounceResultRow, BouncedFileRow, ImportTargetRow, JobKindRow, JobRow, JobStateRow,
    ProjectFormatRow, ProjectInfoRow, ReportRow, VizError,
};

fn model(f: &Arc<FakeSession>) -> DocumentModel {
    DocumentModel::new(f.clone()).unwrap()
}

fn fake() -> Arc<FakeSession> {
    Arc::new(demo_fake())
}

fn has_job_banner(m: &DocumentModel) -> bool {
    m.banners()
        .iter()
        .any(|b| matches!(b, ProjectBanner::Job { .. }))
}

fn info(mismatched: u32, rate_mismatch: bool, device_rate: f64) -> ProjectInfoRow {
    ProjectInfoRow {
        format: ProjectFormatRow {
            sample_rate: 48_000,
            depth: BitDepthRow::Int24,
        },
        mismatched,
        rate_mismatch,
        device_rate,
    }
}

fn row(kind: JobKindRow, state: JobStateRow) -> JobRow {
    JobRow {
        id: 7,
        kind,
        progress: 1.0,
        state,
        message: None,
        problems: vec![],
        created: vec![],
        report: None,
        written: vec![],
        merge: None,
    }
}

#[test]
fn the_import_target_uses_the_selected_track_for_one_file() {
    let sel = [LaneRef { seq: 2, lane: 9 }, LaneRef { seq: 1, lane: 5 }];
    let at = |seq, lane, tick| Some(ImportTargetRow { seq, lane, tick });
    assert_eq!(import_target(Some(1), &sel, 1, 960), at(1, Some(5), 960));
    assert_eq!(import_target(Some(1), &sel, 2, 960), at(1, None, 960));
    assert_eq!(import_target(Some(1), &[], 1, -100), at(1, None, 0));
    assert_eq!(import_target(None, &sel, 1, 0), None);
}

#[test]
fn an_import_runs_as_a_job_and_refreshes_when_it_ends() {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.4]).created(&[3]));
    let mut m = model(&f);
    m.import_audio(vec!["/a.wav".into()]);
    let started = f.jobs_started();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].kind, JobKindRow::Import);
    // Song (3) is shown, nothing is selected, the transport is at 0.
    assert_eq!(
        started[0].request,
        JobRequest::Import {
            paths: vec!["/a.wav".into()],
            target: Some(ImportTargetRow {
                seq: 3,
                lane: None,
                tick: 0
            }),
        }
    );
    assert!(has_job_banner(&m));
    assert_eq!(m.banners()[0].text(), "Importing audio…");
    let before = m.media_version();
    let infos = f.reads("project_info");
    m.poll_jobs();
    assert_eq!(m.media_version(), before + 1);
    assert_eq!(f.reads("project_info"), infos + 1);
    assert!(!has_job_banner(&m));
    assert!(m.alert.is_none());
    // Empty paths start nothing.
    m.import_audio(vec![]);
    assert_eq!(f.jobs_started().len(), 1);
}

#[test]
fn a_failed_job_shows_an_alert_and_problems_show_a_banner() {
    let f = fake();
    f.script_job(
        JobScript::done()
            .steps(&[0.5])
            .created(&[7])
            .problems(&["b.wav: not audio"]),
    );
    let mut m = model(&f);
    m.import_audio(vec!["/a.wav".into(), "/b.wav".into()]);
    m.poll_jobs();
    let problems = ProjectBanner::Problems(vec!["b.wav: not audio".into()]);
    assert!(m.banners().contains(&problems));
    assert_eq!(problems.text(), "b.wav: not audio");
    assert_eq!(problems.button(), "Dismiss");
    assert!(m.alert.is_none());
    m.run_banner(problems.action());
    assert!(!m.banners().contains(&problems));

    f.script_job(JobScript::failed("disk full").steps(&[0.5]));
    m.import_audio(vec!["/c.wav".into()]);
    m.poll_jobs();
    let alert = m.alert.clone().unwrap();
    assert!(alert.message.contains("disk full"), "{alert:?}");
    assert_eq!(alert.title, "Import Failed");
}

#[test]
fn a_job_that_created_nothing_shows_an_alert_listing_the_problems() {
    let f = fake();
    f.script_job(
        JobScript::done()
            .steps(&[0.5])
            .problems(&["a.txt: not audio"]),
    );
    let mut m = model(&f);
    m.import_audio(vec!["/a.txt".into()]);
    m.poll_jobs();
    assert!(
        m.alert
            .as_ref()
            .is_some_and(|a| a.message.contains("a.txt: not audio"))
    );
    assert!(
        !m.banners()
            .iter()
            .any(|b| matches!(b, ProjectBanner::Problems(_)))
    );
}

#[test]
fn a_failed_jobs_alert_is_the_control_answer() {
    let f = fake();
    f.script_job(
        JobScript::failed("disk full")
            .steps(&[0.5])
            .problems(&["a.wav: short"]),
    );
    let mut m = model(&f);
    let id = m
        .start_job(JobKindRow::Import, "Import Failed", |s| {
            s.import_audio(vec!["/a.wav".into()], None)
        })
        .unwrap();
    m.poll_jobs();
    let ended = f.job(id).unwrap();
    let JobAnswer::Failed(message) = m.job_answer(&ended) else {
        panic!("expected a failure");
    };
    assert_eq!(message, "disk full; a.wav: short");
    let alert = m.alert.clone().unwrap();
    assert_eq!(alert.message, message);
    assert_eq!(alert.title, "Import Failed");
    assert!(m.jobs().is_empty());
}

#[test]
fn a_cancelled_job_ends_without_an_alert() {
    let f = fake();
    f.script_job(JobScript::cancelled().steps(&[0.5]));
    let mut m = model(&f);
    m.import_audio(vec!["/a.wav".into()]);
    assert_eq!(m.jobs().len(), 1);
    m.poll_jobs();
    assert!(m.alert.is_none());
    assert!(m.jobs().is_empty());
}

#[test]
fn a_job_the_session_no_longer_knows_is_dropped_and_polling_stops() {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.1, 0.2, 0.3]));
    let mut m = model(&f);
    m.import_audio(vec!["/a.wav".into()]);
    // Busy is transient: the job is kept.
    f.set_call_error(
        "job",
        Some(VizError::Busy {
            message: "busy".into(),
        }),
    );
    m.poll_jobs();
    assert_eq!(m.jobs().len(), 1);
    f.set_call_error(
        "job",
        Some(VizError::Model {
            message: "unknown job 1".into(),
        }),
    );
    m.poll_jobs();
    assert!(m.jobs().is_empty());
    assert!(!has_job_banner(&m));
    assert!(m.alert.is_none());
    f.set_call_error("job", None);
    let reads = f.reads("job");
    m.idle();
    m.idle();
    assert_eq!(f.reads("job"), reads);
}

#[test]
fn idle_polls_the_listed_jobs_until_they_end() {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.25, 0.5]));
    let mut m = model(&f);
    m.import_audio(vec!["/a.wav".into()]);
    assert_eq!(m.jobs()[0].progress, 0.25);
    assert!(m.idle());
    assert_eq!(m.jobs()[0].progress, 0.5);
    m.idle();
    assert!(m.jobs().is_empty());
    let reads = f.reads("job");
    m.idle();
    assert_eq!(f.reads("job"), reads);
}

#[test]
fn mismatched_media_offers_convert() {
    let f = fake();
    f.set_project_info(info(3, false, 0.0));
    let mut m = model(&f);
    let banner = ProjectBanner::Mismatched {
        count: 3,
        label: "48 kHz, 24-bit".into(),
    };
    assert!(m.banners().contains(&banner));
    assert_eq!(
        banner.text(),
        "3 media files are not in the project format (48 kHz, 24-bit) and stay silent."
    );
    assert_eq!(banner.button(), "Convert 3 Files");
    let one = ProjectBanner::Mismatched {
        count: 1,
        label: "44.1 kHz, 16-bit".into(),
    };
    assert_eq!(
        one.text(),
        "1 media file is not in the project format (44.1 kHz, 16-bit) and stay silent."
    );
    assert_eq!(one.button(), "Convert 1 File");

    f.script_job(JobScript::done().steps(&[0.5]));
    m.run_banner(banner.action());
    assert_eq!(f.jobs_started()[0].request, JobRequest::Convert);
    // Hidden while converting.
    assert!(
        !m.banners()
            .iter()
            .any(|b| matches!(b, ProjectBanner::Mismatched { .. }))
    );
    assert_eq!(m.banners()[0].text(), "Converting media…");
    f.set_project_info(info(0, false, 0.0));
    m.poll_jobs();
    assert!(m.banners().is_empty(), "{:?}", m.banners());
}

#[test]
fn a_device_at_another_rate_offers_to_change_the_project_rate() {
    let f = fake();
    f.set_project_info(info(0, true, 44_100.0));
    let mut m = model(&f);
    let banner = ProjectBanner::RateMismatch {
        device: 44_100.0,
        project: 48_000,
    };
    assert!(m.banners().contains(&banner));
    assert_eq!(
        banner.text(),
        "The audio device runs at 44.1 kHz; this project is 48 kHz. Audio regions are muted."
    );
    assert_eq!(banner.button(), "Change Project Rate to 44.1 kHz");
    m.run_banner(banner.action());
    assert_eq!(
        f.jobs_started()[0].request,
        JobRequest::Format(ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int24
        })
    );
    assert_eq!(khz(96_000.0), "96 kHz");
    assert_eq!(khz(88_200.0), "88.2 kHz");
}

#[test]
fn the_rate_banner_follows_the_transport() {
    let f = fake();
    let mut m = model(&f);
    assert!(m.banners().is_empty());
    // The device moved: the transport says so, idle reads the info again.
    f.set_project_info(info(0, true, 44_100.0));
    f.set_transport(|t| t.rate_mismatch = true);
    m.poll_transport().unwrap();
    m.idle();
    assert!(
        m.banners()
            .iter()
            .any(|b| matches!(b, ProjectBanner::RateMismatch { .. }))
    );
    // Media changed (idle says so): the info is read again.
    f.set_project_info(info(2, true, 44_100.0));
    f.set_idle_media_changed();
    let v = m.media_version();
    m.idle();
    assert_eq!(m.media_version(), v + 1);
    assert_eq!(m.project_info().unwrap().mismatched, 2);
}

#[test]
fn disk_too_slow_follows_the_faults() {
    let f = fake();
    let mut m = model(&f);
    let underrun = |f: &FakeSession, on: bool| {
        f.set_transport(|t| {
            t.faults = if on {
                vec!["stream-underrun".into()]
            } else {
                vec![]
            }
        })
    };
    underrun(&f, true);
    m.poll_transport().unwrap();
    let banner = ProjectBanner::DiskTooSlow;
    assert!(m.banners().contains(&banner));
    assert_eq!(
        banner.text(),
        "Disk too slow: some audio played as silence."
    );
    assert_eq!(banner.button(), "Dismiss");
    m.run_banner(BannerAction::DismissDiskWarning);
    assert!(!m.banners().contains(&banner));
    m.poll_transport().unwrap();
    assert!(!m.banners().contains(&banner));
    // Cleared at the next Play: a new underrun shows again.
    underrun(&f, false);
    m.poll_transport().unwrap();
    underrun(&f, true);
    m.poll_transport().unwrap();
    assert!(m.banners().contains(&banner));

    // The transport model's reads (the window's poll) clear it too.
    let mut store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|_| Ok(Arc::new(FakeSession::new(vec![])))),
    );
    let mut t = TransportModel::new(&m).unwrap();
    m.dismiss_disk_warning();
    underrun(&f, false);
    t.poll(&mut store);
    underrun(&f, true);
    t.poll(&mut store);
    assert!(m.banners().contains(&banner));
}

#[test]
fn cancelling_a_job_asks_the_session() {
    let f = fake();
    f.script_job(JobScript::done().steps(&[0.4, 0.4]));
    let mut m = model(&f);
    m.import_audio(vec!["/a.wav".into()]);
    let banner = ProjectBanner::Job {
        id: 1,
        kind: JobKindRow::Import,
        progress: 0.4,
    };
    assert_eq!(m.banners(), vec![banner.clone()]);
    assert_eq!(banner.button(), "Cancel");
    m.run_banner(banner.action());
    assert_eq!(f.cancelled_jobs(), vec![1]);
    m.poll_jobs();
    assert!(m.jobs().is_empty());
    assert!(m.alert.is_none());
}

#[test]
fn a_refused_start_is_an_alert_with_the_callers_title() {
    let f = fake();
    f.fail_next(
        "convert_media",
        VizError::Busy {
            message: "A bounce is running.".into(),
        },
    );
    let mut m = model(&f);
    m.convert_media();
    let alert = m.alert.clone().unwrap();
    assert_eq!(alert.title, "Convert Failed");
    assert_eq!(alert.message, "A bounce is running.");
    assert!(m.jobs().is_empty());
    // try_start_job hands the refusal back instead.
    f.fail_next(
        "set_project_format",
        VizError::Model {
            message: "no".into(),
        },
    );
    m.alert = None;
    let e = m.try_start_job(JobKindRow::Format, |s| {
        s.set_project_format(ProjectFormatRow {
            sample_rate: 96_000,
            depth: BitDepthRow::Int24,
        })
    });
    assert!(e.is_err());
    assert!(m.alert.is_none());
}

#[test]
fn the_default_format_is_remembered() {
    let mut p = Prefs::in_memory();
    let default = ProjectFormatRow {
        sample_rate: 48_000,
        depth: BitDepthRow::Int24,
    };
    assert_eq!(p.default_format(), default);
    let f = ProjectFormatRow {
        sample_rate: 96_000,
        depth: BitDepthRow::Float32,
    };
    p.set_default_format(f).unwrap();
    assert_eq!(p.default_format(), f);
    assert_eq!(
        p.raw("defaultProjectFormat").and_then(|v| v.as_str()),
        Some("96000/32f")
    );
    for bad in ["garbage", "12345/24", "48000/20"] {
        p.set_raw("defaultProjectFormat", bad.into()).unwrap();
        assert_eq!(p.default_format(), default, "{bad}");
    }
    // Written through to the file.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prefs.json");
    Prefs::at(&path).set_default_format(f).unwrap();
    assert_eq!(Prefs::at(&path).default_format(), f);
}

#[test]
fn job_texts_and_failure_titles_are_swifts() {
    use JobKindRow as K;
    let all = [
        (K::Import, "Importing audio…", "Import Failed"),
        (K::Relink, "Relinking media…", "Relink Failed"),
        (K::Convert, "Converting media…", "Conversion Failed"),
        (K::Format, "Converting media…", "Conversion Failed"),
        (K::Bounce, "Bouncing…", "Bounce Failed"),
        (
            K::ExternalEdit,
            "Importing the external edit…",
            "External Edit Failed",
        ),
        (
            K::ImportMidi,
            "Importing the MIDI file…",
            "MIDI Import Failed",
        ),
        (K::ExportMidi, "Exporting MIDI…", "MIDI Export Failed"),
        (
            K::ExportProject,
            "Exporting for another DAW…",
            "Export Failed",
        ),
        (
            K::ExportDawproject,
            "Exporting DAWproject…",
            "DAWproject Export Failed",
        ),
        (
            K::ImportProject,
            "Importing DAWproject…",
            "DAWproject Import Failed",
        ),
        (
            K::MergeDawproject,
            "Merging DAWproject…",
            "DAWproject Merge Failed",
        ),
        (K::ExportAaf, "Exporting AAF…", "AAF Export Failed"),
        (K::ImportAaf, "Importing AAF…", "AAF Import Failed"),
    ];
    for (k, text, title) in all {
        assert_eq!(job_text(k), text);
        assert_eq!(failure_title(k), title);
    }
}

#[test]
fn the_job_answer_is_swifts() {
    use JobKindRow as K;
    use JobStateRow as S;
    assert_eq!(answer(&row(K::Import, S::Running), None), JobAnswer::Wait);
    assert_eq!(answer(&row(K::Import, S::Done), None), JobAnswer::Ok(None));
    let partial = JobRow {
        problems: vec!["b: bad".into(), "c: bad".into()],
        created: vec![1],
        ..row(K::Import, S::Done)
    };
    assert_eq!(
        answer(&partial, None),
        JobAnswer::Ok(Some("warnings: b: bad; c: bad".into()))
    );
    let nothing = JobRow {
        created: vec![],
        ..partial.clone()
    };
    assert_eq!(
        answer(&nothing, None),
        JobAnswer::Failed("b: bad; c: bad".into())
    );
    // A format change with problems and nothing created is still ok.
    let format = JobRow {
        kind: K::Format,
        ..nothing.clone()
    };
    assert!(matches!(answer(&format, None), JobAnswer::Ok(Some(_))));
    // The interchange jobs answer their report's summary.
    let midi = JobRow {
        report: Some(ReportRow {
            summary: "Imported 2 tracks".into(),
            items: vec![],
        }),
        ..row(K::ImportMidi, S::Done)
    };
    assert_eq!(
        answer(&midi, None),
        JobAnswer::Ok(Some("Imported 2 tracks".into()))
    );
    // Failed and cancelled; an export names what it wrote.
    assert_eq!(
        answer(&row(K::Import, S::Cancelled), None),
        JobAnswer::Failed("the job was cancelled".into())
    );
    let export = JobRow {
        message: Some("disk full".into()),
        written: vec!["/a.mid".into(), "/b.mid".into()],
        ..row(K::ExportMidi, S::Failed)
    };
    assert_eq!(
        answer(&export, None),
        JobAnswer::Failed("disk full; already written: /a.mid, /b.mid".into())
    );
    // An import's `written` is not named.
    let import = JobRow {
        kind: K::Import,
        ..export.clone()
    };
    assert_eq!(answer(&import, None), JobAnswer::Failed("disk full".into()));
    // A bounce answers its files.
    let bounced = BounceResultRow {
        files: vec![BouncedFileRow {
            path: "/o/Bounce.wav".into(),
            frames: 480,
            rate: 48_000,
            bits: "24-bit".into(),
            channels: 2,
            clipped: 0,
        }],
        media: vec![4],
    };
    let bounce = JobRow {
        problems: vec!["clipped".into()],
        ..row(K::Bounce, S::Done)
    };
    let status =
        "wrote /o/Bounce.wav (480 frames, 48000 Hz, 24-bit); added 1 media; warnings: clipped";
    assert_eq!(
        answer(&bounce, Some(&bounced)),
        JobAnswer::Ok(Some(status.into()))
    );
    assert_eq!(answer(&row(K::Bounce, S::Done), None), JobAnswer::Ok(None));
    assert_eq!(
        bounce_status(&bounced, &[]).unwrap(),
        "wrote /o/Bounce.wav (480 frames, 48000 Hz, 24-bit); added 1 media"
    );
    let failed_bounce = JobRow {
        message: Some("no space".into()),
        ..row(K::Bounce, S::Failed)
    };
    assert_eq!(
        answer(&failed_bounce, Some(&bounced)),
        JobAnswer::Failed("no space; already written: /o/Bounce.wav".into())
    );
    assert_eq!(
        expired_message(&row(K::Import, S::Running)),
        "job 7 is still running after 10 minutes; it was not cancelled"
    );
}

#[test]
fn the_model_answers_a_bounce_with_its_files() {
    let f = fake();
    let bounced = BounceResultRow {
        files: vec![],
        media: vec![9, 10],
    };
    f.script_job(JobScript::done().steps(&[0.5]).bounce(bounced));
    let id = f
        .bounce(vizdaw_ffi::BounceRequestRow {
            seq: 3,
            source: vizdaw_ffi::BounceSourceRow::Mix,
            range: vizdaw_ffi::BounceRangeRow::Song,
            path: None,
            container: vizdaw_ffi::ContainerRow::Wav,
            rate: 48_000,
            depth: BitDepthRow::Int24,
            dither: false,
            add_to_project: true,
            replace: false,
        })
        .unwrap();
    let m = model(&f);
    f.job(id).unwrap();
    let ended = f.job(id).unwrap();
    assert_eq!(
        m.job_answer(&ended),
        JobAnswer::Ok(Some("added 2 media".into()))
    );
}

// Real sessions: each job kind the model starts, end to end.

/// A short 48 kHz mono 16-bit WAV at `path`.
fn write_wav(path: &std::path::Path) {
    let frames: u32 = 4_800;
    let data = frames * 2;
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&96_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data.to_le_bytes());
    for i in 0..frames {
        let v = ((i as f32 * 0.05).sin() * 8_000.0) as i16;
        b.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// Polls `m`'s jobs until none is listed.
fn settle(m: &mut DocumentModel) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !m.jobs().is_empty() {
        assert!(Instant::now() < deadline, "the job never ended");
        std::thread::sleep(Duration::from_millis(10));
        m.poll_jobs();
    }
}

fn real() -> DocumentModel {
    let s: Arc<dyn DocSession> = vizdaw_ffi::session::Session::new_empty().unwrap();
    DocumentModel::new(s).unwrap()
}

#[test]
fn a_real_import_runs_as_an_import_job() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("tone.wav");
    write_wav(&wav);
    let mut m = real();
    let v = m.media_version();
    let id = m
        .start_job(JobKindRow::Import, "Import Failed", |s| {
            s.import_audio(vec![wav.display().to_string()], None)
        })
        .unwrap();
    settle(&mut m);
    let r = m.session().job(id).unwrap();
    assert_eq!(r.kind, JobKindRow::Import);
    if cfg!(target_os = "linux") {
        // Linux gap, answered by host track H4 (Linux audio import reads
        // WAV/AIFF and other formats): today only VizDAW CAF files import,
        // so the WAV is refused. When H4 lands this branch fails and forces
        // the update.
        assert!(r.created.is_empty(), "{r:?}");
        assert!(
            r.problems
                .iter()
                .any(|p| p.contains("not a VizDAW CAF file")),
            "{r:?}"
        );
        return;
    }
    assert_eq!(r.state, JobStateRow::Done, "{:?}", r.message);
    assert!(!r.created.is_empty(), "{:?}", r.problems);
    assert!(m.alert.is_none(), "{:?}", m.alert);
    assert_eq!(m.media_version(), v + 1);
    assert_eq!(m.session().media().unwrap().len(), 1);
}

#[test]
fn a_real_import_of_a_missing_file_is_an_import_failed_alert() {
    let dir = tempfile::tempdir().unwrap();
    let mut m = real();
    m.import_audio_at(
        vec![dir.path().join("gone.wav").display().to_string()],
        None,
    );
    settle(&mut m);
    let alert = m.alert.clone().expect("an alert");
    // The title comes from the session's row: the kind made it through.
    assert_eq!(alert.title, "Import Failed");
    assert!(alert.message.contains("gone.wav"), "{alert:?}");
}

#[test]
fn a_real_convert_runs_as_a_convert_job() {
    let mut m = real();
    let id = m
        .start_job(JobKindRow::Convert, "Convert Failed", |s| s.convert_media())
        .unwrap();
    settle(&mut m);
    let r = m.session().job(id).unwrap();
    assert_eq!(r.kind, JobKindRow::Convert);
    assert_eq!(r.state, JobStateRow::Done, "{:?}", r.message);
    assert!(m.alert.is_none(), "{:?}", m.alert);
    assert_eq!(m.project_info().unwrap().mismatched, 0);
}
