//! `vizdaw-mcp` end to end: the built binary over pipes, a fake app on a
//! temp Co-Producer socket (plan 8a Task 5, Rulings 1, 2, 10 and 12).
//! Every child gets `VIZDAW_SUPPORT_DIR`; this process never sets it.

#[path = "support/fake_app.rs"]
mod fake_app;

use core_control::registry::ArgValue;
use core_control::{Command, ControlError, ErrorCode};
use fake_app::{Answer, DroppingApp, FakeApp, PNG, preview_token, support_dir, two_items};
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

const INSTRUCTIONS: &str = "VizDAW's Co-Producer. Adds are free; anything that changes or deletes the composer's own work goes through the change tool and their yes. Read the song's notes first (read_notes) and follow what the composer wrote there. Start every request with begin_request and finish it with end_request.";

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
        app.seen()
            .iter()
            .any(|c| matches!(c, Command::CoProducerBegin { name, .. } if name == "x")),
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

/// The child's resident memory in KiB.
fn rss_kib(pid: u32) -> u64 {
    let out = Process::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
}

#[test]
fn a_flood_while_a_call_waits_stays_bounded() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(Answer::After(
        Duration::from_secs(4),
        json!({"status": "slow"}),
    ));
    m.send(
        &json!({"jsonrpc": "2.0", "id": "slow", "method": "tools/call",
        "params": {"name": "track_new_audio", "arguments": {}}}),
    );
    // 150 lines of 900 KB (135 MB) written while the call waits.
    let mut stdin = m.stdin.take().unwrap();
    let flood = std::thread::spawn(move || {
        let pad = "x".repeat(900_000);
        for i in 0..150 {
            let line = json!({"jsonrpc": "2.0", "id": i, "method": "ping", "params": {"pad": pad}});
            stdin.write_all(format!("{line}\n").as_bytes()).unwrap();
        }
        stdin
    });
    std::thread::sleep(Duration::from_millis(2500));
    let kib = rss_kib(m.child.id());
    assert!(
        kib < 64 * 1024,
        "vizdaw-mcp holds {kib} KiB while a call waits"
    );
    m.stdin = Some(flood.join().unwrap());
    let first = m.recv();
    assert_eq!(first["id"], "slow", "{first}");
    for i in 0..150 {
        let r = m.recv();
        assert_eq!(r["id"], i, "{r}");
        assert_eq!(r["result"], json!({}), "{r}");
    }
    m.ping_answers();
}

#[test]
fn it_will_not_start_without_an_absolute_home() {
    for home in [None, Some("relative/home")] {
        let mut p = Process::new(env!("CARGO_BIN_EXE_vizdaw-mcp"));
        p.env_remove("VIZDAW_SUPPORT_DIR").env_remove("HOME");
        if let Some(h) = home {
            p.env("HOME", h);
        }
        let out = p
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .unwrap();
        assert!(!out.status.success(), "{home:?}");
        assert!(out.stdout.is_empty());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("HOME is not set to an absolute path"), "{err}");
    }
}

#[test]
fn it_will_not_connect_in_a_folder_others_can_write() {
    use std::os::unix::fs::PermissionsExt;
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o770)).unwrap();
    let r = m.call("track_new_audio", json!({}));
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(is_error(&r), "{r}");
    assert!(text(&r).contains("can be written by other users"), "{r}");
    assert!(app.runs().is_empty());
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

// ---- Task 6: the change tool (Ruling 8, P5, P6) ----

const SAID_NO: &str = "The composer said no. Nothing changed.";
const UNREADABLE: &str = "The composer's answer could not be read. Nothing changed.";
const EXPIRED: &str = "This question expired; call change again.";
const MESSAGE: &str = "Co-Producer wants to change your work:\n- Track 2, Vox: deleted\n- Track 3, Bass: renamed to \"Low\"\n\nAllow these changes?";

/// The plan the fake previews as two items of the composer's work.
fn two_changes() -> Value {
    json!([
        {"id": "track.delete", "params": {"seq": 1, "lane": 2}},
        {"id": "track.rename", "params": {"seq": 1, "lane": 3, "name": "Low"}},
    ])
}

fn schema() -> Value {
    json!({"type": "object", "properties": {"allow": {"type": "boolean", "title": "Allow these changes", "default": false}}, "required": ["allow"]})
}

fn previews(app: &FakeApp) -> Vec<Command> {
    app.seen()
        .into_iter()
        .filter(|c| matches!(c, Command::CoProducerPreview { .. }))
        .collect()
}

impl Mcp {
    /// Spawned and initialized as a client that can show a form.
    fn with_elicitation(support: &Path) -> Mcp {
        let mut m = Mcp::spawn(support);
        m.result(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {"elicitation": {}}}),
        );
        m.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        m
    }

    /// Sends a `change` call with id `id`, and returns the elicitation
    /// request it brings.
    fn ask(&mut self, id: &str, commands: &Value) -> Value {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": {"name": "change", "arguments": {"commands": commands}}}));
        let e = self.recv();
        assert_eq!(e["method"], "elicitation/create", "{e}");
        assert!(
            e["id"]
                .as_str()
                .is_some_and(|i| i.starts_with("vz-elicit-")),
            "{e}"
        );
        e
    }

    fn answer(&mut self, elicit: &Value, result: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": elicit["id"], "result": result}));
    }

    /// The reply to call `id`.
    fn reply_to(&self, id: &str) -> Value {
        let r = self.recv();
        assert_eq!(r["id"], id, "{r}");
        assert!(r.get("error").is_none(), "{r}");
        r["result"].clone()
    }
}

#[test]
fn no_yes_needed_applies_at_once() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::with_elicitation(d.path());
    let r = m.call("change", json!({"commands": [{"id": "edit.undo"}]}));
    assert!(!is_error(&r), "{r}");
    assert!(text(&r).contains("\"results\""), "{r}");
    assert!(text(&r).contains("Undo step: Co-Producer: Tidy Up"), "{r}");
    let seen = app.seen();
    assert_eq!(seen.len(), 2, "{seen:?}");
    assert!(matches!(seen[0], Command::CoProducerPreview { .. }));
    match &seen[1] {
        Command::CoProducerApply { token, plan, .. } => {
            assert_eq!(token, &preview_token(1));
            assert_eq!(plan.len(), 1);
            assert_eq!(plan[0].id, "edit.undo");
        }
        other => panic!("{other:?}"),
    }
    m.ping_answers();
}

#[test]
fn legacy_elicitation_accept_applies() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::with_elicitation(d.path());
    let e = m.ask("c1", &two_changes());
    assert_eq!(e["params"]["message"], MESSAGE);
    assert_eq!(e["params"]["requestedSchema"], schema());
    assert!(app.applies().is_empty(), "nothing applied before the yes");
    m.answer(&e, json!({"action": "accept", "content": {"allow": true}}));
    let r = m.reply_to("c1");
    assert!(!is_error(&r), "{r}");
    let t = text(&r);
    assert!(t.contains("\"results\""), "{t}");
    assert!(
        !t.contains(&preview_token(1)),
        "no token reaches the model: {t}"
    );
    let previewed = match &previews(&app)[0] {
        Command::CoProducerPreview { plan, .. } => plan.clone(),
        other => panic!("{other:?}"),
    };
    match app.applies().as_slice() {
        [Command::CoProducerApply { token, plan, .. }] => {
            assert_eq!(token, &preview_token(1));
            assert_eq!(plan, &previewed);
        }
        other => panic!("{other:?}"),
    }
    // The token was never handed out, so apply_change refuses it.
    let r = m.call(
        "apply_change",
        json!({"token": preview_token(1), "commands": two_changes()}),
    );
    assert!(is_error(&r), "{r}");
    assert_eq!(app.applies().len(), 1);
}

#[test]
fn legacy_elicitation_no_changes_nothing() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::with_elicitation(d.path());
    let answers = [
        json!({"result": {"action": "decline"}}),
        json!({"result": {"action": "cancel"}}),
        json!({"result": {"action": "accept", "content": {"allow": false}}}),
        json!({"result": {"action": "accept", "content": {"allow": "yes"}}}),
        json!({"error": {"code": -32603, "message": "the host failed"}}),
    ];
    for (i, a) in answers.iter().enumerate() {
        let id = format!("n{i}");
        let e = m.ask(&id, &two_changes());
        let mut line = a.clone();
        line["jsonrpc"] = json!("2.0");
        line["id"] = e["id"].clone();
        m.send(&line);
        let r = m.reply_to(&id);
        assert!(!is_error(&r), "{a}: {r}");
        assert_eq!(text(&r), SAID_NO, "{a}");
    }
    assert!(app.applies().is_empty(), "{:?}", app.applies());
    m.ping_answers();
}

