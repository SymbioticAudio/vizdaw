//! `vizdaw-mcp`: the stdio MCP server through which Claude reaches
//! VizDAW's Co-Producer socket (Co-Producer spec §6.1, plan 8a Rulings 1,
//! 2, 10 and 12). JSON-RPC by hand over `serde_json`; tools built from the
//! linked Co-Producer command list, so `tools/list` works while VizDAW is
//! closed. Every message is hostile: each is handled inside
//! `catch_unwind`, and no input ends the server.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )
)]

pub mod app;
pub mod base64;
pub mod rpc;
pub mod tools;

use app::{App, AppError};
use rpc::{Kind, Message, RpcError};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

/// The longest input line; a longer one is answered `-32700` and skipped.
pub const MAX_LINE: usize = 1 << 20;

/// The largest screenshot file read (a 6K window at scale 1 is far less).
const MAX_SCREENSHOT: u64 = 64 << 20;

/// Where VizDAW keeps its files, found as the app finds it (Ruling 10):
/// `VIZDAW_SUPPORT_DIR`, else `$HOME/Library/Application Support/VizDAW`.
pub fn support_dir_from_env() -> PathBuf {
    if let Some(d) = std::env::var_os("VIZDAW_SUPPORT_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from);
    home.join("Library")
        .join("Application Support")
        .join("VizDAW")
}

/// The server's state: its tools, what the client declared, and the app
/// connection.
pub struct Server {
    app: App,
    commands: Value,
    tools: Vec<Value>,
    /// Command tools by name, with their `commands.list` entries.
    by_name: HashMap<String, Value>,
    /// The client declared form elicitation at `initialize` (Task 6).
    elicitation: bool,
}

/// A tool's answer: text, as an error or not.
fn tool_text(text: impl Into<String>, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }], "isError": is_error })
}

fn tool_error(text: impl Into<String>) -> Value {
    tool_text(text, true)
}

impl Server {
    /// A server for the app whose support folder is `support`.
    pub fn new(support: PathBuf) -> Server {
        let commands = core_control::co_producer::list_json();
        let tools = tools::tools_json(&commands);
        let by_name = tools::command_tools(&commands).into_iter().collect();
        Server {
            app: App::new(&support),
            commands,
            tools,
            by_name,
            elicitation: false,
        }
    }

