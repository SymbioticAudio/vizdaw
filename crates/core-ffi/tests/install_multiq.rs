//! "Install Multi-Q for other apps" (plan 6g Task 4, Ruling 9): the copy of
//! the bundled AU into a Components folder. Every test works in temp
//! folders; none touches the real `~/Library/Audio/Plug-Ins`. Plain Rust, so
//! it runs on Linux too.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use vizdaw_ffi::VizError;
use vizdaw_ffi::bundled::{bundled_au_present, bundled_eq_name, install_from, install_multiq};
use vizdaw_ffi::host::force_offline;

static SERIAL: Mutex<()> = Mutex::new(());

/// One test at a time; offline; real-Library installs refused.
fn serial() -> MutexGuard<'static, ()> {
    let g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    force_offline();
    // SAFETY: the tests of this file run one at a time under `SERIAL`.
    unsafe { std::env::set_var("VIZDAW_TEST_NO_LIBRARY", "1") };
    g
}

/// A fake `Multi-Q.component` under `root`; returns its path.
fn fake_bundle(root: &Path) -> PathBuf {
    let b = root.join("Multi-Q.component");
    fs::create_dir_all(b.join("Contents/MacOS")).unwrap();
    fs::write(b.join("Contents/Info.plist"), b"<plist>multi-q</plist>").unwrap();
    fs::write(b.join("Contents/MacOS/x"), [0u8, 1, 2, 3, 255]).unwrap();
    b
}

