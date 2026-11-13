//! The merge's three sides and how their rows are matched (plan 6e Task 4,
//! Ruling 4): theirs as a project of its own, tracks by ID, tag, name and
//! order, strips with their tracks, buses, markers, notes and regions,
//! within the tolerances.

mod common;

use common::merge::{
    self, EXPORT, Strip, Trip, fixture, hostile, media, return_file, theirs_of, trip,
};
use common::package::TestPlist;
use core_engine::fixtures::{BAR, Builder};
use core_io::dawproject::import::{ImportEnv, Origins, map};
use core_io::dawproject::merge::matching::{
    Matched, MatchedBy, Matches, match_all, notes, regions,
};
use core_io::dawproject::merge::theirs::{Theirs, theirs};
use core_io::dawproject::merge::{FRAMES, LEVEL_DB, normalised};
use core_io::dawproject::package::open;
use core_io::dawproject::record::{
    Identity, ObjectRef, PNote, PRegion, PTrack, Projection, TrackIds, projection_with_identity,
};
use core_io::dawproject::{DpClip, DpProject, DpStructureItem, DpTimeline, DpTrack, DpUnit};
use core_io::limits::PackageLimits;
use core_io::zip::{EntryMethod, ZipWriter};
use core_model::document::Document;
use core_model::event::{Fade, FadeShape};
use core_model::mixer::PluginFormat;
use core_model::time::PPQ;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

// ---------------------------------------------------------------- helpers

fn never() -> AtomicBool {
    AtomicBool::new(false)
}

/// Ours: the project as exported (unchanged since).
fn ours_of(t: &Trip) -> (Projection, Identity) {
    projection_with_identity(&t.base_doc, t.seq).expect("projects")
}

/// The returned file at `path` as theirs, and every row matched.
fn matched(t: &Trip, path: &Path) -> (Theirs, Matches) {
    let th = theirs_of(t, path);
    let m = match_all(&t.snapshot, &th, &ours_of(t), &never()).expect("matches");
    (th, m)
}

/// Each row of a side of `rows` appears exactly once (`n` rows there).
#[track_caller]
fn once(rows: &[Matched], side: impl Fn(&Matched) -> Option<usize>, n: usize, what: &str) {
    let mut seen = vec![0u32; n];
    for r in rows {
        if let Some(i) = side(r) {
            assert!(i < n, "{what}: row {i} of {n}");
            seen[i] += 1;
        }
    }
    assert!(seen.iter().all(|c| *c == 1), "{what}: {seen:?} in {rows:?}");
}

/// Every row of each side of `m` appears in exactly one `Matched`.
#[track_caller]
fn each_once(t: &Trip, th: &Theirs, m: &Matches) {
    let b = &t.snapshot.projection;
    let tp = &th.projection;
    let o = ours_of(t).0;
    for (rows, nb, nt, no, what) in [
        (
            &m.tracks,
            b.tracks.len(),
            tp.tracks.len(),
            o.tracks.len(),
            "tracks",
        ),
        (
            &m.strips,
            b.strips.len(),
            tp.strips.len(),
            o.strips.len(),
            "strips",
        ),
        (
            &m.markers,
            b.markers.len(),
            tp.markers.len(),
            o.markers.len(),
            "markers",
        ),
    ] {
        once(rows, |r| r.base, nb, &format!("base {what}"));
        once(rows, |r| r.theirs, nt, &format!("theirs {what}"));
        once(rows, |r| r.ours, no, &format!("ours {what}"));
    }
}

/// The row of `rows` holding base row `b`.
#[track_caller]
fn of_base(rows: &[Matched], b: usize) -> Matched {
    *rows
        .iter()
        .find(|r| r.base == Some(b))
        .unwrap_or_else(|| panic!("no row for base {b}: {rows:?}"))
}

/// The base track row named `name` (with content `audio`).
fn base_track(t: &Trip, name: &str) -> usize {
    t.snapshot
        .projection
        .tracks
        .iter()
        .position(|x| x.name == name)
        .unwrap_or_else(|| panic!("no base track {name}"))
}

fn theirs_track(th: &Theirs, name: &str) -> usize {
    th.projection
        .tracks
        .iter()
        .position(|x| x.name == name)
        .unwrap_or_else(|| panic!("no theirs track {name}: {:?}", names(&th.projection)))
}

fn names(p: &Projection) -> Vec<&str> {
    p.tracks.iter().map(|t| t.name.as_str()).collect()
}

/// Every track row of `m` is on all three sides, `by` `want`.
#[track_caller]
fn all_tracks_by(m: &Matches, want: MatchedBy) {
    for r in &m.tracks {
        assert!(
            r.base.is_some() && r.theirs.is_some() && r.ours.is_some(),
            "{r:?}"
        );
        assert_eq!(r.by, want, "{:?}", m.tracks);
    }
}

/// The top-level tracks of `p`.
fn tracks(p: &mut DpProject) -> impl Iterator<Item = &mut DpTrack> {
    p.structure.iter_mut().filter_map(|i| match i {
        DpStructureItem::Track(t) => Some(t),
        DpStructureItem::Channel(_) => None,
    })
}

fn track<'a>(p: &'a mut DpProject, name: &str) -> &'a mut DpTrack {
    tracks(p)
        .find(|t| t.named.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no track {name}"))
}

/// The clips on the track with XML ID `id`.
fn clips<'a>(p: &'a mut DpProject, id: &str) -> &'a mut Vec<DpClip> {
    let Some(arr) = p.arrangement.as_mut() else {
        panic!("no arrangement")
    };
    let Some(lanes) = arr.lanes.as_mut() else {
        panic!("no lanes")
    };
    for item in &mut lanes.items {
        if let DpTimeline::Lanes(l) = item
            && l.head.track.as_deref() == Some(id)
        {
            for inner in &mut l.items {
                if let DpTimeline::Clips(c) = inner {
                    return &mut c.clips;
                }
            }
        }
    }
    panic!("no clips on {id}")
}

