//! `vizdaw-mcp` end to end: the built binary over pipes, a fake app on a
//! temp Co-Producer socket (plan 8a Task 5, Rulings 1, 2, 10 and 12).
//! Every child gets `VIZDAW_SUPPORT_DIR`; this process never sets it.

#[path = "support/fake_app.rs"]
mod fake_app;

use core_control::registry::ArgValue;
use core_control::{Command, ControlError, ErrorCode};
use fake_app::{Answer, DroppingApp, FakeApp, PNG, support_dir};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command as Process, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::Duration;

const LONG: Duration = Duration::from_secs(20);

/// The server under test, spoken to over its stdin and stdout.
struct Mcp {
    child: Child,
    stdin: Option<ChildStdin>,
    out: Receiver<String>,
    next: i64,
}

impl Mcp {
    fn spawn(support: &Path) -> Mcp {
        let mut child = Process::new(env!("CARGO_BIN_EXE_vizdaw-mcp"))
            .env("VIZDAW_SUPPORT_DIR", support)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, out) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Mcp {
            stdin: child.stdin.take(),
            child,
            out,
            next: 1,
        }
    }

    fn send_raw(&mut self, line: &str) {
        let w = self.stdin.as_mut().unwrap();
        w.write_all(line.as_bytes()).unwrap();
        w.write_all(b"\n").unwrap();
        w.flush().unwrap();
    }

    fn send(&mut self, v: &Value) {
        self.send_raw(&v.to_string());
    }

    fn recv(&self) -> Value {
        let line = self.out.recv_timeout(LONG).expect("vizdaw-mcp answered");
        assert!(!line.contains('\n'));
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"))
    }

    fn silent_for(&self, d: Duration) {
        match self.out.recv_timeout(d) {
            Err(RecvTimeoutError::Timeout) => {}
            other => panic!("expected no output, got {other:?}"),
        }
    }

    /// Sends a request and returns its whole reply.
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next;
        self.next += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        let reply = self.recv();
        assert_eq!(reply["jsonrpc"], "2.0", "{reply}");
        assert_eq!(reply["id"], id, "{reply}");
        reply
    }

    fn result(&mut self, method: &str, params: Value) -> Value {
        let r = self.request(method, params);
        assert!(r.get("error").is_none(), "{r}");
        r["result"].clone()
    }

    fn error_code(&mut self, method: &str, params: Value) -> i64 {
        let r = self.request(method, params);
        r["error"]["code"].as_i64().unwrap_or_else(|| panic!("{r}"))
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.result("tools/call", json!({"name": name, "arguments": arguments}))
    }

    fn ping_answers(&mut self) {
        assert_eq!(self.result("ping", json!({})), json!({}));
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn text(result: &Value) -> String {
    result["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("{result}"))
        .to_string()
}

fn is_error(result: &Value) -> bool {
    result["isError"]
        .as_bool()
        .unwrap_or_else(|| panic!("{result}"))
}

fn run_of(id: &str, args: Vec<(&str, ArgValue)>) -> Command {
    Command::CommandRun {
        id: id.into(),
        args: args.into_iter().map(|(n, v)| (n.into(), v)).collect(),
        window: None,
    }
}

const INSTRUCTIONS: &str = "VizDAW's Co-Producer. Adds are free; anything that changes or deletes the composer's own work goes through the change tool and their yes. Start every request with begin_request and finish it with end_request.";

#[test]
fn initialize_era_handshake() {
    let d = support_dir();
    let mut m = Mcp::spawn(d.path());
    let r = m.result(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    );
    assert_eq!(r["protocolVersion"], "2025-06-18");
    assert!(r["capabilities"]["tools"].is_object(), "{r}");
    assert!(r["capabilities"]["resources"].is_object(), "{r}");
    assert_eq!(r["serverInfo"]["name"], "vizdaw-mcp");
    assert_eq!(r["instructions"], INSTRUCTIONS);
    let r = m.result("initialize", json!({"protocolVersion": "1999-01-01"}));
    assert_eq!(r["protocolVersion"], "2025-11-25");
    m.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    m.silent_for(Duration::from_millis(300));
    m.ping_answers();
}

#[test]
fn modern_era_discover_and_versions() {
    let d = support_dir();
    let mut m = Mcp::spawn(d.path());
    let meta = |v: &str| json!({"_meta": {"io.modelcontextprotocol/protocolVersion": v}});
    let r = m.result("server/discover", meta("2026-07-28"));
    assert_eq!(
        r["supportedVersions"],
        json!(["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26"])
    );
    assert!(r["capabilities"]["tools"].is_object(), "{r}");
    let r = m.result("tools/list", meta("2026-07-28"));
    assert_eq!(r["resultType"], "complete");
    assert_eq!(r["ttlMs"], 0);
    assert_eq!(r["cacheScope"], "private");
    assert!(r["tools"].as_array().unwrap().len() > 10);
    let r = m.request("tools/list", meta("2030-01-01"));
    assert_eq!(r["error"]["code"], -32022, "{r}");
    assert_eq!(r["error"]["data"]["requested"], "2030-01-01");
    assert_eq!(
        r["error"]["data"]["supported"],
        json!(["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26"])
    );
    m.ping_answers();
}

#[test]
fn tools_come_from_the_allow_list() {
    let d = support_dir();
    let mut m = Mcp::spawn(d.path());
    let r = m.result("tools/list", json!({}));
    let tools = r["tools"].as_array().unwrap();
    let find = |n: &str| tools.iter().find(|t| t["name"] == n).cloned();
    for n in [
        "track_new_audio",
        "transport_play_stop",
        "file_export_midi",
        "begin_request",
        "end_request",
        "change",
        "apply_change",
        "screenshot",
    ] {
        assert!(find(n).is_some(), "{n} is a tool");
    }
    assert_eq!(
        find("track_new_audio").unwrap()["annotations"]["destructiveHint"],
        false
    );
    assert_eq!(
        find("transport_play_stop").unwrap()["annotations"]["readOnlyHint"],
        true
    );
    for n in [
        "ui_click",
        "media_compact",
        "transport_record",
        "track_delete",
        "settings_co_producer",
    ] {
        assert!(find(n).is_none(), "{n} is not a tool");
    }
    let mut names = std::collections::HashSet::new();
    for t in tools {
        let name = t["name"].as_str().unwrap();
        assert!(
            !name.is_empty()
                && name.len() <= 64
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)),
            "{name}"
        );
        assert!(names.insert(name.to_string()), "{name} is listed once");
        let s = &t["inputSchema"];
        assert_eq!(s["type"], "object", "{name}");
        assert_eq!(s["additionalProperties"], false, "{name}");
        assert!(s["properties"].get("replace").is_none(), "{name}");
        assert!(t["description"].as_str().is_some_and(|d| !d.is_empty()));
    }
    assert_eq!(
        find("file_export_midi").unwrap()["inputSchema"]["properties"]["path"]["maxLength"],
        1024
    );
}

