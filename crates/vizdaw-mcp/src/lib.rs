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
use rpc::{Answer, Era, Kind, Message, RpcError};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tools::ChangeArgs;

/// The longest input line; a longer one is answered `-32700` and skipped.
pub const MAX_LINE: usize = 1 << 20;

/// How long the composer has to answer (Ruling 8, P6), and how long a
/// 2026-07-28 `requestState` is kept.
pub const ASK_TIMEOUT: Duration = Duration::from_secs(600);

/// The most `requestState`s kept, and the most tokens handed to the model
/// that are remembered.
const MAX_HELD: usize = 8;

/// The most lines kept while the composer is asked, and their bytes.
const MAX_QUEUED: usize = 16;
const MAX_QUEUED_BYTES: usize = 8 << 20;

/// The most documents whose notes are remembered as shown.
const MAX_NOTED: usize = 64;

/// The most bytes of the notes the first answer carries.
const MAX_FIRST_NOTES: usize = 64 << 10;

/// The largest screenshot file read (a 6K window at scale 1 is far less).
const MAX_SCREENSHOT: u64 = 64 << 20;

/// Where VizDAW keeps its files, found as the app finds it (Ruling 10):
/// `VIZDAW_SUPPORT_DIR`, else `$HOME/Library/Application Support/VizDAW`.
/// Err, a sentence for stderr, when the one used is missing or not
/// absolute: there is no fallback to a folder other users may write.
pub fn support_dir_from_env() -> Result<PathBuf, String> {
    if let Some(d) = std::env::var_os("VIZDAW_SUPPORT_DIR") {
        let d = PathBuf::from(d);
        if !d.is_absolute() {
            return Err("vizdaw-mcp: VIZDAW_SUPPORT_DIR must be an absolute path.".into());
        }
        return Ok(d);
    }
    match std::env::var_os("HOME").map(PathBuf::from) {
        Some(home) if home.is_absolute() => Ok(home
            .join("Library")
            .join("Application Support")
            .join("VizDAW")),
        _ => Err(
            "vizdaw-mcp: HOME is not set to an absolute path, so VizDAW's folder cannot be found."
                .into(),
        ),
    }
}

/// A previewed plan waiting for the composer's yes: never shown to the
/// model.
#[derive(Debug, Clone)]
struct Pending {
    token: String,
    commands: Value,
    window: Option<i64>,
}

/// An `initialize`-era question in flight: the `tools/call` it answers,
/// the `elicitation/create` id, and when it counts as no answer.
struct Wait {
    call: Value,
    elicit: String,
    pending: Pending,
    deadline: Instant,
}

/// A 2026-07-28 `requestState`, kept here: the client holds only its key.
struct Held {
    key: String,
    pending: Pending,
    expires: Instant,
}

/// The server's state: its tools, what the client declared, and the app
/// connection.
pub struct Server {
    app: App,
    commands: Value,
    tools: Vec<Value>,
    /// Command tools by name, with their `commands.list` entries.
    by_name: HashMap<String, Value>,
    /// The client declared form elicitation at `initialize`.
    elicitation: bool,
    ask_timeout: Duration,
    /// The question in flight, if any; lines meanwhile wait in `queue`.
    wait: Option<Wait>,
    queue: VecDeque<String>,
    queued_bytes: usize,
    next_elicit: u64,
    /// 2026-07-28 questions awaiting their retry, oldest first.
    held: Vec<Held>,
    /// Tokens handed to the model (the fallback), oldest first.
    tokens: VecDeque<String>,
    /// The documents whose notes the first answer already gave, oldest
    /// first, at most [`MAX_NOTED`].
    noted: VecDeque<String>,
}