/// A region that does not survive the 44.1 kHz grid (found by scanning).
const OFFSET: u64 = 0;
const LENGTH: u64 = 1_028;

/// One beat in ticks.
const BEAT: f64 = PPQ as f64;

// ---------------------------------------------------------------- tests

#[test]
fn unchanged_file_matches_everything() {
    // The insert and the Keys instrument have states, so the hashes are
    // compared for real.
    let (doc, seq) = fixture();
    let mut doc = doc;
    let slots: Vec<_> = doc
        .mixer()
        .strips()
        .values()
        .flat_map(|s| s.inserts.iter().map(|i| i.id))
        .collect();
    assert_eq!(slots.len(), 1, "the fixture's insert");
    doc = core_model::state::capture(&doc, slots[0], b"comp state")
        .unwrap()
        .0;
    let t = trip(&doc, seq);
    let path = return_file(&t, Strip::Nothing, |_, _| {});
    let (th, m) = matched(&t, &path);
    each_once(&t, &th, &m);
    assert!(!m.heuristic);
    let b = &t.snapshot.projection;
    assert_eq!(m.tracks.len(), b.tracks.len());
    assert_eq!(m.strips.len(), b.strips.len());
    assert_eq!(m.markers.len(), b.markers.len());
    for rows in [&m.tracks, &m.strips, &m.markers] {
        for r in rows {
            assert!(
                r.base.is_some() && r.theirs.is_some() && r.ours.is_some(),
                "{r:?}"
            );
            assert_eq!(r.by, MatchedBy::Id, "{rows:?}");
        }
    }

    let nb = normalised(b, &t.snapshot);
    let ours = ours_of(&t);
    let no = normalised(&ours.0, &t.snapshot);
    let ids = t.snapshot.identity.as_ref().unwrap();
    let mut paired = 0;
    for r in &m.tracks {
        let (bi, ti, oi) = (r.base.unwrap(), r.theirs.unwrap(), r.ours.unwrap());
        let tt = &th.projection.tracks[ti];
        for (a, a_ids) in [
            (&nb.tracks[bi], &ids.tracks[bi]),
            (&no.tracks[oi], &ours.1.tracks[oi]),
        ] {
            let n = notes(a, tt);
            assert_eq!(n.len(), a.notes_played.len(), "{}: {n:?}", a.name);
            assert!(n.iter().all(|(x, y)| x.is_some() && y.is_some()), "{n:?}");
            let g = regions((a, a_ids), (tt, &th.identity.tracks[ti], &th.origins));
            assert_eq!(g.len(), a.regions.len(), "{}: {g:?}", a.name);
            assert!(g.iter().all(|(x, y)| x.is_some() && y.is_some()), "{g:?}");
            paired += n.len() + g.len();
        }
    }
    assert_eq!(paired, 2 * (7 + 1 + 2), "every note and region, twice");

    let mut hashes = 0;
    for r in &m.strips {
        let bs = &b.strips[r.base.unwrap()];
        let ts = &th.projection.strips[r.theirs.unwrap()];
        assert_eq!(bs.devices.len(), ts.devices.len(), "{}", bs.name);
        for (x, y) in bs.devices.iter().zip(&ts.devices) {
            assert_eq!(x.3, y.3, "{}: {} state", bs.name, x.1);
            hashes += usize::from(x.3.is_some());
        }
    }
    assert_eq!(hashes, 1, "the insert's state is compared");
}

#[test]
fn tracks_match_by_tag_then_name_then_order() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);

    let (th, m) = matched(&t, &return_file(&t, Strip::Record, |_, _| {}));
    each_once(&t, &th, &m);
    all_tracks_by(&m, MatchedBy::Tag);
    assert!(!m.heuristic);

    let (th, m) = matched(&t, &return_file(&t, Strip::Both, |_, _| {}));
    each_once(&t, &th, &m);
    all_tracks_by(&m, MatchedBy::Name);
    assert!(m.heuristic);

    // A tagged file whose new track took a deleted track's name and type
    // still has its tags: no `BY_NAME`.
    let (th, m) = matched(
        &t,
        &return_file(&t, Strip::Record, |p, _| {
            track(p, "Bass").named.comment = None;
        }),
    );
    each_once(&t, &th, &m);
    assert_eq!(
        of_base(&m.tracks, base_track(&t, "Bass")).by,
        MatchedBy::Name
    );
    assert!(!m.heuristic);

    let renamed = return_file(&t, Strip::Both, |p, _| {
        for (i, tr) in tracks(p).enumerate() {
            if tr.named.name.as_deref() != Some("Master") {
                tr.named.name = Some(format!("Renamed {i}"));
            }
        }
    });
    let (th, m) = matched(&t, &renamed);
    each_once(&t, &th, &m);
    // The bus was renamed too: it is new plus deleted (not a track).
    all_tracks_by(&m, MatchedBy::Order);
    assert!(m.heuristic);
    for (i, r) in m.tracks.iter().enumerate() {
        assert_eq!(r.base, Some(i));
        assert_eq!(r.theirs, Some(i), "the file kept the order");
    }
}

/// The fixture with a second audio track, "Drums" (one region of media
/// "kit").
fn two_audio_tracks() -> (Document, core_model::ids::SeqId) {
    let (doc, seq) = fixture();
    let mut b = Builder::from_doc(doc);
    let drums = b.lane(seq, "Drums", None).unwrap();
    let kit = media(&mut b, "kit", 48_000, 2);
    let none = Fade {
        length_frames: 0,
        shape: FadeShape::Linear,
    };
    b.region(seq, drums, 0, kit, 0, 48_000, 0.0, none, none)
        .unwrap();
    (b.finish(), seq)
}

