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
