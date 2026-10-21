#![cfg(target_os = "macos")]

use core_engine::backend::MidiBackend;
use platform_macos::midi::{MidiMonitor, MidiSystem, now_ns, ns_to_host};
use std::time::{Duration, Instant};

#[test]
fn virtual_source_roundtrip() {
    let mut sys = MidiSystem::new("VizDAW Test").unwrap();
    let mut out = sys.open_virtual_source("VizDAW Test Out").unwrap();
    let monitor = MidiMonitor::watch(&sys, "VizDAW Test Out").unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let t0 = now_ns() + 20_000_000; // 20 ms ahead, like a block's output time
    let mut sent = vec![];
    for i in 0..100u64 {
        let at = t0 + i * 1_000_000;
        let msg: Vec<u8> = match i % 4 {
            0 => vec![0x90, (40 + i % 40) as u8, 100],
            1 => vec![0x80, (40 + i % 40) as u8, 0],
            2 => vec![0xC3, (i % 128) as u8],
            _ => vec![0xB1, 7, (i % 128) as u8],
        };
        assert!(out.send(at, &msg));
        sent.push((at, msg));
    }
    let sysex = vec![0xF0, 0x7E, 0x7F, 0x09, 0x01, 1, 2, 3, 4, 5, 6, 7, 8, 0xF7];
    let at = t0 + 200_000_000;
    assert!(out.send(at, &sysex));
    sent.push((at, sysex));

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut got = vec![];
    while got.len() < sent.len() && Instant::now() < deadline {
        got.extend(monitor.take());
        std::thread::sleep(Duration::from_millis(5));
    }
    let got: Vec<(u64, Vec<u8>)> = got.into_iter().map(|r| (r.host_time, r.bytes)).collect();
    // Same bytes, same order, and exactly the requested host times.
    let want: Vec<(u64, Vec<u8>)> = sent
        .into_iter()
        .map(|(ns, b)| (ns_to_host(ns), b))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn destinations_can_be_listed() {
    let sys = MidiSystem::new("VizDAW List").unwrap();
    for d in sys.destinations() {
        assert!(!d.name.is_empty());
    }
}

#[test]
fn a_note_listener_hears_every_source() {
    use platform_macos::midi::NoteListener;
    use std::sync::{Arc, Mutex};
    let mut sys = MidiSystem::new("VizDAW Keys Test").unwrap();
    let mut out = sys.open_virtual_source("VizDAW Keys Test Out").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let got = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let sink = Arc::clone(&got);
    let listener = NoteListener::connect_including_own(
        "VizDAW Keys Test In",
        Box::new(move |b: &[u8]| sink.lock().unwrap().push(b.to_vec())),
    )
    .unwrap();
    assert!(listener.sources() >= 1, "the virtual source counts");
    std::thread::sleep(Duration::from_millis(50));
    assert!(out.send(now_ns(), &[0x92, 61, 100]));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !got.lock().unwrap().contains(&vec![0x92, 61, 100]) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(got.lock().unwrap().contains(&vec![0x92, 61, 100]));
}

#[test]
fn a_note_listener_skips_this_processs_own_virtual_source() {
    use platform_macos::midi::NoteListener;
    use std::sync::{Arc, Mutex};
    let mut sys = MidiSystem::new("VizDAW Loop Test").unwrap();
    let mut out = sys.open_virtual_source("VizDAW Loop Test Out").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let own_id = sys.virtual_source_unique_id("VizDAW Loop Test Out");
    assert!(own_id.is_some());
    let got = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let sink = Arc::clone(&got);
    let listener = NoteListener::connect_all(
        "VizDAW Loop Test In",
        Box::new(move |b: &[u8]| sink.lock().unwrap().push(b.to_vec())),
    )
    .unwrap();
    assert!(
        !listener.source_ids().contains(&own_id),
        "our own virtual source is excluded"
    );
    std::thread::sleep(Duration::from_millis(50));
    assert!(out.send(now_ns(), &[0x93, 63, 100]));
    std::thread::sleep(Duration::from_millis(300));
    assert!(!got.lock().unwrap().contains(&vec![0x93, 63, 100]));
}

/// Plan 5c Task 12: the record listener tells sources apart by the key of
/// their name and keeps each message's host time (spec §6.3). It listens
/// only to this process's own virtual sources here (no real device is
/// connected in tests).
#[test]
fn a_source_listener_names_the_source_and_keeps_the_time() {
    use core_engine::input::port_key;
    use platform_macos::midi::SourceListener;
    use std::sync::{Arc, Mutex};
    let mut sys = MidiSystem::new("VizDAW Record Test").unwrap();
    let mut out = sys.open_virtual_source("VizDAW Record Test Out").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let got = Arc::new(Mutex::new(Vec::<(u32, u64, Vec<u8>)>::new()));
    let sink = Arc::clone(&got);
    let listener = SourceListener::connect_own_only(
        "VizDAW Record Test In",
        Box::new(move |key: u32, ns: u64, b: &[u8]| {
            sink.lock().unwrap().push((key, ns, b.to_vec()))
        }),
    )
    .unwrap();
    assert!(
        listener
            .sources()
            .iter()
            .any(|n| n == "VizDAW Record Test Out")
    );
    std::thread::sleep(Duration::from_millis(50));
    let at = now_ns() + 20_000_000;
    assert!(out.send(at, &[0x94, 62, 90]));
    let deadline = Instant::now() + Duration::from_secs(3);
    let mine =
        |g: &Vec<(u32, u64, Vec<u8>)>| g.iter().find(|(_, _, b)| b == &vec![0x94, 62, 90]).cloned();
    while mine(&got.lock().unwrap()).is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    let (key, ns, _) = mine(&got.lock().unwrap()).expect("the message arrived");
    assert_eq!(key, port_key("VizDAW Record Test Out"));
    // Delivered with the time it was scheduled for (host-clock rounding).
    assert!(ns.abs_diff(at) < 1_000, "{ns} vs {at}");
}

/// Pre-flight M7: one client and port for the listener's life; a
/// reconnect connects the sources that appeared since and leaves the
/// ones already connected alone (each message is heard once).
#[test]
fn a_source_listener_reconnects_new_sources_without_doubling_old_ones() {
    use platform_macos::midi::SourceListener;
    use std::sync::{Arc, Mutex};
    let mut sys = MidiSystem::new("VizDAW Reconnect Test").unwrap();
    let mut first = sys.open_virtual_source("VizDAW Reconnect A").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let got = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
    let sink = Arc::clone(&got);
    let mut listener = SourceListener::connect_own_only(
        "VizDAW Reconnect In",
        Box::new(move |_: u32, _: u64, b: &[u8]| sink.lock().unwrap().push(b.to_vec())),
    )
    .unwrap();
    assert!(listener.sources().iter().any(|n| n == "VizDAW Reconnect A"));
    assert!(!listener.sources().iter().any(|n| n == "VizDAW Reconnect B"));
    let mut second = sys.open_virtual_source("VizDAW Reconnect B").unwrap();
    std::thread::sleep(Duration::from_millis(50));
    let added = listener.reconnect();
    assert!(added >= 1, "B is new");
    assert!(listener.sources().iter().any(|n| n == "VizDAW Reconnect B"));
    assert_eq!(
        listener
            .sources()
            .iter()
            .filter(|n| *n == "VizDAW Reconnect A")
            .count(),
        1
    );
    // A second reconnect with nothing new connects nothing.
    assert_eq!(listener.reconnect(), 0);
    std::thread::sleep(Duration::from_millis(50));
    assert!(first.send(now_ns(), &[0x95, 60, 1]));
    assert!(second.send(now_ns(), &[0x96, 61, 2]));
    let deadline = Instant::now() + Duration::from_secs(3);
    let both = |g: &Vec<Vec<u8>>| g.contains(&vec![0x95, 60, 1]) && g.contains(&vec![0x96, 61, 2]);
    while !both(&got.lock().unwrap()) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    // Late duplicates would arrive within this window.
    std::thread::sleep(Duration::from_millis(100));
    let g = got.lock().unwrap();
    assert!(both(&g), "{g:?}");
    assert_eq!(g.iter().filter(|b| *b == &vec![0x95, 60, 1]).count(), 1);
    assert_eq!(g.iter().filter(|b| *b == &vec![0x96, 61, 2]).count(), 1);
}
