//! The project formats as the channel spells them and the format, import
//! and relink requests (slice 7a Task 3; Swift `ControlFormats`,
//! `ControlCommandsTests` file.new / project_settings / media.import /
//! media.relink rows), and the extension a typed save name gets.

mod support;

use std::sync::Arc;

use core_control::error::ErrorCode;
use core_control::registry::ArgValue;
use support::demo_fake;
use vizdaw_app::formats::{
    FALLBACK_FORMAT, new_project_format, parse, parse_stored, project_settings_format,
    with_extension,
};
use vizdaw_app::jobs::{FileJob, media_import_request, relink_request};
use vizdaw_app::testing::JobRequest;
use vizdaw_app::{DocumentModel, Prefs};
use vizdaw_ffi::{
    BitDepthRow, ImportTargetRow, JobKindRow, MediaOriginRow, MediaRow, ProjectFormatRow,
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

fn fmt(rate: u32, depth: BitDepthRow) -> ProjectFormatRow {
    ProjectFormatRow {
        sample_rate: rate,
        depth,
    }
}

#[test]
fn formats_parse_the_channel_s_spelling() {
    assert_eq!(
        parse(Some("44100"), Some("16"), FALLBACK_FORMAT),
        Some(fmt(44_100, BitDepthRow::Int16))
    );
    assert_eq!(
        parse(None, Some("32f"), FALLBACK_FORMAT).map(|f| f.depth),
        Some(BitDepthRow::Float32)
    );
    assert_eq!(parse(Some("nope"), None, FALLBACK_FORMAT), None);
    assert_eq!(
        parse(Some("12345"), None, FALLBACK_FORMAT),
        None,
        "not a project rate"
    );
    assert_eq!(parse(None, Some("20"), FALLBACK_FORMAT), None);
    let base = fmt(96_000, BitDepthRow::Int16);
    assert_eq!(
        parse(None, None, base),
        Some(base),
        "the base fills the rest"
    );
    assert_eq!(
        parse(Some("192000"), None, base),
        Some(fmt(192_000, BitDepthRow::Int16))
    );
}

#[test]
fn file_new_takes_rate_and_depth() {
    let default = fmt(48_000, BitDepthRow::Int16);
    assert_eq!(
        new_project_format(&args(&[("rate", t("96000"))]), default).unwrap(),
        fmt(96_000, BitDepthRow::Int16)
    );
    assert_eq!(new_project_format(&[], default).unwrap(), default);
    let e = new_project_format(&args(&[("depth", t("8"))]), default).unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
    assert_eq!(e.message, "`rate` or `depth` is not a project format");
}

#[test]
fn project_settings_needs_a_rate_or_a_depth() {
    let base = fmt(44_100, BitDepthRow::Int24);
    let d = project_settings_format(&[], base).unwrap_err();
    assert_eq!(d.code, ErrorCode::DialogRequired);
    assert_eq!(d.dialog.as_deref(), Some("Project Settings"));
    assert_eq!(
        d.message,
        "this would open the \"Project Settings\" dialog; pass `rate` instead"
    );
    assert_eq!(
        project_settings_format(&args(&[("depth", t("32f"))]), base).unwrap(),
        fmt(44_100, BitDepthRow::Float32),
        "the project's rate stays"
    );
    let bad = project_settings_format(&args(&[("rate", t("1"))]), base).unwrap_err();
    assert_eq!(bad.code, ErrorCode::InvalidParams);
    assert_eq!(bad.message, "`rate` or `depth` is not a project format");
    let j = FileJob::Format(fmt(96_000, BitDepthRow::Int24));
    assert_eq!(j.kind(), JobKindRow::Format);
    assert_eq!(j.refusal_title(), "Project Settings Failed");
}

#[test]
fn the_stored_default_format_keeps_what_it_can() {
    assert_eq!(parse_stored("96000/32f"), fmt(96_000, BitDepthRow::Float32));
    // A rate alone keeps 24-bit (Swift: the parts given change 48/24).
    assert_eq!(parse_stored("96000"), fmt(96_000, BitDepthRow::Int24));
    for bad in ["", "garbage", "12345/24", "48000/20", "/"] {
        assert_eq!(parse_stored(bad), FALLBACK_FORMAT, "{bad:?}");
    }
    let mut p = Prefs::in_memory();
    p.set_raw("defaultProjectFormat", "88200".into()).unwrap();
    assert_eq!(p.default_format(), fmt(88_200, BitDepthRow::Int24));
}

#[test]
fn a_typed_save_name_gets_its_extension() {
    assert_eq!(
        with_extension("/tmp/Song", "mid", &["mid"]),
        ("/tmp/Song.mid".into(), true)
    );
    assert_eq!(
        with_extension("/tmp/Song.mid", "mid", &["mid"]),
        ("/tmp/Song.mid".into(), false)
    );
    assert_eq!(
        with_extension("/tmp/Song.MID", "mid", &["mid"]),
        ("/tmp/Song.MID".into(), false)
    );
    assert_eq!(
        with_extension("/tmp/Bounce.aiff", "aif", &["aif", "aiff"]),
        ("/tmp/Bounce.aiff".into(), false)
    );
    assert_eq!(
        with_extension("/tmp/Bounce.wav", "aif", &["aif", "aiff"]),
        ("/tmp/Bounce.wav.aif".into(), true)
    );
    assert_eq!(
        with_extension("/tmp/Song.v2", "dawproject", &["dawproject"]),
        ("/tmp/Song.v2.dawproject".into(), true)
    );
    assert_eq!(
        with_extension("/tmp/MIDI Report", "txt", &["txt"]),
        ("/tmp/MIDI Report.txt".into(), true)
    );
}

#[test]
fn media_import_takes_one_path_and_an_optional_target() {
    let d = media_import_request(&[]).unwrap_err();
    assert_eq!(d.code, ErrorCode::DialogRequired);
    assert_eq!(d.dialog.as_deref(), Some("Open"));
    let j = media_import_request(&args(&[
        ("path", t("/tmp/x.wav")),
        ("seq", ArgValue::Int(4)),
        ("tick", ArgValue::Int(960)),
    ]))
    .unwrap();
    assert_eq!(
        j,
        FileJob::Import {
            paths: vec!["/tmp/x.wav".into()],
            target: Some(ImportTargetRow {
                seq: 4,
                lane: None,
                tick: 960
            }),
        }
    );
    assert_eq!(j.kind(), JobKindRow::Import);
    assert_eq!(j.refusal_title(), "Import Failed");
    // Without `seq`: into the pool only (a lane alone means nothing).
    assert_eq!(
        media_import_request(&args(&[
            ("path", t("/tmp/x.wav")),
            ("lane", ArgValue::Int(3))
        ]))
        .unwrap(),
        FileJob::Import {
            paths: vec!["/tmp/x.wav".into()],
            target: None
        }
    );
    let e = media_import_request(&args(&[
        ("path", t("/tmp/x.wav")),
        ("seq", ArgValue::Int(-1)),
    ]))
    .unwrap_err();
    assert_eq!(e.code, ErrorCode::InvalidParams);
}

#[test]
fn relink_needs_a_path_and_a_known_media_item() {
    let f = Arc::new(demo_fake());
    f.set_media(vec![MediaRow {
        id: 7,
        ..media_row()
    }]);
    let m = DocumentModel::new(f.clone()).unwrap();
    let d = relink_request(&args(&[("media", ArgValue::Int(7))]), &m).unwrap_err();
    assert_eq!(d.dialog.as_deref(), Some("Relink"));
    for bad in [
        vec![],
        vec![("media", ArgValue::Int(8))],
        vec![("media", ArgValue::Int(-7))],
    ] {
        let mut a = vec![("path", t("/tmp/x.wav"))];
        a.extend(bad);
        let e = relink_request(&args(&a), &m).unwrap_err();
        assert_eq!(e.code, ErrorCode::InvalidParams);
        assert_eq!(
            e.message,
            "media.relink: `media` must name a media item (see state.get media)"
        );
    }
    let j = relink_request(
        &args(&[("path", t("/tmp/x.wav")), ("media", ArgValue::Int(7))]),
        &m,
    )
    .unwrap();
    assert_eq!(j.kind(), JobKindRow::Relink);
    assert_eq!(j.refusal_title(), "Relink Failed");
    let id = j.start(m.session().as_ref()).unwrap();
    let s = f.jobs_started().pop().unwrap();
    assert_eq!((s.id, s.kind), (id, JobKindRow::Relink));
    assert_eq!(
        s.request,
        JobRequest::Relink {
            media: 7,
            path: "/tmp/x.wav".into()
        }
    );
}

#[test]
fn the_media_jobs_start_their_session_calls() {
    let f = Arc::new(demo_fake());
    let m = DocumentModel::new(f.clone()).unwrap();
    let format = fmt(96_000, BitDepthRow::Int16);
    for (j, request) in [
        (
            FileJob::Import {
                paths: vec!["/tmp/a.wav".into()],
                target: None,
            },
            JobRequest::Import {
                paths: vec!["/tmp/a.wav".into()],
                target: None,
            },
        ),
        (FileJob::Convert, JobRequest::Convert),
        (FileJob::Format(format), JobRequest::Format(format)),
    ] {
        j.start(m.session().as_ref()).unwrap();
        let s = f.jobs_started().pop().unwrap();
        assert_eq!(s.kind, j.kind());
        assert_eq!(s.request, request);
    }
    assert_eq!(FileJob::Convert.refusal_title(), "Convert Failed");
}

fn media_row() -> MediaRow {
    MediaRow {
        id: 0,
        name: "x.wav".into(),
        path: "Audio/x.wav".into(),
        channels: 2,
        frames: 48_000,
        sample_rate: 48_000,
        origin: MediaOriginRow::Imported,
        original_name: None,
        offline: None,
        uses: 0,
        used_percent: 0.0,
        peak: None,
    }
}
