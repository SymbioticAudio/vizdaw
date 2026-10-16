#![cfg(target_os = "macos")]

use core_engine::backend::{AudioBackend, AudioConfig};
use core_engine::graph::exec::{Graph, InsertInit, Runner};
use core_engine::graph::{
    GraphSpec, InputSpec, InsertSpec, NodeSpec, StripSpec, Tap, TestMode, TestNode, plan,
};
use core_engine::pool::{RtEnv, RtPlatform, RtWaker};
use platform_macos::hal::CoreAudioBackend;
use platform_macos::rt::{
    MacRtPlatform, MachWaker, default_workers, performance_cores, promote_current_thread,
};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn mach_waker_wakes_a_waiting_thread() {
    let w = Arc::new(MachWaker::new().unwrap());
    let (tx, rx) = mpsc::channel();
    let w2 = Arc::clone(&w);
    let t = std::thread::spawn(move || {
        w2.wait();
        tx.send(()).unwrap();
    });
    std::thread::sleep(Duration::from_millis(20));
    assert!(rx.try_recv().is_err(), "still waiting");
    w.wake(1);
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    t.join().unwrap();
    // A wake with nobody waiting is kept for the next wait.
    w.wake(1);
    w.wait();
}

#[test]
fn worker_threads_can_be_promoted() {
    let r = std::thread::spawn(|| promote_current_thread(10_666_667))
        .join()
        .unwrap();
    assert_eq!(r, Ok(()));
    let r = std::thread::spawn(|| MacRtPlatform.promote(2_666_667))
        .join()
        .unwrap();
    assert_eq!(r, Ok(()));
}

#[test]
fn core_counts() {
    assert!(performance_cores() >= 1);
    assert_eq!(
        default_workers(),
        performance_cores().saturating_sub(1).min(7)
    );
}

fn noise_graph() -> Graph {
    let mut spec = GraphSpec {
        nodes: Vec::new(),
        master: 4,
    };
    for i in 0..4 {
        let mut s = StripSpec::new(None);
        s.inserts.push(InsertSpec {
            instance: None,
            latency: 10 * i,
            bypass: false,
            sidechain: None,
        });
        spec.nodes.push(NodeSpec::Strip(s));
    }
    let mut m = StripSpec::new(None);
    m.inputs = (0..4)
        .map(|from| InputSpec {
            from,
            tap: Tap::Post,
            gain: 0.5,
        })
        .collect();
    spec.nodes.push(NodeSpec::Strip(m));
    let p = plan(&spec, 96_000).unwrap();
    Graph::build(&spec, &p, 512, |idx, ins| {
        InsertInit::Fresh(Box::new(TestNode::new(
            TestMode::Noise {
                seed: idx as u32 + 1,
                level: 0.25,
            },
            ins.latency,
        )))
    })
}

#[test]
fn mac_platform_workers_render_like_serial() {
    let (want, _) = Runner::serial(noise_graph()).render(48_000, &[512]);
    let mut r = Runner::parallel(noise_graph(), 4, Arc::new(MacRtPlatform)).unwrap();
    r.set_env(RtEnv {
        period_ns: 10_666_667,
        workgroup: None,
    });
    let (got, _) = r.render(48_000, &[256, 512, 64]);
    assert_eq!(
        got.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
        want.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
    );
    assert!(r.shared().take_faults().is_empty());
}

#[test]
#[ignore = "needs an audio output device"]
fn the_default_device_has_a_workgroup_workers_can_join() {
    let mut backend = CoreAudioBackend::new();
    backend.open(&AudioConfig::default()).unwrap();
    let wg = backend
        .workgroup()
        .expect("macOS 11+ devices expose an IO workgroup");
    let t = std::thread::spawn(move || {
        let token = wg.join().unwrap();
        wg.leave(token);
    });
    t.join().unwrap();
}