#[test]
fn tools_reach_the_app() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let r = m.call("track_new_audio", json!({"seq": 1}));
    assert!(!is_error(&r), "{r}");
    assert_eq!(
        serde_json::from_str::<Value>(&text(&r)).unwrap(),
        json!({"status": "ran track.new_audio"})
    );
    assert_eq!(
        app.runs(),
        vec![run_of("track.new_audio", vec![("seq", ArgValue::Int(1))])]
    );
    app.script(Answer::Err(ControlError::new(
        ErrorCode::NoDocument,
        "no document is open",
    )));
    let r = m.call("track_new_audio", json!({}));
    assert!(is_error(&r), "{r}");
    assert!(text(&r).contains("no_document"), "{r}");
    assert!(text(&r).contains("no document is open"), "{r}");
    // A step of the open request says so.
    app.script(Answer::Ok(
        json!({"status": "ran", "undo": "Co-Producer: Set Up a Drum Bus"}),
    ));
    let r = m.call("track_new_midi", json!({}));
    assert!(
        text(&r).ends_with("\nUndo step: Co-Producer: Set Up a Drum Bus"),
        "{r}"
    );
    let r = m.call("begin_request", json!({"name": "x"}));
    assert!(!is_error(&r), "{r}");
    assert!(
        matches!(app.seen().last(), Some(Command::CoProducerBegin { name, .. }) if name == "x"),
        "{:?}",
        app.seen()
    );
    let r = m.call("end_request", json!({}));
    assert!(!is_error(&r), "{r}");
    assert!(matches!(
        app.seen().last(),
        Some(Command::CoProducerEnd { .. })
    ));
}