#[test]
fn legacy_elicitation_meanwhile() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::with_elicitation(d.path());
    let e = m.ask("c1", &two_changes());
    m.send(&json!({"jsonrpc": "2.0", "id": "list", "method": "tools/list"}));
    m.send(&json!({"jsonrpc": "2.0", "id": "p", "method": "ping"}));
    let r = m.recv();
    assert_eq!(r["id"], "p", "a ping is answered at once: {r}");
    m.silent_for(Duration::from_millis(300));
    m.answer(&e, json!({"action": "accept", "content": {"allow": true}}));
    assert!(!is_error(&m.reply_to("c1")));
    let r = m.reply_to("list");
    assert!(r["tools"].is_array(), "{r}");
    assert_eq!(app.applies().len(), 1);
    // A cancel ends the wait with no apply and no reply.
    let e = m.ask("c2", &two_changes());
    m.send(
        &json!({"jsonrpc": "2.0", "method": "notifications/cancelled",
        "params": {"requestId": "c2", "reason": "the user stopped it"}}),
    );
    m.silent_for(Duration::from_millis(300));
    // A late answer to the cancelled question is ignored.
    m.answer(&e, json!({"action": "accept", "content": {"allow": true}}));
    m.silent_for(Duration::from_millis(300));
    m.ping_answers();
    assert_eq!(app.applies().len(), 1, "{:?}", app.applies());
}

/// More lines than vizdaw-mcp keeps arrive during a wait: the extra
/// requests are refused at once, nothing blocks, and the composer's
/// answer still arrives and applies.
#[test]
fn a_full_queue_never_drops_the_answer() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::with_elicitation(d.path());
    let e = m.ask("c1", &two_changes());
    for i in 0..40 {
        m.send(&json!({"jsonrpc": "2.0", "id": i, "method": "resources/list"}));
    }
    let mut refused = Vec::new();
    for _ in 0..24 {
        let r = m.recv();
        assert_eq!(r["error"]["code"], -32001, "{r}");
        refused.push(r["id"].as_i64().unwrap());
    }
    assert_eq!(refused, (16..40).collect::<Vec<_>>());
    m.answer(&e, json!({"action": "accept", "content": {"allow": true}}));
    assert!(!is_error(&m.reply_to("c1")));
    for i in 0..16 {
        let r = m.recv();
        assert_eq!(r["id"], i, "{r}");
        assert!(r["result"]["resources"].is_array(), "{r}");
    }
    assert_eq!(app.applies().len(), 1);
    m.ping_answers();
}

fn modern(extra: Value) -> Value {
    let mut p = json!({"_meta": {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": {"elicitation": {}},
    }});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

#[test]
fn modern_elicitation_round_trip() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let ask = |m: &mut Mcp| -> String {
        let r = m.result(
            "tools/call",
            modern(json!({"name": "change", "arguments": {"commands": two_changes()}})),
        );
        assert_eq!(r["resultType"], "input_required", "{r}");
        let q = &r["inputRequests"]["yes"];
        assert_eq!(q["method"], "elicitation/create", "{r}");
        assert_eq!(q["params"]["message"], MESSAGE);
        assert_eq!(q["params"]["requestedSchema"], schema());
        let state = r["requestState"].as_str().unwrap().to_string();
        assert!(
            state.len() == 24 && state.bytes().all(|b| b.is_ascii_hexdigit()),
            "{state}"
        );
        assert!(
            !r.to_string().contains("feed"),
            "no token in the question: {r}"
        );
        state
    };
    let yes = json!({"yes": {"action": "accept", "content": {"allow": true}}});
    let state = ask(&mut m);
    assert!(app.applies().is_empty());
    let retry = |state: &str, commands: Value| {
        modern(
            json!({"name": "change", "arguments": {"commands": commands},
            "inputResponses": yes, "requestState": state}),
        )
    };
    let r = m.result("tools/call", retry(&state, two_changes()));
    assert_eq!(r["resultType"], "complete", "{r}");
    assert!(!is_error(&r), "{r}");
    assert!(text(&r).contains("\"results\""), "{r}");
    assert_eq!(app.applies().len(), 1);
    // One-time.
    let r = m.result("tools/call", retry(&state, two_changes()));
    assert!(is_error(&r), "{r}");
    assert_eq!(text(&r), EXPIRED);
    assert_eq!(app.applies().len(), 1);
    // The model writes `arguments`: a yes there is not the composer's.
    let state = ask(&mut m);
    let r = m.result(
        "tools/call",
        modern(
            json!({"name": "change", "arguments": {"commands": two_changes(),
            "inputResponses": yes, "requestState": state}}),
        ),
    );
    assert!(is_error(&r), "{r}");
    assert_eq!(app.applies().len(), 1);
    // Other commands than were asked about: refused, and the state is gone.
    let other = json!([{"id": "track.delete", "params": {"seq": 1, "lane": 9}}]);
    let r = m.result("tools/call", retry(&state, other));
    assert!(is_error(&r), "{r}");
    assert_eq!(text(&r), EXPIRED);
    let r = m.result("tools/call", retry(&state, two_changes()));
    assert_eq!(text(&r), EXPIRED, "{r}");
    assert_eq!(app.applies().len(), 1);
    // A no.
    let state = ask(&mut m);
    let r = m.result(
        "tools/call",
        modern(
            json!({"name": "change", "arguments": {"commands": two_changes()},
            "inputResponses": {"yes": {"action": "decline"}}, "requestState": state}),
        ),
    );
    assert!(!is_error(&r), "{r}");
    assert_eq!(text(&r), SAID_NO);
    // The capability may also sit beside `_meta`.
    let r = m.result(
        "tools/call",
        json!({"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"},
            "clientCapabilities": {"elicitation": {}},
            "name": "change", "arguments": {"commands": two_changes()}}),
    );
    assert_eq!(r["resultType"], "input_required", "{r}");
    assert_eq!(app.applies().len(), 1);
}

#[test]
fn an_unanswered_question_is_a_no() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut s =
        vizdaw_mcp::Server::with_ask_timeout(d.path().to_path_buf(), Duration::from_millis(50));
    let line = |v: Value| v.to_string();
    let parse = |l: &str| -> Value { serde_json::from_str(l).unwrap() };
    s.handle_line(&line(
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {"elicitation": {}}}}),
    ));
    let out = s.handle_line(&line(
        json!({"jsonrpc": "2.0", "id": "c", "method": "tools/call",
        "params": {"name": "change", "arguments": {"commands": two_changes()}}}),
    ));
    assert_eq!(out.len(), 1, "{out:?}");
    assert_eq!(parse(&out[0])["method"], "elicitation/create");
    let out = s.handle_line(&line(
        json!({"jsonrpc": "2.0", "id": "l", "method": "tools/list"}),
    ));
    assert!(out.is_empty(), "queued: {out:?}");
    assert!(s.tick().is_empty(), "not yet");
    std::thread::sleep(Duration::from_millis(80));
    let out: Vec<Value> = s.tick().iter().map(|l| parse(l)).collect();
    let reply = out
        .iter()
        .find(|v| v["id"] == "c")
        .unwrap_or_else(|| panic!("{out:?}"));
    assert_eq!(
        reply["result"]["content"][0]["text"],
        "The composer did not answer. Nothing changed."
    );
    assert_eq!(reply["result"]["isError"], false);
    let list = out
        .iter()
        .position(|v| v["id"] == "l")
        .unwrap_or_else(|| panic!("{out:?}"));
    assert!(out[list]["result"]["tools"].is_array());
    assert!(out.iter().position(|v| v["id"] == "c").unwrap() < list);
    assert!(app.applies().is_empty());
    assert!(s.tick().is_empty());
}

#[test]
fn without_elicitation_the_model_gets_the_list_and_a_token() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    m.result(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}}),
    );
    let r = m.call("change", json!({"commands": two_changes()}));
    assert!(!is_error(&r), "{r}");
    let token = preview_token(1);
    assert_eq!(
        text(&r),
        format!(
            "- Track 2, Vox: deleted\n- Track 3, Bass: renamed to \"Low\"\nShow this list to the composer and ask for a yes. Then call apply_change with token {token} and the same commands."
        )
    );
    assert!(app.applies().is_empty());
    let r = m.call(
        "apply_change",
        json!({"token": token, "commands": two_changes()}),
    );
    assert!(!is_error(&r), "{r}");
    match app.applies().as_slice() {
        [Command::CoProducerApply { token: t, .. }] => assert_eq!(t, &token),
        other => panic!("{other:?}"),
    }
    let before = app.seen().len();
    for t in ["0".repeat(32), token.clone()] {
        let r = m.call(
            "apply_change",
            json!({"token": t, "commands": two_changes()}),
        );
        assert!(is_error(&r), "{r}");
        assert_eq!(
            text(&r),
            "Use the token change returned in this conversation."
        );
    }
    assert_eq!(app.seen().len(), before, "refused before reaching the app");
    let _ = two_items();
}

#[test]
fn the_list_is_capped() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let items: Vec<Value> = (1..=45)
        .map(
            |i| json!({"thing": format!("Track {i}"), "change": "deleted", "composers_work": true}),
        )
        .collect();
    app.script(Answer::Ok(
        json!({"token": preview_token(1), "needs_yes": true, "items": items}),
    ));
    let r = m.call("change", json!({"commands": two_changes()}));
    let t = text(&r);
    let lines: Vec<&str> = t.lines().filter(|l| l.starts_with("- ")).collect();
    assert_eq!(lines.len(), 41, "{t}");
    assert_eq!(lines[39], "- Track 40: deleted");
    assert_eq!(lines[40], "- and 5 more");
}