#[test]
fn renamed_and_reordered_tracks_match_by_order() {
    let (doc, seq) = two_audio_tracks();
    let t = trip(&doc, seq);
    let base = &t.snapshot.projection;
    assert_eq!(
        names(base),
        vec!["Bass", "Keys", "Keys audio", "Drums"],
        "the base"
    );
    let path = return_file(&t, Strip::Both, |p, _| {
        let at = |p: &DpProject, n: &str| {
            p.structure
                .iter()
                .position(|i| matches!(i, DpStructureItem::Track(t) if t.named.name.as_deref() == Some(n)))
                .unwrap()
        };
        let (k, d) = (at(p, "Keys audio"), at(p, "Drums"));
        p.structure.swap(k, d);
        track(p, "Drums").named.name = Some("Audio 1".into());
        track(p, "Keys audio").named.name = Some("Audio 2".into());
    });
    let (th, m) = matched(&t, &path);
    each_once(&t, &th, &m);
    assert!(m.heuristic);
    assert!(
        m.tracks
            .iter()
            .all(|r| r.base.is_some() && r.theirs.is_some() && r.ours.is_some()),
        "no unmatched rows: {:?}",
        m.tracks
    );
    // The notes tracks kept their names; the audio tracks pair in order.
    for (name, by) in [("Bass", MatchedBy::Name), ("Keys", MatchedBy::Name)] {
        let r = of_base(&m.tracks, base_track(&t, name));
        assert_eq!(r.theirs, Some(theirs_track(&th, name)));
        assert_eq!(r.by, by);
    }
    for (base, theirs) in [("Keys audio", "Audio 1"), ("Drums", "Audio 2")] {
        let r = of_base(&m.tracks, base_track(&t, base));
        assert_eq!(r.theirs, Some(theirs_track(&th, theirs)), "{base}");
        assert_eq!(r.by, MatchedBy::Order, "{base}");
    }
}

#[test]
fn ids_are_ignored_without_the_record() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    // A hand file: its Track IDs t0 and t1 now name the other track.
    let edit = |p: &mut DpProject, _: &mut _| {
        p.application = Some(("Fixture".into(), "1".into()));
        track(p, "Bass").named.name = Some("Keys was here".into());
        track(p, "Keys").named.name = Some("Bass".into());
        track(p, "Keys was here").named.name = Some("Keys".into());
    };
    let (th, m) = matched(&t, &return_file(&t, Strip::Both, edit));
    each_once(&t, &th, &m);
    for name in ["Bass", "Keys"] {
        let r = of_base(&m.tracks, base_track(&t, name));
        assert_eq!(r.theirs, Some(theirs_track(&th, name)), "{name} by name");
        assert_eq!(r.by, MatchedBy::Name);
    }

    // With the record, the same IDs are trusted and win over the names.
    let (th, m) = matched(&t, &return_file(&t, Strip::Tags, edit));
    each_once(&t, &th, &m);
    let r = of_base(&m.tracks, base_track(&t, "Bass"));
    assert_eq!(r.theirs, Some(theirs_track(&th, "Keys")), "t0 is Bass");
    assert_eq!(r.by, MatchedBy::Id);
}

#[test]
fn split_tracks_match_their_parts() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let (notes_part, audio_part) = (base_track(&t, "Keys"), base_track(&t, "Keys audio"));
    let ours = ours_of(&t);
    for strip in [Strip::Nothing, Strip::Record, Strip::Both] {
        let (th, m) = matched(&t, &return_file(&t, strip, |_, _| {}));
        each_once(&t, &th, &m);
        for (b, name) in [(notes_part, "Keys"), (audio_part, "Keys audio")] {
            let r = of_base(&m.tracks, b);
            assert_eq!(r.theirs, Some(theirs_track(&th, name)), "{strip:?} {name}");
            let o = r.ours.unwrap();
            assert_eq!(ours.0.tracks[o].name, name, "{strip:?}");
        }
    }

    // The other DAW put the audio on the Keys track: one track with notes
    // and audio, which comes back as its two parts.
    let one = return_file(&t, Strip::Record, |p, _| {
        p.structure.retain(
            |i| !matches!(i, DpStructureItem::Track(t) if t.named.name.as_deref() == Some("Keys audio")),
        );
        let keys = track(p, "Keys");
        keys.content.push(core_io::dawproject::DpContentType::Audio);
        let audio = std::mem::take(clips(p, "t1a"));
        clips(p, "t1").extend(audio);
    });
    let (th, m) = matched(&t, &one);
    each_once(&t, &th, &m);
    assert_eq!(names(&th.projection), vec!["Bass", "Keys", "Keys audio"]);
    for (b, name) in [(notes_part, "Keys"), (audio_part, "Keys audio")] {
        let r = of_base(&m.tracks, b);
        assert_eq!(r.theirs, Some(theirs_track(&th, name)), "{name}");
        assert_eq!(r.by, MatchedBy::Tag, "{name}");
    }
}

