#![cfg(target_os = "macos")]
//! The property-list codec (plan 6d Task 9, Ruling 8): an AU state goes
//! from the binary property list VizDAW stores to the XML one an
//! `.aupreset` holds and back, unchanged.

use core_host::PluginInstance;
use core_model::mixer::ParamId;
use platform_macos::au;
use platform_macos::plist::{to_binary, to_xml, written_size};

const DELAY: &str = "aufx/dely/appl";

/// A dictionary holding data, numbers and strings, as XML.
const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>data</key>
	<data>AAECAwQFBgc=</data>
	<key>name</key>
	<string>Test Gain</string>
	<key>type</key>
	<integer>1635083896</integer>
	<key>level</key>
	<real>0.25</real>
</dict>
</plist>
"#;

#[test]
fn xml_and_binary_round_trip() {
    let binary = to_binary(XML.as_bytes()).unwrap();
    assert!(binary.starts_with(b"bplist00"), "a binary property list");
    let xml = to_xml(&binary).unwrap();
    let text = String::from_utf8(xml.clone()).unwrap();
    assert!(text.contains("<plist"), "{text}");
    assert!(text.contains("<string>Test Gain</string>"), "{text}");
    assert!(text.contains("<real>0.25</real>"), "{text}");
    assert_eq!(to_binary(&xml).unwrap(), binary);
    // XML in, XML out, the same.
    assert_eq!(to_xml(&xml).unwrap(), xml);
}

#[test]
fn garbage_is_refused() {
    assert!(to_xml(b"junk").is_err());
    assert!(to_binary(b"junk").is_err());
    assert!(to_xml(b"").is_err());
    assert!(to_binary(&[0xff; 64]).is_err());
}

#[test]
fn au_state_survives_the_aupreset_conversion() {
    let mut a = au::load(DELAY, 48_000.0, 512).unwrap();
    a.set_params(&[(ParamId(1), 0.123)]).unwrap();
    let state = a.save_state().unwrap();
    let preset = to_xml(&state).unwrap();
    assert!(preset.starts_with(b"<?xml"), "an XML property list");
    let back = to_binary(&preset).unwrap();
    let mut b = au::load(DELAY, 48_000.0, 512).unwrap();
    b.load_state(&back).unwrap();
    assert!((b.param_value(ParamId(1)).unwrap() - 0.123).abs() < 1e-6);
}

/// A binary property list (bplist00, one-byte offsets and references)
/// holding `objects` in order, object 0 at the top. Hand-made.
fn bplist(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"bplist00".to_vec();
    let mut offsets = Vec::new();
    for o in objects {
        offsets.push(u8::try_from(out.len()).unwrap());
        out.extend_from_slice(o);
    }
    let table = out.len();
    out.extend_from_slice(&offsets);
    // Trailer: 6 unused, offset size 1, reference size 1, object count,
    // top object 0, offset table offset.
    out.extend_from_slice(&[0; 6]);
    out.extend_from_slice(&[1, 1]);
    out.extend_from_slice(&(objects.len() as u64).to_be_bytes());
    out.extend_from_slice(&0u64.to_be_bytes());
    out.extend_from_slice(&(table as u64).to_be_bytes());
    out
}

/// `{k: A0}` where each `Ai = [Ai+1, Ai+1]` and the last is `[]`: a DAG
/// whose every path a writer writes out, so its written size doubles with
/// each level.
fn dag_bomb(depth: u8) -> Vec<u8> {
    assert!(depth < 60);
    let mut objects = vec![vec![0xD1, 1, 2], vec![0x51, b'k']];
    for i in 0..depth {
        objects.push(vec![0xA2, 3 + i, 3 + i]);
    }
    objects.push(vec![0xA0]);
    bplist(&objects)
}

