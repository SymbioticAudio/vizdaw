//! Audio regions' waveforms (plan 8c Task 4; Swift `MediaModel.waveform`,
//! `ArrangementView.drawRegions`, `RegionA11y`): what is asked per zoom
//! and slice, offline media, overview news, media changes, region edits,
//! the generation guard and reads that never run on the calling thread.

use std::sync::Arc;
use std::time::Duration;

use vizdaw_app::arrangement::{MeterBars, region_a11y_label, region_name};
use vizdaw_app::region_waves::{RegionWant, RegionWaves, Slice, WaveRequest, WaveState};
use vizdaw_app::testing::{FakeSession, ScriptedWave, demo_fake};
use vizdaw_app::{DocId, DocumentStore};
use vizdaw_ffi::events::{EventKindRow, EventRow, FadeRow, FadeShapeRow, MeterEntryRow};
use vizdaw_ffi::selection::TickRange;
use vizdaw_ffi::{MediaOriginRow, MediaRow, OfflineReasonRow, OverviewStateRow};

const BAR: i64 = 61_440;

fn media(id: u64, offline: Option<OfflineReasonRow>) -> MediaRow {
    MediaRow {
        id,
        name: format!("m{id}.wav"),
        path: format!("Audio/m{id}.caf"),
        channels: 1,
        frames: 48_000,
        sample_rate: 48_000,
        origin: MediaOriginRow::Imported,
        original_name: None,
        offline,
        uses: 1,
        used_percent: 100.0,
        peak: None,
    }
}

fn region(id: u64, tick: i64, duration: i64, media: u64) -> EventRow {
    let mut r = EventRow::blank(id, tick, EventKindRow::AudioRegion);
    r.duration = duration;
    r.media = Some(media);
    r.frames = 48_000;
    r
}

fn span(start: i64, end: i64) -> TickRange {
    TickRange { start, end }
}

fn wave(state: OverviewStateRow) -> ScriptedWave {
    ScriptedWave {
        state,
        channels: 1,
        min: -0.5,
        max: 0.5,
    }
}

fn waves(fake: &Arc<FakeSession>) -> RegionWaves {
    RegionWaves::new(fake.clone())
}

/// Runs `r` on another thread and lands it.
fn run_off(w: &mut RegionWaves, r: WaveRequest) -> bool {
    let reply = std::thread::spawn(move || r.run()).join().unwrap();
    w.land(reply)
}

/// Columns follow the visible width (rounded up) and cap at 8192; a zoom
/// change asks again at the new ticks per point; a scroll at the same
/// zoom asks only for the new slice.
#[test]
fn the_request_follows_the_zoom_and_the_slice() {
    let fake = Arc::new(demo_fake());
    let m = vec![media(5, None)];
    let mut w = waves(&fake);
    let r = region(1, 0, 4 * BAR, 5);
    w.zoom(384.);
    let req = w.want(&r, &m, span(0, 4 * BAR), 640.4).expect("asked");
    assert_eq!(
        (req.slice.from_tick, req.slice.to_tick, req.slice.columns),
        (0, 4 * BAR, 641)
    );
    assert_eq!(req.event, 1);
    assert_eq!(req.media, 5);
    assert!(run_off(&mut w, req));
    assert!(w.want(&r, &m, span(0, 4 * BAR), 640.4).is_none(), "held");
    // Wider than the core's limit: capped.
    let wide = region(2, 0, 100_000 * BAR, 5);
    let req = w.want(&wide, &m, span(0, 9000 * 384), 9000.).unwrap();
    assert_eq!(req.slice.columns, 8192);
    // A scroll at the same zoom: only the new slice is asked.
    assert!(w.want(&r, &m, span(0, 4 * BAR), 640.4).is_none());
    let req = w
        .want(&r, &m, span(BAR, 4 * BAR), 480.)
        .expect("the new slice");
    assert_eq!((req.slice.from_tick, req.slice.columns), (BAR, 480));
    assert!(run_off(&mut w, req));
    assert!(w.want(&r, &m, span(BAR, 4 * BAR), 480.).is_none());
    // A zoom change drops the rows and asks at the new ticks per point.
    assert_eq!(w.look(&r, &m, 384.).state, WaveState::Pending);
    w.zoom(192.);
    assert!(w.is_empty(), "dropped");
    assert_eq!(w.look(&r, &m, 192.).state, WaveState::None);
    assert_eq!(w.look(&r, &m, 384.).state, WaveState::None, "old zoom");
    let req = w.want(&r, &m, span(0, 4 * BAR), 1280.).unwrap();
    assert_eq!(req.slice.columns, 1280);
    assert_eq!(fake.count("region_waveform"), 2, "only the reads run");
}