#[test]
fn change_commands_are_checked_first() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let many: Vec<Value> = (0..33).map(|_| json!({"id": "edit.undo"})).collect();
    let long = "x".repeat(70_000);
    for bad in [
        json!({}),
        json!({"commands": "edit.undo"}),
        json!({"commands": []}),
        json!({"commands": many}),
        json!({"commands": [7]}),
        json!({"commands": [{}]}),
        json!({"commands": [{"id": 7}]}),
        json!({"commands": [{"id": long}]}),
        json!({"commands": [{"id": "edit.undo", "params": [1]}]}),
        json!({"commands": [{"id": "edit.undo", "extra": 1}]}),
        json!({"commands": [{"id": "edit.undo"}], "nope": 1}),
        json!({"commands": [{"id": "edit.undo"}], "window": 0}),
    ] {
        for tool in ["change", "apply_change"] {
            let mut args = bad.clone();
            if tool == "apply_change" {
                args["token"] = json!(preview_token(1));
            }
            let r = m.call(tool, args.clone());
            assert!(is_error(&r), "{tool} {args}: {r}");
        }
    }
    for t in [
        json!(7),
        json!("x".repeat(70_000)),
        json!("A".repeat(32)),
        json!(null),
    ] {
        let r = m.call(
            "apply_change",
            json!({"token": t, "commands": [{"id": "edit.undo"}]}),
        );
        assert!(is_error(&r), "{r}");
    }
    assert!(app.seen().is_empty(), "{:?}", app.seen());
}

#[test]
fn hostile_elicitation_replies() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::with_elicitation(d.path());
    let e = m.ask("c1", &two_changes());
    // An answer to a question never asked is ignored.
    m.send(&json!({"jsonrpc": "2.0", "id": "vz-elicit-999", "result": {"action": "accept", "content": {"allow": true}}}));
    m.send(&json!({"jsonrpc": "2.0", "id": 5, "result": {"action": "accept", "content": {"allow": true}}}));
    m.send(&json!({"jsonrpc": "2.0", "id": "c1", "result": {"action": "accept", "content": {"allow": true}}}));
    m.silent_for(Duration::from_millis(300));
    m.ping_answers();
    // Content that is an array is not a yes.
    m.answer(
        &e,
        json!({"action": "accept", "content": [{"allow": true}]}),
    );
    let r = m.reply_to("c1");
    assert!(is_error(&r), "{r}");
    assert_eq!(text(&r), UNREADABLE);
    let long = "x".repeat(70_000);
    let hostile = [
        (
            json!({"action": long, "content": {"allow": true}}),
            UNREADABLE,
        ),
        (
            json!({"action": "accept", "content": {"allow": true, "message": long}}),
            UNREADABLE,
        ),
        (
            json!({"action": "accept", "content": {"allow": long}}),
            SAID_NO,
        ),
        (json!({"action": "accept"}), SAID_NO),
        (json!("accept"), UNREADABLE),
        (json!(null), UNREADABLE),
    ];
    for (i, (h, want)) in hostile.iter().enumerate() {
        let id = format!("h{i}");
        let e = m.ask(&id, &two_changes());
        m.answer(&e, h.clone());
        let r = m.reply_to(&id);
        assert_eq!(text(&r), *want, "{h}");
        assert_eq!(is_error(&r), *want == UNREADABLE, "{r}");
    }
    assert!(app.applies().is_empty(), "{:?}", app.applies());
    // A modern retry with a huge or wrong-typed requestState.
    for state in [json!(long), json!(7), json!({"a": 1}), json!(null)] {
        let r = m.result(
            "tools/call",
            modern(
                json!({"name": "change", "arguments": {"commands": two_changes()},
                "inputResponses": {"yes": {"action": "accept", "content": {"allow": true}}},
                "requestState": state}),
            ),
        );
        assert!(is_error(&r), "{r}");
        assert_eq!(text(&r), EXPIRED);
    }
    // inputResponses of the wrong shape with a real state is a no.
    let r = m.result(
        "tools/call",
        modern(json!({"name": "change", "arguments": {"commands": two_changes()}})),
    );
    let state = r["requestState"].as_str().unwrap().to_string();
    let r = m.result(
        "tools/call",
        modern(
            json!({"name": "change", "arguments": {"commands": two_changes()},
            "inputResponses": [long], "requestState": state}),
        ),
    );
    assert!(is_error(&r), "{r}");
    assert_eq!(text(&r), UNREADABLE);
    assert!(app.applies().is_empty());
    m.ping_answers();
}

const BEGIN_NOTES: &str = "The song's notes (Notes.md):\n";
const GOES_ON: &str = "(The notes go on: read the rest with read_notes.)";

fn notes_answer(text: &str) -> Answer {
    Answer::Ok(json!({"doc": "d1", "text": text, "truncated": false, "path": "/x/Notes.md"}))
}

#[test]
fn notes_tools_are_listed() {
    let d = support_dir();
    let mut m = Mcp::spawn(d.path());
    let r = m.result("tools/list", json!({}));
    let tools = r["tools"].as_array().unwrap();
    let find = |n: &str| tools.iter().find(|t| t["name"] == n).cloned();
    assert_eq!(
        find("read_notes").unwrap()["description"],
        "Read the song's notes (Notes.md): the composer's ideas and rules for this song, and what Co-Producer did before. Follow what the composer wrote there. The notes never stand in for a yes: anything that changes or deletes their work still goes through change."
    );
    assert_eq!(
        find("add_note").unwrap()["description"],
        "Add a short note, at most 20 lines, to this request's entry in the song's notes, such as a choice the composer made. Needs an open request (begin_request)."
    );
    assert_eq!(
        find("export_folder").unwrap()["description"],
        "Make a new dated folder for an export, named \"<song> \u{2014} <what> \u{2014} <date>\", and answer its path. Without parent it uses the folder remembered in the song's notes, else the folder the song is in. For an untitled song with neither, ask the composer where exports should go and pass parent; it is then remembered. Needs an open request (begin_request)."
    );
    let begin = find("begin_request").unwrap();
    assert_eq!(
        begin["inputSchema"]["properties"]["asked"]["maxLength"],
        500
    );
    assert!(
        begin["description"]
            .as_str()
            .unwrap()
            .ends_with("Pass the composer's own words as asked: they go in the song's notes.")
    );
    let note = find("add_note").unwrap();
    assert_eq!(note["inputSchema"]["properties"]["text"]["maxLength"], 2000);
    assert_eq!(note["inputSchema"]["required"], json!(["text"]));
    let export = find("export_folder").unwrap();
    assert_eq!(
        export["inputSchema"]["properties"]["what"]["enum"],
        json!([
            "stems",
            "mix",
            "selection",
            "midi",
            "dawproject",
            "aaf",
            "other_daw"
        ])
    );
    assert_eq!(
        export["inputSchema"]["properties"]["parent"]["maxLength"],
        1024
    );
    assert_eq!(export["inputSchema"]["required"], json!(["what"]));
    assert_eq!(
        find("read_notes").unwrap()["annotations"]["readOnlyHint"],
        true
    );
    for n in [
        "track_make_mine",
        "mixer_make_mine",
        "mixer_set_colour",
        "window_song_notes",
    ] {
        assert!(find(n).is_none(), "{n} is not a tool");
    }
}

#[test]
fn the_first_request_reads_the_notes() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let r = m.call(
        "begin_request",
        json!({"name": "set up a bus", "asked": "set up a bus please"}),
    );
    assert!(!is_error(&r), "{r}");
    assert!(
        text(&r).ends_with(&format!("{BEGIN_NOTES}Keep the drums dry.\n")),
        "{r}"
    );
    let seen = app.seen();
    assert!(
        matches!(&seen[..], [
            Command::CoProducerBegin { name, asked: Some(a), .. },
            Command::CoProducerNotes { .. },
        ] if name == "set up a bus" && a == "set up a bus please"),
        "{seen:?}"
    );
    m.call("end_request", json!({}));
    let r = m.call("begin_request", json!({"name": "again"}));
    assert!(!is_error(&r), "{r}");
    assert!(!text(&r).contains("Notes.md"), "{r}");
    let notes = app
        .seen()
        .iter()
        .filter(|c| matches!(c, Command::CoProducerNotes { .. }))
        .count();
    assert_eq!(notes, 1);
}

#[test]
fn long_notes_are_cut_on_a_character() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    // 65 535 bytes, then 4-byte characters: the cut at 65 536 is inside one.
    let long = format!("{}{}", "a".repeat(65_535), "\u{1F600}".repeat(10_000));
    assert!(long.len() > 100 * 1024);
    app.script(Answer::Ok(json!({"doc": "d1", "begun": "x"})));
    app.script(notes_answer(&long));
    let r = m.call("begin_request", json!({"name": "x"}));
    let t = text(&r);
    let at = t.find(BEGIN_NOTES).unwrap() + BEGIN_NOTES.len();
    assert_eq!(
        &t[at..],
        format!("{}\n{GOES_ON}", "a".repeat(65_535)),
        "cut whole"
    );
}

