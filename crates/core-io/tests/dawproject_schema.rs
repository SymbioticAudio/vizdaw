//! The DAWproject typed model (plan 6d Task 3): reading, writing, ID checks
//! and the vendored schema files.

mod common;

use common::xmllint::schema_check;
use core_io::IoError;
use core_io::dawproject::schema::*;
use core_io::limits::{MAX_TEXT_BYTES, PackageLimits};
use core_io::report::{Item, ItemKind, Report};
use core_io::xml::Tree;
use std::process::Command;
use std::sync::atomic::AtomicBool;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/dawproject");

fn path(name: &str) -> String {
    format!("{DIR}/{name}")
}

fn text(name: &str) -> String {
    std::fs::read_to_string(path(name)).expect("fixture")
}

fn parse(xml: &str) -> Tree {
    Tree::parse(
        xml.as_bytes(),
        &PackageLimits::default(),
        &AtomicBool::new(false),
    )
    .expect("test XML parses")
}

fn read(xml: &str) -> (Result<DpProject, IoError>, Report) {
    let mut report = Report::default();
    let p = read_project(&parse(xml), &mut report);
    (p, report)
}

fn read_ok(xml: &str) -> (DpProject, Report) {
    let (p, r) = read(xml);
    (p.expect("reads"), r)
}

/// A project holding `body` after its application.
fn project(body: &str) -> String {
    format!("<Project version=\"1.0\"><Application name=\"t\" version=\"1\"/>{body}</Project>")
}

fn item<'a>(r: &'a Report, kind: ItemKind, what: &str) -> Option<&'a Item> {
    r.items.iter().find(|i| i.kind == kind && i.what == what)
}

fn lanes_of(p: &DpProject) -> &DpLanes {
    p.arrangement
        .as_ref()
        .and_then(|a| a.lanes.as_ref())
        .expect("lanes")
}

#[test]
fn xsds_are_the_pinned_ones() {
    for (file, want) in [
        (
            "Project.xsd",
            "58e2fd9864772850aac3eab1f3de8693857dc5384df6d29fbc320ae1de2347cc",
        ),
        (
            "MetaData.xsd",
            "fb3ba378271770dddbcced8990aba537de3d36ff2d58573523460a699221c99f",
        ),
    ] {
        let tools: [(&str, &[&str]); 2] = [("shasum", &["-a", "256"]), ("sha256sum", &[])];
        let out = tools
            .iter()
            .find_map(|(tool, args)| Command::new(tool).args(*args).arg(path(file)).output().ok());
        let Some(out) = out else {
            if cfg!(target_os = "macos") {
                panic!("macOS always has shasum");
            }
            println!("skipped: no SHA-256 tool");
            return;
        };
        let got = String::from_utf8_lossy(&out.stdout);
        assert_eq!(got.split_whitespace().next(), Some(want), "{file}");
    }
}

