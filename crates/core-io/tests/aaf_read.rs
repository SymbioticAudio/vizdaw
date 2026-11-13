//! Reading an AAF composition (plan 6f Task 2): its slots, clips, fades,
//! gain, markers and media, in the shapes other DAWs write (Review Focus 1)
//! and with media that cannot be read where the file says (Review Focus 2).
//! Every fixture is built by `common::aaf`.

mod common;

use common::aaf::{
    ClipSpec, FileSpec, Fx, MXF_CONTAINER, Media, PICTURE, Seg, SlotKind, SlotSpec, Value, clip,
    file_url, gain, sound, trans, wav_bytes,
};
use core_io::IoError;
use core_io::aaf::dict::{class, def};
use core_io::aaf::timeline::{
    AafFile, Clip, FROM_VIZDAW, MXF, NO_SOURCE, NON_AUDIO, SELECTED, SEVERAL_COMPOSITIONS, Shape,
    locate,
};
use core_io::dawproject::import::NAMES_CUT;
use core_io::dawproject::package::{AudioPackage, AudioSource};
use core_io::limits::AafLimits;
use core_io::report::{ItemKind, Report};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const R48: (i32, i32) = (48_000, 1);

fn no() -> AtomicBool {
    AtomicBool::new(false)
}

fn open_in(dir: &Path, fx: &Fx, media: Option<&Path>) -> AafFile {
    let p = dir.join(format!("f{}.aaf", next()));
    fx.write(&p);
    AafFile::open(&p, media, &AafLimits::default(), &no()).unwrap()
}

fn next() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

fn open(fx: &Fx) -> AafFile {
    let dir = tempfile::tempdir().unwrap();
    open_in(dir.path(), fx, None)
}

fn has(r: &Report, kind: ItemKind, what: &str) -> bool {
    r.items.iter().any(|i| i.kind == kind && i.what == what)
}

fn one_clip(f: &AafFile) -> &Clip {
    let s = &f.composition().slots;
    assert_eq!(s.len(), 1, "{s:?}");
    assert_eq!(s[0].clips.len(), 1, "{:?}", s[0].clips);
    &s[0].clips[0]
}

fn source_of(f: &AafFile, key: usize) -> &AudioSource {
    &f.audio().iter().find(|a| a.key == key).unwrap().source
}

#[test]
fn reads_a_plain_composition() {
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("Gtr take", 48_000, 1, 200_000));
    let b = fx.file(&FileSpec::wav("Bass take", 48_000, 1, 200_000));
    fx.comp(
        "Song",
        true,
        &[
            sound(
                1,
                "Gtr",
                R48,
                Seg::Seq(vec![
                    Seg::Fill(4_800),
                    trans(4_800, None),
                    clip(&a, 1, 0, 100_800),
                    trans(4_800, Some(0)),
                    clip(&a, 1, 1_000, 48_000),
                ]),
            ),
            sound(
                2,
                "Bass",
                R48,
                Seg::Seq(vec![gain(1, 2, clip(&b, 1, 0, 48_000))]),
            ),
            SlotSpec {
                id: 3,
                name: "Markers".into(),
                rate: R48,
                kind: SlotKind::Markers(vec![(48_000, "Verse".into())]),
            },
        ],
    );
    let f = open(&fx);
    let c = f.composition();
    assert_eq!(c.name, "Song");
    assert_eq!(c.edit_rate, R48);
    assert!(!c.from_vizdaw);
    assert_eq!(
        c.slots.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["Gtr", "Bass"]
    );
    assert!(c.slots.iter().all(|s| s.channels == 1));
    let gtr = &c.slots[0].clips;
    assert_eq!(gtr.len(), 2);
    let (x, y) = (&gtr[0], &gtr[1]);
    assert_eq!((x.start, x.length, x.source_offset), (0, 100_800, 0));
    let fi = x.fade_in.as_ref().unwrap();
    assert_eq!((fi.frames, fi.shape), (4_800, Shape::Linear));
    assert!(x.fade_out.is_none() && x.crossfade_in.is_none());
    assert_eq!(
        (y.start, y.length, y.source_offset),
        (96_000, 48_000, 1_000)
    );
    let xf = y.crossfade_in.as_ref().unwrap();
    assert_eq!((xf.frames, xf.shape), (4_800, Shape::Linear));
    assert!(y.fade_in.is_none() && y.fade_out.is_none());
    assert_eq!(x.media, y.media);
    assert_eq!((x.channel, x.gain, x.varying_gain), (None, 1.0, false));
    assert_eq!(x.name.as_deref(), Some("Gtr take"));
    let bass = &c.slots[1].clips;
    assert_eq!(bass.len(), 1);
    assert_eq!((bass[0].start, bass[0].length), (0, 48_000));
    assert!((bass[0].gain - 0.5).abs() < 1e-12);
    assert_ne!(bass[0].media, x.media);
    assert_eq!(c.markers.len(), 1);
    assert_eq!(
        (c.markers[0].position, c.markers[0].name.as_str()),
        (48_000, "Verse")
    );
    assert!(c.slots[0].gain.constant.is_none() && c.slots[0].gain.points.is_empty());
    // Two embedded files, read from the AAF.
    assert_eq!(f.audio().len(), 2);
    for a in f.audio() {
        assert!(
            matches!(&a.source, AudioSource::Entry(n) if n.ends_with(".wav")),
            "{a:?}"
        );
        assert_eq!((a.channels, a.sample_rate, a.frames), (1, 48_000, 200_000));
    }
    let info = f.info();
    assert_eq!(
        (
            info.sample_rate,
            info.tracks,
            info.audio_files,
            info.offline
        ),
        (48_000, 2, 2, 0)
    );
}

