//! What the app shows says "track", never "lane" (plan Tracks Ruling 7).
//! Refusals, warnings, status text and Undo labels live in string literals
//! in the library sources; this scans them. Ids and keys (`lane.*`,
//! `` `lane` ``) and the allow-listed "velocity lane" and "automation lane"
//! stay. `core-control/tests/names.rs` holds the same rule for the registry.

use core_model::command::LaneCmd;
use core_model::ids::{LaneId, SeqId, TakeId};
use core_model::lane::{InputChannels, MonitorMode};
use core_model::time::Tick;
use std::path::{Path, PathBuf};

/// Ruling 7's rule on one literal: strip `{…}` format arguments, the id
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
    // `lane.<id>` tokens.
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
    let lower = t.to_ascii_lowercase();
    // The allow-list (a drum-grid row; automation rows).
    let mut u = lower.clone();
    for phrase in [
        "velocity lanes",
        "velocity lane",
        "automation lanes",
        "automation lane",
    ] {
        u = u.replace(phrase, "");
    }
    if t == "Lane" || t == "Lanes" {
        return true;
    }
    if !t.chars().any(char::is_whitespace) {
        return false;
    }
    let b = u.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = 0;
    while let Some(k) = u[i..].find("lane") {
        let at = i + k;
        let mut end = at + 4;
        if b.get(end) == Some(&b's') {
            end += 1;
        }
        if (at == 0 || !word(b[at - 1])) && (end >= b.len() || !word(b[end])) {
            return true;
        }
        i = at + 4;
    }
    false
}

fn self_check() {
    for bad in [
        "Select a lane first.",
        "Lanes",
        "Lane ",
        "lane {lane} records",
        "Lane {n}",
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
        "Select a track first.",
        "lane",
    ] {
        assert!(!says_lane(clean), "must pass {clean:?}");
    }
}