/// Nothing is asked for an empty slice, a zero width or a region without
/// media.
#[test]
fn nothing_is_asked_for_nothing_to_draw() {
    let fake = Arc::new(demo_fake());
    let m = vec![media(5, None)];
    let mut w = waves(&fake);
    w.zoom(384.);
    let r = region(1, 0, BAR, 5);
    assert!(w.want(&r, &m, span(BAR, BAR), 10.).is_none());
    assert!(w.want(&r, &m, span(0, BAR), 0.).is_none());
    assert!(w.want(&r, &m, span(0, BAR), f32::NAN).is_none());
    let mut bare = r.clone();
    bare.media = None;
    assert!(w.want(&bare, &m, span(0, BAR), 10.).is_none());
    assert_eq!(w.look(&bare, &m, 384.).state, WaveState::None);
}

/// Offline media is never asked and reads `offline`.
#[test]
fn offline_media_asks_nothing() {
    let fake = Arc::new(demo_fake());
    let m = vec![media(5, Some(OfflineReasonRow::Missing))];
    let mut w = waves(&fake);
    w.zoom(384.);
    let r = region(1, 0, BAR, 5);
    assert!(w.want(&r, &m, span(0, BAR), 160.).is_none());
    assert_eq!(w.look(&r, &m, 384.).state, WaveState::Offline);
    assert_eq!(fake.count("region_waveform"), 0);
}

/// `Pending` is kept as pending and asked again once after the overview
/// news, not before; `Coarse` is drawn and asked again after it too.
#[test]
fn pending_asks_again_once_after_overview_news() {
    let fake = Arc::new(demo_fake());
    let m = vec![media(5, None)];
    let mut w = waves(&fake);
    w.zoom(384.);
    let r = region(1, 0, BAR, 5);
    let req = w.want(&r, &m, span(0, BAR), 160.).unwrap();
    assert_eq!(w.look(&r, &m, 384.).state, WaveState::Pending, "asked");
    assert!(w.want(&r, &m, span(0, BAR), 160.).is_none(), "in flight");
    assert!(run_off(&mut w, req));
    assert_eq!(w.look(&r, &m, 384.).state, WaveState::Pending);
    for _ in 0..3 {
        assert!(w.want(&r, &m, span(0, BAR), 160.).is_none(), "no busy loop");
    }
    fake.set_region_waveform(1, wave(OverviewStateRow::Coarse));
    w.overviews_changed();
    let req = w.want(&r, &m, span(0, BAR), 160.).expect("asked again");
    assert!(w.want(&r, &m, span(0, BAR), 160.).is_none(), "once");
    assert!(run_off(&mut w, req));
    let look = w.look(&r, &m, 384.);
    assert_eq!(look.state, WaveState::Coarse);
    assert_eq!(look.held.unwrap().rows.max[0], 0.5, "coarse rows drawn");
    assert!(w.want(&r, &m, span(0, BAR), 160.).is_none());
    fake.set_region_waveform(1, wave(OverviewStateRow::Ready));
    w.overviews_changed();
    let req = w
        .want(&r, &m, span(0, BAR), 160.)
        .expect("coarse asks again");
    assert!(run_off(&mut w, req));
    assert_eq!(w.look(&r, &m, 384.).state, WaveState::Ready);
    // Ready stays; news does not ask it again.
    w.overviews_changed();
    assert!(w.want(&r, &m, span(0, BAR), 160.).is_none());
    assert_eq!(fake.count("region_waveform"), 3);
}