#[test]
fn notes_tools_reach_the_app() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let r = m.call("read_notes", json!({}));
    assert_eq!(text(&r), "Keep the drums dry.\n");
    assert!(!is_error(&r));
    let r = m.call("add_note", json!({"text": "Kept the kick dry."}));
    assert!(!is_error(&r), "{r}");
    let parent = tempfile::Builder::new()
        .prefix("vzx")
        .tempdir_in("/tmp")
        .unwrap();
    let r = m.call(
        "export_folder",
        json!({"what": "stems", "parent": parent.path()}),
    );
    assert_eq!(text(&r), "/x/Song \u{2014} Stems \u{2014} 2026-11-16");
    let seen = app.seen();
    assert!(
        matches!(&seen[..], [
            Command::CoProducerNotes { .. },
            Command::CoProducerNote { text, .. },
            Command::CoProducerExportFolder { parent: Some(p), .. },
        ] if text == "Kept the kick dry." && p == parent.path()),
        "{seen:?}"
    );
}

#[test]
fn a_failed_notes_read_leaves_the_begin_answer() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(Answer::Ok(json!({"doc": "d1", "begun": "x"})));
    app.script(Answer::Err(ControlError::new(
        ErrorCode::Failed,
        "no notes",
    )));
    let r = m.call("begin_request", json!({"name": "x"}));
    assert!(!is_error(&r), "{r}");
    assert!(!text(&r).contains("Notes.md"), "{r}");
    assert!(text(&r).contains("begun"), "{r}");
}

#[test]
fn notes_tools_refuse_hostile_arguments() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let long = "x".repeat(70_000);
    for (tool, args) in [
        ("add_note", json!({})),
        ("add_note", json!({"text": 5})),
        ("add_note", json!({ "text": long })),
        ("add_note", json!({"text": "ok", "extra": 1})),
        ("export_folder", json!({"what": "video"})),
        ("export_folder", json!({"what": "stems", "extra": 1})),
        ("export_folder", json!({"what": "stems", "parent": 3})),
        (
            "export_folder",
            json!({"what": "stems", "parent": "x".repeat(2000)}),
        ),
        ("export_folder", json!({})),
        ("read_notes", json!({"extra": 1})),
        ("begin_request", json!({"name": "x", "asked": ["a"]})),
        (
            "begin_request",
            json!({"name": "x", "asked": "y".repeat(501)}),
        ),
    ] {
        let r = m.call(tool, args.clone());
        assert!(is_error(&r), "{tool} {args}: {r}");
    }
    m.ping_answers();
    assert!(app.seen().is_empty(), "no socket call: {:?}", app.seen());
}

#[test]
fn instructions_say_read_the_notes() {
    let d = support_dir();
    let mut m = Mcp::spawn(d.path());
    let r = m.result(
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "t", "version": "1"}}),
    );
    assert!(
        r["instructions"].as_str().unwrap().contains(
            "Read the song's notes first (read_notes) and follow what the composer wrote there."
        ),
        "{r}"
    );
}

#[test]
fn notes_from_another_song_are_not_shown() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    // The front document changes between begin (d1) and notes (d2).
    app.script(Answer::Ok(json!({"doc": "d1", "begun": "x"})));
    app.script(Answer::Ok(
        json!({"doc": "d2", "text": "Other song.\n", "truncated": false, "path": "/y/Notes.md"}),
    ));
    let r = m.call("begin_request", json!({"name": "x"}));
    assert!(!is_error(&r), "{r}");
    assert!(!text(&r).contains("Other song"), "{r}");
    assert!(!text(&r).contains("Notes.md"), "{r}");
    m.call("end_request", json!({}));
    // Not remembered as shown: the next begin reads the notes again.
    let r = m.call("begin_request", json!({"name": "y"}));
    assert!(text(&r).contains("Keep the drums dry."), "{r}");
}

/// Plan 8b Task 6 (Ruling 11): VizDAW's own no.
const VIZDAW_NO: &str = "The composer did not allow this in VizDAW.";
/// The sentence `change` and `apply_change` add.
const ASKS_TOO: &str = "VizDAW then asks the composer too, unless they always allow this app.";

impl Mcp {
    /// Spawned and initialized with `clientInfo.name` = `name`, able to
    /// show a form when `elicitation`.
    fn named(support: &Path, name: Value, elicitation: bool) -> Mcp {
        let mut m = Mcp::spawn(support);
        let caps = if elicitation {
            json!({"elicitation": {}})
        } else {
            json!({})
        };
        m.result(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": caps, "clientInfo": {"name": name, "version": "1"}}),
        );
        m.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        m
    }
}

/// The `client` of each `co_producer.apply` the app saw.
fn clients(app: &FakeApp) -> Vec<Option<String>> {
    app.applies()
        .into_iter()
        .map(|c| match c {
            Command::CoProducerApply { client, .. } => client,
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn apply_sends_the_client_label() {
    // An accepted `change`.
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::named(d.path(), json!("Helper Chat"), true);
    let e = m.ask("c1", &two_changes());
    m.answer(&e, json!({"action": "accept", "content": {"allow": true}}));
    assert!(!is_error(&m.reply_to("c1")));
    // A change that needs no yes applies at once, with the label too.
    let r = m.call("change", json!({"commands": [{"id": "edit.undo"}]}));
    assert!(!is_error(&r), "{r}");
    assert_eq!(
        clients(&app),
        [
            Some("Helper Chat".to_string()),
            Some("Helper Chat".to_string())
        ]
    );
    drop(m);
    drop(app);

    // `apply_change`, the fallback's second half.
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::named(d.path(), json!("Helper Chat"), false);
    m.call("change", json!({"commands": two_changes()}));
    let r = m.call(
        "apply_change",
        json!({"token": preview_token(1), "commands": two_changes()}),
    );
    assert!(!is_error(&r), "{r}");
    assert_eq!(clients(&app), [Some("Helper Chat".to_string())]);
}

#[test]
fn a_long_client_name_is_cut_on_a_character() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::named(d.path(), json!("é".repeat(300)), false);
    m.call("change", json!({"commands": [{"id": "edit.undo"}]}));
    assert_eq!(clients(&app), [Some("é".repeat(100))]);
}

#[test]
fn a_hostile_client_name_is_not_sent() {
    for name in [
        json!("Helper\u{0007}Chat"),
        json!("Helper\nChat"),
        json!("Helper \u{202E}Chat"),
        json!("Helper\u{2028}Chat"),
        json!("Helper\0Chat"),
        json!("   "),
        json!(42),
        json!(["Helper Chat"]),
        json!(null),
    ] {
        let d = support_dir();
        let app = FakeApp::start(d.path());
        let mut m = Mcp::named(d.path(), name.clone(), false);
        let r = m.call("change", json!({"commands": [{"id": "edit.undo"}]}));
        assert!(!is_error(&r), "{name}: {r}");
        assert_eq!(clients(&app), [None], "{name}");
    }
    // No clientInfo at all.
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    m.result("initialize", json!({"protocolVersion": "2025-06-18"}));
    m.call("change", json!({"commands": [{"id": "edit.undo"}]}));
    assert_eq!(clients(&app), [None]);
}

#[test]
fn vizdaws_no_reaches_the_model() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::named(d.path(), json!("Helper Chat"), false);
    m.call("change", json!({"commands": two_changes()}));
    app.script(Answer::Err(ControlError::new(ErrorCode::Failed, VIZDAW_NO)));
    let r = m.call(
        "apply_change",
        json!({"token": preview_token(1), "commands": two_changes()}),
    );
    assert!(is_error(&r), "{r}");
    assert!(text(&r).contains(VIZDAW_NO), "{r}");
    let tools = m.result("tools/list", json!({}));
    for name in ["change", "apply_change"] {
        let t = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap();
        assert!(
            t["description"].as_str().unwrap().contains(ASKS_TOO),
            "{name}: {t}"
        );
    }
}

#[test]
fn note_and_folder_refuse_when_the_front_song_changed() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(Answer::Ok(json!({"doc": "d1", "begun": "x"})));
    app.script(Answer::Ok(
        json!({"doc": "d1", "text": "n\n", "truncated": false, "path": "/y/Notes.md"}),
    ));
    let r = m.call("begin_request", json!({"name": "x"}));
    assert!(!is_error(&r), "{r}");
    // The front document is now d2: status names it, so nothing is sent.
    app.script(Answer::Ok(json!({"doc": "d2", "request": null})));
    let r = m.call("add_note", json!({"text": "Wrong song."}));
    assert!(is_error(&r), "{r}");
    app.script(Answer::Ok(json!({"doc": "d2", "request": null})));
    let r = m.call("export_folder", json!({"what": "stems"}));
    assert!(is_error(&r), "{r}");
    let seen = app.seen();
    assert!(
        !seen.iter().any(|c| matches!(
            c,
            Command::CoProducerNote { .. } | Command::CoProducerExportFolder { .. }
        )),
        "{seen:?}"
    );
}

