use core_engine::backend::StereoBuf;
use core_engine::graph::delay::DelayLine;
use core_engine::graph::dsp::{copy, is_faulty, mix_into, peaks, scale, strip_gains};
use core_engine::graph::{
    EffectNode, InsertFactory, NoInserts, ProcessStatus, TestInserts, TestMode, TestNode,
};
use core_model::ids::PluginInstanceId;
use core_model::mixer::{PluginFormat, PluginRef, PluginSlot};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

fn buf(l: &[f32], r: &[f32]) -> StereoBuf {
    StereoBuf {
        left: l.to_vec().into_boxed_slice(),
        right: r.to_vec().into_boxed_slice(),
    }
}

#[test]
fn mixing_scaling_and_peaks() {
    let mut a = buf(&[1.0, 2.0, 3.0], &[0.0, 0.0, 0.0]);
    let b = buf(&[1.0, 1.0, 1.0], &[2.0, -4.0, 1.0]);
    mix_into(&mut a, &b, 0.5, 2);
    assert_eq!(&a.left[..], &[1.5, 2.5, 3.0]);
    assert_eq!(&a.right[..], &[1.0, -2.0, 0.0]);
    scale(&mut a, 2.0, -1.0, 3);
    assert_eq!(&a.left[..], &[3.0, 5.0, 6.0]);
    assert_eq!(&a.right[..], &[-1.0, 2.0, -0.0]);
    assert_eq!(peaks(&a, 3), (6.0, 2.0));
    assert_eq!(peaks(&a, 2), (5.0, 2.0));
    copy(&mut a, &b, 3);
    assert_eq!(&a.right[..], &[2.0, -4.0, 1.0]);
    // Frames beyond the buffers are clamped, never a panic.
    mix_into(&mut a, &b, 1.0, 99);
    assert_eq!(peaks(&a, 99), (2.0, 8.0));
}

#[test]
fn faults_are_nan_inf_or_beyond_64() {
    assert!(!is_faulty(&buf(&[64.0, -64.0], &[0.0, 1.0]), 2));
    assert!(is_faulty(&buf(&[64.5, 0.0], &[0.0, 0.0]), 2));
    assert!(is_faulty(&buf(&[0.0, 0.0], &[f32::NAN, 0.0]), 2));
    assert!(is_faulty(&buf(&[f32::NEG_INFINITY, 0.0], &[0.0, 0.0]), 2));
    assert!(!is_faulty(&buf(&[0.0, f32::NAN], &[0.0, 0.0]), 1));
}

#[test]
fn strip_gain_laws() {
    let (l, r) = strip_gains(0.0, 0.0, true);
    assert!((l - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6 && (l - r).abs() < 1e-9);
    assert_eq!(strip_gains(0.0, 0.0, false), (1.0, 1.0));
    assert_eq!(strip_gains(-144.0, 0.3, true), (0.0, 0.0));
    let (l, r) = strip_gains(0.0, -1.0, true);
    assert_eq!(l, 1.0);
    assert!(r.abs() < 1e-7);
}

#[test]
fn delay_lines_are_exact_across_blocks() {
    let mut d = DelayLine::new(5);
    assert_eq!(d.delay(), 5);
    let mut out = Vec::new();
    for block in 0..4 {
        let mut l = [0.0f32; 3];
        let mut r = [0.0f32; 3];
        if block == 0 {
            l[0] = 1.0;
            r[0] = -1.0;
        }
        d.process(&mut l, &mut r);
        out.extend(l.iter().zip(r.iter()).map(|(a, b)| (*a, *b)));
    }
    let hits: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, x)| x.0 != 0.0)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(hits, vec![5]);
    assert_eq!(out[5], (1.0, -1.0));
    let mut zero = DelayLine::new(0);
    let mut l = [1.0f32, 2.0];
    let mut r = [3.0f32, 4.0];
    zero.process(&mut l, &mut r);
    assert_eq!((l, r), ([1.0, 2.0], [3.0, 4.0]));
}

#[test]
fn test_nodes_apply_gain_and_latency() {
    let mut n = TestNode::new(TestMode::Gain(0.5), 2);
    assert_eq!(n.latency(), 2);
    let mut io = buf(&[1.0, 0.0, 0.0, 0.0], &[2.0, 0.0, 0.0, 0.0]);
    assert_eq!(n.process(4, &mut io, None), ProcessStatus::Ok);
    assert_eq!(&io.left[..], &[0.0, 0.0, 0.5, 0.0]);
    assert_eq!(&io.right[..], &[0.0, 0.0, 1.0, 0.0]);
}

