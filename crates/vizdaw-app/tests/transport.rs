mod support;

use std::sync::Arc;
use std::time::Duration;

use support::{FakeSession, demo_fake};
use vizdaw_app::transport::{PollResult, ReadoutTexts, TransportModel, TransportReadouts};
use vizdaw_app::{DocumentModel, DocumentStore};
use vizdaw_ffi::VizError;
use vizdaw_ffi::events::MeterEntryRow;
use vizdaw_ffi::snapshot::TransportState;

fn store() -> DocumentStore {
    DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(demo_fake()))),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    )
}

fn setup(f: &Arc<FakeSession>) -> (DocumentModel, TransportModel) {
    let doc = DocumentModel::new(f.clone()).unwrap();
    let t = TransportModel::new(&doc).unwrap();
    (doc, t)
}

#[test]
fn no_device_on_play_becomes_an_alert_and_stays_stopped() {
    let f = Arc::new(demo_fake());
    f.set_play_error(Some(VizError::NoDevice {
        message: "no audio output device".into(),
    }));
    let (doc, mut t) = setup(&f);
    let mut s = store();
    t.play_stop(&doc, &mut s);
    let alert = s.alert().unwrap();
    assert_eq!(alert.title, "Play Failed");
    assert_eq!(alert.message, "No audio output: no audio output device");
    assert!(!t.is_playing());
}

#[test]
fn play_stop_toggles_and_poll_interval_follows() {
    let f = Arc::new(demo_fake());
    let (doc, mut t) = setup(&f);
    let mut s = store();
    assert_eq!(t.poll_interval(), Duration::from_millis(250));
    t.play_stop(&doc, &mut s);
    assert!(t.is_playing());
    assert_eq!(t.state().seq, Some(3));
    assert_eq!(t.poll_interval(), Duration::from_millis(33));
    t.play_stop(&doc, &mut s);
    assert!(!t.is_playing());
    assert!(s.alert().is_none());
}

#[test]
fn failing_polls_raise_one_alert() {
    let f = Arc::new(demo_fake());
    let (_doc, mut t) = setup(&f);
    let mut s = store();
    f.set_transport_error(Some(VizError::Internal {
        message: "boom".into(),
    }));
    assert_eq!(t.poll(&mut s), PollResult::Failed);
    let first = s.alert().unwrap().clone();
    t.poll(&mut s);
    assert_eq!(s.alert().unwrap().id, first.id);
    assert_eq!(first.message, "Internal error: boom");
    // Nothing queued behind it.
    s.take_alert();
    assert!(s.take_alert().is_none());
}

#[test]
fn a_busy_poll_keeps_the_last_transport_without_an_alert() {
    let f = Arc::new(demo_fake());
    f.set_transport(|t| {
        t.playing = true;
        t.tick = 480;
    });
    let (_doc, mut t) = setup(&f);
    let mut s = store();
    f.set_transport_error(Some(VizError::Busy {
        message: "plugin host is busy".into(),
    }));
    f.set_transport(|t| t.tick = 960);
    assert_eq!(t.poll(&mut s), PollResult::Busy);
    t.poll(&mut s);
    assert!(s.alert().is_none());
    assert_eq!(t.state().tick, 480);
    assert!(t.is_playing());
    f.set_transport_error(None);
    assert_eq!(t.poll(&mut s), PollResult::Updated);
    assert_eq!(t.state().tick, 960);
    assert!(s.alert().is_none());
}

#[test]
fn a_document_opened_while_busy_starts_stopped() {
    let f = Arc::new(demo_fake());
    f.set_transport_error(Some(VizError::Busy {
        message: "plugin host is busy".into(),
    }));
    let (_doc, mut t) = setup(&f);
    let mut s = store();
    assert!(s.alert().is_none());
    assert!(!t.is_playing());
    assert_eq!(t.state().tick, 0);
    f.set_transport_error(None);
    f.set_transport(|t| t.tick = 240);
    t.poll(&mut s);
    assert_eq!(t.state().tick, 240);
}

#[test]
fn stop_and_return_goes_back_to_where_playing_started() {
    let f = Arc::new(demo_fake());
    f.set_transport(|t| t.tick = 480);
    let (mut doc, mut t) = setup(&f);
    let mut s = store();
    // The Song is empty (length 0): the engine restarts at bar 1.
    t.play_stop(&doc, &mut s);
    assert_eq!(t.play_start(), Some(0));
    t.play_stop(&doc, &mut s);
    // Give the Song a length; playing then starts where the transport is.
    let mut next = f.snap();
    next.sequences[2].length_ticks = 1920;
    f.bump(next);
    doc.undo();
    f.set_transport(|t| t.tick = 480);
    t.poll(&mut s);
    t.play_stop(&doc, &mut s);
    assert_eq!(t.play_start(), Some(480));
    t.stop_and_return(&mut s).unwrap();
    assert!(!t.is_playing());
    assert!(f.calls().contains(&"locate(480)".to_string()));
    assert_eq!(t.state().tick, 480);
}