const EXPORT_DESCRIPTION: &str = "Export the song in one step into a new dated folder, \"<song> \u{2014} <what> \u{2014} <date>\": stems (one audio file per strip; pass strips to choose them), mix (one audio file), midi (one MIDI file), dawproject (a DAWproject file that other music apps open), aaf, or other_daw (stems plus MIDI files). Without parent it uses the folder remembered in the song's notes, else the folder the song is in; for an untitled song with neither, ask the composer where exports should go and pass parent. Never replaces a file. Needs an open request (begin_request). Answers what was written.";

#[test]
fn chore_tools_are_listed() {
    let d = support_dir();
    let mut m = Mcp::spawn(d.path());
    let r = m.result("tools/list", json!({}));
    let tools = r["tools"].as_array().unwrap();
    let find = |n: &str| tools.iter().find(|t| t["name"] == n).cloned();
    let export = find("export").unwrap();
    assert_eq!(export["description"], EXPORT_DESCRIPTION);
    assert_eq!(
        export["inputSchema"]["properties"]["what"]["enum"],
        json!(["stems", "mix", "midi", "dawproject", "aaf", "other_daw"])
    );
    assert_eq!(
        export["inputSchema"]["properties"]["parent"]["maxLength"],
        1024
    );
    assert_eq!(
        export["inputSchema"]["properties"]["strips"]["maxLength"],
        512
    );
    assert_eq!(export["inputSchema"]["required"], json!(["what"]));
    let show = find("show_tracks").unwrap();
    assert_eq!(
        show["description"],
        "List the song's tracks and mixer strips, with their kinds and colours and which ones Co-Producer added. Changes nothing."
    );
    assert_eq!(show["annotations"]["readOnlyHint"], true);
    assert!(find("marker_add").is_some());
    for n in [
        "modify_quantize_notes",
        "modify_transpose_notes",
        "modify_velocity_notes",
        "marker_rename",
        "marker_remove",
    ] {
        assert!(find(n).is_none(), "{n} is not a tool");
    }
    let change = find("change").unwrap()["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        change.contains(
            "quantizing, transposing or changing the velocity of notes, and renaming or removing a marker"
        ),
        "{change}"
    );
}

fn exported(files: usize, summary: Value) -> Answer {
    let written: Vec<String> = (0..files)
        .map(|i| format!("/x/Song \u{2014} Stems \u{2014} 2026-11-16/T{i}.wav"))
        .collect();
    Answer::Ok(json!({
        "doc": "d1",
        "folder": "/x/Song \u{2014} Stems \u{2014} 2026-11-16",
        "written": written,
        "summary": summary,
    }))
}

#[test]
fn export_reaches_the_app() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(Answer::Ok(json!({
        "doc": "d1",
        "folder": "/x/Song \u{2014} Stems \u{2014} 2026-11-16",
        "written": ["/x/\u{2026}/Kick.wav", "/x/\u{2026}/Bass.wav"],
        "summary": null,
    })));
    let r = m.call("export", json!({"what": "stems"}));
    assert!(!is_error(&r), "{r}");
    assert_eq!(
        text(&r),
        "Wrote 2 files in /x/Song \u{2014} Stems \u{2014} 2026-11-16:\n- /x/\u{2026}/Kick.wav\n- /x/\u{2026}/Bass.wav"
    );
    let seen = app.seen();
    assert!(
        matches!(&seen[..], [Command::CoProducerExport { what, parent: None, strips: None, .. }]
            if what.name() == "stems"),
        "{seen:?}"
    );
    // A summary adds a Report line.
    app.script(exported(1, json!("2 tracks, 0 problems")));
    let parent = tempfile::Builder::new()
        .prefix("vzx")
        .tempdir_in("/tmp")
        .unwrap();
    let r = m.call(
        "export",
        json!({"what": "dawproject", "parent": parent.path()}),
    );
    assert!(text(&r).ends_with("\nReport: 2 tracks, 0 problems"), "{r}");
    // Strips go through as given.
    let r = m.call("export", json!({"what": "stems", "strips": "3, 4"}));
    assert!(!is_error(&r), "{r}");
    assert!(
        matches!(app.seen().last(), Some(Command::CoProducerExport { strips: Some(s), .. }) if s == &vec![3, 4]),
        "{:?}",
        app.seen()
    );
    // An app failure is an error with its message.
    app.script(Answer::Err(ControlError::new(
        ErrorCode::Busy,
        "A job is running.",
    )));
    let r = m.call("export", json!({"what": "mix"}));
    assert!(is_error(&r), "{r}");
    assert!(text(&r).contains("A job is running."), "{r}");
}

fn doc_state(lanes: Value) -> Answer {
    Answer::Ok(json!({"doc": "d1", "title": "Song", "selected_seq": 1, "lanes": lanes}))
}

fn mixer_state(strips: Value) -> Answer {
    Answer::Ok(json!({"doc": "d1", "strips": strips}))
}

#[test]
fn show_tracks_lists_tracks_and_strips() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(doc_state(json!([
        {"id": 7, "name": "Keys", "kind": "instrument", "co_producer": false},
        {"id": 9, "name": "Pad", "kind": "audio", "co_producer": true},
    ])));
    app.script(mixer_state(json!([
        {"id": 1, "name": "Keys", "kind_label": "Instrument", "colour": "blue", "co_producer": false},
        {"id": 2, "name": "Bus", "kind_label": "Bus", "colour": null, "co_producer": true},
    ])));
    let r = m.call("show_tracks", json!({}));
    assert!(!is_error(&r), "{r}");
    assert_eq!(
        text(&r),
        "Song: Song\nTracks:\n- Track 1, Keys: instrument\n- Track 2, Pad: audio \u{2014} added by Co-Producer\nStrips:\n- Keys: Instrument, colour blue\n- Bus: Bus, colour none \u{2014} added by Co-Producer"
    );
    let seen = app.seen();
    assert!(
        matches!(
            &seen[..],
            [Command::StateGet { .. }, Command::StateGet { .. }]
        ),
        "{seen:?}"
    );
}

#[test]
fn long_lists_are_capped() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let lanes: Vec<Value> = (0..500)
        .map(
            |i| json!({"id": i + 1, "name": format!("T{i}"), "kind": "midi", "co_producer": false}),
        )
        .collect();
    app.script(doc_state(json!(lanes)));
    app.script(mixer_state(json!([])));
    let r = m.call("show_tracks", json!({}));
    let t = text(&r);
    assert_eq!(t.matches("\n- Track ").count(), 200, "{t}");
    assert!(t.contains("\n- and 300 more"), "{t}");
    app.script(exported(45, Value::Null));
    let r = m.call("export", json!({"what": "stems"}));
    let t = text(&r);
    assert!(t.starts_with("Wrote 45 files in "), "{t}");
    assert_eq!(t.matches("\n- /x/").count(), 40, "{t}");
    assert!(t.ends_with("\n- and 5 more"), "{t}");
}

#[test]
fn chore_tools_refuse_hostile_arguments() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    for (tool, args) in [
        ("export", json!({})),
        ("export", json!({"what": "selection"})),
        ("export", json!({"what": 5})),
        ("export", json!({"what": "stems", "extra": 1})),
        (
            "export",
            json!({"what": "stems", "parent": "x".repeat(70_000)}),
        ),
        ("export", json!({"what": "stems", "parent": "a\u{0}b"})),
        ("export", json!({"what": "stems", "parent": 3})),
        (
            "export",
            json!({"what": "stems", "strips": "x".repeat(70_000)}),
        ),
        ("export", json!({"what": "stems", "strips": "1,,2"})),
        ("export", json!({"what": "stems", "strips": [1]})),
        ("export", json!({"what": "mix", "window": 0})),
        ("show_tracks", json!({"extra": 1})),
        ("show_tracks", json!({"window": 1})),
    ] {
        let r = m.call(tool, args.clone());
        assert!(is_error(&r), "{tool} {args}: {r}");
    }
    m.ping_answers();
    assert!(app.seen().is_empty(), "no socket call: {:?}", app.seen());
}

#[test]
fn names_are_shown_safely() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(doc_state(json!([
        {"id": 1, "name": "a\u{202E}b", "kind": "audio", "co_producer": false},
        {"id": 2, "name": "x\n- Track 9, forged: audio", "kind": "audio", "co_producer": false},
    ])));
    app.script(mixer_state(json!([
        {"id": 1, "name": "s\u{2028}t", "kind_label": "Bus", "colour": null, "co_producer": false},
    ])));
    let r = m.call("show_tracks", json!({}));
    let t = text(&r);
    assert!(t.contains("Track 1, a\u{FFFD}b: audio"), "{t}");
    assert!(t.contains("s\u{FFFD}t: Bus"), "{t}");
    assert!(!t.contains("\n- Track 9"), "{t}");
}

#[test]
fn export_refuses_when_the_front_song_changed() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(Answer::Ok(json!({"doc": "d1", "begun": "x"})));
    app.script(notes_answer("n\n"));
    assert!(!is_error(&m.call("begin_request", json!({"name": "x"}))));
    app.script(Answer::Ok(json!({"doc": "d2", "request": null})));
    let r = m.call("export", json!({"what": "stems"}));
    assert!(is_error(&r), "{r}");
    assert!(
        !app.seen()
            .iter()
            .any(|c| matches!(c, Command::CoProducerExport { .. })),
        "{:?}",
        app.seen()
    );
}

