//! Schema validation through `xmllint`, for the DAWproject tests.

use std::process::Command;

/// Asserts that `xml` is valid against the schema file `xsd`, by running
/// `xmllint --noout --schema <xsd> <tmpfile>`. `xmllint` is required on
/// macOS; elsewhere a missing one prints "skipped: no xmllint".
pub fn schema_check(xsd: &str, xml: &str) {
    let dir = tempfile::tempdir().expect("temp dir");
    let file = dir.path().join("check.xml");
    std::fs::write(&file, xml).expect("write the document");
    let out = match Command::new("xmllint")
        .arg("--noout")
        .arg("--schema")
        .arg(xsd)
        .arg(&file)
        .output()
    {
        Ok(out) => out,
        Err(e) if cfg!(target_os = "macos") => panic!("xmllint is required on macOS: {e}"),
        Err(_) => {
            println!("skipped: no xmllint");
            return;
        }
    };
    assert!(
        out.status.success(),
        "xmllint refused the document:\n{}\n{}",
        String::from_utf8_lossy(&out.stderr),
        xml
    );
}
