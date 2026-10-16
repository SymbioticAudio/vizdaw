//! The bundle builds and exposes every test plugin through its factory.

use clack_host::prelude::*;
use test_plugins::{Kind, bundle};

#[test]
fn the_bundle_lists_every_test_plugin() {
    let path = bundle();
    assert!(path.exists(), "{path:?}");
    // SAFETY: the bundle is this crate's own clack-plugin binary.
    let entry = unsafe { PluginEntry::load(&path) }.unwrap();
    let factory = entry.get_plugin_factory().unwrap();
    let ids: Vec<String> = factory
        .plugin_descriptors()
        .filter_map(|d| d.id().map(|i| i.to_string_lossy().into_owned()))
        .collect();
    let want: Vec<String> = Kind::ALL.iter().map(|k| k.id().to_string()).collect();
    assert_eq!(ids, want);
}