#[test]
fn reads_the_hand_minimal_project() {
    let (p, report) = read_ok(&text("hand_minimal.project.xml"));
    assert!(report.is_empty(), "{report:?}");
    assert_eq!(p.version, "1.0");
    assert_eq!(p.application, Some(("Fixture".to_owned(), "1".to_owned())));
    let tempo = p.tempo.as_ref().expect("tempo");
    assert_eq!(tempo.named.id.as_deref(), Some("tempo"));
    assert_eq!((tempo.unit, tempo.value), (DpUnit::Bpm, Some(120.0)));
    let sig = p.time_signature.as_ref().expect("time signature");
    assert_eq!((sig.numerator, sig.denominator), (4, 4));
    assert_eq!(p.structure.len(), 2);

    let DpStructureItem::Track(keys) = &p.structure[0] else {
        panic!("a track first");
    };
    assert_eq!(keys.named.name.as_deref(), Some("Keys"));
    assert_eq!(keys.named.color.as_deref(), Some("#3366cc"));
    assert_eq!(keys.content, [DpContentType::Notes]);
    let ch = keys.channel.as_ref().expect("channel");
    assert_eq!(ch.role, Some(DpMixerRole::Regular));
    assert_eq!(ch.audio_channels, Some(2));
    assert_eq!(ch.destination.as_deref(), Some("cmaster"));
    assert_eq!(ch.mute.as_ref().and_then(|m| m.value), Some(false));
    let pan = ch.pan.as_ref().expect("pan");
    assert_eq!((pan.unit, pan.value), (DpUnit::Normalized, Some(0.5)));
    let vol = ch.volume.as_ref().expect("volume");
    assert_eq!(
        (vol.value, vol.min, vol.max),
        (Some(0.8), Some(0.0), Some(2.0))
    );
    assert_eq!(ch.sends.len(), 1);
    assert_eq!(ch.sends[0].destination.as_deref(), Some("cmaster"));
    assert_eq!(ch.sends[0].kind, Some(DpSendType::Post));
    assert_eq!(ch.sends[0].volume.value, Some(0.25));
    let dev = &ch.devices[0];
    assert_eq!(dev.kind, DpDeviceKind::Clap);
    assert_eq!(dev.device_name, "Pad");
    assert_eq!(dev.role, DpDeviceRole::Instrument);
    assert_eq!(dev.vendor.as_deref(), Some("Example"));
    assert_eq!(dev.version.as_deref(), Some("2.1"));
    assert_eq!(dev.loaded, Some(true));
    assert_eq!(
        dev.state,
        Some(DpFileRef {
            path: "plugins/001.clap-preset".to_owned(),
            external: false
        })
    );

    let DpStructureItem::Track(master) = &p.structure[1] else {
        panic!("a track second");
    };
    assert_eq!(master.content, [DpContentType::Audio, DpContentType::Notes]);
    assert_eq!(
        master.channel.as_ref().and_then(|c| c.role),
        Some(DpMixerRole::Master)
    );

    let lanes = lanes_of(&p);
    assert_eq!(lanes.head.time_unit, Some(DpTimeUnit::Beats));
    let DpTimeline::Clips(clips) = &lanes.items[0] else {
        panic!("clips");
    };
    assert_eq!(clips.head.track.as_deref(), Some("t1"));
    let clip = &clips.clips[0];
    assert_eq!(clip.named.name.as_deref(), Some("Verse pad"));
    assert_eq!((clip.time, clip.duration), (0.0, Some(8.0)));
    assert_eq!((clip.play_start, clip.play_stop), (Some(0.0), Some(8.0)));
    let Some(content) = clip.content.as_deref() else {
        panic!("clip content");
    };
    let DpTimeline::Notes(notes) = content else {
        panic!("notes");
    };
    assert_eq!(notes.notes.len(), 2);
    assert_eq!((notes.notes[0].key, notes.notes[0].vel), (60, Some(0.8)));
    assert_eq!(notes.notes[0].rel, Some(0.5));
    let n1 = &notes.notes[1];
    assert_eq!((n1.time, n1.duration, n1.channel), (1.0, 1.5, 0));
    let Some(DpTimeline::Points(pts)) = n1.content.as_deref() else {
        panic!("note points");
    };
    assert_eq!(pts.unit, Some(DpUnit::Semitones));
    assert_eq!(pts.target.expression, Some(DpExpression::Transpose));
    assert_eq!((pts.target.channel, pts.target.key), (Some(0), Some(64)));
    assert_eq!(pts.target.parameter, None);
    assert_eq!(
        pts.points,
        [
            DpPoint {
                time: 0.0,
                value: DpPointValue::Real(0.0, Some(DpInterpolation::Linear))
            },
            DpPoint {
                time: 0.5,
                value: DpPointValue::Real(1.5, None)
            },
        ]
    );
    let arr = p.arrangement.as_ref().expect("arrangement");
    assert_eq!(arr.markers.len(), 1);
    assert_eq!(arr.markers[0].named.name.as_deref(), Some("Verse"));

    let mut r = Report::default();
    let ids = check_ids(&p, &mut r).expect("ids");
    assert!(r.is_empty(), "{r:?}");
    assert_eq!(ids.kind("t1"), Some(IdKind::Track));
    assert_eq!(ids.kind("cmaster"), Some(IdKind::Channel));
    assert_eq!(ids.kind("c1v"), Some(IdKind::Parameter));
    assert_eq!(ids.kind("keysclips"), Some(IdKind::Timeline));
    assert_eq!(ids.kind("c1d1"), Some(IdKind::Device));
    assert_eq!(ids.kind("c1s0"), Some(IdKind::Send));
    assert_eq!(ids.kind("arr"), Some(IdKind::Other));
    assert_eq!(ids.kind("nope"), None);
}

#[test]
fn reads_the_hand_minimal_metadata() {
    let mut report = Report::default();
    let m = read_metadata(&parse(&text("hand_minimal.metadata.xml")), &mut report);
    assert!(report.is_empty());
    assert_eq!(m.fields[0].as_deref(), Some("Hand minimal"));
    assert_eq!(m.fields[1].as_deref(), Some("Nobody & Co"));
    assert_eq!(m.fields[12].as_deref(), Some("Written by hand."));
    assert_eq!(m.fields.iter().flatten().count(), 3);
    assert_eq!(m.dropped_text, 0);
    assert!(m.cut.iter().all(|c| !c));
}

#[test]
fn writes_the_hand_minimal_project_byte_for_byte() {
    let want = text("hand_minimal.project.xml");
    let (p, _) = read_ok(&want);
    assert_eq!(write_project(&p), want);
    let want = text("hand_minimal.metadata.xml");
    let mut report = Report::default();
    let m = read_metadata(&parse(&want), &mut report);
    assert_eq!(write_metadata(&m), want);
}

fn named(id: &str) -> DpNamed {
    DpNamed {
        id: Some(id.to_owned()),
        name: Some(format!("name {id}")),
        color: Some("#102030".to_owned()),
        comment: Some(format!("comment {id}")),
    }
}

fn head(id: &str) -> DpHead {
    DpHead {
        named: named(id),
        time_unit: Some(DpTimeUnit::Beats),
        track: None,
    }
}

fn real(id: &str, unit: DpUnit, v: f64) -> DpRealParam {
    DpRealParam {
        named: named(id),
        parameter_id: Some(7),
        unit,
        value: Some(v),
        min: Some(0.0),
        max: Some(f64::INFINITY),
    }
}

fn boolean(id: &str, v: bool) -> DpBoolParam {
    DpBoolParam {
        named: named(id),
        parameter_id: Some(1),
        value: Some(v),
    }
}

fn device(kind: DpDeviceKind, id: &str) -> DpDevice {
    DpDevice {
        kind,
        named: named(id),
        device_id: Some("dev".to_owned()),
        device_name: "Device".to_owned(),
        role: DpDeviceRole::AudioFx,
        vendor: Some("Vendor".to_owned()),
        version: Some("1".to_owned()),
        loaded: Some(true),
        parameters: vec![real(&format!("{id}q"), DpUnit::Hertz, 440.0)],
        enabled: Some(boolean(&format!("{id}e"), true)),
        state: Some(DpFileRef {
            path: "a/b.bin".to_owned(),
            external: true,
        }),
        settings: vec![],
        bands: vec![],
        other_ids: vec![],
    }
}