#[test]
fn reads_other_shapes() {
    // A 25 fps composition over 48 kHz sources.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 480_000));
    fx.comp(
        "Film",
        true,
        &[sound(
            1,
            "Dialog",
            (25, 1),
            Seg::Seq(vec![Seg::Fill(25), clip(&a, 1, 10, 50)]),
        )],
    );
    let f = open(&fx);
    assert_eq!(f.composition().edit_rate, (25, 1));
    let c = one_clip(&f);
    assert_eq!((c.start, c.length, c.source_offset), (25, 50, 19_200));

    // Nested sequences: flattened.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    fx.comp(
        "Nest",
        true,
        &[sound(
            1,
            "T",
            R48,
            Seg::Seq(vec![
                Seg::Fill(100),
                Seg::Seq(vec![
                    clip(&a, 1, 0, 200),
                    Seg::Fill(50),
                    Seg::Seq(vec![clip(&a, 1, 0, 100)]),
                ]),
            ]),
        )],
    );
    let f = open(&fx);
    let starts: Vec<(i64, i64)> = f.composition().slots[0]
        .clips
        .iter()
        .map(|c| (c.start, c.length))
        .collect();
    assert_eq!(starts, [(100, 200), (350, 100)]);

    // Nested gain groups: the product.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    fx.comp(
        "Gain",
        true,
        &[sound(
            1,
            "T",
            R48,
            Seg::Seq(vec![gain(1, 2, gain(1, 2, clip(&a, 1, 0, 100)))]),
        )],
    );
    assert!((one_clip(&open(&fx)).gain - 0.25).abs() < 1e-12);

    // A transition with no CutPoint: read the same.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    fx.comp(
        "X",
        true,
        &[sound(
            1,
            "T",
            R48,
            Seg::Seq(vec![
                clip(&a, 1, 0, 1_000),
                trans(200, None),
                clip(&a, 1, 0, 1_000),
            ]),
        )],
    );
    let f = open(&fx);
    let cs = &f.composition().slots[0].clips;
    assert_eq!(
        (cs[1].start, cs[1].crossfade_in.as_ref().unwrap().frames),
        (800, 200)
    );

    // SourceClip fade properties.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    fx.comp(
        "F",
        true,
        &[sound(
            1,
            "T",
            R48,
            Seg::Seq(vec![Seg::Clip(ClipSpec {
                mob: a.clone(),
                slot: 1,
                start: 0,
                len: 10_000,
                fade_in: Some((2_400, 1)),
                fade_out: Some((1_200, 2)),
            })]),
        )],
    );
    let f = open(&fx);
    let c = one_clip(&f);
    let (i, o) = (c.fade_in.as_ref().unwrap(), c.fade_out.as_ref().unwrap());
    assert_eq!(
        (i.frames, i.shape, o.frames, o.shape),
        (2_400, Shape::Linear, 1_200, Shape::Power)
    );

    // Two compositions, none top-level: the one no other references.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    let sub = fx.comp(
        "Sub",
        false,
        &[sound(1, "S", R48, Seg::Seq(vec![clip(&a, 1, 0, 500)]))],
    );
    fx.comp(
        "Main",
        false,
        &[sound(1, "M", R48, Seg::Seq(vec![clip(&sub, 1, 0, 500)]))],
    );
    let f = open(&fx);
    assert_eq!(f.composition().name, "Main");
    assert!(has(
        &f.composition().report,
        ItemKind::Note,
        SEVERAL_COMPOSITIONS
    ));

    // A sub-composition used as a clip: its clips at composed positions.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    let sub = fx.comp(
        "Sub",
        false,
        &[sound(
            1,
            "S",
            R48,
            Seg::Seq(vec![Seg::Fill(1_000), clip(&a, 1, 0, 5_000)]),
        )],
    );
    fx.comp(
        "Main",
        true,
        &[sound(
            1,
            "M",
            R48,
            Seg::Seq(vec![Seg::Fill(10_000), clip(&sub, 1, 500, 4_000)]),
        )],
    );
    let f = open(&fx);
    assert_eq!(f.composition().name, "Main");
    assert!(!has(
        &f.composition().report,
        ItemKind::Note,
        SEVERAL_COMPOSITIONS
    ));
    let c = one_clip(&f);
    assert_eq!((c.start, c.length, c.source_offset), (10_500, 3_500, 0));

    // A Selector: its selected clip, with the Note.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    let b = fx.file(&FileSpec::wav("B", 48_000, 1, 48_000));
    fx.comp(
        "Sel",
        true,
        &[sound(
            1,
            "T",
            R48,
            Seg::Seq(vec![Seg::Select(
                Box::new(clip(&a, 1, 0, 300)),
                vec![clip(&b, 1, 0, 300)],
            )]),
        )],
    );
    let f = open(&fx);
    let c = one_clip(&f);
    let name = &f.audio().iter().find(|r| r.key == c.media).unwrap().name;
    assert_eq!(name, "A");
    assert!(has(&f.composition().report, ItemKind::Note, SELECTED));

    // A mono slot playing channel 2 of a stereo file.
    let mut fx = Fx::new();
    let st = fx.file(&FileSpec::wav("Stereo", 48_000, 2, 48_000));
    fx.comp(
        "Ch",
        true,
        &[sound(1, "R", R48, Seg::Seq(vec![clip(&st, 2, 0, 300)]))],
    );
    let f = open(&fx);
    assert_eq!(one_clip(&f).channel, Some(1));
    assert_eq!(f.composition().slots[0].channels, 1);

    // A picture slot: left out.
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    fx.comp(
        "Pic",
        true,
        &[
            SlotSpec {
                id: 1,
                name: "V1".into(),
                rate: (25, 1),
                kind: SlotKind::Data(PICTURE, Seg::Seq(vec![Seg::Fill(100)])),
            },
            sound(2, "A1", R48, Seg::Seq(vec![clip(&a, 1, 0, 300)])),
        ],
    );
    let f = open(&fx);
    assert_eq!(f.composition().slots.len(), 1);
    assert_eq!(f.composition().slots[0].name, "A1");
    assert!(has(&f.composition().report, ItemKind::LeftOut, NON_AUDIO));
}

