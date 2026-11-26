//! `file.bounce` over the control channel (slice 7a Task 3; Swift
//! `BounceCommandsTests`): the request as built, hostile arguments as
//! `invalid_params` before any session call, and the Bounce dialog. The
//! job answers (files, warnings, already written) are in `jobs.rs`.

mod support;

use std::sync::Arc;

use core_control::error::ErrorCode;
use core_control::registry::ArgValue;
use support::{FakeSession, demo_fake};
use vizdaw_app::DocumentModel;
use vizdaw_app::bounce_commands::{job, request};
use vizdaw_app::jobs::FileJob;
use vizdaw_app::testing::JobRequest;
use vizdaw_ffi::selection::TickRange;
use vizdaw_ffi::{
    BitDepthRow, BounceRangeRow, BounceRequestRow, BounceSourceRow, ContainerRow, JobKindRow,
    ProjectFormatRow, ProjectInfoRow, VizError,
};

fn t(s: &str) -> ArgValue {
    ArgValue::Text(s.into())
}

fn args(pairs: &[(&str, ArgValue)]) -> Vec<(String, ArgValue)> {
    pairs
        .iter()
        .map(|(n, v)| ((*n).to_owned(), v.clone()))
        .collect()
}

fn demo() -> (Arc<FakeSession>, DocumentModel) {
    let f = Arc::new(demo_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    (f, m)
}

#[test]
fn bounce_commands_check_their_arguments() {
    let (f, m) = demo();
    let wav = ("path", t("/tmp/x.wav"));
    let folder = ("path", t("/tmp/stems"));
    let stems = ("source", t("stems"));
    let selection = ("range", t("selection"));
    let strips = |s: &str| ("strips", t(s));
    let cases: Vec<Vec<(&str, ArgValue)>> = vec![
        vec![("path", t("/tmp/x.aif"))],
        vec![wav.clone(), ("format", t("caf"))],
        vec![stems.clone(), folder.clone()],
        vec![stems.clone(), folder.clone(), strips("1,,2")],
        vec![stems.clone(), folder.clone(), strips("1, 2")],
        vec![stems.clone(), folder.clone(), strips("-1")],
        vec![stems.clone(), folder.clone(), strips("a,b")],
        vec![stems.clone(), folder.clone(), strips("1,1")],
        vec![stems.clone(), folder.clone(), strips("")],
        vec![stems.clone(), folder.clone(), strips("1,")],
        vec![stems.clone(), folder.clone(), strips(",1")],
        vec![stems.clone(), folder.clone(), strips("+1")],
        vec![
            stems.clone(),
            folder.clone(),
            strips("99999999999999999999999"),
        ],
        vec![stems.clone(), folder.clone(), strips("١")],
        vec![wav.clone(), selection.clone(), ("start", ArgValue::Int(0))],
        vec![
            wav.clone(),
            selection.clone(),
            ("start", ArgValue::Int(100)),
            ("end", ArgValue::Int(100)),
        ],
        vec![
            wav.clone(),
            ("source", t("selection")),
            ("range", t("song")),
        ],
        vec![
            wav.clone(),
            ("start", ArgValue::Int(0)),
            ("end", ArgValue::Int(100)),
        ],
        // No time selection.
        vec![wav.clone(), selection.clone()],
        // A wrong type is refused, never taken as absent.
        vec![wav.clone(), ("dither", t("yes"))],
        vec![("path", ArgValue::Flag(true))],
    ];
    for a in cases {
        let e = request(&args(&a), &m).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams, "{a:?}: {e:?}");
        assert!(e.message.starts_with("file.bounce: "), "{e:?}");
    }
    // `add_to_project: false` alone still needs the dialog.
    for a in [vec![], vec![("add_to_project", ArgValue::Flag(false))]] {
        let d = request(&args(&a), &m).unwrap_err();
        assert_eq!(d.code, ErrorCode::DialogRequired);
        assert_eq!(d.dialog.as_deref(), Some("Bounce"));
        assert_eq!(
            d.message,
            "this would open the \"Bounce\" dialog; pass `path` instead"
        );
    }
    assert!(f.jobs_started().is_empty(), "nothing reached the core");
}

