//! On-screen and command text names no third-party product (plan 7d
//! Ruling 10, Task B8). The names are the owner's list; the match is
//! case-sensitive on whole words, so `kontrol_mk3` (an id) and the device
//! name matcher's `KONTROL` are allowed. `core-control` has no `regex`
//! dependency, so the word-boundary match is written by hand.

use core_control::registry::{COMMANDS, OPEN_DIALOGS};
use std::path::{Path, PathBuf};

const NAMES: &[&str] = &[
    "Opcode",
    "Studio Vision",
    "SVP",
    "Logic",
    "Apple",
    "Midas",
    "Soundcraft",
    "Steinberg",
    "Cubase",
    "NI",
    "Native Instruments",
    "Komplete",
    "Kontrol",
    "Microsoft",
    // DAWproject's origin and the other readers and converters its README
    // lists (plan 6d Task 13).
    "Bitwig",
    "PreSonus",
    "Studio One",
    "Cubasis",
    "VST Live",
    "n-Track",
    "DawVert",
    "ProjectConverter",
    "Cockos",
    "Reaper",
];

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// The first listed name `text` holds as a whole ASCII word (`\bname\b`).
fn third_party_name(text: &str) -> Option<&'static str> {
    let b = text.as_bytes();
    for name in NAMES {
        let n = name.as_bytes();
        if n.len() > b.len() {
            continue;
        }
        for i in 0..=(b.len() - n.len()) {
            if &b[i..i + n.len()] != n {
                continue;
            }
            let before = i == 0 || !is_word(b[i - 1]);
            let after = i + n.len() == b.len() || !is_word(b[i + n.len()]);
            if before && after {
                return Some(name);
            }
        }
    }
    None
}

/// The matcher catches the known-bad forms and passes the clean ones.
fn self_check() {
    for bad in [
        "NI's keyboard",
        "NI x",
        "x NI",
        "NI",
        "a Kontrol keyboard",
        "Native Instruments GmbH",
        "(Logic)",
        "Studio Vision Pro",
        "a Bitwig file",
        "PreSonus Studio One",
        "n-Track Studio",
        "Cockos Reaper",
    ] {
        assert!(third_party_name(bad).is_some(), "must catch {bad:?}");
    }
    for clean in [
        "kontrol_mk3",
        "KONTROL",
        "NIGHT",
        "ANI",
        "Logical",
        "SVPN",
        "a host-integration keyboard",
        "VST Left out",
        "a DAWproject file",
        "",
    ] {
        assert_eq!(third_party_name(clean), None, "must pass {clean:?}");
    }
}

