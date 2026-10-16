use core_engine::fixtures::{Builder, Q, demo_document};
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_vizdaw-play"))
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vizdaw-play-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn checked_in_demo_matches_a_fresh_build() {
    let checked_in =
        std::fs::read_to_string(repo().join("fixtures/demo.vizdaw/project.json")).unwrap();
    let fresh = core_model::file::to_json(&demo_document().unwrap()).unwrap();
    assert_eq!(
        checked_in, fresh,
        "run: cargo run -p vizdaw-play -- --write-demo fixtures/demo.vizdaw"
    );
}

#[test]
fn write_demo_writes_a_loadable_package() {
    let dir = temp("write");
    let pkg = dir.join("demo.vizdaw");
    let out = bin().arg("--write-demo").arg(&pkg).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let doc = core_model::file::load(&pkg).unwrap();
    assert_eq!(doc, demo_document().unwrap());
}

#[test]
fn offline_render_of_the_demo_matches_the_checked_in_log() {
    let dir = temp("offline");
    let log = dir.join("demo.txt");
    let out = bin()
        .arg(repo().join("fixtures/demo.vizdaw"))
        .args(["--seq", "Song", "--offline"])
        .arg(&log)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&log).unwrap();
    let want = std::fs::read_to_string(repo().join("fixtures/demo.offline.txt")).unwrap();
    assert!(
        got == want,
        "offline log differs from fixtures/demo.offline.txt"
    );
}

#[test]
fn offline_loop_renders_three_passes() {
    let dir = temp("loop");
    let log = dir.join("loop.txt");
    let out = bin()
        .arg(repo().join("fixtures/demo.vizdaw"))
        .args(["--loop", "--to", "virtual", "--offline"])
        .arg(&log)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = std::fs::read_to_string(&log).unwrap();
    let program_33 = text.lines().filter(|l| l.ends_with(" c1 21")).count();
    assert_eq!(program_33, 6, "two Groove uses per pass, three passes");
}