#[test]
fn the_bomb_is_a_property_list() {
    // Shallow, it is read and written as CF does (so the deep one is
    // refused for its shape, not for being malformed).
    let xml = to_xml(&dag_bomb(3)).unwrap();
    let text = String::from_utf8(xml).unwrap();
    assert_eq!(text.matches("<array/>").count(), 8, "{text}");
    // Every path counts: the dictionary 32, the key 32 + 2, and arrays
    // 32 + 2 × the next (32, 96, 224, 480).
    assert_eq!(written_size(&dag_bomb(3), u64::MAX), Ok(546));
    // The deep one stops counting past the cap.
    let n = written_size(&dag_bomb(40), 1 << 20).unwrap();
    assert!(n > 1 << 20 && n < 1 << 22, "{n}");
}

#[test]
fn import_refuses_binary_and_other_non_xml_input() {
    let bomb = dag_bomb(40);
    assert!(bomb.len() < 1024, "{}", bomb.len());
    let start = std::time::Instant::now();
    let err = to_binary(&bomb).unwrap_err();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(err, "the settings are not an XML property list");
    // A small, well-formed binary list is refused too (any version).
    let binary = to_binary(XML.as_bytes()).unwrap();
    assert_eq!(
        to_binary(&binary).unwrap_err(),
        "the settings are not an XML property list"
    );
    let mut later = binary.clone();
    later[6..8].copy_from_slice(b"15");
    assert!(to_binary(&later).is_err());
    // The old text form is not XML.
    assert_eq!(
        to_binary(b"{ a = b; }").unwrap_err(),
        "the settings are not an XML property list"
    );
    // XML with a byte-order mark and leading space still reads.
    let mut bom = b"\xEF\xBB\xBF\n ".to_vec();
    bom.extend_from_slice(XML.as_bytes());
    assert!(to_binary(&bom).unwrap().starts_with(b"bplist00"));
}

#[test]
fn import_refuses_input_past_the_cap_before_reading_it() {
    let mut big = XML.as_bytes().to_vec();
    big.resize(platform_macos::plist::MAX_PLIST_BYTES + 1, b' ');
    let start = std::time::Instant::now();
    let err = to_binary(&big).unwrap_err();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(err, "the settings are too large");
}

#[test]
fn export_refuses_a_binary_list_that_writes_out_too_large() {
    let bomb = dag_bomb(40);
    let start = std::time::Instant::now();
    let err = to_xml(&bomb).unwrap_err();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(err, "the settings would be too large written out");
}

#[test]
fn export_refuses_malformed_binary_lists_without_reading_them() {
    let key = vec![0x51, b'k'];
    // A reference past the object count.
    let bad = bplist(&[vec![0xD1, 1, 2], key.clone(), vec![0xA1, 200]]);
    // A cycle: the array holds itself.
    let cycle = bplist(&[vec![0xD1, 1, 2], key.clone(), vec![0xA1, 2]]);
    // A truncated trailer.
    let bomb = dag_bomb(4);
    let cut = bomb[..bomb.len() - 4].to_vec();
    // Counts past the input.
    let long = bplist(&[
        vec![0xD1, 1, 2],
        key,
        vec![0x4F, 0x13, 0x7f, 0, 0, 0, 0, 0, 0, 0],
    ]);
    for list in [bad, cycle, cut, long] {
        assert!(written_size(&list, u64::MAX).is_err(), "{list:?}");
        assert!(to_xml(&list).is_err(), "{list:?}");
    }
}

/// Final review follow-up: a stored state that is a binary DAG bomb is
/// refused before the unit reads it, quickly, and the unit keeps working.
#[test]
fn a_unit_refuses_a_bomb_state_unread() {
    let mut a = au::load(DELAY, 48_000.0, 512).unwrap();
    let start = std::time::Instant::now();
    let err = a.load_state(&dag_bomb(40)).unwrap_err();
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    assert!(err.to_string().contains("too large"), "{err}");
    let good = a.save_state().unwrap();
    a.load_state(&good).unwrap();
}