    /// One input line → zero or more output lines (a reply, or Task 6's
    /// elicitation request).
    pub fn handle_line(&mut self, line: &str) -> Vec<String> {
        let line = line.trim();
        if line.is_empty() {
            return Vec::new();
        }
        let (id, method, params) = match rpc::parse(line) {
            Err(reply) => return vec![reply],
            Ok(Message::Notification { .. } | Message::Response { .. }) => return Vec::new(),
            Ok(Message::Request { id, method, params }) => (id, method, params),
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| self.request(&method, &params)));
        vec![match outcome {
            Ok(Ok(result)) => rpc::result_line(&id, result),
            Ok(Err(e)) => rpc::error_line(&id, &e),
            Err(_) => rpc::error_line(
                &id,
                &RpcError::new(rpc::INTERNAL_ERROR, "Internal error: vizdaw-mcp went on."),
            ),
        }]
    }

    /// A line longer than [`MAX_LINE`] that the reader consumed without
    /// keeping: its `-32700` reply.
    pub fn too_long(&mut self) -> Vec<String> {
        vec![rpc::error_line(
            &Value::Null,
            &RpcError::new(
                rpc::PARSE_ERROR,
                format!("Parse error: a message longer than {MAX_LINE} bytes was skipped."),
            ),
        )]
    }

    /// A line that is not UTF-8: its `-32700` reply.
    pub fn not_utf8(&mut self) -> Vec<String> {
        vec![rpc::error_line(
            &Value::Null,
            &RpcError::new(rpc::PARSE_ERROR, "Parse error: the message is not UTF-8."),
        )]
    }

    /// Called when no line came for a while: ends waits that expired
    /// (Task 6). Nothing waits yet.
    pub fn tick(&mut self) -> Vec<String> {
        Vec::new()
    }

    fn request(&mut self, method: &str, params: &Map<String, Value>) -> Result<Value, RpcError> {
        let era = rpc::era(params)?;
        let (kind, result) = match method {
            "initialize" => (Kind::Plain, self.initialize(params)),
            "ping" => (Kind::Plain, json!({})),
            #[cfg(test)]
            "test/panic" => panic!("a test panic"),
            "server/discover" => (
                Kind::Plain,
                json!({
                    "supportedVersions": rpc::SUPPORTED,
                    "capabilities": capabilities(),
                    "serverInfo": server_info(),
                    "instructions": tools::INSTRUCTIONS,
                }),
            ),
            "tools/list" => (Kind::Cacheable, json!({ "tools": self.tools })),
            "tools/call" => (Kind::Plain, self.call_tool(params)?),
            "resources/list" => (
                Kind::Cacheable,
                json!({ "resources": tools::resources_json() }),
            ),
            "resources/templates/list" => (Kind::Cacheable, json!({ "resourceTemplates": [] })),
            "resources/read" => (Kind::Cacheable, self.read_resource(params)?),
            _ => {
                return Err(RpcError::new(
                    rpc::METHOD_NOT_FOUND,
                    format!("Method not found: {}", rpc::truncate(method, 64)),
                ));
            }
        };
        Ok(rpc::finish(era, kind, result))
    }

    fn initialize(&mut self, params: &Map<String, Value>) -> Value {
        self.elicitation = params
            .get("capabilities")
            .and_then(|c| c.get("elicitation"))
            .is_some_and(Value::is_object);
        json!({
            "protocolVersion": rpc::negotiate(params.get("protocolVersion")),
            "capabilities": capabilities(),
            "serverInfo": server_info(),
            "instructions": tools::INSTRUCTIONS,
        })
    }

    fn call_tool(&mut self, params: &Map<String, Value>) -> Result<Value, RpcError> {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return Err(RpcError::new(
                rpc::INVALID_PARAMS,
                "tools/call needs the tool's `name`.",
            ));
        };
        let empty = Map::new();
        let args = match params.get("arguments") {
            None | Some(Value::Null) => &empty,
            Some(Value::Object(a)) => a,
            Some(_) => return Ok(tool_error("`arguments` must be an object.")),
        };
        if let Some(spec) = self.by_name.get(name).cloned() {
            return Ok(self.run_command(&spec, args));
        }
        Ok(match name {
            tools::BEGIN_REQUEST => self.begin_request(args),
            tools::END_REQUEST => self.simple("co_producer.end", args, &[]),
            tools::SCREENSHOT => self.screenshot(args),
            tools::CHANGE | tools::APPLY_CHANGE => {
                tool_error("Co-Producer cannot change existing work yet.")
            }
            _ => {
                return Err(RpcError::new(
                    rpc::INVALID_PARAMS,
                    format!("No tool {:?}.", rpc::truncate(name, 64)),
                ));
            }
        })
    }

    /// The text of a failed call.
    fn failed(&self, e: &AppError) -> Value {
        tool_error(self.app.message(e))
    }

    fn run_command(&mut self, spec: &Value, args: &Map<String, Value>) -> Value {
        let (params, window) = match tools::arguments_to_params(spec, args) {
            Ok(p) => p,
            Err(m) => return tool_error(m),
        };
        let id = spec.get("id").and_then(Value::as_str).unwrap_or("");
        let mut call = json!({ "id": id, "params": params });
        if let (Some(w), Some(o)) = (window, call.as_object_mut()) {
            o.insert("window".into(), json!(w));
        }
        match self.app.call("command.run", call) {
            Ok(result) => {
                let mut text = result.to_string();
                let read = spec.get("class").and_then(Value::as_str) == Some("read");
                if let Some(label) = result.get("undo").and_then(Value::as_str) {
                    if !read && label.starts_with("Co-Producer: ") {
                        text.push_str("\nUndo step: ");
                        text.push_str(label);
                    }
                }
                tool_text(text, false)
            }
            Err(e) => self.failed(&e),
        }
    }

    /// A call whose arguments are `window` and `allowed`, passed as given.
    fn simple(&mut self, cmd: &str, args: &Map<String, Value>, allowed: &[&str]) -> Value {
        let mut params = Map::new();
        for (k, v) in args {
            if k == "window" {
                match tools::window_arg(v) {
                    Ok(Some(w)) => {
                        params.insert(k.clone(), json!(w));
                    }
                    Ok(None) => {}
                    Err(m) => return tool_error(m),
                }
            } else if allowed.contains(&k.as_str()) {
                params.insert(k.clone(), v.clone());
            } else {
                return tool_error(format!("unknown argument `{}`", rpc::truncate(k, 64)));
            }
        }
        match self.app.call(cmd, Value::Object(params)) {
            Ok(result) => tool_text(result.to_string(), false),
            Err(e) => self.failed(&e),
        }
    }

    fn begin_request(&mut self, args: &Map<String, Value>) -> Value {
        match args.get("name") {
            Some(Value::String(s))
                if s.chars().count() <= core_control::request::MAX_REQUEST_NAME =>
            {
                self.simple("co_producer.begin", args, &["name"])
            }
            Some(Value::String(_)) => tool_error(format!(
                "`name` must be at most {} characters",
                core_control::request::MAX_REQUEST_NAME
            )),
            Some(_) => tool_error("`name` must be a string"),
            None => tool_error("missing `name`"),
        }
    }

    fn screenshot(&mut self, args: &Map<String, Value>) -> Value {
        let mut params = Map::new();
        for (k, v) in args {
            match (k.as_str(), tools::window_arg(v)) {
                ("window", Ok(Some(w))) => {
                    params.insert("window".into(), json!(w));
                }
                ("window", Ok(None)) => {}
                ("window", Err(m)) => return tool_error(m),
                _ => {
                    return tool_error(format!("unknown argument `{}`", rpc::truncate(k, 64)));
                }
            }
        }
        let result = match self.app.call("ui.screenshot", Value::Object(params)) {
            Ok(r) => r,
            Err(e) => return self.failed(&e),
        };
        let Some(path) = result.get("path").and_then(Value::as_str) else {
            return tool_error("VizDAW's screenshot answer named no file.");
        };
        match take_screenshot(self.app.support(), Path::new(path)) {
            Ok(bytes) => json!({
                "content": [{ "type": "image", "mimeType": "image/png", "data": base64::encode(&bytes) }],
                "isError": false,
            }),
            Err(m) => tool_error(m),
        }
    }

    fn read_resource(&mut self, params: &Map<String, Value>) -> Result<Value, RpcError> {
        let Some(uri) = params.get("uri").and_then(Value::as_str) else {
            return Err(RpcError::new(
                rpc::INVALID_PARAMS,
                "resources/read needs a `uri`.",
            ));
        };
        let text = if uri == tools::COMMANDS_URI {
            self.commands.to_string()
        } else if let Some(target) = tools::state_target(uri) {
            match self.app.call("state.get", json!({ "target": target })) {
                Ok(v) => v.to_string(),
                Err(e) => return Err(RpcError::new(rpc::APP_ERROR, self.app.message(&e))),
            }
        } else {
            return Err(RpcError::new(
                rpc::INVALID_PARAMS,
                format!("No resource {:?}.", rpc::truncate(uri, 128)),
            ));
        };
        Ok(json!({
            "contents": [{ "uri": uri, "mimeType": "application/json", "text": text }],
        }))
    }
}