/// Every file under `p` (relative path, bytes), sorted.
fn tree(p: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn walk(base: &Path, p: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in fs::read_dir(p).unwrap() {
            let e = e.unwrap();
            let path = e.path();
            let rel = path.strip_prefix(base).unwrap().to_path_buf();
            let ft = fs::symlink_metadata(&path).unwrap().file_type();
            if ft.is_dir() {
                out.push((rel, Vec::new()));
                walk(base, &path, out);
            } else if ft.is_symlink() {
                out.push((
                    rel,
                    fs::read_link(&path)
                        .unwrap()
                        .into_os_string()
                        .into_encoded_bytes(),
                ));
            } else {
                out.push((rel, fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(p, p, &mut out);
    out.sort();
    out
}

/// Names in `dir` that contain `.vizdaw-`.
fn leftovers(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".vizdaw-"))
        .collect()
}

const NOT_MULTIQ: &str = "Something that is not Multi-Q is in the way:";

#[test]
fn install_copies_into_a_fresh_folder() {
    let _g = serial();
    let src_root = tempfile::tempdir().unwrap();
    let src = fake_bundle(src_root.path());
    let dest_root = tempfile::tempdir().unwrap();
    let dest = dest_root.path().join("Plug-Ins/Components");
    assert!(!dest.exists());

    let got = install_from(&src, &dest, false).unwrap();
    assert_eq!(got, dest.join("Multi-Q.component"));
    assert_eq!(tree(&got), tree(&src));
    assert!(leftovers(&dest).is_empty(), "{:?}", leftovers(&dest));
}

#[test]
fn install_refuses_to_replace_unasked() {
    let _g = serial();
    let src_root = tempfile::tempdir().unwrap();
    let src = fake_bundle(src_root.path());
    let dest = tempfile::tempdir().unwrap();
    let old = dest.path().join("Multi-Q.component");
    fs::create_dir_all(old.join("Contents")).unwrap();
    fs::write(old.join("Contents/Info.plist"), b"older").unwrap();
    let before = tree(&old);

    let e = install_from(&src, dest.path(), false).unwrap_err();
    assert_eq!(
        e,
        VizError::Exists {
            path: old.display().to_string()
        }
    );
    assert_eq!(e.to_string(), format!("{NOT_MULTIQ} {}.", old.display()));
    assert_eq!(tree(&old), before);

    let got = install_from(&src, dest.path(), true).unwrap();
    assert_eq!(got, old);
    assert_eq!(tree(&old), tree(&src));
    assert!(leftovers(dest.path()).is_empty());
}

#[cfg(unix)]
#[test]
fn install_refuses_symlinks() {
    use std::os::unix::fs::symlink;
    let _g = serial();
    let src_root = tempfile::tempdir().unwrap();
    let src = fake_bundle(src_root.path());

    // A symlink at the target: refused, never followed, even with replace.
    let dest = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let pointee = fake_bundle(elsewhere.path());
    fs::write(pointee.join("Contents/Info.plist"), b"theirs").unwrap();
    let pointee_before = tree(&pointee);
    let link = dest.path().join("Multi-Q.component");
    symlink(&pointee, &link).unwrap();
    for replace in [false, true] {
        let e = install_from(&src, dest.path(), replace).unwrap_err();
        assert!(matches!(e, VizError::Model { .. }), "{e:?}");
        assert_eq!(e.to_string(), format!("{NOT_MULTIQ} {}.", link.display()));
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_link(&link).unwrap(), pointee);
        assert_eq!(tree(&pointee), pointee_before);
        assert!(leftovers(dest.path()).is_empty());
    }

    // A symlink inside the source: refused, nothing left behind.
    let bad_root = tempfile::tempdir().unwrap();
    let bad = fake_bundle(bad_root.path());
    symlink("/etc/hosts", bad.join("Contents/MacOS/link")).unwrap();
    let dest2 = tempfile::tempdir().unwrap();
    let e = install_from(&bad, dest2.path(), false).unwrap_err();
    assert!(
        e.to_string().starts_with("Multi-Q could not be installed:"),
        "{e}"
    );
    assert_eq!(fs::read_dir(dest2.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn install_leaves_no_half_bundle() {
    use std::os::unix::fs::PermissionsExt;
    let _g = serial();
    let src_root = tempfile::tempdir().unwrap();
    let src = fake_bundle(src_root.path());
    let dest = tempfile::tempdir().unwrap();
    let other = dest.path().join("Other.component");
    fs::create_dir(&other).unwrap();
    let before = tree(dest.path());
    fs::set_permissions(dest.path(), fs::Permissions::from_mode(0o555)).unwrap();

    let e = install_from(&src, dest.path(), false);
    fs::set_permissions(dest.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let e = e.unwrap_err();
    assert!(matches!(e, VizError::Io { .. }), "{e:?}");
    assert!(
        e.to_string().starts_with("Multi-Q could not be installed:"),
        "{e}"
    );
    assert_eq!(tree(dest.path()), before);
}

#[test]
fn install_without_a_bundled_au() {
    let _g = serial();
    // SAFETY: serialised by `SERIAL`.
    unsafe { std::env::remove_var("VIZDAW_BUNDLED_PLUGINS") };
    let dest = tempfile::tempdir().unwrap();
    let e = install_multiq(Some(dest.path().display().to_string()), false).unwrap_err();
    assert!(matches!(e, VizError::Model { .. }), "{e:?}");
    assert_eq!(e.to_string(), "Multi-Q is not in this build of VizDAW.");
    assert_eq!(fs::read_dir(dest.path()).unwrap().count(), 0);
    assert!(!bundled_au_present());
    assert_eq!(bundled_eq_name(), None);
}

#[test]
fn install_through_the_export_with_a_bundled_au() {
    let _g = serial();
    let bundled = tempfile::tempdir().unwrap();
    let src = fake_bundle(bundled.path());
    fs::create_dir(bundled.path().join("Multi-Q.clap")).unwrap();
    // SAFETY: serialised by `SERIAL`.
    unsafe { std::env::set_var("VIZDAW_BUNDLED_PLUGINS", bundled.path()) };
    assert!(bundled_au_present());
    assert_eq!(bundled_eq_name(), Some("Multi-Q".to_string()));
    let dest = tempfile::tempdir().unwrap();
    let got = install_multiq(Some(dest.path().display().to_string()), false);
    // SAFETY: serialised by `SERIAL`.
    unsafe { std::env::remove_var("VIZDAW_BUNDLED_PLUGINS") };
    let got = got.unwrap();
    assert_eq!(
        got,
        dest.path().join("Multi-Q.component").display().to_string()
    );
    assert_eq!(tree(Path::new(&got)), tree(&src));
}

#[test]
fn install_never_targets_the_real_library_in_tests() {
    let _g = serial();
    // A bundled AU is present, so only the guard can stop the copy.
    let bundled = tempfile::tempdir().unwrap();
    fake_bundle(bundled.path());
    // SAFETY: serialised by `SERIAL`.
    unsafe { std::env::set_var("VIZDAW_BUNDLED_PLUGINS", bundled.path()) };
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let real = home.join("Library/Audio/Plug-Ins/Components");
    let target = real.join("Multi-Q.component");
    let before = fs::symlink_metadata(&target)
        .ok()
        .map(|m| m.modified().ok());
    let before_dir = fs::symlink_metadata(&real).is_ok();

    let explicit = install_multiq(Some(real.display().to_string()), false);
    let implicit = install_multiq(None, true);
    // SAFETY: serialised by `SERIAL`.
    unsafe { std::env::remove_var("VIZDAW_BUNDLED_PLUGINS") };

    for r in [explicit, implicit] {
        let e = r.unwrap_err();
        assert!(matches!(e, VizError::Model { .. }), "{e:?}");
        assert!(e.to_string().contains("real Library"), "{e}");
    }
    assert_eq!(
        fs::symlink_metadata(&target)
            .ok()
            .map(|m| m.modified().ok()),
        before
    );
    assert_eq!(fs::symlink_metadata(&real).is_ok(), before_dir);
}