#[test]
fn mob_chains_that_loop_are_cut() {
    let mut fx = Fx::new();
    let (ma, mb) = (fx.new_id(), fx.new_id());
    fx.mob(
        class::MASTER_MOB,
        &ma,
        "A",
        false,
        &[sound(1, "", R48, clip(&mb, 1, 0, 100))],
    );
    fx.mob(
        class::MASTER_MOB,
        &mb,
        "B",
        false,
        &[sound(1, "", R48, clip(&ma, 1, 0, 100))],
    );
    fx.comp(
        "Loop",
        true,
        &[sound(1, "T", R48, Seg::Seq(vec![clip(&ma, 1, 0, 100)]))],
    );
    let t = std::time::Instant::now();
    let f = open(&fx);
    assert!(t.elapsed().as_secs() < 2);
    let c = one_clip(&f);
    assert!(matches!(source_of(&f, c.media), AudioSource::Offline(_)));
    assert!(has(&f.composition().report, ItemKind::LeftOut, NO_SOURCE));
    assert_eq!(f.info().offline, 1);
}

#[test]
fn media_refs_resolve_or_go_offline() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let media = d.join("media");
    let sub = d.join("sub");
    std::fs::create_dir_all(&media).unwrap();
    std::fs::create_dir_all(&sub).unwrap();

    // `locate` on its own.
    std::fs::write(media.join("a.wav"), wav_bytes(48_000, 1, 10)).unwrap();
    std::fs::write(d.join("a b.wav"), wav_bytes(48_000, 1, 10)).unwrap();
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap();
    let p = locate("file://host/C:/a.wav", &sub, Some(&media)).unwrap();
    assert_eq!(canon(&p), canon(&media.join("a.wav")));
    let p = locate("../a%20b.wav", &sub, None).unwrap();
    assert_eq!(canon(&p), canon(&d.join("a b.wav")));
    assert_eq!(
        locate("http://x/a.wav", &sub, Some(&media)).unwrap_err(),
        "it is a URL, not a file"
    );
    assert!(locate("../missing.wav", &sub, None).is_err());
    assert!(
        locate("../sub", &sub, None).is_err(),
        "a folder is not a file"
    );

    // Through a file.
    let fifo = d.join("fifo.wav");
    let have_fifo = matches!(
        std::process::Command::new("mkfifo").arg(&fifo).status(),
        Ok(s) if s.success()
    );
    let mut fx = Fx::new();
    let specs = [
        FileSpec::url("Mxf", 48_000, 1, 100, &file_url(&d.join("x.mxf"))),
        FileSpec {
            container: Some(MXF_CONTAINER),
            ..FileSpec::url("MxfLabel", 48_000, 1, 100, &file_url(&d.join("y.wav")))
        },
        FileSpec::url("Web", 48_000, 1, 100, "http://x/a.wav"),
        FileSpec::url("Fifo", 48_000, 1, 100, &file_url(&fifo)),
        FileSpec::url("Found", 48_000, 1, 100, "file:///nowhere/at/all/a.wav"),
        FileSpec::url("Rel", 48_000, 1, 100, "../a%20b.wav"),
    ];
    let mobs: Vec<Vec<u8>> = specs.iter().map(|s| fx.file(s)).collect();
    let segs = mobs.iter().map(|m| clip(m, 1, 0, 100)).collect();
    fx.comp("Media", true, &[sound(1, "T", R48, Seg::Seq(segs))]);
    let p = sub.join("media.aaf");
    fx.write(&p);
    let f = AafFile::open(&p, Some(&media), &AafLimits::default(), &no()).unwrap();
    let by = |n: &str| &f.audio().iter().find(|a| a.name == n).unwrap().source;
    assert_eq!(by("Mxf"), &AudioSource::Offline(MXF.into()));
    assert_eq!(by("MxfLabel"), &AudioSource::Offline(MXF.into()));
    assert_eq!(
        by("Web"),
        &AudioSource::Offline("it is a URL, not a file".into())
    );
    if have_fifo {
        assert!(
            matches!(by("Fifo"), AudioSource::Offline(_)),
            "{:?}",
            by("Fifo")
        );
    } else {
        println!("mkfifo is missing: FIFO case skipped");
    }
    match by("Found") {
        AudioSource::External(p) => assert_eq!(canon(p), canon(&media.join("a.wav"))),
        s => panic!("{s:?}"),
    }
    match by("Rel") {
        AudioSource::External(p) => assert_eq!(canon(p), canon(&d.join("a b.wav"))),
        s => panic!("{s:?}"),
    }
    // Nothing was written beside the file.
    let mut names: Vec<_> = std::fs::read_dir(&sub)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["media.aaf"]);
}