fn channel(id: &str) -> DpChannel {
    DpChannel {
        named: named(id),
        role: Some(DpMixerRole::Effect),
        audio_channels: Some(2),
        destination: Some("cmaster".to_owned()),
        solo: Some(false),
        devices: [
            DpDeviceKind::Au,
            DpDeviceKind::Clap,
            DpDeviceKind::Vst2,
            DpDeviceKind::Vst3,
            DpDeviceKind::Builtin,
            DpDeviceKind::Equalizer,
            DpDeviceKind::Compressor,
            DpDeviceKind::NoiseGate,
            DpDeviceKind::Limiter,
            DpDeviceKind::Other,
        ]
        .iter()
        .enumerate()
        .map(|(i, k)| device(*k, &format!("{id}d{i}")))
        .collect(),
        mute: Some(Box::new(boolean(&format!("{id}m"), false))),
        pan: Some(Box::new(real(&format!("{id}p"), DpUnit::Normalized, 0.5))),
        sends: vec![DpSend {
            named: named(&format!("{id}s")),
            destination: Some("cmaster".to_owned()),
            kind: Some(DpSendType::Pre),
            enable: Some(boolean(&format!("{id}se"), true)),
            pan: Some(real(&format!("{id}sp"), DpUnit::Normalized, 0.5)),
            volume: real(&format!("{id}sv"), DpUnit::Decibel, -6.0),
        }],
        volume: Some(Box::new(real(&format!("{id}v"), DpUnit::Linear, 1.0))),
    }
}

fn points(id: &str, values: Vec<DpPointValue>) -> DpPoints {
    DpPoints {
        head: head(id),
        unit: Some(DpUnit::Normalized),
        target: DpTarget {
            parameter: Some("cmasterv".to_owned()),
            expression: Some(DpExpression::ChannelController),
            channel: Some(1),
            key: Some(2),
            controller: Some(3),
        },
        points: values
            .into_iter()
            .enumerate()
            .map(|(i, value)| DpPoint {
                time: i as f64 * 0.25,
                value,
            })
            .collect(),
    }
}

fn marker(id: &str, time: f64) -> DpMarker {
    DpMarker {
        named: named(id),
        time,
    }
}

fn clip(id: &str, content: Option<DpTimeline>) -> DpClip {
    DpClip {
        named: named(id),
        time: 1.5,
        duration: Some(4.0),
        content_time_unit: Some(DpTimeUnit::Seconds),
        play_start: Some(0.25),
        play_stop: Some(2.0),
        loop_start: Some(0.0),
        loop_end: Some(1.0),
        fade_time_unit: Some(DpTimeUnit::Beats),
        fade_in: Some(0.1),
        fade_out: Some(-0.2),
        enable: Some(false),
        reference: None,
        content: content.map(Box::new),
    }
}

fn note(id: &str, content: Option<DpTimeline>) -> DpNote {
    let _ = id;
    DpNote {
        time: 0.5,
        duration: 1.0,
        channel: 3,
        key: 60,
        vel: Some(0.75),
        rel: Some(0.5),
        content: content.map(Box::new),
    }
}

fn audio(id: &str) -> DpAudio {
    DpAudio {
        head: head(id),
        duration: 12.5,
        channels: 2,
        sample_rate: 48_000,
        algorithm: Some("stretch".to_owned()),
        file: DpFileRef {
            path: "audio/1.wav".to_owned(),
            external: false,
        },
    }
}

