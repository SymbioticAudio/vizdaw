//! The recording ring and writer (plan 5c Task 4; sub-project 5 spec
//! §6.2): blocks go through the ring whole or not at all, each lane's
//! channels land in its own CAF in the project format, lost blocks and a
//! short end are silence so file frame k is run frame k, files read while
//! they are written (a crash leaves them valid), and a writer whose ring
//! never closes still finishes.

use core_media::caf::CafReader;
use core_media::record::{RecordWriter, WriterConfig, WriterLane, free_bytes, input_ring};
use core_model::format::BitDepth;
use std::path::Path;
use std::time::{Duration, Instant};

/// Sample `f` of ring channel `c` in the tests' blocks: unique per
/// channel and frame, within ±1.
fn value(c: usize, f: u64) -> f32 {
    ((f % 1000) as f32 / 1000.0 - 0.5) * (1.0 - 0.25 * c as f32)
}

fn read(path: &Path) -> (core_media::caf::CafInfo, Vec<f32>) {
    let r = CafReader::open(path).unwrap();
    let info = r.info().clone();
    let mut out = vec![0.0; info.frames as usize * usize::from(info.channels)];
    r.read(0, &mut out, &mut Vec::new()).unwrap();
    (info, out)
}

#[test]
fn blocks_go_through_whole_or_not_at_all() {
    let (mut w, mut r) = input_ring(3, 256);
    let mut out = Vec::new();
    assert!(r.pop(&mut out).is_none());
    let mut run = 0u64;
    // Many blocks: the indices wrap around the buffer many times.
    for k in 0..500u64 {
        let frames = 13 + (k % 50) as u32;
        let base = run;
        assert!(w.push(run, frames, |c, f| value(c, base + f as u64)));
        let b = r.pop(&mut out).unwrap();
        assert_eq!((b.run, b.frames), (run, frames));
        assert_eq!(out.len(), frames as usize * 3);
        for (i, x) in out.iter().enumerate() {
            assert_eq!(*x, value(i % 3, run + (i / 3) as u64));
        }
        run += u64::from(frames);
    }
    // A full ring refuses a block and keeps what it holds.
    let mut pushed = 0;
    while w.push(run + pushed * 64, 64, |_, _| 0.5) {
        pushed += 1;
    }
    assert!(pushed > 0);
    for k in 0..pushed {
        let b = r.pop(&mut out).unwrap();
        assert_eq!(b.run, run + k * 64);
        assert!(out.iter().all(|x| *x == 0.5));
    }
    assert!(r.pop(&mut out).is_none());
    // A block bigger than the whole ring never fits.
    assert!(!w.push(0, 1_000_000, |_, _| 0.0));
    assert!(!r.closed());
    drop(w);
    assert!(r.closed(), "dropping the writer closes the ring");
}

fn lanes(dir: &Path) -> Vec<WriterLane> {
    vec![
        WriterLane {
            path: dir.join("Recorded/mono.caf"),
            channels: vec![2],
        },
        WriterLane {
            path: dir.join("Recorded/stereo.caf"),
            channels: vec![0, 1],
        },
    ]
}

#[test]
fn each_lane_gets_its_channels_and_gaps_are_silence() {
    let dir = tempfile::tempdir().unwrap();
    let (mut w, r) = input_ring(3, 48_000);
    let writer = RecordWriter::start(
        r,
        WriterConfig {
            rate: 48_000,
            depth: BitDepth::Float32,
            lanes: lanes(dir.path()),
        },
    )
    .unwrap();
    // Run frames 0..512 and 1024..1536; 512..1024 were lost; the end is at
    // 2048 (the last block was lost too).
    for (run, n) in [(0u64, 256u32), (256, 256), (1024, 512)] {
        assert!(w.push(run, n, |c, f| value(c, run + f as u64)));
    }
    w.set_end(2048);
    w.close();
    let res = writer.finish(Duration::from_secs(30));
    assert_eq!(res.error, None);
    assert_eq!(res.gap_frames, 512 + 512);
    assert_eq!(res.lanes.len(), 2);
    let (info, mono) = read(&dir.path().join("Recorded/mono.caf"));
    assert_eq!(
        (info.channels, info.frames, info.size_known, info.recording),
        (1, 2048, true, true)
    );
    for (f, x) in mono.iter().enumerate() {
        let f = f as u64;
        let want = if f < 512 || (1024..1536).contains(&f) {
            value(2, f)
        } else {
            0.0
        };
        assert_eq!(*x, want, "frame {f}");
    }
    let (info, st) = read(&dir.path().join("Recorded/stereo.caf"));
    assert_eq!((info.channels, info.frames), (2, 2048));
    assert_eq!(st[0], value(0, 0));
    assert_eq!(st[1], value(1, 0));
    assert_eq!(st[2 * 1100], value(0, 1100));
    assert_eq!(st[2 * 1100 + 1], value(1, 1100));
    assert_eq!(res.lanes[0].written.frames, 2048);
    assert_eq!(res.lanes[0].written.content_hash.len(), 64);
}