#[test]
fn pcm_essence_gets_a_wav_header() {
    let dir = tempfile::tempdir().unwrap();
    let mut fx = Fx::new();
    let data: Vec<u8> = (0..400u32).map(|i| i as u8).collect();
    let m = fx.file(&FileSpec {
        name: "Raw".into(),
        rate: 44_100,
        channels: 2,
        bits: 16,
        frames: 100,
        media: Media::Pcm(data.clone()),
        container: None,
    });
    fx.comp(
        "P",
        true,
        &[sound(
            1,
            "T",
            (44_100, 1),
            Seg::Seq(vec![clip(&m, 1, 0, 100)]),
        )],
    );
    let f = open_in(dir.path(), &fx, None);
    let a = &f.audio()[0];
    assert_eq!((a.channels, a.sample_rate, a.frames), (2, 44_100, 100));
    let to = dir.path().join("out.wav");
    let n = f.extract(a, &to, &no()).unwrap();
    let w = std::fs::read(&to).unwrap();
    assert_eq!(n, 444);
    assert_eq!(w.len(), 444);
    assert_eq!(&w[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 436);
    assert_eq!(&w[8..16], b"WAVEfmt ");
    let u16_at = |i: usize| u16::from_le_bytes(w[i..i + 2].try_into().unwrap());
    let u32_at = |i: usize| u32::from_le_bytes(w[i..i + 4].try_into().unwrap());
    assert_eq!(u32_at(16), 16);
    assert_eq!((u16_at(20), u16_at(22)), (1, 2));
    assert_eq!((u32_at(24), u32_at(28)), (44_100, 176_400));
    assert_eq!((u16_at(32), u16_at(34)), (4, 16));
    assert_eq!(&w[36..40], b"data");
    assert_eq!(u32_at(40), 400);
    assert_eq!(&w[44..], &data[..]);
    // A second extract to the same path refuses to overwrite it.
    assert!(f.extract(a, &to, &no()).is_err());
    assert_eq!(std::fs::read(&to).unwrap(), w);
}

#[test]
fn short_essence_is_refused_per_file() {
    let mut fx = Fx::new();
    let short = fx.file(&FileSpec {
        media: Media::Wav(wav_bytes(48_000, 1, 100)),
        ..FileSpec::wav("Short", 48_000, 1, 48_000)
    });
    let fine = fx.file(&FileSpec::wav("Fine", 48_000, 1, 1_000));
    fx.comp(
        "S",
        true,
        &[sound(
            1,
            "T",
            R48,
            Seg::Seq(vec![clip(&short, 1, 0, 100), clip(&fine, 1, 0, 100)]),
        )],
    );
    let f = open(&fx);
    assert_eq!(f.composition().slots[0].clips.len(), 2);
    let by = |n: &str| &f.audio().iter().find(|a| a.name == n).unwrap().source;
    assert!(
        matches!(by("Short"), AudioSource::Offline(_)),
        "{:?}",
        by("Short")
    );
    assert!(matches!(by("Fine"), AudioSource::Entry(_)));
}

#[test]
fn strings_are_sanitised() {
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("a/b", 48_000, 1, 1_000));
    // A property's length is a u16, so no stored name can hold 70 000
    // characters (Review Focus 5's figure); 30 000 three-byte characters
    // (60 000 bytes stored, 90 000 as UTF-8) pass both the 64 KiB text
    // limit and the 4 096-byte name cut.
    let long = "\u{20AC}".repeat(30_000);
    let names = ["A\0hidden", "x/y", "\u{202E}evil", long.as_str()];
    let slots: Vec<SlotSpec> = names
        .iter()
        .enumerate()
        .map(|(i, n)| sound(i as u32 + 1, n, R48, Seg::Seq(vec![clip(&a, 1, 0, 10)])))
        .collect();
    fx.comp("Names", true, &slots);
    let f = open(&fx);
    let got: Vec<&str> = f
        .composition()
        .slots
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(&got[..3], ["A", "x/y", "evil"]);
    assert_eq!(got[3].len(), 4_095, "cut at a character boundary");
    assert!(got[3].chars().all(|c| c == '\u{20AC}'));
    assert!(has(&f.composition().report, ItemKind::Changed, NAMES_CUT));
    // The media's entry name is a safe file name.
    assert_eq!(f.audio()[0].source, AudioSource::Entry("a-b.wav".into()));
}