/// A project with every element kind the writer makes and an `id` on every
/// `named` (the ones the writer must not write included).
fn everything() -> DpProject {
    let all_points = points(
        "pts",
        vec![
            DpPointValue::Real(0.5, Some(DpInterpolation::Hold)),
            DpPointValue::Bool(true),
            DpPointValue::Integer(-4),
            DpPointValue::Enum(2),
            DpPointValue::TimeSig(3, 4),
        ],
    );
    let notes = DpNotes {
        head: head("notes"),
        notes: vec![
            note("n1", None),
            note(
                "n2",
                Some(DpTimeline::Points(points(
                    "npts",
                    vec![DpPointValue::Real(1.0, None)],
                ))),
            ),
        ],
    };
    let lanes = DpLanes {
        head: DpHead {
            track: Some("tr1".to_owned()),
            ..head("lanes")
        },
        items: vec![
            DpTimeline::Clips(DpClips {
                head: head("clips"),
                clips: vec![
                    clip("clip1", Some(DpTimeline::Notes(notes))),
                    DpClip {
                        reference: Some("audio".to_owned()),
                        ..clip("clip2", None)
                    },
                    clip(
                        "clip3",
                        Some(DpTimeline::Warps(DpWarps {
                            head: head("warps"),
                            content_time_unit: DpTimeUnit::Seconds,
                            content: Box::new(DpTimeline::Audio(audio("audio"))),
                            warps: vec![(0.0, 0.0), (4.0, 2.0)],
                        })),
                    ),
                ],
            }),
            DpTimeline::Lanes(DpLanes {
                head: head("inner"),
                items: vec![DpTimeline::Points(all_points)],
            }),
            DpTimeline::Markers(head("tmarkers"), vec![marker("m9", 2.0)]),
            DpTimeline::ClipSlot(head("slot"), Some(Box::new(clip("slotclip", None)))),
            DpTimeline::ClipSlot(head("slot2"), None),
        ],
    };
    DpProject {
        version: "1.0".to_owned(),
        application: Some(("App".to_owned(), "9".to_owned())),
        tempo: Some(real("tempo", DpUnit::Bpm, 123.5)),
        time_signature: Some(DpTimeSigParam {
            named: named("timesig"),
            numerator: 7,
            denominator: 8,
        }),
        structure: vec![
            DpStructureItem::Track(DpTrack {
                named: named("tr1"),
                content: vec![
                    DpContentType::Audio,
                    DpContentType::Notes,
                    DpContentType::Tracks,
                ],
                loaded: Some(true),
                channel: Some(Box::new(channel("c1"))),
                children: vec![DpTrack {
                    named: named("tr2"),
                    content: vec![DpContentType::Automation, DpContentType::Markers],
                    loaded: None,
                    channel: None,
                    children: vec![],
                }],
            }),
            DpStructureItem::Track(DpTrack {
                named: named("trm"),
                content: vec![DpContentType::Video],
                loaded: None,
                channel: Some(Box::new(DpChannel {
                    role: Some(DpMixerRole::Master),
                    destination: None,
                    sends: vec![],
                    devices: vec![],
                    ..channel("cmaster")
                })),
                children: vec![],
            }),
            DpStructureItem::Channel(DpChannel {
                role: Some(DpMixerRole::Vca),
                destination: Some("cmaster".to_owned()),
                ..channel("bare")
            }),
        ],
        arrangement: Some(DpArrangement {
            named: named("arr"),
            lanes: Some(lanes),
            markers_head: DpHead {
                time_unit: Some(DpTimeUnit::Seconds),
                ..head("amarkers")
            },
            markers: vec![marker("m1", 0.0), marker("m2", 8.0)],
            tempo_automation: Some(points("tempoauto", vec![DpPointValue::Real(120.0, None)])),
            time_signature_automation: Some(points("sigauto", vec![DpPointValue::TimeSig(6, 8)])),
        }),
        scenes: vec![DpScene {
            named: named("scene"),
            content: Some(DpTimeline::ClipSlot(head("sceneslot"), None)),
        }],
        dropped_text: 0,
        model_bytes: 0,
    }
}

#[test]
fn written_projects_pass_the_vendored_schema() {
    let xsd = path("Project.xsd");
    schema_check(&xsd, &text("hand_minimal.project.xml"));
    schema_check(&path("MetaData.xsd"), &text("hand_minimal.metadata.xml"));

    let p = everything();
    let xml = write_project(&p);
    schema_check(&xsd, &xml);
    // Nothing referenceable is lost, and nothing else gets an id.
    for id in ["clip1", "m1", "m9", "scene", "slotclip"] {
        assert!(!xml.contains(&format!("id=\"{id}\"")), "{id} written");
    }
    for id in [
        "tr1",
        "c1",
        "c1d3",
        "c1sv",
        "pts",
        "tempoauto",
        "arr",
        "lanes",
    ] {
        assert!(xml.contains(&format!("id=\"{id}\"")), "{id} missing");
    }
    // What was written reads back the same, apart from the ids not written.
    let (back, report) = read_ok(&xml);
    assert!(report.is_empty(), "{report:?}");
    assert_eq!(write_project(&back), xml);

    // What the writer cannot write validly is left out and reported, never
    // written invalid (each of these is valid, once written).
    let hostile = DpProject {
        version: "1.0".to_owned(),
        application: None,
        arrangement: Some(DpArrangement {
            named: DpNamed::default(),
            lanes: Some(DpLanes {
                head: DpHead::default(),
                items: vec![
                    DpTimeline::Warps(DpWarps {
                        head: head("w1"),
                        content_time_unit: DpTimeUnit::Beats,
                        content: Box::new(DpTimeline::Audio(audio("a1"))),
                        warps: vec![],
                    }),
                    DpTimeline::Warps(DpWarps {
                        head: head("w2"),
                        content_time_unit: DpTimeUnit::Beats,
                        content: Box::new(DpTimeline::Video(head("v1"))),
                        warps: vec![(0.0, 0.0)],
                    }),
                    DpTimeline::Warps(DpWarps {
                        head: head("w3"),
                        content_time_unit: DpTimeUnit::Beats,
                        content: Box::new(DpTimeline::Markers(head("m3"), vec![])),
                        warps: vec![(0.0, 0.0)],
                    }),
                    DpTimeline::Warps(DpWarps {
                        head: head("w4"),
                        content_time_unit: DpTimeUnit::Beats,
                        content: Box::new(DpTimeline::Unknown("Hologram".to_owned())),
                        warps: vec![(0.0, 0.0)],
                    }),
                    DpTimeline::Video(head("v2")),
                    DpTimeline::Clips(DpClips {
                        head: DpHead::default(),
                        clips: vec![clip("c", Some(DpTimeline::Video(head("v3"))))],
                    }),
                ],
            }),
            markers_head: head("emptymarkers"),
            markers: vec![],
            tempo_automation: None,
            time_signature_automation: None,
        }),
        scenes: vec![
            DpScene {
                named: named("s1"),
                content: None,
            },
            DpScene {
                named: named("s2"),
                content: Some(DpTimeline::Video(head("v4"))),
            },
            DpScene {
                named: named("s3"),
                content: Some(DpTimeline::ClipSlot(head("s3c"), None)),
            },
        ],
        ..DpProject::default()
    };
    let mut rep = Report::default();
    let xml = write_project_reporting(&hostile, &mut rep);
    schema_check(&xsd, &xml);
    assert!(xml.contains("<Application name=\"VizDAW\""), "{xml}");
    assert!(
        xml.contains("s3c") && !xml.contains("s1") && !xml.contains("Warp"),
        "{xml}"
    );
    let it = item(
        &rep,
        ItemKind::LeftOut,
        "Parts the DAWproject file cannot hold",
    )
    .expect("reported");
    assert_eq!(
        it.details,
        [
            "Warps", "Warps", "Warps", "Warps", "Video", "Video", "Markers", "Scene", "Scene"
        ]
    );
    assert_eq!(write_project(&hostile), xml);

    let full = DpMetaData {
        fields: std::array::from_fn(|i| Some(format!("field {i} <&>"))),
        ..DpMetaData::default()
    };
    let xml = write_metadata(&full);
    schema_check(&path("MetaData.xsd"), &xml);
    let mut r = Report::default();
    assert_eq!(read_metadata(&parse(&xml), &mut r), full);
    schema_check(
        &path("MetaData.xsd"),
        &write_metadata(&DpMetaData::default()),
    );
}