/// A failed read draws nothing and waits for news to try again.
#[test]
fn a_failed_read_waits_for_news() {
    let fake = Arc::new(demo_fake());
    let m = vec![media(5, None)];
    let mut w = waves(&fake);
    w.zoom(384.);
    let r = region(1, 0, BAR, 5);
    fake.set_call_error(
        "region_waveform",
        Some(vizdaw_ffi::VizError::Model {
            message: "no".into(),
        }),
    );
    let req = w.want(&r, &m, span(0, BAR), 160.).unwrap();
    assert!(run_off(&mut w, req));
    assert_eq!(w.look(&r, &m, 384.).state, WaveState::None);
    assert!(
        w.want(&r, &m, span(0, BAR), 160.).is_none(),
        "no retry loop"
    );
    w.overviews_changed();
    assert!(w.want(&r, &m, span(0, BAR), 160.).is_some());
}

/// An edit to a region row.
type Edit = Box<dyn Fn(&mut EventRow)>;

/// A region edit (gain, fades, offset, length, media) drops its rows; the
/// media changing drops them all.
#[test]
fn an_edit_or_a_media_change_drops_the_rows() {
    let fake = Arc::new(demo_fake());
    fake.set_region_waveform(1, wave(OverviewStateRow::Ready));
    let m = vec![media(5, None)];
    let mut w = waves(&fake);
    w.zoom(384.);
    let r = region(1, 0, BAR, 5);
    let req = w.want(&r, &m, span(0, BAR), 160.).unwrap();
    assert!(run_off(&mut w, req));
    assert_eq!(w.look(&r, &m, 384.).state, WaveState::Ready);
    let edits: Vec<Edit> = vec![
        Box::new(|r| r.gain_db = 3.),
        Box::new(|r| {
            r.fade_in = FadeRow {
                length: 100,
                shape: FadeShapeRow::Linear,
            }
        }),
        Box::new(|r| {
            r.fade_out = FadeRow {
                length: 100,
                shape: FadeShapeRow::Linear,
            }
        }),
        Box::new(|r| r.source_offset = 10),
        Box::new(|r| r.frames = 100),
        Box::new(|r| r.duration = 2 * BAR),
    ];
    for edit in edits {
        let mut e = r.clone();
        edit(&mut e);
        assert_eq!(w.look(&e, &m, 384.).state, WaveState::None, "{e:?}");
        let req = w.want(&e, &m, span(0, BAR), 160.).expect("asked again");
        // The old row's reply is not the edited region's.
        assert!(run_off(&mut w, req));
        assert_eq!(w.look(&e, &m, 384.).state, WaveState::Ready);
        let req = w.want(&r, &m, span(0, BAR), 160.).unwrap();
        assert!(run_off(&mut w, req));
    }
    w.media_changed();
    assert!(w.is_empty());
    assert_eq!(w.look(&r, &m, 384.).state, WaveState::None);
}

/// A reply asked before a region edit never lands on the edited region.
#[test]
fn a_reply_from_before_an_edit_is_dropped() {
    let fake = Arc::new(demo_fake());
    fake.set_region_waveform(1, wave(OverviewStateRow::Ready));
    let m = vec![media(5, None)];
    let mut w = waves(&fake);
    w.zoom(384.);
    let r = region(1, 0, BAR, 5);
    let old = w.want(&r, &m, span(0, BAR), 160.).unwrap();
    let mut e = r.clone();
    e.gain_db = -6.;
    let new = w.want(&e, &m, span(0, BAR), 160.).unwrap();
    assert!(!run_off(&mut w, old), "asked before the edit");
    assert_eq!(w.look(&e, &m, 384.).state, WaveState::Pending);
    assert!(run_off(&mut w, new));
    assert_eq!(w.look(&e, &m, 384.).state, WaveState::Ready);
}