#[test]
fn no_command_text_names_a_third_party_product() {
    self_check();
    let mut hits = Vec::new();
    let mut check = |what: String, text: &str| {
        if let Some(n) = third_party_name(text) {
            hits.push(format!("{what}: {n} in {text:?}"));
        }
    };
    for c in COMMANDS {
        check(format!("{} title", c.id), c.title);
        check(format!("{} menu", c.id), c.menu);
        for p in c.params {
            check(format!("{} param {} help", c.id, p.name), p.help);
        }
        for d in c.dialogs {
            check(format!("{} dialog name", c.id), d.name);
            check(format!("{} dialog when", c.id), d.when);
        }
    }
    for d in OPEN_DIALOGS {
        check(format!("open dialog {}", d.name), d.name);
        check(format!("open dialog {}", d.name), d.when);
    }
    assert!(
        hits.is_empty(),
        "third-party names in command text:\n{}",
        hits.join("\n")
    );
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// The string literals on one line, up to a `//` outside a string. A
/// literal that runs past the line is read to its end.
fn literals(line: &str) -> Vec<String> {
    let b = line.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => break,
            b'"' => {
                let mut s = Vec::new();
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' && i + 1 < b.len() {
                        i += 1;
                    }
                    s.push(b[i]);
                    i += 1;
                }
                out.push(String::from_utf8_lossy(&s).into_owned());
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn literal_check() {
    assert_eq!(
        literals(r#"f("a \"Kontrol\" b", "c"); // "Apple""#).len(),
        2
    );
    assert!(literals("/// the Kontrol keyboard").is_empty());
    assert_eq!(literals(r#"x("NI's")"#), vec!["NI's".to_string()]);
}

#[test]
fn no_refusal_text_names_a_third_party_product() {
    self_check();
    literal_check();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for d in ["core-ffi/src", "core-surface/src", "core-io/src"] {
        rust_files(&root.join(d), &mut files);
    }
    assert!(files.len() > 20, "the source directories were found");
    let mut hits = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for (i, line) in text.lines().enumerate() {
            // Test modules come last in these files: stop at the first.
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            if line.trim_start().starts_with("//") {
                continue;
            }
            for lit in literals(line) {
                if let Some(n) = third_party_name(&lit) {
                    hits.push(format!("{}:{}: {n} in {lit:?}", f.display(), i + 1));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "third-party names in refusal text:\n{}",
        hits.join("\n")
    );
}

/// Ruling 7's rule on one registry text (plan Tracks): strip `{…}`, the id
/// tokens `lane.xxx` and `` `lane` ``, and the allow-listed phrases; a hit
/// has whitespace and the word lane/lanes, or is exactly `Lane`/`Lanes`.
fn says_lane(lit: &str) -> bool {
    let mut s = String::new();
    let mut depth = 0u32;
    for c in lit.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => s.push(c),
            _ => {}
        }
    }
    for tok in ["`lanes`", "`lane`"] {
        s = s.replace(tok, "");
    }
    let mut t = String::new();
    let mut rest = s.as_str();
    while let Some(i) = rest.find("lane.") {
        let tail = &rest[i + 5..];
        let n = tail
            .find(|c: char| !(c.is_ascii_lowercase() || c == '_'))
            .unwrap_or(tail.len());
        if n == 0 {
            // "The lane." is a sentence, not an id: keep it.
            t.push_str(&rest[..i + 5]);
        } else {
            t.push_str(&rest[..i]);
        }
        rest = &tail[n..];
    }
    t.push_str(rest);
    if t == "Lane" || t == "Lanes" {
        return true;
    }
    if !t.chars().any(char::is_whitespace) {
        return false;
    }
    let mut u = t.to_ascii_lowercase();
    for phrase in [
        "velocity lanes",
        "velocity lane",
        "automation lanes",
        "automation lane",
    ] {
        u = u.replace(phrase, "");
    }
    let b = u.as_bytes();
    let mut i = 0;
    while let Some(k) = u[i..].find("lane") {
        let at = i + k;
        let mut end = at + 4;
        if b.get(end) == Some(&b's') {
            end += 1;
        }
        if (at == 0 || !is_word(b[at - 1])) && (end >= b.len() || !is_word(b[end])) {
            return true;
        }
        i = at + 4;
    }
    false
}

#[test]
fn no_command_text_says_lane() {
    for bad in [
        "Select a lane first.",
        "Lanes",
        "Lane ",
        "lane {lane} records",
        "The lane.",
    ] {
        assert!(says_lane(bad), "must catch {bad:?}");
    }
    for clean in [
        "lane.track_synth",
        "lanes",
        "`lane` names the track",
        "{lane.name}",
        "a velocity lane",
        "automation lanes",
        "lane",
    ] {
        assert!(!says_lane(clean), "must pass {clean:?}");
    }
    let mut hits = Vec::new();
    let mut check = |what: String, text: &str| {
        if says_lane(text) {
            hits.push(format!("{what}: {text:?}"));
        }
    };
    for c in COMMANDS {
        check(format!("{} title", c.id), c.title);
        check(format!("{} menu", c.id), c.menu);
        for p in c.params {
            check(format!("{} param {} help", c.id, p.name), p.help);
        }
        for d in c.dialogs {
            check(format!("{} dialog name", c.id), d.name);
            check(format!("{} dialog when", c.id), d.when);
        }
    }
    for d in OPEN_DIALOGS {
        check(format!("open dialog {}", d.name), d.name);
        check(format!("open dialog {}", d.name), d.when);
    }
    assert!(
        hits.is_empty(),
        "{} registry texts say lane:\n{}",
        hits.len(),
        hits.join("\n")
    );
}
