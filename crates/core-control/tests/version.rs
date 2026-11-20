//! The release version lives in one place: the workspace `Cargo.toml`.

#[test]
fn bundle_and_crate_versions_agree() {
    assert_eq!(env!("CARGO_PKG_VERSION"), "26.0.0");

    let script = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../scripts/make-app.sh"
    ))
    .expect("read scripts/make-app.sh");
    for line in script
        .lines()
        .filter(|l| l.contains("CFBundleShortVersionString"))
    {
        assert!(
            !line.contains("<string>"),
            "CFBundleShortVersionString must not be a literal: {line}"
        );
    }
    assert!(
        script.contains("CFBundleShortVersionString"),
        "make-app.sh must write CFBundleShortVersionString"
    );
    assert!(
        script.contains("[workspace.package]") && script.contains("Cargo.toml"),
        "make-app.sh must read the version from the workspace Cargo.toml"
    );
    assert!(
        !script.contains("<string>0."),
        "literal 0.x version in make-app.sh"
    );

    let root = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"))
        .expect("read Cargo.toml");
    assert!(root.contains("version = \"26.0.0\""));
}