// --- MCP Apps views (plan 8c Task 6, Ruling 7) ---

use vizdaw_mcp::views::{EXTENSION, MIME, VIEWS};

fn ui_caps(mime: &str) -> Value {
    json!({"extensions": {EXTENSION: {"mimeTypes": [mime]}}})
}

/// The 2026-07-28 params, with `caps` as the request's client capabilities.
fn modern_with(caps: Value, extra: Value) -> Value {
    let mut p = json!({"_meta": {
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientCapabilities": caps,
    }});
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

/// What a client sees of views in one era: the server's capabilities, the
/// three tools' `_meta`, the `ui://` resources, and each view tool's
/// result (with its text kept apart).
struct Seen {
    extension: Option<Value>,
    tool_meta: Vec<Value>,
    ui_resources: Vec<Value>,
    results: Vec<Value>,
}

fn look(m: &mut Mcp, app: &FakeApp, wrap: &dyn Fn(Value) -> Value, caps: Value) -> Seen {
    let extension = caps["capabilities"]["extensions"].get(EXTENSION).cloned();
    let tools = m.result("tools/list", wrap(json!({})));
    let tool_meta = VIEWS
        .iter()
        .map(|(_, tool)| {
            let t = tools["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == *tool)
                .unwrap_or_else(|| panic!("{tool}"));
            t.get("_meta").cloned().unwrap_or(Value::Null)
        })
        .collect();
    let res = m.result("resources/list", wrap(json!({})));
    let ui_resources = res["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["uri"].as_str().unwrap().starts_with("ui://"))
        .cloned()
        .collect();
    let mut results = Vec::new();
    for (name, args) in [
        ("change", json!({"commands": [{"id": "edit.undo"}]})),
        ("show_tracks", json!({})),
        ("export", json!({"what": "stems"})),
    ] {
        if name == "show_tracks" {
            app.script(doc_state(json!([
                {"id": 7, "name": "Keys", "kind": "instrument", "co_producer": false},
            ])));
            app.script(mixer_state(json!([])));
        }
        let r = m.result("tools/call", wrap(json!({"name": name, "arguments": args})));
        assert!(!is_error(&r), "{name}: {r}");
        results.push(r);
    }
    Seen {
        extension,
        tool_meta,
        ui_resources,
        results,
    }
}

fn assert_views(s: &Seen) {
    assert_eq!(s.extension, Some(json!({})));
    for ((uri, tool), meta) in VIEWS.iter().zip(&s.tool_meta) {
        assert_eq!(meta["ui"]["resourceUri"], *uri, "{tool}");
    }
    let uris: Vec<&str> = s
        .ui_resources
        .iter()
        .map(|r| r["uri"].as_str().unwrap())
        .collect();
    assert_eq!(uris, VIEWS.iter().map(|v| v.0).collect::<Vec<_>>());
    for r in &s.ui_resources {
        assert_eq!(r["mimeType"], MIME, "{r}");
    }
    for (r, view) in s.results.iter().zip(["change", "tracks", "export"]) {
        assert_eq!(r["structuredContent"]["view"], view, "{r}");
    }
}

fn assert_no_views(s: &Seen, with: &Seen) {
    assert_eq!(s.extension, None);
    assert!(s.tool_meta.iter().all(Value::is_null), "{:?}", s.tool_meta);
    assert!(s.ui_resources.is_empty(), "{:?}", s.ui_resources);
    for (r, w) in s.results.iter().zip(&with.results) {
        assert!(r.get("structuredContent").is_none(), "{r}");
        assert_eq!(text(r), text(w), "the text is the same");
        assert_eq!(r["content"], w["content"]);
    }
}

#[test]
fn views_only_for_hosts_that_ask() {
    let legacy = |version: &str, caps: Value| {
        let d = support_dir();
        let app = FakeApp::start(d.path());
        let mut m = Mcp::spawn(d.path());
        let init = m.result(
            "initialize",
            json!({"protocolVersion": version, "capabilities": caps}),
        );
        let s = look(
            &mut m,
            &app,
            &|p| p,
            json!({"capabilities": init["capabilities"]}),
        );
        m.ping_answers();
        s
    };
    let with = legacy("2025-11-25", ui_caps(MIME));
    assert_views(&with);
    assert_no_views(&legacy("2025-11-25", json!({})), &with);
    assert_no_views(&legacy("2025-11-25", ui_caps("text/html")), &with);
    assert_no_views(&legacy("2025-03-26", ui_caps(MIME)), &with);

    let modern_era = |caps: Value| {
        let d = support_dir();
        let app = FakeApp::start(d.path());
        let mut m = Mcp::spawn(d.path());
        let wrap = |p: Value| modern_with(caps.clone(), p);
        let discover = m.result("server/discover", wrap(json!({})));
        let s = look(&mut m, &app, &wrap, discover);
        m.ping_answers();
        s
    };
    let with_modern = modern_era(ui_caps(MIME));
    assert_views(&with_modern);
    assert_no_views(&modern_era(json!({})), &with_modern);
    assert_no_views(&modern_era(ui_caps("text/html")), &with_modern);
    for (a, b) in with.results.iter().zip(&with_modern.results) {
        assert_eq!(a["structuredContent"], b["structuredContent"]);
    }
}

#[test]
fn views_are_served() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    for (uri, _) in VIEWS {
        let r = m.result("resources/read", json!({"uri": uri}));
        let c = r["contents"].as_array().unwrap();
        assert_eq!(c.len(), 1, "{r}");
        let c = &c[0];
        assert_eq!(c["uri"], *uri);
        assert_eq!(c["mimeType"], MIME);
        assert_eq!(c["_meta"]["ui"]["prefersBorder"], true);
        assert!(c["_meta"]["ui"].get("csp").is_none(), "{c}");
        let html = c["text"].as_str().unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"), "{uri}");
        assert!(!html.contains("<!--"), "{uri}: no marker left");
        assert!(
            html.contains("ui/notifications/tool-result"),
            "{uri}: bridge in place"
        );
        assert_eq!(Some(html.to_string()), vizdaw_mcp::views::html(uri));
    }
    assert!(app.seen().is_empty(), "a view holds no song data");
    assert_eq!(
        m.error_code("resources/read", json!({"uri": "ui://vizdaw/nope"})),
        -32002
    );
    assert_eq!(vizdaw_mcp::views::html("ui://vizdaw/nope"), None);
    m.ping_answers();
}

fn view_sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("views");
    [
        "bridge.js",
        "change.html",
        "tracks.html",
        "export.html",
        "mixer.html",
    ]
    .iter()
    .map(|f| (f.to_string(), std::fs::read_to_string(dir.join(f)).unwrap()))
    .collect()
}

#[test]
fn views_draw_names_as_text() {
    for (f, s) in view_sources() {
        for bad in [
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "document.write",
            "eval(",
            "new Function",
            "http:",
            "https:",
            "src=",
            "<link",
        ] {
            assert!(!s.contains(bad), "{f} holds {bad}");
        }
        assert!(s.contains("textContent"), "{f}");
    }
    // Hostile names reach a view only as escaped JSON values.
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    m.result(
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": ui_caps(MIME)}),
    );
    let hostile = [
        "<script>alert(1)</script>",
        "<img src=x onerror=alert(1)>",
        "\"quoted\" 'name'",
        "a\u{202E}b\u{2066}c",
    ];
    let lanes: Vec<Value> = hostile
        .iter()
        .enumerate()
        .map(|(i, n)| json!({"id": i + 1, "name": n, "kind": "audio", "co_producer": true}))
        .collect();
    app.script(Answer::Ok(
        json!({"doc": "d1", "title": "</script><script>x()</script>", "selected_seq": 1, "lanes": lanes}),
    ));
    app.script(mixer_state(json!([
        {"id": 1, "name": "<b>bold</b>", "kind_label": "Bus", "colour": "red;background:url(x)", "co_producer": false},
    ])));
    let r = m.call("show_tracks", json!({}));
    let sc = &r["structuredContent"];
    assert_eq!(sc["title"], "</script><script>x()</script>");
    assert_eq!(sc["tracks"][0]["name"], hostile[0]);
    assert_eq!(sc["tracks"][1]["name"], hostile[1]);
    assert_eq!(sc["tracks"][2]["name"], hostile[2]);
    assert_eq!(sc["tracks"][3]["name"], "a\u{FFFD}b\u{FFFD}c");
    assert_eq!(sc["strips"][0]["name"], "<b>bold</b>");
    // The page served is the same whatever the song holds.
    for (uri, _) in VIEWS {
        let html = m.result("resources/read", json!({"uri": uri}))["contents"][0]["text"].clone();
        let html = html.as_str().unwrap();
        assert!(
            !html.contains("alert(1)") && !html.contains("bold"),
            "{uri}"
        );
    }
}

#[test]
fn views_speak_the_extension() {
    let sources: std::collections::HashMap<String, String> = view_sources().into_iter().collect();
    for m in [
        "\"ui/initialize\"",
        "\"ui/notifications/initialized\"",
        "\"ui/notifications/tool-result\"",
    ] {
        assert!(sources["bridge.js"].contains(m), "{m}");
    }
    assert!(sources["export.html"].contains("\"ui/open-link\""));
    assert!(sources["export.html"].contains("Open it in Finder: "));
    assert!(
        sources["bridge.js"].contains("hostContext"),
        "the views follow the host's theme"
    );
    for f in ["change.html", "tracks.html", "export.html"] {
        assert!(sources[f].contains("<!--BRIDGE-->"), "{f}");
    }
}

#[test]
fn change_view_states() {
    let state = |r: &Value| r["structuredContent"]["state"].clone();
    // The token fallback.
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    m.result(
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": ui_caps(MIME)}),
    );
    let r = m.call("change", json!({"commands": two_changes()}));
    assert_eq!(state(&r), "ask_in_chat", "{r}");
    let token = preview_token(1);
    assert!(text(&r).contains(&token));
    let rest = {
        let mut r = r.clone();
        r.as_object_mut().unwrap().remove("content");
        r.to_string()
    };
    assert!(
        !rest.contains(&token),
        "the token is only in the text: {rest}"
    );
    assert_eq!(
        r["structuredContent"]["items"],
        json!([
            {"thing": "Track 2, Vox", "change": "deleted", "composers_work": true},
            {"thing": "Track 3, Bass", "change": "renamed to \"Low\"", "composers_work": true},
        ])
    );
    // No yes needed.
    let r = m.call("change", json!({"commands": [{"id": "edit.undo"}]}));
    assert_eq!(state(&r), "applied_without_asking", "{r}");
    assert_eq!(r["structuredContent"]["undo_label"], "Co-Producer: Tidy Up");
    drop(m);
    drop(app);

    // Elicitation: a yes, then a no.
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let mut caps = ui_caps(MIME);
    caps["elicitation"] = json!({});
    m.result(
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": caps}),
    );
    let e = m.ask("y", &two_changes());
    m.answer(&e, json!({"action": "accept", "content": {"allow": true}}));
    let r = m.reply_to("y");
    assert_eq!(state(&r), "applied", "{r}");
    assert_eq!(r["structuredContent"]["items"][0]["thing"], "Track 2, Vox");
    let e = m.ask("n", &two_changes());
    m.answer(&e, json!({"action": "decline"}));
    let r = m.reply_to("n");
    assert_eq!(state(&r), "said_no", "{r}");
    assert_eq!(text(&r), SAID_NO);
    assert_eq!(app.applies().len(), 1);

    // The 2026-07-28 retry.
    let mut caps = ui_caps(MIME);
    caps["elicitation"] = json!({});
    let ask = |m: &mut Mcp| {
        let r = m.result(
            "tools/call",
            modern_with(
                caps.clone(),
                json!({"name": "change", "arguments": {"commands": two_changes()}}),
            ),
        );
        r["requestState"].as_str().unwrap().to_string()
    };
    for (answer, want) in [
        (
            json!({"action": "accept", "content": {"allow": true}}),
            "applied",
        ),
        (json!({"action": "decline"}), "said_no"),
    ] {
        let key = ask(&mut m);
        let r = m.result(
            "tools/call",
            modern_with(
                caps.clone(),
                json!({"name": "change", "arguments": {"commands": two_changes()},
                    "inputResponses": {"yes": answer}, "requestState": key}),
            ),
        );
        assert_eq!(state(&r), want, "{r}");
    }
}

// --- Plug-in tools and the mixer (plan 8d Task 6, Ruling 5) ---

const PLUGINS_HEAD: &str = "Plug-ins you may use (pass the first part as plugin):";
const NOT_RUNNING: &str = "The plug-in in slot 7 is not running yet, so its parameters are not known. Try again in a moment.";

fn tool_named(m: &mut Mcp, name: &str) -> Value {
    let tools = m.result("tools/list", json!({}));
    tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == name)
        .cloned()
        .unwrap_or(Value::Null)
}