#[test]
fn strips_follow_their_tracks_and_buses_match_by_tag_then_name() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let base = &t.snapshot.projection;
    let bus = base.strips.iter().position(|s| s.name == "Reverb").unwrap();
    let ours = ours_of(&t);
    for (strip, by) in [
        (Strip::Nothing, MatchedBy::Id),
        (Strip::Record, MatchedBy::Tag),
        (Strip::Both, MatchedBy::Name),
    ] {
        let (th, m) = matched(&t, &return_file(&t, strip, |_, _| {}));
        each_once(&t, &th, &m);
        for r in &m.tracks {
            let (b, ti, o) = (r.base.unwrap(), r.theirs.unwrap(), r.ours.unwrap());
            let s = of_base(&m.strips, base.tracks[b].strip.unwrap());
            assert_eq!(s.theirs, th.projection.tracks[ti].strip, "{strip:?}");
            assert_eq!(s.ours, ours.0.tracks[o].strip, "{strip:?}");
            assert_eq!(s.by, r.by, "{strip:?}: the track's");
        }
        let s = of_base(&m.strips, bus);
        assert_eq!(th.projection.strips[s.theirs.unwrap()].name, "Reverb");
        assert_eq!(s.by, by, "{strip:?}");
    }

    let renamed = return_file(&t, Strip::Both, |p, _| {
        let r = track(p, "Reverb");
        r.named.name = Some("Hall".into());
        r.channel.as_mut().unwrap().named.name = Some("Hall".into());
    });
    let (th, m) = matched(&t, &renamed);
    each_once(&t, &th, &m);
    let s = of_base(&m.strips, bus);
    assert_eq!(s.theirs, None, "deleted there");
    let hall = th
        .projection
        .strips
        .iter()
        .position(|s| s.name == "Hall")
        .unwrap();
    let new = m.strips.iter().find(|r| r.theirs == Some(hall)).unwrap();
    assert_eq!((new.base, new.ours), (None, None), "new there");
}

#[test]
fn markers_match_by_tag_then_name_and_tick() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let chorus = t
        .snapshot
        .projection
        .markers
        .iter()
        .position(|m| m.1 == "Chorus")
        .unwrap();
    let moved = |ticks: f64| {
        move |p: &mut DpProject, _: &mut _| {
            let arr = p.arrangement.as_mut().unwrap();
            let m = arr
                .markers
                .iter_mut()
                .find(|m| m.named.name.as_deref() == Some("Chorus"))
                .unwrap();
            m.time += ticks / BEAT;
        }
    };
    for (strip, ticks, found) in [
        (Strip::Tags, 1.0, Some(MatchedBy::Name)),
        (Strip::Tags, 2.0, None),
        (Strip::Nothing, 2.0, Some(MatchedBy::Id)),
        (Strip::Record, 5.0, Some(MatchedBy::Tag)),
    ] {
        let (th, m) = matched(&t, &return_file(&t, strip, moved(ticks)));
        each_once(&t, &th, &m);
        let r = of_base(&m.markers, chorus);
        match found {
            Some(by) => {
                let ti = r.theirs.expect("matched");
                assert_eq!(th.projection.markers[ti].1, "Chorus");
                assert_eq!(r.by, by, "{strip:?} {ticks}");
            }
            None => {
                assert_eq!(r.theirs, None, "deleted there");
                assert!(r.ours.is_some());
                let new: Vec<_> = m.markers.iter().filter(|r| r.base.is_none()).collect();
                assert_eq!(new.len(), 1, "new there: {:?}", m.markers);
                let ti = new[0].theirs.unwrap();
                assert_eq!(th.projection.markers[ti].1, "Chorus");
            }
        }
    }
}

fn note(tick: i64, pitch: u8, duration: i64) -> PNote {
    PNote {
        tick,
        pitch,
        channel: 0,
        velocity: 0x8000,
        release: 0,
        duration,
        expression: Vec::new(),
    }
}

fn part(notes: Vec<PNote>) -> PTrack {
    PTrack {
        name: "Part".into(),
        notes: true,
        audio: false,
        strip: None,
        notes_played: notes,
        midi: Vec::new(),
        regions: Vec::new(),
        automation: Vec::new(),
    }
}

#[test]
fn notes_match_within_a_tick() {
    let q = PPQ;
    let a = part(vec![note(0, 60, q), note(q, 62, q), note(2 * q, 64, q)]);
    let b = part(vec![
        note(1, 60, q),
        note(q + 2, 62, q),
        note(2 * q, 64, q + 1),
    ]);
    assert_eq!(
        notes(&a, &b),
        vec![
            (Some(0), Some(0)),
            (Some(1), None),
            (Some(2), Some(2)),
            (None, Some(1))
        ]
    );
    // Length, pitch and channel.
    let b = part(vec![note(0, 60, q + 2), note(q, 63, q), {
        let mut n = note(2 * q, 64, q);
        n.channel = 1;
        n
    }]);
    let got = notes(&a, &b);
    assert_eq!(
        got.iter()
            .filter(|(x, y)| x.is_some() && y.is_some())
            .count(),
        0,
        "{got:?}"
    );
    assert_eq!(got.len(), 6);
    // Each takes the first unpaired within the tolerances.
    let a = part(vec![note(1, 60, q), note(1, 60, q)]);
    let b = part(vec![note(2, 60, q), note(0, 60, q), note(9, 60, q)]);
    assert_eq!(
        notes(&a, &b),
        vec![(Some(0), Some(1)), (Some(1), Some(0)), (None, Some(2))]
    );

    // 50 000 notes, every one moved by a tick, many at the same tick.
    let many: Vec<PNote> = (0..50_000i64)
        .map(|i| note((i / 4) * 120, 36 + (i % 3) as u8, 60))
        .collect();
    let moved: Vec<PNote> = many
        .iter()
        .rev()
        .map(|n| note(n.tick + 1, n.pitch, n.duration))
        .collect();
    let (a, b) = (part(many), part(moved));
    let start = std::time::Instant::now();
    let got = notes(&a, &b);
    let took = start.elapsed();
    assert!(took.as_secs_f64() < 1.0, "{took:?}");
    assert_eq!(got.len(), 50_000);
    assert!(got.iter().all(|(x, y)| x.is_some() && y.is_some()));
}

