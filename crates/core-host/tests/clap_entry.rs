//! CLAP libraries stay loaded for the process's life (host track H1
//! Task 4, as VST3 modules do since Task 3): a plug-in that gave a thread
//! a thread-exit destructor must still be loaded when that thread exits,
//! after its entry's `deinit`. The entry's `init` and `deinit` still run
//! once per load. The test library counts both and its thread-exit
//! destructors (`test_plugins::Vst3Probe::open_clap`).

mod common;

use test_plugins::Vst3Probe;

/// A private copy of the CLAP test bundle (its own loaded image).
fn private() -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let copy = d.path().join("Private.clap");
    test_plugins::copy_bundle(&copy);
    (d, copy)
}

#[test]
fn a_thread_that_used_a_clap_plug_in_exits_safely_after_its_entry_deinit() {
    let (_d, copy) = private();
    // No probe yet: a probe would hold the image open and hide the bug.
    let path = copy.clone();
    std::thread::spawn(move || {
        let inst = common::load_from(&path, "vizdaw.test.gain");
        // The entry's last handle goes here: `deinit`, before the thread
        // exits.
        drop(inst);
    })
    .join()
    .unwrap();
    let c = Vst3Probe::open_clap(&copy).counters();
    assert_eq!((c.clap_inits, c.clap_deinits), (1, 1), "{c:?}");
    if cfg!(unix) {
        assert_eq!(c.thread_exits, 1, "the destructor ran: {c:?}");
    }
}

#[test]
fn each_load_runs_init_and_deinit_once() {
    let (_d, copy) = private();
    let probe = Vst3Probe::open_clap(&copy);
    let a = common::load_from(&copy, "vizdaw.test.gain");
    let b = common::load_from(&copy, "vizdaw.test.ducker");
    assert_eq!(probe.counters().clap_inits, 1, "one entry for both");
    drop(a);
    drop(b);
    let c = probe.counters();
    assert_eq!((c.clap_inits, c.clap_deinits), (1, 1), "{c:?}");
    // Loading again after the deinit runs init again, on the same image.
    drop(common::load_from(&copy, "vizdaw.test.gain"));
    let c = probe.counters();
    assert_eq!((c.clap_inits, c.clap_deinits), (2, 2), "{c:?}");
}