#[test]
fn ids_are_checked() {
    let refused =
        |xml: &str| check_ids(&read_ok(xml).0, &mut Report::default()).expect_err("refused");
    let damaged = |why: &str| IoError::Refused(format!("This DAWproject file is damaged: {why}."));

    // A duplicate id.
    let dup = project(
        "<Structure><Track id=\"a\"/><Track id=\"b\"><Channel id=\"a\"/></Track></Structure>",
    );
    assert_eq!(refused(&dup), damaged("two parts share the ID 'a'"));

    // A clip whose reference holds itself, and a loop through two clips.
    let own = project(
        "<Arrangement><Lanes><Clips id=\"k\"><Clip time=\"0\" reference=\"k\"/></Clips></Lanes></Arrangement>",
    );
    assert_eq!(
        refused(&own),
        damaged("clips refer to each other in a loop")
    );
    let two = project(
        "<Arrangement><Lanes>\
           <Clips id=\"k1\"><Clip time=\"0\" reference=\"k2\"/></Clips>\
           <Clips id=\"k2\"><Clip time=\"0\" reference=\"k1\"/></Clips>\
         </Lanes></Arrangement>",
    );
    assert_eq!(
        refused(&two),
        damaged("clips refer to each other in a loop")
    );
    // A reference that is not a loop is fine, even a chain.
    let chain = project(
        "<Arrangement><Lanes>\
           <Clips id=\"k1\"><Clip time=\"0\" reference=\"k2\"/></Clips>\
           <Clips id=\"k2\"><Clip time=\"0\" reference=\"k3\"/></Clips>\
           <Notes id=\"k3\"/>\
         </Lanes></Arrangement>",
    );
    let mut r = Report::default();
    check_ids(&read_ok(&chain).0, &mut r).expect("a chain is not a loop");
    assert!(r.is_empty(), "{r:?}");

    // Dangling and wrong-kind references stay in the model and are reported.
    let what = "References to parts that are not in the file";
    for body in [
        "<Structure><Channel id=\"c\" destination=\"missing\"/></Structure>",
        "<Structure><Track id=\"t\"/><Channel id=\"c\" destination=\"t\"/></Structure>",
        "<Structure><Channel id=\"c\"><Sends><Send destination=\"c2\"><Volume unit=\"linear\"/></Send></Sends></Channel></Structure>",
        "<Structure><Channel id=\"c\"/></Structure><Arrangement><Lanes track=\"c\"/></Arrangement>",
        "<Structure><Track id=\"t\"/></Structure><Arrangement><Lanes><Points><Target parameter=\"t\"/></Points></Lanes></Arrangement>",
        "<Structure><Track id=\"t\"/></Structure><Arrangement><Lanes><Clips><Clip time=\"0\" reference=\"t\"/></Clips></Lanes></Arrangement>",
    ] {
        let (p, _) = read_ok(&project(body));
        let mut r = Report::default();
        check_ids(&p, &mut r).expect("not refused");
        let it = item(&r, ItemKind::LeftOut, what).unwrap_or_else(|| panic!("{body}: {r:?}"));
        assert_eq!((it.count, it.details.len()), (1, 1), "{body}");
    }
    // The reference survives in the model.
    let (p, _) = read_ok(&project(
        "<Structure><Channel id=\"c\" destination=\"missing\"/></Structure>",
    ));
    let DpStructureItem::Channel(c) = &p.structure[0] else {
        panic!("channel")
    };
    assert_eq!(c.destination.as_deref(), Some("missing"));
}

#[test]
fn unknown_parts_are_noted() {
    let what = "Parts of the file VizDAW doesn't know";
    let (p, r) = read_ok(&project(
        "<Structure><Track id=\"t\"><Future x=\"1\"><Deep/></Future></Track></Structure>\
         <Arrangement><Lanes><Clips><Clip time=\"0\" mystery=\"1\"/></Clips></Lanes></Arrangement>",
    ));
    assert_eq!(
        item(&r, ItemKind::Note, what).map(|i| i.count),
        Some(2),
        "{r:?}"
    );
    assert_eq!(p.structure.len(), 1);
    assert_eq!(lanes_of(&p).items.len(), 1);

    // A namespace declaration is not unknown; a future timeline keeps its place.
    let (p, r) = read_ok(&project(
        "<Arrangement xmlns:x=\"urn:x\"><Lanes><Hologram/><Clips/></Lanes></Arrangement>",
    ));
    assert_eq!(
        item(&r, ItemKind::Note, what).map(|i| i.count),
        Some(1),
        "{r:?}"
    );
    assert_eq!(lanes_of(&p).items.len(), 2);
    assert_eq!(
        lanes_of(&p).items[0],
        DpTimeline::Unknown("Hologram".to_owned())
    );
}