fn state(bbu: &str, seconds: f64, tps: f64, tick: i64) -> TransportState {
    let f = demo_fake();
    f.set_transport(|t| {
        t.position_bbu = bbu.into();
        t.seconds = seconds;
        t.ticks_per_second = tps;
        t.tick = tick;
    });
    use vizdaw_app::DocSession;
    f.transport().unwrap()
}

fn st(tick: i64) -> TransportState {
    state("12.3.480", 22.875, 30_720.0, tick)
}

fn meter(entries: &[(i64, u8, u8)]) -> Vec<MeterEntryRow> {
    entries
        .iter()
        .map(|&(tick, num, den)| MeterEntryRow {
            tick,
            num,
            den,
            sharps: 0,
            minor: false,
        })
        .collect()
}

#[test]
fn the_readouts_read_the_position_time_tempo_and_meter() {
    let m = meter(&[(0, 4, 4)]);
    assert_eq!(
        TransportReadouts::texts(&st(0), Some(&m)),
        ReadoutTexts {
            bar: "12.3.480".into(),
            time: "0:22.875".into(),
            tempo: "120.00".into(),
            signature: "4/4".into()
        }
    );
    let slower = state("12.3.480", 22.875, 23_040.0, 0);
    assert_eq!(TransportReadouts::texts(&slower, Some(&m)).tempo, "90.00");
    use vizdaw_app::transport::ReadoutBox;
    assert_eq!(
        ReadoutBox::ALL.map(|b| b.title()),
        ["Bar", "Time", "Tempo", "Sig"]
    );
    assert_eq!(
        ReadoutBox::ALL.map(|b| b.accessibility_label()),
        [
            "Bar Readout",
            "Time Readout",
            "Tempo Readout",
            "Signature Readout"
        ]
    );
}

#[test]
fn the_signature_follows_the_meter_map() {
    let m = meter(&[(0, 4, 4), (122_880, 7, 8)]);
    let sig =
        |tick: i64, m: Option<&[MeterEntryRow]>| TransportReadouts::texts(&st(tick), m).signature;
    assert_eq!(sig(122_879, Some(&m)), "4/4");
    assert_eq!(sig(122_880, Some(&m)), "7/8");
    assert_eq!(sig(-1, Some(&m)), "4/4", "the pickup");
    assert_eq!(sig(0, None), "\u{2013}");
}

#[test]
fn hostile_transport_values_read_as_dashes() {
    let tempo = TransportReadouts::tempo_text;
    let dash = "\u{2013}";
    for bad in [0.0, -5.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(tempo(bad), dash, "{bad}");
    }
    assert_eq!(tempo(256_000.0), dash, "1000 BPM");
    assert_eq!(tempo(1e12), dash);
    assert_eq!(tempo(999.99 * 256.0), "999.99");
    assert_eq!(tempo(999.996 * 256.0), dash, "rounds to 1000.00");
    assert_eq!(
        TransportReadouts::texts(&state("1.1.0", f64::NAN, 30_720.0, 0), None).time,
        "--:--.---"
    );
    assert_eq!(
        TransportReadouts::texts(&state("", 0.0, 30_720.0, 0), None).bar,
        "-.-.-"
    );
    let four = meter(&[(0, 4, 4)]);
    for tick in [i64::MIN, i64::MAX] {
        assert_eq!(
            TransportReadouts::texts(&st(tick), Some(&four)).signature,
            "4/4"
        );
    }
    assert_eq!(TransportReadouts::TICK_LIMIT, 1 << 50);
    assert_eq!(TransportReadouts::WIDEST.signature, "255/128");
}

#[test]
fn time_formats_pickups_and_huge_values() {
    use vizdaw_app::transport::format_time;
    assert_eq!(format_time(22.875), "0:22.875");
    assert_eq!(format_time(-0.5), "-0:00.500");
    assert_eq!(format_time(-0.0001), "0:00.000");
    assert_eq!(format_time(75.0), "1:15.000");
    assert!(format_time(f64::MAX).starts_with(char::is_numeric));
}
