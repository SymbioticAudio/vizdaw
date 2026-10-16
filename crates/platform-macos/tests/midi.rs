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