#[test]
fn noise_is_deterministic_and_sidechain_is_added() {
    let mut a = TestNode::new(
        TestMode::Noise {
            seed: 9,
            level: 0.25,
        },
        0,
    );
    let mut b = TestNode::new(
        TestMode::Noise {
            seed: 9,
            level: 0.25,
        },
        0,
    );
    let (mut x, mut y) = (StereoBuf::new(64), StereoBuf::new(64));
    a.process(64, &mut x, None);
    b.process(32, &mut y, None);
    let mut tail = StereoBuf::new(32);
    b.process(32, &mut tail, None);
    assert_eq!(&x.left[..32], &y.left[..32]);
    assert_eq!(&x.left[32..], &tail.left[..]);
    assert!(x.left.iter().all(|s| s.abs() <= 0.25) && x.left.iter().any(|s| *s != 0.0));
    let mut sc_node = TestNode::new(TestMode::AddSidechain, 0);
    let mut io = buf(&[1.0, 1.0], &[0.0, 0.0]);
    let sc = buf(&[0.5, 0.0], &[0.25, 0.0]);
    sc_node.process(2, &mut io, Some(&sc));
    assert_eq!(
        (&io.left[..], &io.right[..]),
        (&[1.5, 1.0][..], &[0.25, 0.0][..])
    );
}

#[test]
fn bad_nodes_report_themselves() {
    let mut nan = TestNode::new(TestMode::Nan, 0);
    let mut io = StereoBuf::new(4);
    nan.process(4, &mut io, None);
    assert!(is_faulty(&io, 4));
    let mut err = TestNode::new(TestMode::Error, 0);
    assert_eq!(err.process(4, &mut io, None), ProcessStatus::Error);
    let mut imp = TestNode::new(TestMode::Impulse, 0);
    let mut io = StereoBuf::new(4);
    imp.process(4, &mut io, None);
    assert_eq!(&io.left[..], &[1.0, 0.0, 0.0, 0.0]);
    let mut io = StereoBuf::new(4);
    imp.process(4, &mut io, None);
    assert_eq!(&io.left[..], &[0.0; 4]);
}

#[test]
fn stall_waits_for_its_flag() {
    let flag = Arc::new(AtomicBool::new(false));
    let f2 = Arc::clone(&flag);
    let (tx, rx) = std::sync::mpsc::channel();
    let t = std::thread::spawn(move || {
        let mut s = TestNode::new(TestMode::Stall(f2), 0);
        let mut io = StereoBuf::new(1);
        tx.send(()).unwrap();
        s.process(1, &mut io, None);
    });
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    // Only a late-scheduled thread could make this pass wrongly, never fail.
    assert!(
        !t.is_finished(),
        "the node returned before its flag was set"
    );
    flag.store(true, Ordering::Release);
    t.join().unwrap();
}

fn slot(id: &str) -> PluginSlot {
    PluginSlot::new(
        PluginInstanceId(0),
        PluginRef {
            format: PluginFormat::Clap,
            id: id.into(),
            name: "x".into(),
            vendor: String::new(),
            version: String::new(),
        },
    )
}

#[test]
fn test_inserts_parse_their_ids() {
    let mut f = TestInserts::new();
    let p = TestInserts::plugin("gain=0.5,latency=64");
    assert_eq!(p.id, "vizdaw.test:gain=0.5,latency=64");
    assert_eq!(p.format, PluginFormat::Clap);
    let node = f
        .make(&PluginSlot::new(PluginInstanceId(0), p), 48_000.0, 512)
        .unwrap();
    assert_eq!(node.latency(), 64);
    assert_eq!(f.made(), 1);
    assert!(f.make(&slot("vizdaw.test:bogus"), 48_000.0, 512).is_err());
    assert!(f.make(&slot("com.example.synth"), 48_000.0, 512).is_err());
    assert!(
        f.make(&slot("vizdaw.test:stall"), 48_000.0, 512).is_err(),
        "no stall flag"
    );
    let mut g = TestInserts::with_stall(Arc::new(AtomicBool::new(true)));
    assert!(g.make(&slot("vizdaw.test:stall"), 48_000.0, 512).is_ok());
    for ok in [
        "noise=3",
        "duck",
        "sc",
        "nan",
        "error",
        "impulse",
        "latency=0",
        "",
    ] {
        assert!(f.parse(&format!("vizdaw.test:{ok}")).is_ok(), "{ok}");
    }
    let e = NoInserts
        .make(&slot("com.example.synth"), 48_000.0, 512)
        .err()
        .unwrap();
    assert!(e.contains("x"), "{e}");
}