/// The pairs of `regions` between base track `name` and its theirs row.
fn region_pairs(
    t: &Trip,
    th: &Theirs,
    m: &Matches,
    name: &str,
) -> Vec<(Option<usize>, Option<usize>)> {
    let b = base_track(t, name);
    let r = of_base(&m.tracks, b);
    let ti = r.theirs.expect("matched");
    let nb = normalised(&t.snapshot.projection, &t.snapshot);
    let ids = t.snapshot.identity.as_ref().unwrap();
    regions(
        (&nb.tracks[b], &ids.tracks[b]),
        (
            &th.projection.tracks[ti],
            &th.identity.tracks[ti],
            &th.origins,
        ),
    )
}

fn complete(p: &[(Option<usize>, Option<usize>)]) -> bool {
    p.iter().all(|(a, b)| a.is_some() && b.is_some())
}

#[test]
fn regions_match_by_tag_then_media_and_position() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let later = |p: &mut DpProject, _: &mut _| {
        clips(p, "t1a")[1].time += 1.0;
    };
    // Tagged: the moved region is still the same one.
    let (th, m) = matched(&t, &return_file(&t, Strip::Nothing, later));
    let got = region_pairs(&t, &th, &m, "Keys audio");
    assert_eq!(got, vec![(Some(0), Some(0)), (Some(1), Some(1))]);

    // Untagged: media and position; the moved one is new plus deleted.
    let (th, m) = matched(&t, &return_file(&t, Strip::Tags, |_, _| {}));
    assert!(complete(&region_pairs(&t, &th, &m, "Keys audio")));
    let (th, m) = matched(&t, &return_file(&t, Strip::Tags, later));
    let got = region_pairs(&t, &th, &m, "Keys audio");
    assert_eq!(
        got,
        vec![(Some(0), Some(0)), (Some(1), None), (None, Some(1))]
    );

    // Other media at the same place is another region.
    let (th, m) = matched(&t, &return_file(&t, Strip::Tags, |_, _| {}));
    let mut th = th;
    let ti = of_base(&m.tracks, base_track(&t, "Keys audio"))
        .theirs
        .unwrap();
    th.projection.tracks[ti].regions[0].media_hash = Some("hash-other".into());
    let got = region_pairs(&t, &th, &m, "Keys audio");
    assert_eq!(
        got,
        vec![(Some(0), None), (Some(1), Some(1)), (None, Some(0))]
    );
}

/// Every seconds value of `clip` (and its audio) on the 44 100 Hz grid,
/// its audio said to be at 44 100 Hz.
fn at_44k(clip: &mut DpClip) {
    let grid = |x: &mut f64| *x = (*x * 44_100.0).round() / 44_100.0;
    for v in [
        &mut clip.play_start,
        &mut clip.play_stop,
        &mut clip.fade_in,
        &mut clip.fade_out,
    ]
    .into_iter()
    .flatten()
    {
        grid(v);
    }
    if let Some(c) = clip.content.as_deref_mut()
        && let DpTimeline::Audio(a) = c
    {
        a.sample_rate = 44_100;
        grid(&mut a.duration);
    }
}

/// The fixture with a region of `length` frames from `offset` in a media
/// item of `offset + length` frames, exported, sent back at 44 100 Hz
/// without its tags, and matched: each pair's (base, theirs) region.
fn at_another_rate(
    offset: u64,
    length: u64,
) -> Vec<(
    core_io::dawproject::record::PRegion,
    core_io::dawproject::record::PRegion,
)> {
    let (doc, seq) = fixture();
    let mut b = Builder::from_doc(doc);
    let keys = b
        .doc()
        .sequence(seq)
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.name == "Keys")
        .unwrap()
        .id;
    let odd = media(&mut b, "odd", offset + length, 2);
    let none = Fade {
        length_frames: 0,
        shape: FadeShape::Linear,
    };
    b.region(seq, keys, 4 * BAR, odd, offset, length, 0.0, none, none)
        .unwrap();
    let doc = b.finish();
    assert_eq!(doc.settings().format.sample_rate, 48_000);
    let t = trip(&doc, seq);
    let path = return_file(&t, Strip::Tags, |p, _| {
        for c in clips(p, "t1a") {
            at_44k(c);
        }
    });
    let (th, m) = matched(&t, &path);
    each_once(&t, &th, &m);
    let got = region_pairs(&t, &th, &m, "Keys audio");
    assert_eq!(got.len(), 3, "{got:?}");
    assert!(complete(&got), "{got:?}");
    let b = base_track(&t, "Keys audio");
    let ti = of_base(&m.tracks, b).theirs.unwrap();
    got.into_iter()
        .map(|(x, y)| {
            (
                t.snapshot.projection.tracks[b].regions[x.unwrap()].clone(),
                th.projection.tracks[ti].regions[y.unwrap()].clone(),
            )
        })
        .collect()
}

#[test]
fn audio_at_another_rate_matches_within_a_frame() {
    // OFFSET and LENGTH do not survive the 44.1 kHz grid: the region comes
    // back one frame shorter, and still matches.
    let pairs = at_another_rate(OFFSET, LENGTH);
    for (x, y) in &pairs {
        assert!(x.length.abs_diff(y.length) <= FRAMES, "{x:?} vs {y:?}");
    }
    assert_eq!(
        pairs
            .iter()
            .filter(|(x, y)| x.length == y.length + 1)
            .count(),
        1,
        "{pairs:?}"
    );
}