#[test]
fn unreadable_items_are_left_out() {
    let what = "Parts of the file that could not be read";
    let unreadable = |r: &Report| item(r, ItemKind::LeftOut, what).cloned();
    let only = |r: &Report, name: &str| {
        let it = unreadable(r).unwrap_or_else(|| panic!("nothing left out: {r:?}"));
        assert_eq!(
            (it.count, it.details.as_slice()),
            (1, [name.to_owned()].as_slice()),
            "{r:?}"
        );
        assert_eq!(r.items.len(), 1, "{r:?}");
    };

    let (p, r) = read_ok(&project(
        "<Arrangement><Lanes><Clips>\
           <Clip name=\"bad\"/>\
           <Clip name=\"good\" time=\"2\"/>\
         </Clips></Lanes></Arrangement>",
    ));
    only(&r, "Clip");
    let DpTimeline::Clips(c) = &lanes_of(&p).items[0] else {
        panic!("clips")
    };
    assert_eq!(c.clips.len(), 1);
    assert_eq!(c.clips[0].named.name.as_deref(), Some("good"));

    let (p, r) = read_ok(&project(
        "<Arrangement><Lanes><Notes>\
           <Note time=\"0\" duration=\"1\" channel=\"0\" key=\"x\"/>\
           <Note time=\"0\" duration=\"1\" channel=\"0\" key=\"61\"/>\
         </Notes></Lanes></Arrangement>",
    ));
    only(&r, "Note");
    let DpTimeline::Notes(n) = &lanes_of(&p).items[0] else {
        panic!("notes")
    };
    assert_eq!(n.notes.iter().map(|n| n.key).collect::<Vec<_>>(), [61]);

    let (p, r) = read_ok(&project(
        "<Arrangement><Lanes><Points><Target/>\
           <RealPoint time=\"0\" value=\"NaN\"/>\
           <RealPoint time=\"1\" value=\"0.5\"/>\
         </Points></Lanes></Arrangement>",
    ));
    only(&r, "RealPoint");
    let DpTimeline::Points(pts) = &lanes_of(&p).items[0] else {
        panic!("points")
    };
    assert_eq!(pts.points.len(), 1);

    // Nesting: 16 levels of timeline are read, the 17th is left out.
    let nest = |levels: usize| {
        let mut s = String::from("<Arrangement>");
        s.push_str(&"<Lanes>".repeat(levels));
        s.push_str(&"</Lanes>".repeat(levels));
        s.push_str("</Arrangement>");
        s
    };
    let (p, r) = read_ok(&project(&nest(16)));
    assert!(r.is_empty(), "{r:?}");
    let mut depth = 0;
    let mut at = Some(lanes_of(&p));
    while let Some(l) = at {
        depth += 1;
        at = match l.items.first() {
            Some(DpTimeline::Lanes(inner)) => Some(inner),
            _ => None,
        };
    }
    assert_eq!(depth, 16);
    let (p, r) = read_ok(&project(&nest(17)));
    only(&r, "Lanes");
    let mut depth = 0;
    let mut at = Some(lanes_of(&p));
    while let Some(l) = at {
        depth += 1;
        at = match l.items.first() {
            Some(DpTimeline::Lanes(inner)) => Some(inner),
            _ => None,
        };
    }
    assert_eq!(depth, 16);

    // A required value of a parameter, a device, a marker and a warp.
    let (p, r) = read_ok(&project(
        "<Transport><Tempo/><TimeSignature numerator=\"4\"/></Transport>\
         <Structure><Channel><Devices><Vst3Plugin deviceRole=\"audioFX\"/></Devices>\
           <Sends><Send><Pan unit=\"linear\"/></Send></Sends></Channel></Structure>",
    ));
    assert_eq!((p.tempo, p.time_signature), (None, None));
    let it = unreadable(&r).expect("left out");
    assert_eq!(it.count, 4, "{r:?}");
    assert_eq!(it.details, ["Tempo", "TimeSignature", "Vst3Plugin", "Send"]);
}

#[test]
fn versions() {
    for v in ["1.0", "1.1", "1"] {
        let xml = format!("<Project version=\"{v}\"/>");
        let (p, _) = read_ok(&xml);
        assert_eq!(p.version, v);
    }
    let (p, r) = read("<Project version=\"2.0\"/>");
    assert_eq!(
        p.expect_err("refused"),
        IoError::Refused("This DAWproject file is version 2.0; VizDAW reads version 1.".to_owned())
    );
    assert!(r.is_empty());
    for v in ["0.9", "x", "10.0"] {
        let (p, _) = read(&format!("<Project version=\"{v}\"/>"));
        assert!(
            matches!(p, Err(IoError::Refused(m)) if m.contains(v)),
            "{v}"
        );
    }
    let (p, _) = read("<Song version=\"1.0\"/>");
    assert_eq!(
        p.expect_err("refused"),
        IoError::Refused(core_io::zip::NOT_DAWPROJECT.to_owned())
    );
    let (p, _) = read("<Project/>");
    assert!(
        matches!(p, Err(IoError::Refused(m)) if m.starts_with("This DAWproject file is damaged"))
    );
}