#[test]
fn files_are_readable_while_recording_and_in_the_project_depth() {
    let dir = tempfile::tempdir().unwrap();
    let (mut w, r) = input_ring(3, 48_000);
    let writer = RecordWriter::start(
        r,
        WriterConfig {
            rate: 44_100,
            depth: BitDepth::Int16,
            lanes: lanes(dir.path()),
        },
    )
    .unwrap();
    assert!(w.push(0, 4410, |c, f| value(c, f as u64)));
    // A clipped sample on the mono lane.
    assert!(w.push(4410, 1, |c, _| if c == 2 { 1.5 } else { 0.0 }));
    w.set_end(4411);
    let until = Instant::now() + Duration::from_secs(60);
    while writer.status().frames < 4411 {
        assert!(Instant::now() < until, "the writer never caught up");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(writer.status().clipped, vec![1, 0]);
    // As after a crash: open while the writer holds it.
    let (info, _) = read(&dir.path().join("Recorded/mono.caf"));
    assert_eq!(
        (
            info.sample_rate,
            info.depth,
            info.size_known,
            info.recording
        ),
        (44_100, BitDepth::Int16, false, true)
    );
    assert!(info.frames >= 4410, "{}", info.frames);
    w.close();
    let res = writer.finish(Duration::from_secs(30));
    assert_eq!(res.lanes[0].written.clipped, 1);
    let (info, _) = read(&dir.path().join("Recorded/mono.caf"));
    assert_eq!((info.frames, info.size_known), (4411, true));
}

#[test]
fn a_ring_that_never_closes_still_finishes_padded_to_its_end() {
    let dir = tempfile::tempdir().unwrap();
    let (mut w, r) = input_ring(3, 4800);
    let writer = RecordWriter::start(
        r,
        WriterConfig {
            rate: 48_000,
            depth: BitDepth::Int24,
            lanes: lanes(dir.path()),
        },
    )
    .unwrap();
    assert!(w.push(0, 100, |_, _| 0.25));
    w.set_end(300);
    let t = Instant::now();
    // The producer vanished without closing (an input that died).
    let res = writer.finish(Duration::from_millis(200));
    assert!(t.elapsed() < Duration::from_secs(20));
    assert_eq!(res.lanes.len(), 2);
    assert_eq!(res.lanes[1].written.frames, 300);
    assert_eq!(res.gap_frames, 200);
    drop(w);
}

#[test]
fn a_file_that_cannot_be_made_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("Recorded")).unwrap();
    std::fs::write(dir.path().join("Recorded/stereo.caf"), b"taken").unwrap();
    let (_w, r) = input_ring(3, 4800);
    let e = RecordWriter::start(
        r,
        WriterConfig {
            rate: 48_000,
            depth: BitDepth::Int24,
            lanes: lanes(dir.path()),
        },
    );
    assert!(e.is_err());
    assert!(!dir.path().join("Recorded/mono.caf").exists());
    assert_eq!(
        std::fs::read(dir.path().join("Recorded/stereo.caf")).unwrap(),
        b"taken",
        "never overwritten"
    );
    // A lane naming a channel the ring does not carry.
    let (_w, r) = input_ring(1, 4800);
    let bad = RecordWriter::start(
        r,
        WriterConfig {
            rate: 48_000,
            depth: BitDepth::Int24,
            lanes: vec![WriterLane {
                path: dir.path().join("x.caf"),
                channels: vec![3],
            }],
        },
    );
    assert!(bad.is_err());
    assert!(!dir.path().join("x.caf").exists());
}

#[test]
fn free_space_is_known_for_a_real_folder() {
    let dir = tempfile::tempdir().unwrap();
    assert!(free_bytes(dir.path()).is_some_and(|b| b > 0));
    assert_eq!(free_bytes(Path::new("/no/such/folder/anywhere")), None);
}
