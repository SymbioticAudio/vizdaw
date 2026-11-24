//! The VST3 module lifecycle (host track H1 Task 2, Review Focus 1): the
//! module entry runs once per load, the exit once per successful entry and
//! after the factory and every component are released. The test module
//! counts these itself (`test_plugins::Vst3Probe`); each test loads a
//! private copy of the bundle, which is a separate loaded image with its
//! own counters, so the tests need no serialising.

use core_host::{HostError, vst3};
use test_plugins::{VST3_GAIN_ID, Vst3Counters, Vst3Mode, Vst3Probe};

/// A private copy of the test bundle, and a probe on its image.
fn private() -> (tempfile::TempDir, std::path::PathBuf, Vst3Probe) {
    let d = tempfile::tempdir().unwrap();
    let copy = d.path().join("Private.vst3");
    test_plugins::copy_vst3_bundle(&copy);
    let probe = Vst3Probe::open(&copy);
    (d, copy, probe)
}

#[test]
fn listing_enters_once_and_exits_once() {
    let (_d, copy, probe) = private();
    assert_eq!(vst3::list_bundle(&copy).unwrap().len(), 4);
    let c = probe.counters();
    assert_eq!((c.entries, c.exits), (1, 1), "{c:?}");
    assert_eq!(c.factory_releases, 1, "{c:?}");
    assert!(c.last_factory_release < c.last_exit, "{c:?}");
    vst3::list_bundle(&copy).unwrap();
    let c = probe.counters();
    assert_eq!((c.entries, c.exits), (2, 2), "{c:?}");
}

#[test]
fn checking_a_class_releases_the_component_and_factory_before_the_exit() {
    let (_d, copy, probe) = private();
    vst3::check_class(&copy, VST3_GAIN_ID).unwrap();
    let c = probe.counters();
    assert_eq!((c.entries, c.exits), (1, 1), "{c:?}");
    assert_eq!(c.component_releases, 1, "{c:?}");
    assert_eq!(c.factory_releases, 1, "{c:?}");
    assert!(c.last_component_release > 0, "{c:?}");
    assert!(c.last_component_release < c.last_factory_release, "{c:?}");
    assert!(c.last_factory_release < c.last_exit, "{c:?}");
}

#[test]
fn a_module_whose_entry_fails_gets_no_exit() {
    let (_d, copy, probe) = private();
    probe.set_mode(Vst3Mode::EntryFails);
    let e = vst3::list_bundle(&copy).unwrap_err();
    assert!(matches!(e, HostError::LoadFailed(_)), "{e}");
    assert!(e.to_string().contains("entry"), "{e}");
    assert_eq!(probe.counters(), Vst3Counters::default());
}

#[test]
fn a_module_with_no_factory_after_a_good_entry_gets_exactly_one_exit() {
    let (_d, copy, probe) = private();
    probe.set_mode(Vst3Mode::NullFactory);
    let e = vst3::list_bundle(&copy).unwrap_err();
    assert!(e.to_string().contains("no plug-in factory"), "{e}");
    let c = probe.counters();
    assert_eq!((c.entries, c.exits), (1, 1), "{c:?}");
    assert_eq!(c.factory_releases, 0, "{c:?}");
    // The same through `check_class`.
    let e = vst3::check_class(&copy, VST3_GAIN_ID).unwrap_err();
    assert!(matches!(e, HostError::LoadFailed(_)), "{e}");
    let c = probe.counters();
    assert_eq!((c.entries, c.exits), (2, 2), "{c:?}");
}

#[test]
fn parallel_loads_of_one_image_all_find_a_factory() {
    // One image, entered by several threads at once: an exit must not undo
    // another thread's entry before its factory request.
    let (_d, copy, probe) = private();
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                for _ in 0..20 {
                    assert_eq!(vst3::list_bundle(&copy).unwrap().len(), 4);
                }
            });
        }
    });
    let c = probe.counters();
    assert_eq!((c.entries, c.exits), (160, 160), "{c:?}");
}

/// A plug-in that gave a thread a thread-exit destructor (Rust's `std` and
/// many C++ plug-ins do) must still be loaded when that thread exits, after
/// its module exited: the host never unloads a VST3 library. Without that,
/// the thread's exit runs code that is gone (SIGSEGV on Linux).
#[test]
fn a_thread_that_used_a_plug_in_exits_safely_after_its_module_exits() {
    let d = tempfile::tempdir().unwrap();
    let copy = d.path().join("Private.vst3");
    test_plugins::copy_vst3_bundle(&copy);
    // No probe yet: a probe would hold the image open and hide the bug.
    let path = copy.clone();
    std::thread::spawn(move || {
        let inst = vst3::Vst3Instance::load(&path, test_plugins::VST3_SYNTH_ID).unwrap();
        // The module exits and is unloaded here, before the thread exits.
        drop(inst);
    })
    .join()
    .unwrap();
    // The image is the same one: its counters saw the whole lifecycle.
    let c = Vst3Probe::open(&copy).counters();
    assert_eq!((c.entries, c.exits), (1, 1), "{c:?}");
    assert_eq!(c.component_releases, 1, "{c:?}");
    assert!(c.last_component_release < c.last_exit, "{c:?}");
    if cfg!(unix) {
        assert_eq!(c.thread_exits, 1, "the destructor ran: {c:?}");
    }
}