#[test]
fn cut_and_dropped_text_stays_available() {
    // Text cut at the limit, in a field the model reads.
    let long = "x".repeat(MAX_TEXT_BYTES + 10);
    let xml = format!("<MetaData><Title>{long}</Title><Artist>ok</Artist></MetaData>");
    let mut r = Report::default();
    let m = read_metadata(&parse(&xml), &mut r);
    assert_eq!(m.fields[0].as_ref().map(String::len), Some(MAX_TEXT_BYTES));
    assert_eq!(m.cut.iter().filter(|c| **c).count(), 1);
    assert!(m.cut[0] && !m.cut[1]);
    assert_eq!(m.dropped_text, 0);

    // Text dropped from a field that has child elements, and from the project.
    let xml = "<MetaData><Title>a<b/>c</Title></MetaData>";
    let m = read_metadata(&parse(xml), &mut Report::default());
    assert_eq!(m.dropped_text, 2);
    let (p, _) =
        read_ok("<Project version=\"1.0\">stray<Application name=\"a\" version=\"1\"/></Project>");
    assert_eq!(p.dropped_text, 1);

    // An unknown metadata element is noted; a different root gives nothing.
    let mut r = Report::default();
    let m = read_metadata(
        &parse("<MetaData><Mood>x</Mood><Title>t</Title></MetaData>"),
        &mut r,
    );
    assert_eq!(m.fields[0].as_deref(), Some("t"));
    assert_eq!(
        item(&r, ItemKind::Note, "Parts of the file VizDAW doesn't know").map(|i| i.count),
        Some(1)
    );
    let m = read_metadata(
        &parse("<Other><Title>t</Title></Other>"),
        &mut Report::default(),
    );
    assert_eq!(m, DpMetaData::default());
}

#[test]
fn unreadable_optional_values_are_reported_and_infinity_survives() {
    let (p, r) = read_ok(&project(
        "<Transport><Tempo unit=\"bpm\" value=\"fast\" min=\"-inf\" max=\"INF\"/></Transport>",
    ));
    let t = p.tempo.expect("tempo");
    assert_eq!(
        (t.value, t.min, t.max),
        (None, Some(f64::NEG_INFINITY), Some(f64::INFINITY))
    );
    let it = item(
        &r,
        ItemKind::LeftOut,
        "Values in the file that could not be read",
    );
    assert_eq!(
        it.map(|i| i.details.clone()),
        Some(vec!["Tempo@value".to_owned()])
    );
    let xml = write_project(&DpProject {
        version: "1.0".to_owned(),
        tempo: Some(DpRealParam {
            value: Some(f64::NAN),
            ..t
        }),
        ..DpProject::default()
    });
    assert!(xml.contains("min=\"-inf\" max=\"inf\""), "{xml}");
}

#[test]
fn built_in_device_settings_are_kept_as_text() {
    let (p, r) = read_ok(&project(
        "<Structure><Channel><Devices>\
           <Equalizer deviceName=\"EQ\" deviceRole=\"audioFX\">\
             <Band type=\"bell\" order=\"2\"><Freq unit=\"hertz\" value=\"1000\"/><Gain unit=\"decibel\" value=\"3\"/></Band>\
             <InputGain unit=\"decibel\" value=\"0\"/>\
           </Equalizer>\
           <Device deviceName=\"X\" deviceRole=\"audioFX\"><Band/></Device>\
         </Devices></Channel></Structure>",
    ));
    let DpStructureItem::Channel(c) = &p.structure[0] else {
        panic!("channel")
    };
    assert_eq!(c.devices[0].kind, DpDeviceKind::Equalizer);
    assert_eq!(c.devices[0].settings.len(), 2);
    assert!(
        c.devices[0].settings[0].starts_with("Band type=bell order=2"),
        "{:?}",
        c.devices[0].settings
    );
    assert!(c.devices[0].settings[0].contains("Freq unit=hertz value=1000"));
    assert!(c.devices[1].settings.is_empty());
    // Only the generic device's stray element is unknown.
    assert_eq!(
        item(&r, ItemKind::Note, "Parts of the file VizDAW doesn't know").map(|i| i.count),
        Some(1)
    );
}

#[test]
fn the_deepest_allowed_tree_reads_and_drops() {
    // Tracks nested to the XML depth limit (256 elements).
    let mut s = String::from("<Project version=\"1.0\"><Structure>");
    let tracks = 253;
    s.push_str(&"<Track>".repeat(tracks));
    s.push_str(&"</Track>".repeat(tracks));
    s.push_str("</Structure></Project>");
    let (p, r) = read_ok(&s);
    assert!(r.is_empty());
    assert_eq!(p.structure.len(), 1);
    let _ = p.clone();
    drop(p);
}

#[test]
fn the_arrangement_markers_keep_their_head() {
    let xml = project(
        "<Arrangement><Lanes><Clips><Clip time=\"0\" reference=\"mk\"/></Clips></Lanes>\
         <Markers id=\"mk\" timeUnit=\"seconds\" name=\"M\"><Marker time=\"2.5\"/></Markers></Arrangement>",
    );
    let (p, r) = read_ok(&xml);
    assert!(r.is_empty(), "{r:?}");
    let a = p.arrangement.as_ref().expect("arrangement");
    assert_eq!(a.markers_head.named.id.as_deref(), Some("mk"));
    assert_eq!(a.markers_head.time_unit, Some(DpTimeUnit::Seconds));
    assert_eq!(a.markers[0].time, 2.5);
    let mut r = Report::default();
    let ids = check_ids(&p, &mut r).expect("ids");
    assert!(r.is_empty(), "the id resolves: {r:?}");
    assert_eq!(ids.kind("mk"), Some(IdKind::Timeline));
    let out = write_project(&p);
    assert!(
        out.contains("<Markers id=\"mk\" name=\"M\" timeUnit=\"seconds\">"),
        "{out}"
    );
    schema_check(&path("Project.xsd"), &out);
}

