//! The typed DAWproject model under hostile size (plan 6d Task 3, L7): a
//! file whose tree is at its limits reads, or is refused, within the memory
//! the plan promises, counting every allocation (a reallocation as an
//! allocation then a free).

mod common;

use common::{CountingAlloc, measure};
use core_io::IoError;
use core_io::dawproject::schema::{check_ids, read_project};
use core_io::limits::{MAX_XML_NODES, PackageLimits};
use core_io::report::Report;
use core_io::xml::Tree;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const MIB: usize = 1024 * 1024;

/// The most a fuzz case (XML under 64 MiB) may hold at once beyond its text
/// (L7: 256 MiB).
const PEAK: usize = 256 * MIB;

/// A project whose structure or arrangement repeats `item` `n` times.
fn repeated(open: &str, item: &str, n: usize, close: &str) -> String {
    format!(
        "<Project version=\"1.0\"><Application name=\"t\" version=\"1\"/>{open}{}{close}</Project>",
        item.repeat(n)
    )
}

/// Like [`repeated`], with the item's number between `before` and `after`.
fn unique(open: &str, before: &str, after: &str, n: usize, close: &str) -> String {
    use std::fmt::Write as _;
    let mut s = format!("<Project version=\"1.0\"><Application name=\"t\" version=\"1\"/>{open}");
    for i in 0..n {
        let _ = write!(s, "{before}{i}{after}");
    }
    s.push_str(close);
    s.push_str("</Project>");
    s
}

/// Parses, reads and checks `xml`; the tree is dropped before the model.
fn run(xml: &str) -> (Result<(), IoError>, usize, std::time::Duration) {
    let start = Instant::now();
    let (r, seen) = measure(|| {
        let tree = Tree::parse(
            xml.as_bytes(),
            &PackageLimits::default(),
            &AtomicBool::new(false),
        )?;
        let mut report = Report::default();
        let p = read_project(&tree, &mut report)?;
        check_ids(&p, &mut report).map(|_| ())
    });
    (r, seen.peak, start.elapsed())
}

#[test]
fn hostile_sizes_stay_inside_the_promise() {
    // One node over the tree's node cap is not what is tested here (Task 2
    // does that); these sit at the cap, each shape the model costs most for.
    let n = MAX_XML_NODES - 8;
    let shapes = [
        (
            "tracks",
            repeated("<Structure>", "<Track/>", n, "</Structure>"),
        ),
        (
            "clips",
            repeated(
                "<Arrangement><Lanes><Clips>",
                "<Clip time=\"0\"/>",
                n,
                "</Clips></Lanes></Arrangement>",
            ),
        ),
        (
            "notes",
            repeated(
                "<Arrangement><Lanes><Notes>",
                "<Note time=\"0\" duration=\"1\" channel=\"0\" key=\"60\"/>",
                n,
                "</Notes></Lanes></Arrangement>",
            ),
        ),
        (
            "channels with ids",
            repeated("<Structure>", "<Channel id=\"c\"/>", n / 2, "</Structure>"),
        ),
        (
            "unique track ids",
            unique(
                "<Structure>",
                "<Track id=\"t",
                "\"/>",
                1_900_000,
                "</Structure>",
            ),
        ),
        (
            "unique parameter ids",
            unique(
                "<Structure><Channel><Devices><Equalizer deviceName=\"E\" deviceRole=\"audioFX\">",
                "<InputGain unit=\"linear\" id=\"p",
                "\"/>",
                1_900_000,
                "</Equalizer></Devices></Channel></Structure>",
            ),
        ),
        (
            "short markers",
            repeated(
                "<Arrangement><Markers>",
                "<Marker name=\"m\" time=\"1\"/>",
                600_000,
                "</Markers></Arrangement>",
            ),
        ),
    ];
    for (name, xml) in &shapes {
        let (r, peak, took) = run(xml);
        println!(
            "{name}: {} MiB of XML, peak {} MiB, {:?}, {:?}",
            xml.len() / MIB,
            peak / MIB,
            took,
            r.as_ref().err()
        );
        assert!(peak < PEAK, "{name}: peak {} MiB", peak / MIB);
        // Past the budget the file is refused as having too many parts
        // (a duplicate ID is the other refusal here).
        if let Err(e) = r {
            let IoError::Refused(s) = e else {
                panic!("{name}: {e:?}")
            };
            assert!(
                s == "This DAWproject file is damaged: it has too many parts."
                    || s.contains("share the ID"),
                "{name}: {s}"
            );
        }
        assert!(took.as_secs() < 20, "{name}: {took:?}");
    }
}

#[test]
fn a_big_real_project_still_reads() {
    // 100 000 notes and 20 000 clips: well inside the budget.
    let notes =
        "<Note time=\"0\" duration=\"1\" channel=\"0\" key=\"60\" vel=\"0.5\"/>".repeat(100_000);
    let clips = "<Clip name=\"c\" time=\"1\" duration=\"2\"/>".repeat(20_000);
    let xml = repeated(
        "<Arrangement><Lanes><Clips>",
        &clips,
        1,
        &format!("</Clips><Notes>{notes}</Notes></Lanes></Arrangement>"),
    );
    let (r, peak, _) = run(&xml);
    r.expect("reads");
    assert!(peak < 128 * MIB, "peak {} MiB", peak / MIB);
}

#[test]
fn the_id_check_is_bounded_too() {
    use core_io::dawproject::schema::{DpNamed, DpProject, DpStructureItem, DpTrack};
    let track = |i: usize| {
        DpStructureItem::Track(DpTrack {
            named: DpNamed {
                id: Some(format!("t{i}")),
                ..DpNamed::default()
            },
            content: vec![],
            loaded: None,
            channel: None,
            children: vec![],
        })
    };
    let mut p = DpProject {
        version: "1.0".to_owned(),
        structure: (0..1_000_000).map(track).collect(),
        ..DpProject::default()
    };
    let (r, seen) = measure(|| check_ids(&p, &mut Report::default()).map(|ids| ids.kind("t7")));
    println!("1M unique ids: peak {} MiB", seen.peak / MIB);
    assert_eq!(
        r.expect("checks"),
        Some(core_io::dawproject::schema::IdKind::Track)
    );
    assert!(seen.peak < 128 * MIB, "peak {} MiB", seen.peak / MIB);
    // With the model's bytes already near the budget, the same check is
    // refused, early.
    p.model_bytes = core_io::limits::MAX_MODEL_BYTES - 1000;
    let (r, seen) = measure(|| check_ids(&p, &mut Report::default()).map(|_| ()));
    println!("near the budget: peak {} MiB", seen.peak / MIB);
    assert_eq!(
        r,
        Err(IoError::Refused(
            "This DAWproject file is damaged: it has too many parts.".to_owned()
        ))
    );
    assert!(seen.peak < 4 * MIB, "peak {} MiB", seen.peak / MIB);
}