/// The string literals of a Rust source, outside comments, up to the first
/// `#[cfg(test)]` line. Normal, raw and continued strings are read whole;
/// character literals are skipped.
fn literals(src: &str) -> Vec<(usize, String)> {
    // Stop at a test module only: `#[cfg(test)]` followed by a `mod` line,
    // not a field or item attribute.
    let lines: Vec<&str> = src.lines().collect();
    let cut = (0..lines.len()).find(|&i| {
        lines[i].trim_start().starts_with("#[cfg(test)]")
            && lines.get(i + 1).is_some_and(|n| {
                n.trim_start()
                    .trim_start_matches("pub ")
                    .starts_with("mod ")
            })
    });
    let src = match cut {
        Some(n) => src.lines().take(n).collect::<Vec<_>>().join("\n"),
        None => src.to_string(),
    };
    let b: Vec<char> = src.chars().collect();
    let (mut out, mut i, mut line) = (Vec::new(), 0, 1);
    while i < b.len() {
        match b[i] {
            '\n' => line += 1,
            '/' if b.get(i + 1) == Some(&'/') => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            '/' if b.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                    if b[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
                i += 2;
                continue;
            }
            'r' if matches!(b.get(i + 1), Some('"') | Some('#'))
                && (i == 0 || !(b[i - 1].is_alphanumeric() || b[i - 1] == '_')) =>
            {
                let mut j = i + 1;
                let mut hashes = 0;
                while b.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if b.get(j) == Some(&'"') {
                    let start_line = line;
                    j += 1;
                    let mut s = String::new();
                    while j < b.len() {
                        if b[j] == '"' && (0..hashes).all(|k| b.get(j + 1 + k) == Some(&'#')) {
                            break;
                        }
                        if b[j] == '\n' {
                            line += 1;
                        }
                        s.push(b[j]);
                        j += 1;
                    }
                    out.push((start_line, s));
                    i = j + 1 + hashes;
                    continue;
                }
            }
            '"' => {
                let start_line = line;
                let mut s = String::new();
                i += 1;
                while i < b.len() && b[i] != '"' {
                    if b[i] == '\\' && i + 1 < b.len() {
                        i += 1;
                        if b[i] == '\n' {
                            // A continued string drops the break and the
                            // next line's indent.
                            line += 1;
                            i += 1;
                            while i < b.len() && b[i].is_whitespace() {
                                if b[i] == '\n' {
                                    line += 1;
                                }
                                i += 1;
                            }
                            continue;
                        }
                    } else if b[i] == '\n' {
                        line += 1;
                    }
                    s.push(b[i]);
                    i += 1;
                }
                out.push((start_line, s));
            }
            '\'' => {
                // A character literal ('"', '\'') or a lifetime.
                if b.get(i + 1) == Some(&'\\') && b.get(i + 3) == Some(&'\'') {
                    i += 3;
                } else if b.get(i + 2) == Some(&'\'') {
                    i += 2;
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
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

/// The DAWproject schema's element names that contain the word, and the
/// one folder whose sources may spell them: the file format's own names,
/// never text the app shows.
const FORMAT_ELEMENTS: &[(&str, &str)] = &[("core-io/src/dawproject/schema/", "Lanes")];

/// Whether `lit` in the source at `path` is exactly a DAWproject element
/// name written by the schema code.
fn format_element(path: &str, lit: &str) -> bool {
    let path = path.replace('\\', "/");
    FORMAT_ELEMENTS
        .iter()
        .any(|(dir, name)| lit == *name && path.contains(dir))
}

#[test]
fn no_refusal_or_undo_text_says_lane() {
    self_check();
    let lits = literals(
        "let a = \"x\"; // \"Lane \"\nf('\"', \"Lane one\", r#\"y \"lane\" z\"#);\n\"a \\\n   lane b\";",
    );
    assert_eq!(
        lits,
        vec![
            (1, "x".to_string()),
            (2, "Lane one".to_string()),
            (2, "y \"lane\" z".to_string()),
            (3, "a lane b".to_string())
        ]
    );
    // A field attribute does not end the scan; a test module does.
    let after_field = literals(
        "struct S {\n    #[cfg(test)]\n    f: u8,\n}\nlet a = \"kept\";\n#[cfg(test)]\nmod tests {\n    \"dropped\";\n}",
    );
    assert_eq!(after_field, vec![(5, "kept".to_string())]);
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for d in [
        "core-ffi",
        "core-model",
        "core-control",
        "core-engine",
        "core-io",
        "platform-macos",
    ] {
        rust_files(&root.join(d).join("src"), &mut files);
    }
    assert!(files.len() > 50, "the source directories were found");
    assert!(format_element(
        "/x/crates/core-io/src/dawproject/schema/read.rs",
        "Lanes"
    ));
    assert!(!format_element(
        "/x/crates/core-io/src/dawproject/export.rs",
        "Lanes"
    ));
    assert!(!format_element(
        "/x/crates/core-io/src/dawproject/schema/read.rs",
        "Lanes here"
    ));
    let mut hits = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let path = f.to_string_lossy();
        for (line, lit) in literals(&text) {
            if says_lane(&lit) && !format_element(&path, &lit) {
                hits.push(format!("{}:{line}: {lit:?}", f.display()));
            }
        }
    }
    // Every LaneCmd name: no "Lane".
    let (seq, lane) = (SeqId(0), LaneId(0));
    let names = [
        LaneCmd::Add {
            seq,
            name: String::new(),
            index: None,
        },
        LaneCmd::Remove { seq, lane },
        LaneCmd::Reorder { seq, lane, to: 0 },
        LaneCmd::Duplicate { seq, lane },
        LaneCmd::Rename {
            seq,
            lane,
            name: String::new(),
        },
        LaneCmd::SetDest {
            seq,
            lane,
            dest: None,
        },
        LaneCmd::SetOutput {
            seq,
            lane,
            output: None,
        },
        LaneCmd::SetFlags {
            seq,
            lane,
            mute: None,
            solo: None,
            arm: None,
        },
        LaneCmd::SetLength {
            seq,
            lane,
            length: None,
            looped: false,
            locked: false,
        },
        LaneCmd::SetPlayQuantize {
            seq,
            lane,
            quantize: None,
        },
        LaneCmd::SetPlayShift {
            seq,
            lane,
            shift: Tick(0),
        },
        LaneCmd::AddTake {
            seq,
            lane,
            name: String::new(),
        },
        LaneCmd::SetActiveTake {
            seq,
            lane,
            take: TakeId(0),
        },
        LaneCmd::SetInput {
            seq,
            lane,
            input: None,
        },
        LaneCmd::SetMonitor {
            seq,
            lane,
            monitor: MonitorMode::Auto,
        },
        LaneCmd::TrackSynth {
            seq,
            lane,
            channels: InputChannels::Mono(0),
            arm_midi: false,
        },
    ]
    .map(|c| c.name());
    assert_eq!(names.len(), 16);
    for n in names {
        if n.contains("Lane") {
            hits.push(format!("LaneCmd name {n:?}"));
        }
    }
    assert!(hits.is_empty(), "{} hits:\n{}", hits.len(), hits.join("\n"));
}

/// The `<string>…</string>` values of a plist or of a script that writes one.
fn plist_strings(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (n, l) in text.lines().enumerate() {
        let mut rest = l;
        while let Some(i) = rest.find("<string>") {
            let tail = &rest[i + 8..];
            let Some(j) = tail.find("</string>") else {
                break;
            };
            out.push((n + 1, tail[..j].to_string()));
            rest = &tail[j + 9..];
        }
    }
    out
}

#[test]
fn plist_strings_say_track() {
    // A planted line proves the scan catches the permission prompt's text.
    let planted =
        "<key>NSMicrophoneUsageDescription</key><string>VizDAW records into armed lanes.</string>";
    let found = plist_strings(planted);
    assert_eq!(found.len(), 1);
    assert!(says_lane(&found[0].1), "the planted line must be caught");
    assert!(!says_lane("org.vizdaw.project"));

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files: Vec<PathBuf> = std::fs::read_dir(root.join("scripts"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "sh"))
        .collect();
    files.sort();
    // Any Info.plist checked in under app/.
    fn plists(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for p in rd.map(|e| e.unwrap().path()) {
            if p.is_dir() {
                if p.file_name().is_some_and(|n| n != ".build") {
                    plists(&p, out);
                }
            } else if p.file_name().is_some_and(|n| n == "Info.plist") {
                out.push(p);
            }
        }
    }
    plists(&root.join("app"), &mut files);
    assert!(
        files.iter().any(|f| f.ends_with("make-app.sh")),
        "make-app.sh was found"
    );
    let mut hits = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for (line, s) in plist_strings(&text) {
            if says_lane(&s) {
                hits.push(format!("{}:{line}: {s:?}", f.display()));
            }
        }
    }
    assert!(hits.is_empty(), "{} hits:\n{}", hits.len(), hits.join("\n"));
}