#[test]
fn parts_the_model_drops_still_have_their_ids() {
    let (p, r) = read_ok(&project(
        "<Structure><Channel id=\"c\"><Devices>\
           <Device deviceName=\"D\" deviceRole=\"audioFX\"><Parameters>\
             <BoolParameter id=\"flag\" value=\"true\"/></Parameters></Device>\
           <Compressor deviceName=\"C\" deviceRole=\"audioFX\"><Threshold id=\"thr\" unit=\"decibel\"/></Compressor>\
         </Devices></Channel></Structure>\
         <Arrangement><Lanes><Points><Target parameter=\"flag\"/></Points>\
           <Points><Target parameter=\"thr\"/></Points></Lanes></Arrangement>",
    ));
    let mut report = Report::default();
    let ids = check_ids(&p, &mut report).expect("ids");
    assert_eq!(
        item(
            &report,
            ItemKind::LeftOut,
            "References to parts that are not in the file"
        ),
        None
    );
    assert_eq!(ids.kind("flag"), Some(IdKind::Parameter));
    assert_eq!(ids.kind("thr"), Some(IdKind::Parameter));
    // The boolean parameter is not kept (noted), its id is; a clash is caught.
    assert_eq!(
        item(&r, ItemKind::Note, "Parts of the file VizDAW doesn't know").map(|i| i.count),
        Some(1)
    );
    let (p, _) = read_ok(&project(
        "<Structure><Track id=\"flag\"/><Channel><Devices>\
           <Device deviceName=\"D\" deviceRole=\"audioFX\"><Parameters>\
             <BoolParameter id=\"flag\"/></Parameters></Device></Devices></Channel></Structure>",
    ));
    assert!(check_ids(&p, &mut Report::default()).is_err());
}

#[test]
fn smaller_reading_rules() {
    let what = "Parts of the file that could not be read";
    let unknown = "Parts of the file VizDAW doesn't know";
    // Points without their target are left out.
    let (p, r) = read_ok(&project(
        "<Arrangement><Lanes><Points><RealPoint time=\"0\" value=\"1\"/></Points></Lanes></Arrangement>",
    ));
    assert_eq!(lanes_of(&p).items.len(), 0);
    assert_eq!(
        item(&r, ItemKind::LeftOut, what).map(|i| i.details.clone()),
        Some(vec!["Points".to_owned()])
    );
    // A built-in device's misspelled child is unknown; a real setting is not.
    let (_, r) = read_ok(&project(
        "<Structure><Channel><Devices><Limiter deviceName=\"L\" deviceRole=\"audioFX\">\
           <Threshold unit=\"decibel\"/><Treshold unit=\"decibel\"/><Band/></Limiter></Devices></Channel></Structure>",
    ));
    assert_eq!(
        item(&r, ItemKind::Note, unknown).map(|i| i.count),
        Some(2),
        "{r:?}"
    );
    // Attributes of the wrong point kind are unknown.
    let (_, r) = read_ok(&project(
        "<Arrangement><Lanes><Points><Target/>\
           <BoolPoint time=\"0\" value=\"1\" interpolation=\"hold\"/>\
           <RealPoint time=\"0\" value=\"1\" numerator=\"3\"/></Points></Lanes></Arrangement>",
    ));
    assert_eq!(
        item(&r, ItemKind::Note, unknown).map(|i| i.count),
        Some(2),
        "{r:?}"
    );
    // A send dropped for its volume is reported as a send, once.
    let (_, r) = read_ok(&project(
        "<Structure><Channel><Sends><Send><Volume/></Send></Sends></Channel></Structure>",
    ));
    assert_eq!(
        item(&r, ItemKind::LeftOut, what).map(|i| i.details.clone()),
        Some(vec!["Send".to_owned()])
    );
    assert_eq!(r.items.len(), 1);
    // An ID over 256 bytes refuses the file, without echoing it.
    let long = "i".repeat(257);
    let (p, _) = read(&project(&format!(
        "<Structure><Track id=\"{long}\"/></Structure>"
    )));
    let e = p.expect_err("refused");
    assert_eq!(
        e,
        IoError::Refused(
            "This DAWproject file is damaged: an ID is longer than 256 bytes.".to_owned()
        )
    );
    let ok = "i".repeat(256);
    assert!(
        read(&project(&format!(
            "<Structure><Track id=\"{ok}\"/></Structure>"
        )))
        .0
        .is_ok()
    );
    // A duplicate of a long code-built ID is refused with a short message.
    let mut p = read_ok(&project(
        "<Structure><Track id=\"a\"/><Track id=\"b\"/></Structure>",
    ))
    .0;
    for item in &mut p.structure {
        if let DpStructureItem::Track(t) = item {
            t.named.id = Some("z".repeat(10_000));
        }
    }
    let Err(IoError::Refused(m)) = check_ids(&p, &mut Report::default()) else {
        panic!("refused")
    };
    assert!(m.len() < 200, "{}", m.len());
}