#[test]
fn rounded_levels_are_unchanged() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let round = |v: &mut core_io::dawproject::DpRealParam| {
        if v.unit == DpUnit::Linear
            && let Some(x) = v.value.as_mut()
            && *x > 0.0
        {
            let db = (20.0 * x.log10() * 100.0).round() / 100.0 + 0.004;
            *x = 10f64.powf(db / 20.0);
        }
    };
    let path = return_file(&t, Strip::Nothing, |p, _| {
        for tr in tracks(p) {
            if let Some(c) = tr.channel.as_mut() {
                if let Some(v) = c.volume.as_mut() {
                    round(v);
                }
                for s in &mut c.sends {
                    round(&mut s.volume);
                }
            }
        }
    });
    let (th, m) = matched(&t, &path);
    each_once(&t, &th, &m);
    let b = &t.snapshot.projection;
    for r in &m.strips {
        let (x, y) = (
            &b.strips[r.base.unwrap()],
            &th.projection.strips[r.theirs.unwrap()],
        );
        assert_eq!(r.by, MatchedBy::Id);
        assert!(
            (x.gain_db - y.gain_db).abs() <= LEVEL_DB,
            "{}: {} vs {}",
            x.name,
            x.gain_db,
            y.gain_db
        );
        for (s, u) in x.sends.iter().zip(&y.sends) {
            assert!(
                (s.1 - u.1).abs() <= LEVEL_DB,
                "{}: send {} vs {}",
                x.name,
                s.1,
                u.1
            );
        }
    }
}

#[test]
fn format_one_snapshots_match_ours_by_name_and_order() {
    let (doc, seq) = fixture();
    let mut t = trip(&doc, seq);
    t.snapshot.format = 1;
    t.snapshot.identity = None;
    t.snapshot.region_gain = None;
    let (th, m) = matched(&t, &return_file(&t, Strip::Nothing, |_, _| {}));
    each_once(&t, &th, &m);
    all_tracks_by(&m, MatchedBy::Name);
    let ours = ours_of(&t);
    for r in &m.tracks {
        let b = &t.snapshot.projection.tracks[r.base.unwrap()];
        assert_eq!(ours.0.tracks[r.ours.unwrap()].name, b.name);
    }
    let bus = t
        .snapshot
        .projection
        .strips
        .iter()
        .position(|s| s.name == "Reverb")
        .unwrap();
    let s = of_base(&m.strips, bus);
    assert!(s.theirs.is_some() && s.ours.is_some());
    assert_eq!(s.by, MatchedBy::Name);
}

/// A package holding only `project` (no record), in `dir`.
fn bare(dir: &Path, project: &str) -> PathBuf {
    let path = dir.join("Hostile.dawproject");
    let mut z = ZipWriter::new(std::fs::File::create(&path).unwrap());
    z.add("project.xml", EntryMethod::Stored, project.as_bytes())
        .unwrap();
    z.finish().unwrap();
    path
}

/// The file at `path` as theirs, its IDs trusted as `ids`.
fn theirs_trusting(t: &Trip, path: &Path, ids: &BTreeMap<String, ObjectRef>) -> Theirs {
    let pkg = open(path, &PackageLimits::default(), &never()).unwrap();
    let yes = |_: PluginFormat, _: &str| true;
    let imp = map(
        &pkg,
        &BTreeMap::new(),
        &ImportEnv {
            installed: &yes,
            plist: &TestPlist,
            rate: 48_000,
            ids: Some(ids),
        },
        &never(),
    )
    .unwrap();
    theirs(
        imp,
        t.base_doc.settings().format,
        t.snapshot.origin,
        &never(),
    )
    .unwrap()
}

#[test]
fn hostile_tags_never_match_twice() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let ids = t.snapshot.identity.as_ref().unwrap();
    let bass = &ids.tracks[base_track(&t, "Bass")];
    let path = bare(
        t.dir.path(),
        &hostile(10_000, bass.lane, bass.uses[0].event),
    );
    let start = std::time::Instant::now();
    let untrusted = theirs_of(&t, &path);
    let trusted = theirs_trusting(&t, &path, &t.snapshot.ids);
    for (th, winner) in [(untrusted, "A"), (trusted, "E")] {
        let m = match_all(&t.snapshot, &th, &ours_of(&t), &never()).expect("matches");
        each_once(&t, &th, &m);
        // The first tag wins without trust; the trusted ID wins with it.
        let r = of_base(&m.tracks, base_track(&t, "Bass"));
        assert_eq!(r.theirs, Some(theirs_track(&th, winner)), "{winner}");
        assert!(!m.heuristic, "the file kept tags");
        let nb = normalised(&t.snapshot.projection, &t.snapshot);
        for r in &m.tracks {
            let (Some(b), Some(ti)) = (r.base, r.theirs) else {
                continue;
            };
            let tt = &th.projection.tracks[ti];
            for (pairs, na, nt) in [
                (
                    notes(&nb.tracks[b], tt),
                    nb.tracks[b].notes_played.len(),
                    tt.notes_played.len(),
                ),
                (
                    regions(
                        (&nb.tracks[b], &ids.tracks[b]),
                        (tt, &th.identity.tracks[ti], &th.origins),
                    ),
                    nb.tracks[b].regions.len(),
                    tt.regions.len(),
                ),
            ] {
                let mut sa = vec![0; na];
                let mut st = vec![0; nt];
                for (x, y) in pairs {
                    if let Some(x) = x {
                        sa[x] += 1;
                    }
                    if let Some(y) = y {
                        st[y] += 1;
                    }
                }
                assert!(sa.iter().chain(&st).all(|c| *c == 1));
            }
        }
    }
    let took = start.elapsed();
    assert!(took.as_secs() < 2, "{took:?}");
}