#[test]
fn resources_read_state() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let r = m.result("resources/list", json!({}));
    let uris: Vec<&str> = r["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["uri"].as_str().unwrap())
        .collect();
    for u in [
        "vizdaw://state/document",
        "vizdaw://state/mixer",
        "vizdaw://commands",
    ] {
        assert!(uris.contains(&u), "{u} in {uris:?}");
    }
    assert!(!uris.contains(&"vizdaw://state/input"));
    let r = m.result("resources/read", json!({"uri": "vizdaw://state/mixer"}));
    let c = &r["contents"][0];
    assert_eq!(c["uri"], "vizdaw://state/mixer");
    assert_eq!(c["mimeType"], "application/json");
    assert_eq!(
        serde_json::from_str::<Value>(c["text"].as_str().unwrap()).unwrap(),
        json!({"target": "Mixer"})
    );
    assert!(matches!(
        app.seen().last(),
        Some(Command::StateGet {
            target: core_control::request::StateTarget::Mixer,
            ..
        })
    ));
    let r = m.result("resources/read", json!({"uri": "vizdaw://commands"}));
    let list: Value = serde_json::from_str(r["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(list, core_control::co_producer::list_json());
    for uri in [
        "vizdaw://state/input",
        "vizdaw://nothing",
        "file:///etc/passwd",
    ] {
        assert_eq!(
            m.error_code("resources/read", json!({"uri": uri})),
            -32602,
            "{uri}"
        );
    }
    assert_eq!(m.error_code("resources/read", json!({})), -32602);
}

#[test]
fn screenshot_returns_an_image() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let r = m.call("screenshot", json!({}));
    assert!(!is_error(&r), "{r}");
    assert_eq!(
        r["content"][0],
        json!({"type": "image", "mimeType": "image/png", "data": vizdaw_mcp::base64::encode(PNG)})
    );
    assert!(
        !d.path().join("screenshot-1.png").exists(),
        "the screenshot file is deleted"
    );
    // A path outside the support folder is refused and left alone.
    // It has a screenshot's name, so only its folder is wrong.
    let elsewhere = tempfile::tempdir().unwrap();
    let other = elsewhere.path().join("screenshot-5.png");
    std::fs::write(&other, b"keep").unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    app.script(Answer::ScreenshotAt(other.to_path_buf()));
    let r = m.call("screenshot", json!({}));
    assert!(is_error(&r), "{r}");
    assert_eq!(std::fs::read(&other).unwrap(), b"keep");
    // So is a link with a screenshot's name in the folder.
    let link = d.path().join("screenshot-77.png");
    std::os::unix::fs::symlink(&other, &link).unwrap();
    std::thread::sleep(Duration::from_millis(1100));
    app.script(Answer::ScreenshotAt(link.clone()));
    let r = m.call("screenshot", json!({}));
    assert!(is_error(&r), "{r}");
    assert!(std::fs::symlink_metadata(&link).is_ok(), "the link is left");
    assert_eq!(std::fs::read(&other).unwrap(), b"keep");
}

#[test]
fn hostile_messages_never_end_the_server() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let raw = |m: &mut Mcp, line: &str| -> Value {
        m.send_raw(line);
        m.recv()
    };
    let r = raw(&mut m, "this is not json");
    assert_eq!(r["error"]["code"], -32700, "{r}");
    assert_eq!(r["id"], Value::Null);
    m.ping_answers();
    let r = raw(&mut m, &"x".repeat(2 << 20));
    assert_eq!(r["error"]["code"], -32700, "{r}");
    m.ping_answers();
    let r = raw(&mut m, r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#);
    assert_eq!(r["error"]["code"], -32600, "{r}");
    m.ping_answers();
    let r = raw(&mut m, r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#);
    assert_eq!(r["error"]["code"], -32600, "{r}");
    m.ping_answers();
    let r = raw(&mut m, r#"{"jsonrpc":"2.0","id":"s-1","method":"nope"}"#);
    assert_eq!(r["error"]["code"], -32601, "{r}");
    assert_eq!(r["id"], "s-1", "a string id is kept as given");
    m.ping_answers();
    assert_eq!(m.error_code("tools/call", json!({})), -32602);
    m.ping_answers();
    assert_eq!(
        m.error_code("tools/call", json!({"name": "ui_click"})),
        -32602
    );
    m.ping_answers();
    let r = m.result(
        "tools/call",
        json!({"name": "track_new_audio", "arguments": "seq=1"}),
    );
    assert!(is_error(&r), "{r}");
    m.ping_answers();
    let r = m.call("track_new_audio", json!({"seq": "1"}));
    assert!(is_error(&r), "{r}");
    assert!(text(&r).contains("seq"), "{r}");
    m.ping_answers();
    // A NUL reaches the app's own refusal.
    let r = m.call("file_export_midi", json!({"path": "/tmp/a\u{0}b.mid"}));
    assert!(is_error(&r), "{r}");
    assert!(text(&r).contains("invalid_params"), "{r}");
    m.ping_answers();
    let long = "x".repeat(70_000);
    let r = m.call("begin_request", json!({ "name": long }));
    assert!(is_error(&r), "{r}");
    let r = m.call("track_new_audio", json!({ long.as_str(): 1 }));
    assert!(is_error(&r), "{r}");
    m.ping_answers();
    let deep = format!(
        r#"{{"jsonrpc":"2.0","id":9,"method":"ping","params":{}{}}}"#,
        "[".repeat(10_000),
        "]".repeat(10_000)
    );
    let r = raw(&mut m, &deep);
    assert_eq!(r["error"]["code"], -32700, "{r}");
    m.ping_answers();
    assert!(
        app.runs().is_empty(),
        "no hostile call ran: {:?}",
        app.runs()
    );
    // EOF ends it cleanly.
    drop(m.stdin.take());
    assert!(m.child.wait().unwrap().success());
}

#[test]
fn says_why_it_cannot_reach_vizdaw() {
    let d = support_dir();
    std::fs::write(d.path().join("co-producer.off"), b"").unwrap();
    let mut m = Mcp::spawn(d.path());
    let r = m.call("track_new_audio", json!({}));
    assert!(is_error(&r), "{r}");
    assert_eq!(text(&r), "Co-Producer is turned off in VizDAW's Settings.");
    std::fs::remove_file(d.path().join("co-producer.off")).unwrap();
    let r = m.call("track_new_audio", json!({}));
    assert!(is_error(&r), "{r}");
    assert_eq!(
        text(&r),
        "VizDAW is not running, or Co-Producer is off in its Settings."
    );
    // tools/list needs no app.
    assert!(m.result("tools/list", json!({}))["tools"].is_array());
}

#[test]
fn reconnects_after_the_app_drops_it() {
    let d = support_dir();
    let mut app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    assert!(!is_error(&m.call("track_new_audio", json!({"seq": 1}))));
    app.stop();
    drop(app);
    let app = FakeApp::start(d.path());
    let r = m.call("track_new_audio", json!({"seq": 2}));
    assert!(!is_error(&r), "{r}");
    assert_eq!(
        app.runs(),
        vec![run_of("track.new_audio", vec![("seq", ArgValue::Int(2))])],
        "the new app saw the call once"
    );
}

#[test]
fn a_call_that_was_sent_is_never_sent_twice() {
    let d = support_dir();
    let mut dropping = DroppingApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let r = m.call("track_new_audio", json!({"seq": 7}));
    assert!(is_error(&r), "{r}");
    assert_eq!(dropping.lines().len(), 1, "{:?}", dropping.lines());
    assert!(dropping.lines()[0].contains("track.new_audio"));
    dropping.stop();
    let app = FakeApp::start(d.path());
    let r = m.call("transport_stop", json!({}));
    assert!(!is_error(&r), "{r}");
    assert_eq!(app.runs(), vec![run_of("transport.stop", vec![])]);
}

#[test]
fn base64_matches_rfc_4648() {
    use vizdaw_mcp::base64::encode;
    for (input, want) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(encode(input.as_bytes()), want, "{input:?}");
    }
}