/// A store over `fake` with its document open.
fn store_with(fake: &Arc<FakeSession>) -> (DocumentStore, DocId) {
    let shared = fake.clone();
    let mut store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(move || Ok(shared.clone())),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let id = store.new_demo().unwrap();
    (store, id)
}

fn want(r: &EventRow, visible: TickRange, width: f32) -> RegionWant {
    RegionWant {
        region: r.clone(),
        visible,
        width,
    }
}

/// No read runs on the calling thread: the store hands the read out; an
/// older read released after a newer one landed changes nothing.
#[test]
fn reads_run_off_the_calling_thread_and_the_newest_wins() {
    let fake = Arc::new(demo_fake());
    fake.set_media(vec![media(5, None)]);
    let (mut store, id) = store_with(&fake);
    let r = region(1, 0, BAR, 5);
    let hold = fake.hold_region_waveform();
    assert_eq!(
        store.want_region_waves(id, 384., &[want(&r, span(0, BAR), 160.)]),
        1
    );
    assert_eq!(fake.count("region_waveform"), 0, "no read in the call");
    let older = store.take_reads().pop().expect("a read to run");
    let older = std::thread::spawn(move || older.run());
    assert!(hold.wait_started(Duration::from_secs(5)));
    let look = |s: &DocumentStore| s.document(id).unwrap().region_wave(&r, 384.);
    assert_eq!(look(&store).state, WaveState::Pending, "answers while held");
    // Overview news: asked again, and the newer read lands first.
    fake.set_region_waveform(1, wave(OverviewStateRow::Ready));
    fake.set_idle_overviews_changed();
    assert!(store.document_mut(id).unwrap().idle(), "news arrived");
    assert_eq!(
        store.want_region_waves(id, 384., &[want(&r, span(0, BAR), 160.)]),
        1
    );
    let newer = store.take_reads().pop().unwrap();
    let landed = std::thread::spawn(move || newer.run()).join().unwrap();
    store.land(landed);
    assert_eq!(look(&store).state, WaveState::Ready);
    // The older one (it took `Pending`), released last, changes nothing.
    hold.release();
    store.land(older.join().unwrap());
    assert_eq!(look(&store).state, WaveState::Ready);
    let me = std::thread::current().id();
    let threads = fake.region_waveform_threads();
    assert_eq!(threads.len(), 2);
    assert!(threads.iter().all(|t| *t != me), "read off the UI thread");
    assert_eq!(
        fake.region_waveform_asks(),
        vec![(1, 0, BAR, 160), (1, 0, BAR, 160)]
    );
}

/// The idle news's media change drops the document's rows; an offline
/// region is never asked and reads `offline`.
#[test]
fn the_documents_media_news_drops_the_rows() {
    let fake = Arc::new(demo_fake());
    fake.set_region_waveform(1, wave(OverviewStateRow::Ready));
    fake.set_media(vec![media(5, None)]);
    let (mut store, id) = store_with(&fake);
    let r = region(1, 0, BAR, 5);
    store.want_region_waves(id, 384., &[want(&r, span(0, BAR), 160.)]);
    store.run_reads();
    let look = |s: &DocumentStore| s.document(id).unwrap().region_wave(&r, 384.).state;
    assert_eq!(look(&store), WaveState::Ready);
    assert_eq!(
        store.want_region_waves(id, 384., &[want(&r, span(0, BAR), 160.)]),
        0
    );
    fake.set_media(vec![media(5, Some(OfflineReasonRow::Missing))]);
    fake.set_idle_media_changed();
    store.document_mut(id).unwrap().idle();
    assert_eq!(look(&store), WaveState::Offline);
    assert_eq!(
        store.want_region_waves(id, 384., &[want(&r, span(0, BAR), 160.)]),
        0
    );
    assert!(
        store.document(id).unwrap().region_waves().is_empty(),
        "dropped"
    );
    fake.set_media(vec![media(5, None)]);
    fake.set_idle_media_changed();
    store.document_mut(id).unwrap().idle();
    assert_eq!(look(&store), WaveState::None);
    assert_eq!(
        store.want_region_waves(id, 384., &[want(&r, span(0, BAR), 160.)]),
        1
    );
    store.run_reads();
    assert_eq!(look(&store), WaveState::Ready);
    assert_eq!(fake.count("region_waveform"), 2);
}