#[test]
fn vizdaw_files_are_noted() {
    let mut fx = Fx::new();
    fx.product("VizDAW");
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 1_000));
    fx.comp(
        "V",
        true,
        &[sound(1, "T", R48, Seg::Seq(vec![clip(&a, 1, 0, 10)]))],
    );
    let f = open(&fx);
    assert!(f.composition().from_vizdaw);
    assert!(f.info().from_vizdaw);
    assert!(has(&f.composition().report, ItemKind::Note, FROM_VIZDAW));

    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 1_000));
    fx.comp(
        "V",
        true,
        &[sound(1, "T", R48, Seg::Seq(vec![clip(&a, 1, 0, 10)]))],
    );
    let f = open(&fx);
    assert!(!f.composition().from_vizdaw);
    assert!(!has(&f.composition().report, ItemKind::Note, FROM_VIZDAW));
}

#[test]
fn info_reads_the_majority_rate() {
    let mut fx = Fx::new();
    let mut segs = Vec::new();
    for (i, rate) in [44_100, 48_000, 44_100, 44_100].iter().enumerate() {
        let m = fx.file(&FileSpec::wav(&format!("F{i}"), *rate, 1, 100));
        segs.push(clip(&m, 1, 0, 10));
    }
    fx.comp("R", true, &[sound(1, "T", R48, Seg::Seq(segs))]);
    let info = open(&fx).info();
    assert_eq!(info.sample_rate, 44_100);
    assert_eq!((info.tracks, info.audio_files, info.offline), (1, 4, 0));
}