fn capabilities() -> Value {
    json!({ "tools": {}, "resources": {} })
}

fn server_info() -> Value {
    json!({ "name": "vizdaw-mcp", "version": env!("CARGO_PKG_VERSION") })
}

/// Whether `name` is `screenshot-<digits>.png`.
fn is_screenshot_name(name: &str) -> bool {
    name.strip_prefix("screenshot-")
        .and_then(|r| r.strip_suffix(".png"))
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// Reads and deletes the screenshot VizDAW answered (Ruling 12). Only a
/// regular file `<support>/screenshot-<digits>.png`, not a link, is read
/// and deleted; any other path is refused and left alone.
fn take_screenshot(support: &Path, path: &Path) -> Result<Vec<u8>, String> {
    let refuse = || {
        format!(
            "VizDAW answered a screenshot at {}, which is not its own screenshot file; it was left alone.",
            rpc::truncate(&path.display().to_string(), 256)
        )
    };
    let named = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(is_screenshot_name);
    if !named || path.parent() != Some(support) {
        return Err(refuse());
    }
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_file() => {}
        _ => return Err(refuse()),
    }
    let read = std::fs::File::open(path).and_then(|f| {
        let mut bytes = Vec::new();
        f.take(MAX_SCREENSHOT + 1).read_to_end(&mut bytes)?;
        Ok(bytes)
    });
    let _ = std::fs::remove_file(path);
    match read {
        Ok(b) if b.len() as u64 > MAX_SCREENSHOT => Err("The screenshot is too large.".into()),
        Ok(b) => Ok(b),
        Err(e) => Err(format!("The screenshot could not be read ({e}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screenshot_names() {
        assert!(is_screenshot_name("screenshot-12.png"));
        for bad in [
            "screenshot-.png",
            "screenshot-1a.png",
            "screenshot-1.png.png",
            "x-1.png",
        ] {
            assert!(!is_screenshot_name(bad), "{bad}");
        }
    }

    #[test]
    fn a_panic_is_an_internal_error_and_the_server_goes_on() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Server::new(d.path().to_path_buf());
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let r = s.handle_line(r#"{"jsonrpc":"2.0","id":"p","method":"test/panic"}"#);
        std::panic::set_hook(hook);
        let v: Value = serde_json::from_str(&r[0]).unwrap();
        assert_eq!(v["id"], "p");
        assert_eq!(v["error"]["code"], rpc::INTERNAL_ERROR);
        let r = s.handle_line(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#);
        let v: Value = serde_json::from_str(&r[0]).unwrap();
        assert_eq!(v["result"], json!({}));
    }
}