/// Swift `RegionEditingTests.regionLabelsSayWhereWhatAndHowLoud` and
/// `aRegionsNameComesBeforeItsFilesName`.
#[test]
fn region_labels_say_where_what_and_how_loud() {
    let bars = MeterBars::new(&[MeterEntryRow {
        tick: 0,
        num: 4,
        den: 4,
        sharps: 0,
        minor: false,
    }]);
    let mut r = region(1, 0, BAR / 2, 5);
    r.fade_in = FadeRow {
        length: 10,
        shape: FadeShapeRow::Linear,
    };
    r.gain_db = 3.;
    assert_eq!(
        region_a11y_label(&r, "kick", "Audio 3", &bars, false),
        "Region kick on Audio 3, bar 1, gain +3.0 dB, fades"
    );
    r.duration = 2 * BAR;
    r.fade_in.length = 0;
    r.mute = true;
    assert_eq!(
        region_a11y_label(&r, "kick", "Audio 3", &bars, true),
        "Region kick on Audio 3, bars 1 to 2, gain +3.0 dB, offline, muted"
    );
    r.mute = false;
    r.gain_db = -6.5;
    r.crossfade = true;
    r.fade_in.length = 10;
    assert_eq!(
        region_a11y_label(&r, "kick", "Audio 3", &bars, false),
        "Region kick on Audio 3, bars 1 to 2, gain -6.5 dB, crossfade"
    );
    r.gain_db = 0.01;
    assert!(region_a11y_label(&r, "kick", "Audio 3", &bars, false).contains("gain 0.0 dB"));
    let mut n = region(1, 0, BAR, 5);
    assert_eq!(region_name(&n, Some("kick.wav")), "kick.wav");
    assert_eq!(region_name(&n, None), "audio");
    n.region_name = Some("Take 3".into());
    assert_eq!(region_name(&n, Some("kick.wav")), "Take 3");
    n.region_name = Some(String::new());
    assert_eq!(region_name(&n, Some("kick.wav")), "kick.wav");
}

/// A new snapshot (a tempo edit moves the audio under the ticks) asks
/// every region again, once, while its rows stay drawn; render's
/// pre-check agrees with what is asked.
#[test]
fn a_snapshot_asks_again_keeping_the_rows() {
    let fake = Arc::new(demo_fake());
    fake.set_region_waveform(1, wave(OverviewStateRow::Ready));
    let m = vec![media(5, None)];
    let mut w = waves(&fake);
    w.zoom(384.);
    let r = region(1, 0, BAR, 5);
    let slice = Slice::of(span(0, BAR), 160.).unwrap();
    assert!(w.look(&r, &m, 384.).needs(slice));
    let req = w.want(&r, &m, span(0, BAR), 160.).unwrap();
    assert!(!w.look(&r, &m, 384.).needs(slice), "in flight");
    assert!(run_off(&mut w, req));
    assert!(!w.look(&r, &m, 384.).needs(slice), "held");
    w.document_changed();
    let look = w.look(&r, &m, 384.);
    assert_eq!(look.state, WaveState::Ready, "still drawn");
    assert!(look.held.is_some());
    assert!(look.needs(slice));
    let req = w.want(&r, &m, span(0, BAR), 160.).expect("asked again");
    assert!(w.want(&r, &m, span(0, BAR), 160.).is_none(), "once");
    assert!(run_off(&mut w, req));
    assert!(!w.look(&r, &m, 384.).needs(slice));
    let offline = vec![media(5, Some(OfflineReasonRow::Changed))];
    assert!(!w.look(&r, &offline, 384.).needs(slice), "offline: never");
}