#[test]
fn offline_refs_are_not_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut fx = Fx::new();
    let mxf = fx.file(&FileSpec::url(
        "Mxf",
        48_000,
        1,
        1_000_000,
        "file:///a/b.mxf",
    ));
    let wav = wav_bytes(48_000, 1, 1_000);
    let emb = fx.file(&FileSpec {
        media: Media::Wav(wav.clone()),
        ..FileSpec::wav("Emb", 48_000, 1, 1_000)
    });
    fx.comp(
        "O",
        true,
        &[sound(
            1,
            "T",
            R48,
            Seg::Seq(vec![clip(&mxf, 1, 0, 10), clip(&emb, 1, 0, 10)]),
        )],
    );
    let f = open_in(dir.path(), &fx, None);
    let size = wav.len() as u64;
    let expect = size + (1_000u64 * 4).max(size * 4);
    assert_eq!(f.needed_bytes(4, &no()).unwrap(), expect);
    let m = f.audio().iter().find(|a| a.name == "Mxf").unwrap();
    let to = dir.path().join("mxf.wav");
    assert!(matches!(f.extract(m, &to, &no()), Err(IoError::Refused(_))));
    assert!(std::fs::symlink_metadata(&to).is_err());
    // The embedded one extracts whole.
    let e = f.audio().iter().find(|a| a.name == "Emb").unwrap();
    let to = dir.path().join("emb.wav");
    assert_eq!(f.extract(e, &to, &no()).unwrap(), size);
    assert_eq!(std::fs::read(&to).unwrap(), wav);
    // A cancelled read stops.
    let yes = AtomicBool::new(true);
    assert!(matches!(f.needed_bytes(4, &yes), Err(IoError::Cancelled)));
}