/// What a request gives: a result now, or a question for the client while
/// the call waits.
enum Done {
    Result(Value),
    Ask(String),
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
        Server::with_ask_timeout(support, ASK_TIMEOUT)
    }

    /// A server whose composer has `limit` to answer (tests use a short one).
    pub fn with_ask_timeout(support: PathBuf, limit: Duration) -> Server {
        let commands = core_control::co_producer::list_json();
        let tools = tools::tools_json(&commands);
        let by_name = tools::command_tools(&commands).into_iter().collect();
        Server {
            app: App::new(&support),
            commands,
            tools,
            by_name,
            elicitation: false,
            ask_timeout: limit,
            wait: None,
            queue: VecDeque::new(),
            queued_bytes: 0,
            next_elicit: 1,
            held: Vec::new(),
            tokens: VecDeque::new(),
            noted: VecDeque::new(),
        }
    }

    /// One input line → zero or more output lines: replies, and an
    /// `elicitation/create` request when a change needs the composer's yes.
    /// While that question is open, lines are kept and handled after it,
    /// except a `ping`, the answer, and a cancel of the waiting call.
    pub fn handle_line(&mut self, line: &str) -> Vec<String> {
        let mut out = self.tick();
        let line = line.trim();
        if line.is_empty() {
            return out;
        }
        if self.wait.is_some() {
            out.extend(self.while_waiting(line));
        } else {
            out.extend(self.dispatch(line));
        }
        out.extend(self.drain());
        out
    }

    /// A line handled now.
    fn dispatch(&mut self, line: &str) -> Vec<String> {
        let (id, method, params) = match rpc::parse(line) {
            Err(reply) => return vec![reply],
            Ok(Message::Notification { .. } | Message::Response { .. }) => return Vec::new(),
            Ok(Message::Request { id, method, params }) => (id, method, params),
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| self.request(&id, &method, &params)));
        vec![match outcome {
            Ok(Ok(Done::Result(result))) => rpc::result_line(&id, result),
            Ok(Ok(Done::Ask(line))) => line,
            Ok(Err(e)) => rpc::error_line(&id, &e),
            Err(_) => internal_error(&id),
        }]
    }

    /// A line that came while the composer is asked.
    fn while_waiting(&mut self, line: &str) -> Vec<String> {
        let (call, elicit) = match &self.wait {
            Some(w) => (w.call.clone(), w.elicit.clone()),
            None => return self.dispatch(line),
        };
        match rpc::parse(line) {
            Err(reply) => vec![reply],
            Ok(Message::Response { id, result }) => {
                if id.as_str() == Some(elicit.as_str()) {
                    self.answered(result.as_ref())
                } else {
                    Vec::new()
                }
            }
            Ok(Message::Request { method, .. }) if method == "ping" => self.dispatch(line),
            Ok(Message::Notification { method, params })
                if method == "notifications/cancelled"
                    && params.get("requestId") == Some(&call) =>
            {
                self.wait = None;
                Vec::new()
            }
            Ok(m) => {
                if self.queue.len() < MAX_QUEUED
                    && self.queued_bytes.saturating_add(line.len()) <= MAX_QUEUED_BYTES
                {
                    self.queued_bytes = self.queued_bytes.saturating_add(line.len());
                    self.queue.push_back(line.to_string());
                    return Vec::new();
                }
                match m {
                    Message::Request { id, .. } => vec![rpc::error_line(
                        &id,
                        &RpcError::new(
                            rpc::WAITING,
                            "vizdaw-mcp is waiting for the composer's answer to a change: send this again after it.",
                        ),
                    )],
                    _ => Vec::new(),
                }
            }
        }
    }

    /// Lines kept during a question, handled once it ends, until another
    /// question starts.
    fn drain(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        while self.wait.is_none() {
            let Some(line) = self.queue.pop_front() else {
                break;
            };
            self.queued_bytes = self.queued_bytes.saturating_sub(line.len());
            out.extend(self.dispatch(&line));
        }
        out
    }

    /// The composer's answer to the open question: apply on a yes.
    fn answered(&mut self, result: Option<&Value>) -> Vec<String> {
        let Some(w) = self.wait.take() else {
            return Vec::new();
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.decided(rpc::elicitation_answer(result), &w.pending)
        }));
        vec![match outcome {
            Ok(v) => rpc::result_line(&w.call, v),
            Err(_) => internal_error(&w.call),
        }]
    }

    /// The tool result for `answer` to `pending`.
    fn decided(&mut self, answer: Answer, pending: &Pending) -> Value {
        match answer {
            Answer::Yes => self.apply(pending),
            Answer::No => tool_text(tools::SAID_NO, false),
            Answer::Unreadable => tool_error(tools::UNREADABLE),
        }
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

    /// Called at least once a second: a question unanswered for the ask
    /// timeout ends as a no (Ruling 8, P6), and the lines kept meanwhile
    /// are then handled.
    pub fn tick(&mut self) -> Vec<String> {
        let now = Instant::now();
        self.held.retain(|h| h.expires > now);
        let mut out = Vec::new();
        if self.wait.as_ref().is_some_and(|w| now >= w.deadline) {
            if let Some(w) = self.wait.take() {
                out.push(rpc::result_line(
                    &w.call,
                    tool_text(tools::NO_ANSWER, false),
                ));
                out.push(rpc::notification_line(
                    "notifications/cancelled",
                    json!({ "requestId": w.elicit, "reason": "The composer did not answer in time." }),
                ));
            }
        }
        out.extend(self.drain());
        out
    }

    fn request(
        &mut self,
        id: &Value,
        method: &str,
        params: &Map<String, Value>,
    ) -> Result<Done, RpcError> {
        let era = rpc::era(params)?;
        if method == "tools/call" {
            return self.call_tool(id, era, params);
        }
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
        Ok(Done::Result(rpc::finish(era, kind, result)))
    }

    fn initialize(&mut self, params: &Map<String, Value>) -> Value {
        self.elicitation = rpc::form_elicitation(params.get("capabilities"));
        json!({
            "protocolVersion": rpc::negotiate(params.get("protocolVersion")),
            "capabilities": capabilities(),
            "serverInfo": server_info(),
            "instructions": tools::INSTRUCTIONS,
        })
    }

    fn call_tool(
        &mut self,
        id: &Value,
        era: Era,
        params: &Map<String, Value>,
    ) -> Result<Done, RpcError> {
        let done = |v| Ok(Done::Result(rpc::finish(era, Kind::Plain, v)));
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
            Some(_) => return done(tool_error("`arguments` must be an object.")),
        };
        if let Some(spec) = self.by_name.get(name).cloned() {
            return done(self.run_command(&spec, args));
        }
        match name {
            tools::BEGIN_REQUEST => done(self.begin_request(args)),
            tools::END_REQUEST => done(self.simple("co_producer.end", args, &[])),
            tools::READ_NOTES => done(self.read_notes(args)),
            tools::ADD_NOTE => done(self.add_note(args)),
            tools::EXPORT_FOLDER => done(self.export_folder(args)),
            tools::SCREENSHOT => done(self.screenshot(args)),
            tools::CHANGE => match self.change(id, era, params, args) {
                Done::Result(v) => done(v),
                ask => Ok(ask),
            },
            tools::APPLY_CHANGE => done(self.apply_change(args)),
            _ => Err(RpcError::new(
                rpc::INVALID_PARAMS,
                format!("No tool {:?}.", rpc::truncate(name, 64)),
            )),
        }
    }

    /// Ruling 8: preview; apply at once when no yes is needed; else ask
    /// the composer through elicitation when the client can show a form;
    /// else give the model the list and a token. A 2026-07-28 retry
    /// (`params.requestState`, `params.inputResponses`) resumes a question.
    /// The composer's answer is read only from those host-filled fields,
    /// never from `arguments`, which the model writes (P5).
    fn change(
        &mut self,
        id: &Value,
        era: Era,
        params: &Map<String, Value>,
        args: &Map<String, Value>,
    ) -> Done {
        let a = match tools::change_args(args, false) {
            Ok(a) => a,
            Err(m) => return Done::Result(tool_error(m)),
        };
        if params.contains_key("requestState") || params.contains_key("inputResponses") {
            return Done::Result(self.retry(params, &a));
        }
        let preview = match self.app.call("co_producer.preview", plan_params(&a, None)) {
            Ok(p) => p,
            Err(e) => return Done::Result(self.failed(&e)),
        };
        let token = preview.get("token").and_then(Value::as_str).filter(|t| {
            t.len() == core_control::request::TOKEN_LEN
                && t.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        });
        let (Some(token), Some(items)) = (token, preview.get("items").and_then(Value::as_array))
        else {
            return Done::Result(tool_error(
                "VizDAW's preview answer could not be read, so nothing changed.",
            ));
        };
        let pending = Pending {
            token: token.to_string(),
            commands: a.commands.clone(),
            window: a.window,
        };
        // Only an explicit `needs_yes: false` skips the question.
        if preview.get("needs_yes") == Some(&Value::Bool(false)) {
            return Done::Result(self.apply(&pending));
        }
        let question = tools::question(items);
        match era {
            Era::Legacy if self.elicitation => {
                let elicit = format!("vz-elicit-{}", self.next_elicit);
                self.next_elicit = self.next_elicit.wrapping_add(1);
                let line = rpc::request_line(&elicit, "elicitation/create", question);
                self.wait = Some(Wait {
                    call: id.clone(),
                    elicit,
                    pending,
                    deadline: Instant::now() + self.ask_timeout,
                });
                Done::Ask(line)
            }
            Era::Modern if rpc::request_elicitation(params) => {
                let Some(key) = random_hex(12) else {
                    return Done::Result(tool_error(
                        "vizdaw-mcp could not make a one-time key for the question, so nothing changed.",
                    ));
                };
                let now = Instant::now();
                self.held.retain(|h| h.expires > now);
                while self.held.len() >= MAX_HELD {
                    self.held.remove(0);
                }
                self.held.push(Held {
                    key: key.clone(),
                    pending,
                    expires: now + self.ask_timeout,
                });
                Done::Result(json!({
                    "resultType": "input_required",
                    "inputRequests": { "yes": { "method": "elicitation/create", "params": question } },
                    "requestState": key,
                }))
            }
            _ => {
                while self.tokens.len() >= MAX_HELD {
                    self.tokens.pop_front();
                }
                self.tokens.push_back(pending.token.clone());
                Done::Result(tool_text(
                    tools::list_for_the_model(items, &pending.token),
                    false,
                ))
            }
        }
    }

    /// A 2026-07-28 retry: its `requestState` is used up whatever happens,
    /// and must be one this server holds, for the same commands.
    fn retry(&mut self, params: &Map<String, Value>, a: &ChangeArgs) -> Value {
        let now = Instant::now();
        self.held.retain(|h| h.expires > now);
        let key = params.get("requestState").and_then(Value::as_str);
        let Some(at) = key.and_then(|k| self.held.iter().position(|h| h.key == k)) else {
            return tool_error(tools::EXPIRED);
        };
        let held = self.held.remove(at);
        if held.pending.commands != a.commands || held.pending.window != a.window {
            return tool_error(tools::EXPIRED);
        }
        let answer = match params.get("inputResponses") {
            None | Some(Value::Null) => Answer::No,
            Some(Value::Object(r)) => match r.get("yes") {
                None => Answer::No,
                Some(v) => rpc::elicitation_answer(Some(v)),
            },
            Some(_) => Answer::Unreadable,
        };
        self.decided(answer, &held.pending)
    }

    /// The fallback's second half: a token `change` gave the model in this
    /// conversation, used once.
    fn apply_change(&mut self, args: &Map<String, Value>) -> Value {
        let a = match tools::change_args(args, true) {
            Ok(a) => a,
            Err(m) => return tool_error(m),
        };
        let token = a.token.clone().unwrap_or_default();
        let Some(at) = self.tokens.iter().position(|t| *t == token) else {
            return tool_error(tools::NOT_OUR_TOKEN);
        };
        self.tokens.remove(at);
        self.apply(&Pending {
            token,
            commands: a.commands,
            window: a.window,
        })
    }

    /// Sends `co_producer.apply` and answers its results.
    fn apply(&mut self, p: &Pending) -> Value {
        let a = ChangeArgs {
            commands: p.commands.clone(),
            window: p.window,
            token: None,
        };
        match self
            .app
            .call("co_producer.apply", plan_params(&a, Some(&p.token)))
        {
            Ok(result) => {
                let mut text = result.to_string();
                if let Some(label) = result.get("undo_label").and_then(Value::as_str) {
                    if label.starts_with("Co-Producer: ") {
                        text.push_str("\nUndo step: ");
                        text.push_str(label);
                    }
                }
                tool_text(text, false)
            }
            Err(e) => self.failed(&e),
        }
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
    /// Err is the tool error to answer with.
    fn pass(
        &mut self,
        cmd: &str,
        args: &Map<String, Value>,
        allowed: &[&str],
    ) -> Result<Value, Value> {
        let mut params = Map::new();
        for (k, v) in args {
            if k == "window" {
                match tools::window_arg(v) {
                    Ok(Some(w)) => {
                        params.insert(k.clone(), json!(w));
                    }
                    Ok(None) => {}
                    Err(m) => return Err(tool_error(m)),
                }
            } else if allowed.contains(&k.as_str()) {
                params.insert(k.clone(), v.clone());
            } else {
                return Err(tool_error(format!(
                    "unknown argument `{}`",
                    rpc::truncate(k, 64)
                )));
            }
        }
        self.app
            .call(cmd, Value::Object(params))
            .map_err(|e| self.failed(&e))
    }

    fn simple(&mut self, cmd: &str, args: &Map<String, Value>, allowed: &[&str]) -> Value {
        match self.pass(cmd, args, allowed) {
            Ok(result) => tool_text(result.to_string(), false),
            Err(e) => e,
        }
    }

    fn begin_request(&mut self, args: &Map<String, Value>) -> Value {
        match args.get("name") {
            Some(Value::String(s))
                if s.chars().count() <= core_control::request::MAX_REQUEST_NAME => {}
            Some(Value::String(_)) => {
                return tool_error(format!(
                    "`name` must be at most {} characters",
                    core_control::request::MAX_REQUEST_NAME
                ));
            }
            Some(_) => return tool_error("`name` must be a string"),
            None => return tool_error("missing `name`"),
        }
        if let Some(Err(m)) = args.get("asked").map(tools::check_asked) {
            return tool_error(m);
        }
        let begun = match self.pass("co_producer.begin", args, &["name", "asked"]) {
            Ok(r) => r,
            Err(e) => return e,
        };
        let mut text = begun.to_string();
        let doc = begun.get("doc").and_then(Value::as_str).unwrap_or("");
        if !self.noted.iter().any(|d| d == doc) {
            // A failed read leaves the answer as it is; the next request tries again.
            let window: Map<String, Value> = args
                .get("window")
                .map(|w| ("window".to_string(), w.clone()))
                .into_iter()
                .collect();
            // The notes call is separate, so the front document may have
            // changed: notes that name another document are not shown.
            let same_song = |n: &Value| {
                n.get("doc")
                    .and_then(Value::as_str)
                    .is_none_or(|d| d == doc)
            };
            if let Some(notes) = self
                .pass("co_producer.notes", &window, &[])
                .ok()
                .filter(same_song)
            {
                if let Some(notes) = notes.get("text").and_then(Value::as_str) {
                    let (shown, cut) = first_notes(notes);
                    text.push_str("\n\nThe song's notes (Notes.md):\n");
                    text.push_str(shown);
                    if cut {
                        text.push_str("\n(The notes go on: read the rest with read_notes.)");
                    }
                    while self.noted.len() >= MAX_NOTED {
                        self.noted.pop_front();
                    }
                    self.noted.push_back(doc.to_string());
                }
            }
        }
        tool_text(text, false)
    }

    fn read_notes(&mut self, args: &Map<String, Value>) -> Value {
        match self.pass("co_producer.notes", args, &[]) {
            Ok(r) => match r.get("text").and_then(Value::as_str) {
                Some(t) => tool_text(t, false),
                None => tool_error("VizDAW's notes answer held no text."),
            },
            Err(e) => e,
        }
    }

    fn add_note(&mut self, args: &Map<String, Value>) -> Value {
        match tools::check_notes_args(tools::ADD_NOTE, args) {
            Ok(()) => self.simple("co_producer.note", args, &["text"]),
            Err(m) => tool_error(m),
        }
    }

    fn export_folder(&mut self, args: &Map<String, Value>) -> Value {
        if let Err(m) = tools::check_notes_args(tools::EXPORT_FOLDER, args) {
            return tool_error(m);
        }
        match self.pass("co_producer.export_folder", args, &["what", "parent"]) {
            Ok(r) => match r.get("path").and_then(Value::as_str) {
                Some(p) => tool_text(p, false),
                None => tool_text(r.to_string(), false),
            },
            Err(e) => e,
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

/// The notes as the first answer shows them: at most [`MAX_FIRST_NOTES`]
/// bytes, cut on a character boundary, and whether anything was cut.
fn first_notes(notes: &str) -> (&str, bool) {
    if notes.len() <= MAX_FIRST_NOTES {
        return (notes, false);
    }
    let mut end = MAX_FIRST_NOTES;
    while !notes.is_char_boundary(end) {
        end -= 1;
    }
    (notes.get(..end).unwrap_or(""), true)
}

fn internal_error(id: &Value) -> String {
    rpc::error_line(
        id,
        &RpcError::new(rpc::INTERNAL_ERROR, "Internal error: vizdaw-mcp went on."),
    )
}

/// A preview's or apply's params.
fn plan_params(a: &ChangeArgs, token: Option<&str>) -> Value {
    let mut p = Map::new();
    p.insert("commands".into(), a.commands.clone());
    if let Some(w) = a.window {
        p.insert("window".into(), json!(w));
    }
    if let Some(t) = token {
        p.insert("token".into(), json!(t));
    }
    Value::Object(p)
}

/// `n` random bytes from `/dev/urandom` as lowercase hex; None when it
/// cannot be read (no weaker fallback).
fn random_hex(n: usize) -> Option<String> {
    let mut bytes = vec![0u8; n];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut bytes))
        .ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
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