#[test]
fn plugin_tools_are_listed() {
    let d = support_dir();
    let _app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let list = tool_named(&mut m, "list_plugins");
    assert_eq!(
        list["description"],
        "List the installed plug-ins Co-Producer may use: instruments and effects the composer has not turned off in the Plugin Manager, and VizDAW's built-ins (multi-q, the EQ; built-in-synth). Pass a line's first part as plugin to mixer.add_insert, mixer.set_eq or track_new_plugin_instrument. Changes nothing."
    );
    assert_eq!(list["annotations"]["readOnlyHint"], true);
    assert_eq!(
        list["inputSchema"]["properties"]["kind"]["enum"],
        json!(["instrument", "effect"])
    );
    let params = tool_named(&mut m, "plugin_params");
    assert_eq!(
        params["description"],
        "List a loaded plug-in's parameters with their current values and ranges, for mixer.set_param. Slots are in show_mixer. Changes nothing."
    );
    assert_eq!(params["annotations"]["readOnlyHint"], true);
    assert_eq!(
        params["inputSchema"]["properties"]["slot"]["type"],
        "integer"
    );
    assert_eq!(params["inputSchema"]["properties"]["slot"]["minimum"], 1);
    assert_eq!(params["inputSchema"]["required"], json!(["slot"]));
    let show = tool_named(&mut m, "show_mixer");
    assert_eq!(
        show["description"],
        "Show the mixer: every strip with its ID, volume, pan, mute and solo, where it goes, its EQ, inserts (with slot IDs) and sends, and which strips Co-Producer added. Changes nothing."
    );
    assert_eq!(show["annotations"]["readOnlyHint"], true);
    assert_eq!(show["inputSchema"]["properties"], json!({}));
    let inst = tool_named(&mut m, "track_new_plugin_instrument");
    assert_eq!(
        inst["inputSchema"]["properties"]["plugin"]["maxLength"],
        512
    );
    let bus = tool_named(&mut m, "mixer_new_bus");
    assert!(!bus.is_null());
    assert!(bus["inputSchema"]["properties"].get("strip").is_none());
    let tools = m.result("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for n in [
        "mixer_set_output",
        "mixer_add_send",
        "mixer_set_send",
        "mixer_remove_send",
        "mixer_add_insert",
        "mixer_remove_insert",
        "mixer_set_eq",
        "mixer_set_gain",
        "mixer_set_pan",
        "mixer_set_mute",
        "mixer_set_solo",
        "mixer_set_param",
        "mixer_remove_bus",
        "mixer_remove_strip",
    ] {
        assert!(!names.contains(&n), "{n} is not a tool");
    }
    let change = tool_named(&mut m, "change")["description"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        change.contains(
            "mixer changes: routing a strip to a bus, adding or changing a send or insert, setting an EQ, the volume, pan, mute, solo or a plug-in parameter, and removing a send, insert or bus. Make new buses first with mixer_new_bus and read their IDs with show_mixer; mixer changes on Co-Producer's own buses apply without asking."
        ),
        "{change}"
    );
}

fn plugin_row(plugin: &str, name: &str, maker: &str, format: &str, kind: &str) -> Value {
    json!({"plugin": plugin, "name": name, "maker": maker, "format": format, "kind": kind, "version": "1"})
}

#[test]
fn plugins_reach_the_app() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(Answer::Ok(json!({"plugins": [
        plugin_row("multi-q", "Multi-Q", "VizDAW", "built_in", "effect"),
        plugin_row("clap:vizdaw.test.gain", "Test Gain", "Acme", "clap", "effect"),
        plugin_row("au:aufx/gain/Acme", "Plain", "", "au", "effect"),
    ], "truncated": false})));
    let r = m.call("list_plugins", json!({"kind": "effect"}));
    assert!(!is_error(&r), "{r}");
    assert_eq!(
        app.seen(),
        vec![Command::CoProducerPlugins {
            kind: Some(core_control::request::PluginKindArg::Effect)
        }]
    );
    let t = text(&r);
    assert_eq!(t.lines().next().unwrap(), PLUGINS_HEAD);
    assert!(t.contains("\n- multi-q: Multi-Q, built in, effect"), "{t}");
    assert!(
        t.contains("\n- clap:vizdaw.test.gain: Test Gain by Acme, effect"),
        "{t}"
    );
    assert!(t.contains("\n- au:aufx/gain/Acme: Plain, effect"), "{t}");
    assert!(!t.contains("Plain by"), "{t}");
}

fn param_row(id: u64, name: &str, value: f64, stepped: bool) -> Value {
    json!({"id": id, "name": name, "module": "", "min": 0.0, "max": 1.0, "default": 0.0,
        "value": value, "stepped": stepped, "automatable": true})
}