#[test]
fn the_refusals_say_why_as_swift_does() {
    let (_, m) = demo();
    let wav = ("path", t("/tmp/x.wav"));
    let message = |a: Vec<(&str, ArgValue)>| request(&args(&a), &m).unwrap_err().message;
    assert_eq!(
        message(vec![("path", t("/tmp/x.wav")), ("format", t("aiff"))]),
        "file.bounce: `path` must end in .aif for this format"
    );
    assert_eq!(
        message(vec![("path", t("/tmp/x"))]),
        "file.bounce: `path` must end in .wav for this format",
        "a missing extension is refused, never added"
    );
    assert_eq!(
        message(vec![("source", t("stems")), ("path", t("/tmp/s"))]),
        "file.bounce: stems need `strips` (strip IDs, e.g. \"1,2\")"
    );
    assert_eq!(
        message(vec![
            ("source", t("stems")),
            ("path", t("/tmp/s")),
            ("strips", t("3,3"))
        ]),
        "file.bounce: strip 3 is listed twice"
    );
    assert_eq!(
        message(vec![
            ("source", t("stems")),
            ("path", t("/tmp/s")),
            ("strips", t("x"))
        ]),
        "file.bounce: `strips` is strip IDs separated by commas, e.g. \"1,2\""
    );
    assert_eq!(
        message(vec![wav.clone(), ("range", t("selection"))]),
        "file.bounce: no time selection"
    );
    assert_eq!(
        message(vec![
            wav.clone(),
            ("range", t("selection")),
            ("end", ArgValue::Int(5))
        ]),
        "file.bounce: give both `start` and `end`, or neither"
    );
    assert_eq!(
        message(vec![
            wav.clone(),
            ("range", t("selection")),
            ("start", ArgValue::Int(5)),
            ("end", ArgValue::Int(5))
        ]),
        "file.bounce: `start` must be before `end`"
    );
    assert_eq!(
        message(vec![wav.clone(), ("end", ArgValue::Int(5))]),
        "file.bounce: `start` and `end` go with `range: selection`"
    );
    assert_eq!(
        message(vec![
            wav.clone(),
            ("source", t("selection")),
            ("range", t("song"))
        ]),
        "file.bounce: `source: selection` bounces the time selection; leave out `range: song`"
    );
}

#[test]
fn aif_and_aiff_both_fit_the_aiff_format_in_any_case() {
    let (_, m) = demo();
    for p in ["/tmp/x.aif", "/tmp/x.aiff", "/tmp/x.AIF", "/tmp/x.Aiff"] {
        let r = request(&args(&[("path", t(p)), ("format", t("aiff"))]), &m).unwrap();
        assert_eq!(r.container, ContainerRow::Aiff);
        assert_eq!(r.path.as_deref(), Some(p), "the path is sent as given");
    }
    let c = request(
        &args(&[("path", t("/tmp/x.CAF")), ("format", t("caf"))]),
        &m,
    )
    .unwrap();
    assert_eq!(c.container, ContainerRow::Caf);
    // A path that climbs out is the core's to refuse: sent as given.
    let up = request(&args(&[("path", t("/tmp/a/../../x.wav"))]), &m).unwrap();
    assert_eq!(up.path.as_deref(), Some("/tmp/a/../../x.wav"));
}