#[test]
fn several_top_level_sequences_list_and_exit_2() {
    let mut b = Builder::new();
    let d = b.dest("Synth", "builtin:synth", 0).unwrap();
    let (a, al) = b.sequence("Verse", "L", Some(d)).unwrap();
    b.notes(a, al, &[(0, 60, 0x8000, Q)]).unwrap();
    b.sequence("Chorus", "L", Some(d)).unwrap();
    let dir = temp("ambiguous");
    let pkg = dir.join("two.vizdaw");
    core_model::file::save(&b.finish(), &pkg).unwrap();

    let out = bin()
        .arg(&pkg)
        .args(["--offline"])
        .arg(dir.join("x.txt"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("Verse") && err.contains("Chorus"), "{err}");

    let out = bin()
        .arg(&pkg)
        .args(["--seq", "Verse", "--offline"])
        .arg(dir.join("v.txt"))
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(
        std::fs::read_to_string(dir.join("v.txt")).unwrap(),
        "0 t0 90 3c 40\n24000 t0 80 3c 00\n"
    );

    let out = bin()
        .arg(&pkg)
        .args(["--seq", "Bridge", "--offline"])
        .arg(dir.join("b.txt"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Bridge"));
}

#[test]
fn list_shows_sequences() {
    let out = bin()
        .arg(repo().join("fixtures/demo.vizdaw"))
        .arg("--list")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("Song  12 bar(s)  (top level)"), "{text}");
    assert!(text.contains("Beat  1 bar(s)"), "{text}");
}

#[test]
fn bad_arguments_are_refused() {
    let out = bin()
        .arg("x.vizdaw")
        .args(["--buffer", "16"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let out = bin()
        .arg("missing.vizdaw")
        .args(["--offline", "x.txt"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot load"));
}

#[test]
fn offline_render_with_workers_matches_the_checked_in_log() {
    let dir = temp("workers");
    let log = dir.join("demo.txt");
    let out = bin()
        .arg(repo().join("fixtures/demo.vizdaw"))
        .args(["--seq", "Song", "--workers", "4", "--offline"])
        .arg(&log)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = std::fs::read_to_string(&log).unwrap();
    let want = std::fs::read_to_string(repo().join("fixtures/demo.offline.txt")).unwrap();
    assert!(
        got == want,
        "offline log with workers differs from fixtures/demo.offline.txt"
    );
}

#[test]
fn too_many_workers_are_refused() {
    let out = bin()
        .arg(repo().join("fixtures/demo.vizdaw"))
        .args(["--workers", "16", "--offline", "x.txt"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn scan_fills_the_catalog_and_plugins_lists_it() {
    let dir = temp("scan");
    let plugins = dir.join("clap");
    std::fs::create_dir_all(&plugins).unwrap();
    let bundle = test_plugins::bundle();
    copy_dir(&bundle, &plugins.join("VizDAW Test.clap"));
    let catalog = dir.join("plugins.json");
    let out = bin()
        .args([
            "--scan",
            "--no-au",
            "--scan-timeout-ms",
            "2000",
            "--catalog",
        ])
        .arg(&catalog)
        .arg("--clap-dir")
        .arg(&plugins)
        .arg("--scanner")
        .arg(test_plugins::scanner())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    for line in [
        "clap:vizdaw.test.gain  Test Gain (VizDAW)  OK",
        "clap:vizdaw.test.crasher  Test Crasher (VizDAW)  Blocklisted: crashed (signal 11)",
        "clap:vizdaw.test.hanger  Test Hanger (VizDAW)  Blocklisted: timed out",
    ] {
        assert!(
            text.lines().any(|l| l == line),
            "missing {line:?} in\n{text}"
        );
    }
    // The catalog persists; listing does not scan again.
    let out = bin()
        .arg("--plugins")
        .arg("--catalog")
        .arg(&catalog)
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), text);
}

#[test]
fn bad_plugin_arguments_are_errors() {
    let dir = temp("badplugin");
    let catalog = dir.join("plugins.json");
    for (arg, want) in [
        (
            "--insert=Nowhere=au:aufx/dely/appl",
            "no strip named \"Nowhere\"",
        ),
        ("--insert=Master=vst3:x", "expected au:<id> or clap:<id>"),
        (
            "--instrument=Nobody=clap:x",
            "no destination named \"Nobody\"",
        ),
    ] {
        let out = bin()
            .arg(repo().join("fixtures/demo.vizdaw"))
            .arg(arg)
            .arg("--catalog")
            .arg(&catalog)
            .args(["--offline"])
            .arg(dir.join("x.txt"))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1), "{arg}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains(want), "{arg}: {err}");
    }
}

fn copy_dir(from: &Path, to: &Path) {
    if from.is_file() {
        std::fs::copy(from, to).unwrap();
        return;
    }
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        copy_dir(&e.path(), &to.join(e.file_name()));
    }
}

#[test]
fn offline_and_list_leave_the_catalog_alone() {
    let dir = temp("lazycat");
    let home = dir.join("home");
    let cat = home.join("Library/Application Support/VizDAW/plugins.json");
    std::fs::create_dir_all(cat.parent().unwrap()).unwrap();
    std::fs::write(&cat, "not json").unwrap();
    let demo = repo().join("fixtures/demo.vizdaw");
    let out = bin()
        .env("HOME", &home)
        .arg(&demo)
        .arg("--offline")
        .arg(dir.join("x.txt"))
        .output()
        .unwrap();
    assert!(out.status.success());
    let out = bin()
        .env("HOME", &home)
        .arg(&demo)
        .arg("--list")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(std::fs::read_to_string(&cat).unwrap(), "not json");
    assert!(!cat.with_file_name("plugins.json.corrupt").exists());
}

#[test]
fn plugin_argument_validation() {
    let demo = repo().join("fixtures/demo.vizdaw");
    for args in [
        vec!["--scan", "--scan-timeout-ms", "0"],
        vec!["--scan", "--insert", "Master=clap:x"],
        vec!["--plugins", "--instrument", "A=clap:x"],
    ] {
        let out = bin().args(&args).output().unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}");
    }
    let out = bin()
        .arg(&demo)
        .args(["--insert", "Master=clap:", "--offline"])
        .arg(temp("emptyid").join("x.txt"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("empty plugin id"));
}