#[test]
fn harness_strips_what_it_says() {
    // The record goes with Record and Both; tags with Tags and Both.
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    for (strip, record, tags) in [
        (Strip::Nothing, true, true),
        (Strip::Record, false, true),
        (Strip::Tags, true, false),
        (Strip::Both, false, false),
    ] {
        let path = return_file(&t, strip, |_, _| {});
        let pkg = open(&path, &PackageLimits::default(), &never()).unwrap();
        assert_eq!(pkg.ret.record.is_some(), record, "{strip:?}");
        if let Some(r) = &pkg.ret.record {
            assert_eq!(r.export_id, EXPORT);
        }
        assert_eq!(pkg.ret.tagged_tracks > 0, tags, "{strip:?}");
    }
    assert_eq!(
        merge::without_tags(r#"comment="vz:t1 vz:s2""#),
        r#"comment="""#
    );
    assert_eq!(merge::without_tags("a vz:e3 b vz:x"), "a b vz:x");
}

#[test]
fn notes_on_one_pitch_and_tick_are_not_quadratic() {
    // The reviewer's shape: 50 000 notes on one pitch at one tick, whose
    // lengths never match, so nothing pairs and nothing may be scanned
    // twice.
    let a = part((0..50_000).map(|_| note(0, 60, 10)).collect());
    let b = part((0..50_000).map(|_| note(0, 60, 100)).collect());
    let start = std::time::Instant::now();
    let got = notes(&a, &b);
    let took = start.elapsed();
    assert_eq!(got.len(), 100_000);
    assert!(got.iter().all(|(x, y)| x.is_none() || y.is_none()));
    // Linear work takes well under 100 ms in debug; quadratic takes minutes.
    assert!(took.as_secs_f64() < 2.0, "{took:?}");
}

fn region(tick: i64, length: u64, hash: Option<&str>, stem: Option<&str>) -> PRegion {
    PRegion {
        tick,
        media_hash: hash.map(str::to_owned),
        offset: 0,
        length,
        fade_in: 0,
        fade_out: 0,
        muted: false,
        name: None,
        crossfade: false,
        gain_db: 0.0,
        stem: stem.map(str::to_owned),
    }
}

fn audio_part(regions: Vec<PRegion>) -> PTrack {
    PTrack {
        name: "Audio".into(),
        notes: false,
        audio: true,
        strip: None,
        notes_played: Vec::new(),
        midi: Vec::new(),
        regions,
        automation: Vec::new(),
    }
}

/// `regions` on two untagged parts.
fn untagged(a: &PTrack, b: &PTrack) -> Vec<(Option<usize>, Option<usize>)> {
    let none = TrackIds::default();
    regions((a, &none), (b, &none, &Origins::default()))
}

#[test]
fn regions_without_a_hash_match_by_file_stem() {
    // Base: a region with gain under `Apply` (no hash), and an offline one.
    let a = audio_part(vec![
        region(0, 1_000, None, Some("loop")),
        region(5_000, 1_000, None, Some("kit")),
        region(9_000, 1_000, None, None),
    ]);
    let b = audio_part(vec![
        // Its gain copy: another hash, the same stem.
        region(0, 1_000, Some("hash-loop@-6"), Some("loop")),
        // Other media at the same place.
        region(5_000, 1_000, None, Some("snare")),
        // Nothing known about either: never a wildcard.
        region(9_000, 1_000, None, None),
    ]);
    assert_eq!(
        untagged(&a, &b),
        vec![
            (Some(0), Some(0)),
            (Some(1), None),
            (Some(2), None),
            (None, Some(1)),
            (None, Some(2)),
        ]
    );
    // Two hashes decide, whatever the stems say.
    let a = audio_part(vec![region(0, 1_000, Some("h1"), Some("loop"))]);
    let b = audio_part(vec![region(0, 1_000, Some("h2"), Some("loop"))]);
    assert_eq!(untagged(&a, &b), vec![(Some(0), None), (None, Some(0))]);
    let b = audio_part(vec![region(1, 1_001, Some("h1"), Some("other"))]);
    assert_eq!(untagged(&a, &b), vec![(Some(0), Some(0))]);
    // A hashed base region and an offline theirs one go by the stem.
    let b = audio_part(vec![region(0, 1_000, None, Some("loop"))]);
    assert_eq!(untagged(&a, &b), vec![(Some(0), Some(0))]);
}

#[test]
fn regions_on_one_tick_are_not_quadratic() {
    let n = 50_000;
    let a = audio_part(
        (0..n)
            .map(|_| region(0, 10, Some("h"), Some("s")))
            .collect(),
    );
    let b = audio_part(
        (0..n)
            .map(|_| region(0, 100, Some("h"), Some("s")))
            .collect(),
    );
    let start = std::time::Instant::now();
    let got = untagged(&a, &b);
    let took = start.elapsed();
    assert_eq!(got.len(), 2 * n);
    assert!(took.as_secs_f64() < 2.0, "{took:?}");
}

#[test]
fn a_bus_never_takes_a_tracks_strip() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let base = &t.snapshot.projection;
    let bass_strip = base.tracks[base_track(&t, "Bass")].strip.unwrap();
    let reverb = base.strips.iter().position(|s| s.name == "Reverb").unwrap();
    // The other DAW deleted Bass and renamed the bus, which now carries
    // Bass's strip tag.
    let path = return_file(&t, Strip::Record, |p, _| {
        p.structure.retain(
            |i| !matches!(i, DpStructureItem::Track(t) if t.named.name.as_deref() == Some("Bass")),
        );
        clips(p, "t0").clear();
        let r = track(p, "Reverb");
        r.named.name = Some("Hall".into());
        let c = r.channel.as_mut().unwrap();
        c.named.name = Some("Hall".into());
        c.named.comment = Some("vz:s1".into());
    });
    let (th, m) = matched(&t, &path);
    each_once(&t, &th, &m);
    let hall = th
        .projection
        .strips
        .iter()
        .position(|s| s.name == "Hall")
        .unwrap();
    assert_eq!(
        of_base(&m.strips, bass_strip).theirs,
        None,
        "Bass's strip is deleted there"
    );
    assert_eq!(
        of_base(&m.strips, reverb).theirs,
        None,
        "Reverb is deleted there"
    );
    let new = m.strips.iter().find(|r| r.theirs == Some(hall)).unwrap();
    assert_eq!(
        (new.base, new.by),
        (None, MatchedBy::Unmatched),
        "Hall is new"
    );
}