#[test]
fn params_reach_the_app() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(Answer::Ok(json!({"doc": "d1", "slot": 7, "plugin": "clap:vizdaw.test.gain",
        "running": true, "params": [param_row(3, "Gain", 0.5, false), param_row(4, "Mode", 1.0, true)]})));
    let r = m.call("plugin_params", json!({"slot": 7}));
    assert!(!is_error(&r), "{r}");
    assert!(matches!(
        app.seen().as_slice(),
        [Command::CoProducerParams { slot: 7, .. }]
    ));
    let t = text(&r);
    assert!(t.starts_with("Slot 7, clap:vizdaw.test.gain:"), "{t}");
    assert!(t.contains("\n- 3: Gain = 0.5 (0 to 1)\n"), "{t}");
    assert!(t.ends_with("\n- 4: Mode = 1 (0 to 1, whole values)"), "{t}");
    app.script(Answer::Ok(
        json!({"doc": "d1", "slot": 7, "plugin": "multi-q",
        "running": false, "params": []}),
    ));
    let r = m.call("plugin_params", json!({"slot": 7}));
    assert!(!is_error(&r), "{r}");
    assert_eq!(text(&r), NOT_RUNNING);
}

fn strip_row(id: u64, name: &str, kind: &str, extra: Value) -> Value {
    let mut s = json!({"id": id, "name": name, "kind_label": kind, "colour": null,
        "co_producer": false, "gain_db": 0.0, "pan": 0.0, "mute": false, "solo": false,
        "output": null, "output_name": "Master", "eq": null, "inserts": [], "sends": []});
    for (k, v) in extra.as_object().unwrap() {
        s[k] = v.clone();
    }
    s
}

#[test]
fn show_mixer_lists_strips() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    app.script(mixer_state(json!([
        strip_row(
            1,
            "Master",
            "Master",
            json!({"output": null, "output_name": null})
        ),
        strip_row(
            2,
            "Kick",
            "Audio",
            json!({
                "gain_db": -6.0, "output": 9, "output_name": "Drums (Co-Producer)",
                "eq": {"slot": 4, "name": "Multi-Q", "plugin": "multi-q", "bypass": false},
            })
        ),
        strip_row(
            9,
            "Drums (Co-Producer)",
            "Bus",
            json!({"co_producer": true,
                "mute": true, "solo": true, "pan": -0.3,
                "inserts": [{"slot": 5, "name": "Comp", "plugin": "clap:x", "bypass": false}],
                "sends": [{"target": 2, "level_db": -12.5, "pre_fader": true, "enabled": false}],
            })
        ),
    ])));
    let r = m.call("show_mixer", json!({}));
    assert!(!is_error(&r), "{r}");
    assert!(matches!(app.seen().as_slice(), [Command::StateGet { .. }]));
    let t = text(&r);
    assert!(t.starts_with("Mixer:"), "{t}");
    assert!(
        t.contains("\n- [2] Kick (Audio): volume \u{2212}6.0 dB, pan centre, to Drums (Co-Producer); EQ Multi-Q; inserts none; sends none\n"),
        "{t}"
    );
    let bus = t.lines().find(|l| l.starts_with("- [9]")).unwrap();
    assert!(bus.ends_with(" \u{2014} added by Co-Producer"), "{bus}");
    assert!(bus.contains("pan 30% left, muted, soloed"), "{bus}");
    assert!(bus.contains("inserts slot 5 Comp"), "{bus}");
    assert!(bus.contains("sends Kick \u{2212}12.5 dB pre off"), "{bus}");
    // Master has no output name.
    let master = t.lines().find(|l| l.starts_with("- [1]")).unwrap();
    assert!(
        !master.contains("to null") && !master.contains("None"),
        "{master}"
    );
}

#[test]
fn long_mixers_are_capped() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let strips: Vec<Value> = (0..500)
        .map(|i| strip_row(i + 1, &format!("S{i}"), "Audio", json!({})))
        .collect();
    app.script(mixer_state(json!(strips)));
    let t = text(&m.call("show_mixer", json!({})));
    assert_eq!(t.matches("\n- [").count(), 200, "{t}");
    assert!(t.ends_with("\n- and 300 more"), "{t}");
    let rows: Vec<Value> = (0..300)
        .map(|i| plugin_row(&format!("clap:p{i}"), "P", "M", "clap", "effect"))
        .collect();
    app.script(Answer::Ok(json!({"plugins": rows, "truncated": false})));
    let t = text(&m.call("list_plugins", json!({})));
    assert_eq!(t.matches("\n- clap:").count(), 200, "{t}");
    assert!(
        t.ends_with("\n- and 100 more; pass kind to narrow the list"),
        "{t}"
    );
}

#[test]
fn plugin_tools_refuse_hostile_arguments() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    for (tool, args) in [
        ("list_plugins", json!({"kind": "synth"})),
        ("list_plugins", json!({"kind": 3})),
        ("list_plugins", json!({"extra": 1})),
        ("list_plugins", json!({"window": 1})),
        ("plugin_params", json!({})),
        ("plugin_params", json!({"slot": 0})),
        ("plugin_params", json!({"slot": "7"})),
        ("plugin_params", json!({"slot": 1.5})),
        ("plugin_params", json!({"slot": 7, "extra": 1})),
        ("plugin_params", json!({"slot": 7, "window": 0})),
        ("show_mixer", json!({"extra": 1})),
        ("show_mixer", json!({"window": 1})),
        ("mixer_new_bus", json!({"strip": 2})),
        (
            "track_new_plugin_instrument",
            json!({"plugin": "x".repeat(70_000)}),
        ),
        (
            "track_new_plugin_instrument",
            json!({"plugin": "x".repeat(513)}),
        ),
    ] {
        let r = m.call(tool, args.clone());
        assert!(is_error(&r), "{tool} {args}: {r}");
    }
    m.ping_answers();
    assert!(app.seen().is_empty(), "no socket call: {:?}", app.seen());
}

#[test]
fn mixer_names_are_shown_safely() {
    let d = support_dir();
    let app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    m.result(
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": ui_caps(MIME)}),
    );
    app.script(mixer_state(json!([strip_row(
        1,
        "a\u{202E}b",
        "Audio",
        json!({
            "inserts": [{"slot": 3, "name": "x\u{2028}y", "plugin": "clap:z", "bypass": false}],
        })
    ),])));
    let r = m.call("show_mixer", json!({}));
    let t = text(&r);
    assert!(t.contains("a\u{FFFD}b (Audio)"), "{t}");
    assert!(t.contains("x\u{FFFD}y"), "{t}");
    let sc = &r["structuredContent"];
    assert_eq!(sc["view"], "mixer");
    assert_eq!(sc["strips"][0]["name"], "a\u{FFFD}b");
    assert_eq!(sc["strips"][0]["inserts"][0]["name"], "x\u{FFFD}y");
    assert_eq!(sc["strips"][0]["output_name"], "Master");
    app.script(mixer_state(json!([strip_row(
        1,
        "Master",
        "Master",
        json!({"output_name": null})
    ),])));
    let r = m.call("show_mixer", json!({}));
    assert!(r["structuredContent"]["strips"][0]["output_name"].is_null());
}

#[test]
fn the_mixer_view_only_for_hosts_that_ask() {
    let run = |version: &str, caps: Value| {
        let d = support_dir();
        let app = FakeApp::start(d.path());
        let mut m = Mcp::spawn(d.path());
        let init = m.result(
            "initialize",
            json!({"protocolVersion": version, "capabilities": caps}),
        );
        let tools = m.result("tools/list", json!({}));
        let show = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "show_mixer")
            .unwrap()
            .clone();
        let res = m.result("resources/list", json!({}));
        let listed = res["resources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["uri"] == "ui://vizdaw/mixer");
        app.script(mixer_state(json!([strip_row(
            2,
            "Kick",
            "Audio",
            json!({})
        )])));
        let r = m.call("show_mixer", json!({}));
        let _ = init;
        (show, listed, r)
    };
    let (show, listed, with) = run("2025-11-25", ui_caps(MIME));
    assert_eq!(show["_meta"]["ui"]["resourceUri"], "ui://vizdaw/mixer");
    assert!(listed);
    assert_eq!(with["structuredContent"]["view"], "mixer");
    for (version, caps) in [
        ("2025-11-25", json!({})),
        ("2025-11-25", ui_caps("text/html")),
        ("2025-03-26", ui_caps(MIME)),
    ] {
        let (show, listed, r) = run(version, caps);
        assert!(show.get("_meta").is_none(), "{show}");
        assert!(!listed);
        assert!(r.get("structuredContent").is_none(), "{r}");
        assert_eq!(r["content"], with["content"]);
    }
}

#[test]
fn mixer_view_draws_names_as_text() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("views");
    let s = std::fs::read_to_string(dir.join("mixer.html")).unwrap();
    for bad in [
        "innerHTML",
        "outerHTML",
        "insertAdjacentHTML",
        "document.write",
        "eval(",
        "new Function",
        "http:",
        "https:",
        "src=",
        "<link",
    ] {
        assert!(!s.contains(bad), "mixer.html holds {bad}");
    }
    assert!(s.contains("textContent"));
    assert!(s.contains("<!--BRIDGE-->"));
    let d = support_dir();
    let _app = FakeApp::start(d.path());
    let mut m = Mcp::spawn(d.path());
    let r = m.result("resources/read", json!({"uri": "ui://vizdaw/mixer"}));
    let html = r["contents"][0]["text"].as_str().unwrap();
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(!html.contains("<!--"));
}
