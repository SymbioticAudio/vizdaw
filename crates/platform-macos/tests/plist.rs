#![cfg(target_os = "macos")]
//! The property-list codec (plan 6d Task 9, Ruling 8): an AU state goes
//! from the binary property list VizDAW stores to the XML one an
//! `.aupreset` holds and back, unchanged.

use core_host::PluginInstance;
use core_model::mixer::ParamId;
use platform_macos::au;
use platform_macos::plist::{to_binary, to_xml};

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