#[test]
fn a_new_track_never_takes_a_bus() {
    let (doc, seq) = fixture();
    let t = trip(&doc, seq);
    let base = &t.snapshot.projection;
    let reverb = base.strips.iter().position(|s| s.name == "Reverb").unwrap();
    // A new audio track, above the bus, whose channel carries the bus's tag.
    let path = return_file(&t, Strip::Record, |p, _| {
        let mut new = track(p, "Keys audio").clone();
        new.named.id = Some("tnew".into());
        new.named.name = Some("New".into());
        new.named.comment = None;
        let c = new.channel.as_mut().unwrap();
        c.named.id = Some("cnew".into());
        c.named.name = Some("New".into());
        c.named.comment = Some("vz:s4".into());
        for param in [
            c.mute.as_deref_mut().map(|m| &mut m.named),
            c.pan.as_deref_mut().map(|m| &mut m.named),
            c.volume.as_deref_mut().map(|m| &mut m.named),
        ]
        .into_iter()
        .flatten()
        {
            param.id = None;
        }
        let at = p
            .structure
            .iter()
            .position(|i| matches!(i, DpStructureItem::Track(t) if t.named.name.as_deref() == Some("Reverb")))
            .unwrap();
        p.structure.insert(at, DpStructureItem::Track(new));
    });
    let (th, m) = matched(&t, &path);
    each_once(&t, &th, &m);
    let new_track = theirs_track(&th, "New");
    let new_strip = th.projection.tracks[new_track].strip.unwrap();
    assert_eq!(th.projection.strips[new_strip].name, "New");
    let row = m
        .strips
        .iter()
        .find(|r| r.theirs == Some(new_strip))
        .unwrap();
    assert_eq!(
        row.base, None,
        "the new track's strip is new: {:?}",
        m.strips
    );
    let r = of_base(&m.strips, reverb);
    assert_eq!(
        th.projection.strips[r.theirs.expect("the bus")].name,
        "Reverb"
    );
}

#[test]
fn an_exact_length_wins_over_a_near_one() {
    // b's 9 is within a tick of both of a's notes; a's 10 must take b's 10.
    let a = part(vec![note(0, 60, 10), note(0, 60, 9)]);
    let b = part(vec![note(0, 60, 9), note(0, 60, 10)]);
    assert_eq!(notes(&a, &b), vec![(Some(0), Some(1)), (Some(1), Some(0))]);
    let a = audio_part(vec![region(0, 10, Some("h"), None)]);
    let b = audio_part(vec![
        region(0, 9, Some("h"), None),
        region(0, 10, Some("h"), None),
    ]);
    assert_eq!(untagged(&a, &b), vec![(Some(0), Some(1)), (None, Some(0))]);
}

#[test]
fn a_region_with_gain_comes_back_with_its_stem() {
    // A real `Apply` export of a region at −6 dB: its audio comes back as a
    // gain copy, with another hash and the same stem.
    let (doc, seq) = fixture();
    let mut b = Builder::from_doc(doc);
    let keys = b
        .doc()
        .sequence(seq)
        .unwrap()
        .lanes
        .iter()
        .find(|l| l.name == "Keys")
        .unwrap()
        .id;
    let m = media(&mut b, "gained", 48_000, 2);
    let none = Fade {
        length_frames: 0,
        shape: FadeShape::Linear,
    };
    b.region(seq, keys, 4 * BAR, m, 0, 48_000, -6.0, none, none)
        .unwrap();
    let t = trip(&b.finish(), seq);
    let (th, mt) = matched(&t, &return_file(&t, Strip::Tags, |_, _| {}));
    each_once(&t, &th, &mt);
    let bi = base_track(&t, "Keys audio");
    let ti = of_base(&mt.tracks, bi).theirs.unwrap();
    let nb = normalised(&t.snapshot.projection, &t.snapshot);
    let at = |rs: &[PRegion]| rs.iter().position(|r| r.tick == 4 * BAR).unwrap();
    let (base_r, theirs_r) = (
        at(&nb.tracks[bi].regions),
        at(&th.projection.tracks[ti].regions),
    );
    let x = &nb.tracks[bi].regions[base_r];
    let y = &th.projection.tracks[ti].regions[theirs_r];
    assert_eq!(x.stem.as_deref(), Some("gained"));
    assert_eq!(y.stem, x.stem, "the same stem on both sides");
    assert_eq!(x.media_hash, None, "Apply: no hash for a gain copy");
    assert_eq!(y.media_hash.as_deref(), Some("hash-gained@-6"));
    let got = region_pairs(&t, &th, &mt, "Keys audio");
    assert!(complete(&got), "{got:?}");
    assert!(got.contains(&(Some(base_r), Some(theirs_r))), "{got:?}");

    // With theirs' hash gone too (offline), the stem still pairs it.
    let mut th = th;
    th.projection.tracks[ti].regions[theirs_r].media_hash = None;
    let got = region_pairs(&t, &th, &mt, "Keys audio");
    assert!(complete(&got), "{got:?}");
    assert!(got.contains(&(Some(base_r), Some(theirs_r))), "{got:?}");
}