#[test]
fn varying_and_track_curves_are_read() {
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    let inner = Seg::Seq(vec![
        Seg::Op {
            op: def::OPERATION_DEF_MONO_AUDIO_GAIN,
            param: def::PARAMETER_DEF_AMPLITUDE,
            value: Value::Points(
                vec![((0, 1), (1, 4)), ((1, 1), (1, 1))],
                def::INTERPOLATION_DEF_LINEAR,
            ),
            input: Box::new(clip(&a, 1, 0, 1_000)),
        },
        Seg::Fill(1_000),
    ]);
    let panned = Seg::Op {
        op: def::OPERATION_DEF_MONO_AUDIO_PAN,
        param: def::PARAMETER_DEF_PAN,
        value: Value::Points(
            vec![((0, 1), (0, 1)), ((1, 2), (1, 2)), ((1, 1), (1, 1))],
            def::INTERPOLATION_DEF_LINEAR,
        ),
        input: Box::new(inner),
    };
    fx.comp("C", true, &[sound(1, "T", R48, gain(1, 2, panned))]);
    let f = open(&fx);
    let s = &f.composition().slots[0];
    assert_eq!(s.gain.constant, Some(0.5));
    assert_eq!(s.pan.constant, None);
    assert_eq!(s.pan.points, [(0, 0.0), (1_000, 0.5), (2_000, 1.0)]);
    let c = one_clip(&f);
    assert!(c.varying_gain);
    assert!((c.gain - 0.25).abs() < 1e-12);
}

#[test]
fn start_timecode_is_read() {
    let mut fx = Fx::new();
    let a = fx.file(&FileSpec::wav("A", 48_000, 1, 48_000));
    fx.comp(
        "TC",
        true,
        &[
            SlotSpec {
                id: 9,
                name: "TC".into(),
                rate: (25, 1),
                kind: SlotKind::Timecode(90_000, 25, false),
            },
            sound(1, "T", R48, Seg::Seq(vec![clip(&a, 1, 0, 10)])),
        ],
    );
    let f = open(&fx);
    assert_eq!(f.composition().start_tc, Some((90_000, 25, false)));
    assert_eq!(f.composition().slots.len(), 1);
    assert!(!has(&f.composition().report, ItemKind::LeftOut, NON_AUDIO));
}

/// Every `.aaf` in `VIZDAW_INTERCHANGE_SAMPLES` (outside `~/Library`)
/// opens, or is refused in a sentence.
#[test]
fn reads_local_samples() {
    let Some(dir) = std::env::var_os("VIZDAW_INTERCHANGE_SAMPLES") else {
        println!("VIZDAW_INTERCHANGE_SAMPLES is unset: skipped");
        return;
    };
    let dir = PathBuf::from(dir);
    if let Some(home) = std::env::var_os("HOME") {
        let lib = PathBuf::from(home).join("Library");
        let real = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        assert!(
            !real.starts_with(&lib) && !dir.starts_with(&lib),
            "refused: VIZDAW_INTERCHANGE_SAMPLES may not be inside ~/Library"
        );
    }
    let mut stack = vec![dir];
    let mut files = Vec::new();
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            let ft = e.file_type().unwrap();
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("aaf")) {
                files.push(p);
            }
        }
    }
    files.sort();
    for p in files {
        match AafFile::open(&p, None, &AafLimits::default(), &no()) {
            Ok(f) => {
                let c = f.composition();
                let clips: usize = c.slots.iter().map(|s| s.clips.len()).sum();
                println!(
                    "{}: {} tracks, {} clips, {} report lines",
                    p.display(),
                    c.slots.len(),
                    clips,
                    c.report.items.len()
                );
            }
            Err(IoError::Refused(why)) => println!("{}: refused: {why}", p.display()),
            Err(e) => panic!("{}: {e:?}", p.display()),
        }
    }
}
