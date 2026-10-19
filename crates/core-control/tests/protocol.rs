//! Request lines, replies, element IDs and key strokes (spec §5).

use core_control::element::{self, check, element_id};
use core_control::keys::{Modifiers, named_key, text_strokes};
use core_control::protocol::{
    MAX_LINE, Reply, RequestId, depth, read_reply, read_request, reply_line, request_line,
};
use core_control::{ControlError, ErrorCode};
use serde_json::{Value, json};

fn code(line: &str) -> ErrorCode {
    read_request(line.as_bytes()).unwrap_err().error.code
}

#[test]
fn a_request_has_an_id_a_command_and_params() {
    let r = read_request(br#"{"id": 7, "cmd": "state.get", "params": {"target": "transport"}}"#)
        .unwrap();
    assert_eq!(r.id, RequestId::Num(7));
    assert_eq!(r.cmd, "state.get");
    assert_eq!(r.params.get("target"), Some(&json!("transport")));
    let r = read_request(br#"{"id": "a-1", "cmd": "commands.list"}"#).unwrap();
    assert_eq!(r.id, RequestId::Text("a-1".into()));
    assert!(r.params.is_empty(), "absent params are empty");
    let r = read_request(br#"{"id":1,"cmd":"app.quit","params":null}"#).unwrap();
    assert!(r.params.is_empty(), "null params are empty");
}

#[test]
fn malformed_lines_are_refused_with_the_right_code() {
    assert_eq!(code("not json"), ErrorCode::ParseError);
    assert_eq!(code(""), ErrorCode::InvalidRequest);
    assert_eq!(code("[1,2]"), ErrorCode::InvalidRequest);
    assert_eq!(code(r#"{"cmd":"app.quit"}"#), ErrorCode::InvalidRequest);
    assert_eq!(
        code(r#"{"id":1.5,"cmd":"app.quit"}"#),
        ErrorCode::InvalidRequest
    );
    assert_eq!(
        code(r#"{"id":{"a":1},"cmd":"app.quit"}"#),
        ErrorCode::InvalidRequest
    );
    assert_eq!(code(r#"{"id":1}"#), ErrorCode::InvalidRequest);
    assert_eq!(code(r#"{"id":1,"cmd":5}"#), ErrorCode::InvalidRequest);
    assert_eq!(
        code(r#"{"id":1,"cmd":"App.Quit"}"#),
        ErrorCode::UnknownCommand
    );
    assert_eq!(
        code(r#"{"id":1,"cmd":"app.quit","params":[1]}"#),
        ErrorCode::InvalidParams
    );
    assert_eq!(
        code(r#"{"id":1,"cmd":"app.quit","extra":true}"#),
        ErrorCode::InvalidRequest
    );
    let long = format!(r#"{{"id":"{}","cmd":"app.quit"}}"#, "x".repeat(129));
    assert_eq!(code(&long), ErrorCode::InvalidRequest);
    assert_eq!(
        read_request(&[0xff, 0xfe, b'{']).unwrap_err().error.code,
        ErrorCode::ParseError
    );
}

#[test]
fn the_id_is_kept_once_it_was_read() {
    let e = read_request(br#"{"id":"k","cmd":"x y"}"#).unwrap_err();
    assert_eq!(e.id, Some(RequestId::Text("k".into())));
    let e = read_request(b"garbage").unwrap_err();
    assert_eq!(e.id, None);
}

#[test]
fn deep_nesting_and_long_lines_are_refused() {
    let deep = format!(
        r#"{{"id":1,"cmd":"state.get","params":{{"a":{}{}}}}}"#,
        "[".repeat(40),
        "]".repeat(40)
    );
    assert_eq!(code(&deep), ErrorCode::InvalidRequest);
    let v: Value = serde_json::from_str(&format!("{}1{}", "[".repeat(10), "]".repeat(10))).unwrap();
    assert_eq!(depth(&v), 10);
    let huge = vec![b' '; MAX_LINE + 1];
    assert_eq!(
        read_request(&huge).unwrap_err().error.code,
        ErrorCode::ParseError
    );
}

#[test]
fn replies_round_trip() {
    let id = RequestId::Num(3);
    let ok = reply_line(Some(&id), &Ok(json!({"playing": true})));
    assert_eq!(
        read_reply(&ok).unwrap(),
        Reply {
            id: Some(id.clone()),
            outcome: Ok(json!({"playing": true}))
        }
    );
    let e = ControlError::dialog("Save", &["path"]);
    let line = reply_line(Some(&id), &Err(e.clone()));
    let v: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(v["error"]["code"], "dialog_required");
    assert_eq!(v["error"]["dialog"], "Save");
    assert_eq!(read_reply(&line).unwrap().outcome, Err(e));
    let none = reply_line(None, &Err(ControlError::new(ErrorCode::ParseError, "x")));
    assert_eq!(read_reply(&none).unwrap().id, None);
    let req = request_line(&RequestId::Text("q".into()), "app.quit", &json!({}));
    assert_eq!(read_request(req.as_bytes()).unwrap().cmd, "app.quit");
}

#[test]
fn every_error_code_has_a_distinct_wire_name() {
    let names: std::collections::BTreeSet<_> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
    assert_eq!(names.len(), ErrorCode::ALL.len());
    for c in ErrorCode::ALL {
        assert_eq!(ErrorCode::parse(c.as_str()), Some(c));
    }
}

#[test]
fn element_ids_carry_a_path_and_a_check() {
    let id = element_id(&[0, 3, 12], "AXButton", "Play");
    let r = element::parse(&id).unwrap();
    assert_eq!(r.path, vec![0, 3, 12]);
    assert_eq!(r.check, check("AXButton", "Play"));
    assert_ne!(check("AXButton", "Play"), check("AXButton", "Stop"));
    assert_ne!(
        check("AXButtonP", "lay"),
        check("AXButton", "Play"),
        "role and label are separated"
    );
    for bad in [
        "",
        ":",
        "0.1",
        "0..1:00000000",
        "a:00000000",
        "0:0000000",
        "0:0000000g",
        "1234567:00000000",
    ] {
        assert!(element::parse(bad).is_err(), "{bad:?}");
    }
    let too_deep = format!("{}:00000000", vec!["0"; 65].join("."));
    assert!(element::parse(&too_deep).is_err());
}

#[test]
fn keys_have_codes_and_characters() {
    let space = named_key("space", Modifiers::default()).unwrap();
    assert_eq!((space.key_code, space.characters.as_str()), (49, " "));
    let cmd = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    let s = named_key("s", cmd).unwrap();
    assert_eq!(
        (s.key_code, s.characters.as_str(), s.modifiers.command),
        (1, "s", true)
    );
    let upper = named_key("S", cmd).unwrap();
    assert!(upper.modifiers.shift && upper.modifiers.command);
    assert_eq!(upper.unmodified, "s");
    assert!(named_key("hyper", Modifiers::default()).is_err());
    let typed = text_strokes("Hi!\n").unwrap();
    assert_eq!(typed.len(), 4);
    assert_eq!((typed[0].key_code, typed[0].modifiers.shift), (4, true));
    assert_eq!((typed[2].key_code, typed[2].modifiers.shift), (18, true));
    assert_eq!(typed[3].key_code, 36);
    assert_eq!(
        text_strokes("é").unwrap()[0].key_code,
        0xFFFF,
        "sent with its character"
    );
    assert!(text_strokes("").is_err());
    assert!(text_strokes(&"a".repeat(257)).is_err());
    assert!(text_strokes("a\u{7}").is_err());
    let m = Modifiers::parse(Some(&json!(["cmd", "shift"]))).unwrap();
    assert!(m.command && m.shift && !m.option);
    assert!(Modifiers::parse(Some(&json!(["meta"]))).is_err());
    assert!(Modifiers::parse(Some(&json!("shift"))).is_err());
}

mod fuzz {
    //! Random and mutated lines never panic and always answer (spec §11).
    use super::*;
    use proptest::prelude::*;

    /// JSON values of every kind, a few levels deep.
    pub fn any_json() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::from),
            any::<i64>().prop_map(Value::from),
            any::<f64>()
                .prop_filter("finite", |x| x.is_finite())
                .prop_map(Value::from),
            ".{0,40}".prop_map(Value::from),
        ];
        leaf.prop_recursive(4, 64, 8, |inner| {
            prop_oneof![
                prop::collection::vec(inner.clone(), 0..8).prop_map(Value::from),
                prop::collection::btree_map("[a-z_]{0,12}", inner, 0..8)
                    .prop_map(|m| Value::Object(m.into_iter().collect())),
            ]
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn any_bytes_are_answered(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
            let r = read_request(&bytes);
            if let Err(e) = r {
                // The reply line is always valid JSON.
                let line = reply_line(e.id.as_ref(), &Err(e.error));
                prop_assert!(read_reply(&line).is_ok());
            }
        }

        #[test]
        fn any_json_is_answered(v in any_json()) {
            let line = v.to_string();
            // Whatever the document, the answer is one JSON reply line
            // with the request's id (or null) and a matching `ok`.
            let (id, outcome) = match read_request(line.as_bytes()) {
                Ok(r) => (Some(r.id), Ok(json!({"cmd": r.cmd}))),
                Err(e) => (e.id, Err(e.error)),
            };
            let reply = reply_line(id.as_ref(), &outcome);
            let parsed: Value = serde_json::from_str(&reply).unwrap();
            prop_assert_eq!(&parsed["ok"], &Value::Bool(outcome.is_ok()));
            prop_assert_eq!(&parsed["id"], &id.as_ref().map_or(Value::Null, RequestId::to_json));
            prop_assert_eq!(read_reply(&reply).unwrap(), Reply { id, outcome });
        }

        #[test]
        fn requests_with_random_params_are_answered(
            id in any::<i64>(),
            cmd in "[a-z._]{1,20}",
            params in any_json(),
        ) {
            let line = json!({"id": id, "cmd": cmd, "params": params}).to_string();
            match read_request(line.as_bytes()) {
                Ok(r) => prop_assert_eq!(r.id, RequestId::Num(id)),
                Err(e) => prop_assert!(e.id.is_none() || e.id == Some(RequestId::Num(id))),
            }
        }

        #[test]
        fn element_ids_round_trip(path in prop::collection::vec(0u32..1000, 1..20), role in ".{0,20}", label in ".{0,60}") {
            let id = element_id(&path, &role, &label);
            let r = element::parse(&id).unwrap();
            prop_assert_eq!(r.path, path);
            prop_assert_eq!(r.check, check(&role, &label));
        }

        #[test]
        fn any_string_is_an_id_or_refused(s in ".{0,80}") {
            // An element ID either round-trips or is refused with a message.
            match element::parse(&s) {
                Ok(r) => {
                    prop_assert_eq!(&r.id, &s);
                    let canon = format!(
                        "{}:{:08x}",
                        r.path.iter().map(u32::to_string).collect::<Vec<_>>().join("."),
                        r.check
                    );
                    let again = element::parse(&canon).unwrap();
                    prop_assert_eq!((again.path, again.check), (r.path, r.check));
                }
                Err(m) => prop_assert!(!m.is_empty()),
            }
            match named_key(&s, Modifiers::default()) {
                Ok(k) => prop_assert!(!k.characters.is_empty()),
                Err(m) => prop_assert!(!m.is_empty()),
            }
            match text_strokes(&s) {
                Ok(v) => prop_assert_eq!(v.len(), s.chars().count()),
                Err(m) => prop_assert!(!m.is_empty()),
            }
        }
    }
}