#[test]
fn bounce_builds_the_request() {
    let (f, mut m) = demo();
    f.set_project_info(ProjectInfoRow {
        format: ProjectFormatRow {
            sample_rate: 44_100,
            depth: BitDepthRow::Int16,
        },
        mismatched: 0,
        rate_mismatch: false,
        device_rate: 0.0,
    });
    let seq = m.selected_seq().unwrap();
    let r = request(&args(&[("path", t("/tmp/x.wav"))]), &m).unwrap();
    assert_eq!(
        r,
        BounceRequestRow {
            seq,
            source: BounceSourceRow::Mix,
            range: BounceRangeRow::Song,
            container: ContainerRow::Wav,
            depth: BitDepthRow::Int16,
            rate: 44_100,
            dither: true,
            path: Some("/tmp/x.wav".into()),
            add_to_project: false,
            replace: false,
        },
        "the project's rate and depth; dither on at 16 bits"
    );
    let all = request(
        &args(&[
            ("source", t("stems")),
            ("strips", t("2,1")),
            ("path", t("/tmp/stems")),
            ("format", t("aiff")),
            ("bits", t("32f")),
            ("rate", ArgValue::Int(96_000)),
            ("dither", ArgValue::Flag(true)),
            ("range", t("selection")),
            ("start", ArgValue::Int(0)),
            ("end", ArgValue::Int(15_360)),
            ("add_to_project", ArgValue::Flag(true)),
            ("replace", ArgValue::Flag(true)),
        ]),
        &m,
    )
    .unwrap();
    assert_eq!(all.source, BounceSourceRow::Stems { strips: vec![2, 1] });
    assert_eq!(
        all.range,
        BounceRangeRow::Ticks {
            start: 0,
            end: 15_360
        }
    );
    assert_eq!(all.container, ContainerRow::Aiff);
    assert_eq!(all.depth, BitDepthRow::Float32);
    assert_eq!(all.rate, 96_000);
    assert!(!all.dither, "float ignores dither");
    assert!(all.add_to_project && all.replace);
    let sel = request(
        &args(&[
            ("source", t("selection")),
            ("path", t("/tmp/x.wav")),
            ("start", ArgValue::Int(10)),
            ("end", ArgValue::Int(20)),
        ]),
        &m,
    )
    .unwrap();
    assert_eq!(sel.range, BounceRangeRow::Ticks { start: 10, end: 20 });
    assert_eq!(sel.source, BounceSourceRow::Mix);
    // The time selection fills a selection range without ticks.
    m.set_time_range(TickRange {
        start: 960,
        end: 3_840,
    });
    let held = request(
        &args(&[("path", t("/tmp/x.wav")), ("range", t("selection"))]),
        &m,
    )
    .unwrap();
    assert_eq!(
        held.range,
        BounceRangeRow::Ticks {
            start: 960,
            end: 3_840
        }
    );
    // Add to project with no file.
    let add = request(&args(&[("add_to_project", ArgValue::Flag(true))]), &m).unwrap();
    assert!(add.add_to_project && add.path.is_none());
    // 24 bits: dither off unless asked.
    let d = request(&args(&[("path", t("/tmp/x.wav")), ("bits", t("24"))]), &m).unwrap();
    assert!(!d.dither);
    let d = request(
        &args(&[
            ("path", t("/tmp/x.wav")),
            ("bits", t("24")),
            ("dither", ArgValue::Flag(true)),
        ]),
        &m,
    )
    .unwrap();
    assert!(d.dither);
}

#[test]
fn bounce_needs_a_shown_sequence() {
    let f = Arc::new(FakeSession::new(vec![]));
    let m = DocumentModel::new(f).unwrap();
    assert!(m.selected_seq().is_none());
    let e = request(&args(&[("path", t("/tmp/x.wav"))]), &m).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(e.message, "file.bounce: no sequence is shown");
}

#[test]
fn the_job_starts_a_bounce_and_a_refusal_is_the_core_s() {
    let (f, m) = demo();
    let j = job(&args(&[("path", t("/tmp/x.wav"))]), &m).unwrap();
    assert_eq!(j.kind(), JobKindRow::Bounce);
    assert_eq!(j.refusal_title(), "Bounce Failed");
    let id = j.start(m.session().as_ref()).unwrap();
    let started = f.jobs_started();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].id, id);
    assert_eq!(started[0].kind, JobKindRow::Bounce);
    let FileJob::Bounce(req) = &j else {
        panic!("not a bounce")
    };
    assert_eq!(started[0].request, JobRequest::Bounce(req.clone()));
    // The core's refusals: failed with the title, busy as the core says.
    f.fail_next(
        "bounce",
        VizError::Model {
            message: "x.wav exists; choose another name or replace it".into(),
        },
    );
    let e = j.start(m.session().as_ref()).unwrap_err();
    let c = vizdaw_app::mixer::commands::thrown(&e, j.refusal_title());
    assert_eq!(c.code, ErrorCode::Failed);
    assert_eq!(
        c.message,
        "Bounce Failed: x.wav exists; choose another name or replace it"
    );
    f.fail_next(
        "bounce",
        VizError::Busy {
            message: "A bounce is running.".into(),
        },
    );
    let e = j.start(m.session().as_ref()).unwrap_err();
    let c = vizdaw_app::mixer::commands::thrown(&e, j.refusal_title());
    assert_eq!(c.code, ErrorCode::Busy);
    assert_eq!(c.message, "A bounce is running.");
}
