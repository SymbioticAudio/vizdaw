//! A source scan of the audio thread's control code (groups-vcas Task 4,
//! preflight F13): no function marked `// RT` in `graph/controls.rs`
//! takes a lock or allocates. A function is marked when the last line of
//! its doc comment, after the `///`, ends exactly in `// RT`; "Not RT"
//! functions (`build`, `snapshot`, …) are never scanned.

const CONTROLS: &str = include_str!("../src/graph/controls.rs");

/// What an `// RT` body must not hold: locks and allocations.
const FORBIDDEN: &[&str] = &[
    "Mutex",
    "RwLock",
    ".lock(",
    "Vec::new",
    "vec!",
    "Box::new",
    "Arc::new",
    "with_capacity",
    ".collect",
    ".to_vec(",
    "format!",
    "String::",
];

/// A scanned function: its name and its body's lines.
struct RtFn<'a> {
    name: &'a str,
    body: Vec<&'a str>,
}

/// The name of the function `line` declares, if it declares one.
fn declared(line: &str) -> Option<&str> {
    let t = line.trim_start();
    let at = t.find("fn ")?;
    let head = &t[..at];
    let modifiers = head
        .split_whitespace()
        .all(|w| matches!(w, "pub" | "pub(crate)" | "const" | "unsafe"));
    if !modifiers {
        return None;
    }
    let rest = &t[at + 3..];
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_'))?;
    Some(&rest[..end])
}

/// True when `doc` (a `///` line) ends exactly in `// RT`.
fn marks_rt(doc: &str) -> bool {
    doc.trim()
        .strip_prefix("///")
        .is_some_and(|text| text.trim_end().ends_with("// RT"))
}

/// Every function of `src` whose doc comment's last line marks it RT,
/// with its body: from its `fn` line to the closing brace at its indent.
fn rt_functions(src: &str) -> Vec<RtFn<'_>> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(name) = declared(line) else {
            continue;
        };
        // The last doc line above, skipping attributes.
        let doc = lines[..i]
            .iter()
            .rev()
            .map(|l| l.trim())
            .find(|l| !l.starts_with("#["));
        if !doc.is_some_and(marks_rt) {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let close = format!("{}}}", " ".repeat(indent));
        let end = lines[i..]
            .iter()
            .position(|l| *l == close)
            .map_or(lines.len(), |k| i + k + 1);
        out.push(RtFn {
            name,
            body: lines[i..end].to_vec(),
        });
    }
    out
}

#[test]
fn controls_take_no_lock() {
    let scanned = rt_functions(CONTROLS);
    let names: Vec<&str> = scanned.iter().map(|f| f.name).collect();
    // Positive control: the scan finds the audio thread's functions, and
    // none of the Not RT ones.
    assert!(
        scanned.len() >= 10,
        "only {} scanned: {names:?}",
        scanned.len()
    );
    for rt in ["commit", "adopt", "publish", "set_strip", "release"] {
        assert!(names.contains(&rt), "{rt} not scanned: {names:?}");
    }
    for not_rt in ["build", "snapshot", "adopt_snapshot", "controls", "value"] {
        assert!(!names.contains(&not_rt), "{not_rt} scanned: {names:?}");
    }
    let mut found = Vec::new();
    for f in &scanned {
        for line in &f.body {
            for bad in FORBIDDEN {
                if line.contains(bad) {
                    found.push(format!("{}: {bad} in {:?}", f.name, line.trim()));
                }
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn the_scan_sees_what_it_forbids() {
    let src = "\
/// Not
/// RT.
fn build() {
    let v = Vec::new();
}

/// Runs on the audio thread. // RT
pub(crate) fn hot(&self) {
    let m = self.m.lock();
}
";
    let found = rt_functions(src);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "hot");
    assert!(found[0].body.iter().any(|l| l.contains(".lock(")));
    assert!(found[0].body.last().is_some_and(|l| *l == "}"));
}
