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
pub mod recipes;
pub mod rpc;
pub mod tools;
pub mod views;

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
    /// The preview's items, for the change view.
    items: Vec<Value>,
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
    /// The client asked for MCP Apps views at `initialize` (Ruling 7).
    views: bool,
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
    /// The document of the latest begin: a note or an export folder sent
    /// without `window` must land there, not in the front document.
    begun: Option<String>,
    /// The window the latest begin named, if it named one: a `change` or
    /// `apply_change` sent without `window` goes there (plan 8e final
    /// review Minors 1 and 6).
    begun_window: Option<i64>,
    /// What the client called itself at `initialize`, cleaned: every
    /// `co_producer.apply` carries it as `client` (plan 8b Ruling 11).
    client: Option<String>,
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

/// `result` with `structured` as its `structuredContent` (Ruling 8), when
/// views are on; the text content is left as it is.
fn with_view(mut result: Value, structured: Option<Value>) -> Value {
    if let (Some(sc), Some(o)) = (structured, result.as_object_mut()) {
        o.insert("structuredContent".into(), sc);
    }
    result
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
            views: false,
            ask_timeout: limit,
            wait: None,
            queue: VecDeque::new(),
            queued_bytes: 0,
            next_elicit: 1,
            held: Vec::new(),
            tokens: VecDeque::new(),
            noted: VecDeque::new(),
            begun: None,
            begun_window: None,
            client: None,
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
        let views = self.views;
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            self.decided(rpc::elicitation_answer(result), &w.pending, views)
        }));
        vec![match outcome {
            Ok(v) => rpc::result_line(&w.call, v),
            Err(_) => internal_error(&w.call),
        }]
    }

    /// The tool result for `answer` to `pending`, with the change view
    /// when `views` is on.
    fn decided(&mut self, answer: Answer, pending: &Pending, views: bool) -> Value {
        match answer {
            Answer::Yes => self.apply(pending, views.then_some("applied")),
            Answer::No => said_no(tools::SAID_NO, pending, views),
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
                    said_no(tools::NO_ANSWER, &w.pending, self.views),
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
                    "capabilities": capabilities(self.views_for(era, params)),
                    "serverInfo": server_info(),
                    "instructions": tools::INSTRUCTIONS,
                }),
            ),
            "tools/list" if self.views_for(era, params) => (
                Kind::Cacheable,
                json!({ "tools": views::with_tool_meta(&self.tools) }),
            ),
            "tools/list" => (Kind::Cacheable, json!({ "tools": self.tools })),
            "resources/list" => {
                let mut resources = tools::resources_json();
                if self.views_for(era, params) {
                    resources.extend(views::resources_json());
                }
                (Kind::Cacheable, json!({ "resources": resources }))
            }
            "resources/templates/list" => (Kind::Cacheable, json!({ "resourceTemplates": [] })),
            "prompts/list" => (Kind::Cacheable, recipes::prompts_list()),
            "prompts/get" => (Kind::Cacheable, recipes::prompt_get(params)?),
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

    /// Whether a request with `params` in `era` gets views: in the
    /// `initialize` era as the client asked then, in 2026-07-28 as the
    /// request itself asks.
    fn views_for(&self, era: Era, params: &Map<String, Value>) -> bool {
        match era {
            Era::Legacy => self.views,
            Era::Modern => rpc::request_views(params),
        }
    }

    fn initialize(&mut self, params: &Map<String, Value>) -> Value {
        let version = rpc::negotiate(params.get("protocolVersion"));
        self.elicitation = rpc::form_elicitation(params.get("capabilities"));
        self.views = rpc::initialize_views(version, params.get("capabilities"));
        self.client = tools::client_label(params.get("clientInfo").and_then(|c| c.get("name")));
        json!({
            "protocolVersion": version,
            "capabilities": capabilities(self.views),
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
        let views = self.views_for(era, params);
        match name {
            tools::BEGIN_REQUEST => done(self.begin_request(args)),
            tools::END_REQUEST => done(self.end_request(args)),
            tools::READ_NOTES => done(self.read_notes(args)),
            tools::ADD_NOTE => done(self.add_note(args)),
            tools::EXPORT_FOLDER => done(self.export_folder(args)),
            tools::EXPORT => done(self.export(args, views)),
            tools::SHOW_TRACKS => done(self.show_tracks(args, views)),
            tools::SHOW_MIXER => done(self.show_mixer(args, views)),
            tools::LIST_PLUGINS => done(self.list_plugins(args)),
            tools::PLUGIN_PARAMS => done(self.plugin_params(args)),
            tools::SCREENSHOT => done(self.screenshot(args)),
            tools::STARTER_PROJECT => done(self.starter_project(args)),
            tools::READ_MIDI_NOTES => done(self.read_midi_notes(args)),
            tools::PREVIEW_AUTOMATION => done(self.preview_automation(args, views)),
            tools::RECIPES => done(
                match tools::recipe_name(args).and_then(recipes::recipes_answer) {
                    Ok(t) => tool_text(t, false),
                    Err(m) => tool_error(m),
                },
            ),
            tools::CHANGE => match self.change(id, era, params, args, views) {
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
        views: bool,
    ) -> Done {
        let mut a = match tools::change_args(args, false)
            .and_then(|a| tools::listed(&self.commands, &a.commands).map(|()| a))
        {
            Ok(a) => a,
            Err(m) => return Done::Result(tool_error(m)),
        };
        a.window = match self.begun_target(a.window) {
            Ok(w) => w,
            Err(e) => return Done::Result(e),
        };
        if params.contains_key("requestState") || params.contains_key("inputResponses") {
            return Done::Result(self.retry(params, &a, views));
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
            items: items.iter().take(tools::MAX_LISTED).cloned().collect(),
        };
        // Only an explicit `needs_yes: false` skips the question.
        if preview.get("needs_yes") == Some(&Value::Bool(false)) {
            return Done::Result(self.apply(&pending, views.then_some("applied_without_asking")));
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
                // The token is only in the text, never in the view.
                Done::Result(with_view(
                    tool_text(tools::list_for_the_model(items, &pending.token), false),
                    views.then(|| tools::change_view("ask_in_chat", items, None)),
                ))
            }
        }
    }

    /// A 2026-07-28 retry: its `requestState` is used up whatever happens,
    /// and must be one this server holds, for the same commands.
    fn retry(&mut self, params: &Map<String, Value>, a: &ChangeArgs, views: bool) -> Value {
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
        self.decided(answer, &held.pending, views)
    }

    /// The fallback's second half: a token `change` gave the model in this
    /// conversation, used once.
    fn apply_change(&mut self, args: &Map<String, Value>) -> Value {
        let mut a = match tools::change_args(args, true)
            .and_then(|a| tools::listed(&self.commands, &a.commands).map(|()| a))
        {
            Ok(a) => a,
            Err(m) => return tool_error(m),
        };
        a.window = match self.begun_target(a.window) {
            Ok(w) => w,
            Err(e) => return e,
        };
        let token = a.token.clone().unwrap_or_default();
        let Some(at) = self.tokens.iter().position(|t| *t == token) else {
            return tool_error(tools::NOT_OUR_TOKEN);
        };
        self.tokens.remove(at);
        self.apply(
            &Pending {
                token,
                commands: a.commands,
                window: a.window,
                items: Vec::new(),
            },
            None,
        )
    }

    /// Sends `co_producer.apply` and answers its results; with the change
    /// view in `state` when one is given.
    fn apply(&mut self, p: &Pending, state: Option<&str>) -> Value {
        let a = ChangeArgs {
            commands: p.commands.clone(),
            window: p.window,
            token: None,
        };
        let mut params = plan_params(&a, Some(&p.token));
        if let (Some(client), Some(o)) = (&self.client, params.as_object_mut()) {
            o.insert("client".into(), json!(client));
        }
        match self.app.call("co_producer.apply", params) {
            Ok(result) => {
                let mut text = result.to_string();
                if let Some(label) = result.get("undo_label").and_then(Value::as_str) {
                    if label.starts_with("Co-Producer: ") {
                        text.push_str("\nUndo step: ");
                        text.push_str(label);
                    }
                }
                let undo = result.get("undo_label").and_then(Value::as_str);
                with_view(
                    tool_text(text, false),
                    state.map(|s| tools::change_view(s, &p.items, undo)),
                )
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
        self.begun = begun.get("doc").and_then(Value::as_str).map(str::to_string);
        // `pass` already refused a `window` that is not a window number.
        self.begun_window = args
            .get("window")
            .and_then(|w| tools::window_arg(w).ok())
            .flatten();
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

    fn end_request(&mut self, args: &Map<String, Value>) -> Value {
        match self.pass("co_producer.end", args, &[]) {
            Ok(r) => {
                if r.get("doc").and_then(Value::as_str) == self.begun.as_deref() {
                    self.begun = None;
                    self.begun_window = None;
                }
                tool_text(r.to_string(), false)
            }
            Err(e) => e,
        }
    }

    /// A write sent without `window` goes to the front document, which may
    /// not be the begin's: check the front document first, and the answer's.
    fn begun_front(&mut self, args: &Map<String, Value>) -> Result<(), Value> {
        let Some(want) = self.begun.clone() else {
            return Ok(());
        };
        if args.get("window").is_some_and(|w| !w.is_null()) {
            return Ok(());
        }
        let status = self.pass("co_producer.status", &Map::new(), &[])?;
        match status.get("doc").and_then(Value::as_str) {
            Some(d) if d != want => Err(other_song()),
            _ => Ok(()),
        }
    }

    /// Plan 8e final review Minors 1 and 6: the window a `change` or
    /// `apply_change` goes to. Without `window` it is the begin's window
    /// when the begin named one; else the front song, which must be the
    /// begin's. A `window` other than the begin's must hold the begin's
    /// song. Err is the refusal; nothing reached the song.
    fn begun_target(&mut self, window: Option<i64>) -> Result<Option<i64>, Value> {
        let Some(want) = self.begun.clone() else {
            return Ok(window);
        };
        let window = window.or(self.begun_window);
        if window.is_some() && window == self.begun_window {
            return Ok(window);
        }
        let mut p = Map::new();
        if let Some(w) = window {
            p.insert("window".into(), json!(w));
        }
        let status = self.pass("co_producer.status", &p, &[])?;
        match (status.get("doc").and_then(Value::as_str), window) {
            (Some(d), Some(w)) if d != want => Err(another_song(w)),
            (Some(d), None) if d != want => Err(other_song()),
            _ => Ok(window),
        }
    }

    /// The answer must name the begin's document too.
    fn begun_answer(&self, args: &Map<String, Value>, r: &Value) -> Result<(), Value> {
        match (&self.begun, r.get("doc").and_then(Value::as_str)) {
            (Some(want), Some(d))
                if d != want && !args.get("window").is_some_and(|w| !w.is_null()) =>
            {
                Err(other_song())
            }
            _ => Ok(()),
        }
    }

    fn add_note(&mut self, args: &Map<String, Value>) -> Value {
        if let Err(m) = tools::check_notes_args(tools::ADD_NOTE, args) {
            return tool_error(m);
        }
        if let Err(e) = self.begun_front(args) {
            return e;
        }
        match self.pass("co_producer.note", args, &["text"]) {
            Ok(r) => match self.begun_answer(args, &r) {
                Ok(()) => tool_text(r.to_string(), false),
                Err(e) => e,
            },
            Err(e) => e,
        }
    }

    fn export_folder(&mut self, args: &Map<String, Value>) -> Value {
        if let Err(m) = tools::check_notes_args(tools::EXPORT_FOLDER, args) {
            return tool_error(m);
        }
        if let Err(e) = self.begun_front(args) {
            return e;
        }
        match self.pass("co_producer.export_folder", args, &["what", "parent"]) {
            Ok(r) => {
                if let Err(e) = self.begun_answer(args, &r) {
                    return e;
                }
                match r.get("path").and_then(Value::as_str) {
                    Some(p) => tool_text(p, false),
                    None => tool_text(r.to_string(), false),
                }
            }
            Err(e) => e,
        }
    }

    /// Ruling 8: one call exports into a new dated folder. The front
    /// document must be the begin's, as for the other writes.
    fn export(&mut self, args: &Map<String, Value>, views: bool) -> Value {
        if let Err(m) = tools::check_export_args(args) {
            return tool_error(m);
        }
        if let Err(e) = self.begun_front(args) {
            return e;
        }
        let r = match self.pass("co_producer.export", args, &["what", "parent", "strips"]) {
            Ok(r) => r,
            Err(e) => return e,
        };
        if let Err(e) = self.begun_answer(args, &r) {
            return e;
        }
        let what = args.get("what").and_then(Value::as_str).unwrap_or("");
        match tools::export_answer(what, &r) {
            Ok((text, structured)) => {
                with_view(tool_text(text, false), views.then_some(structured))
            }
            Err(m) => tool_error(m),
        }
    }

    /// Ruling 8: the tracks and strips, from two state reads.
    fn show_tracks(&mut self, args: &Map<String, Value>, views: bool) -> Value {
        if let Some(k) = args.keys().next() {
            return tool_error(format!(
                "show_tracks takes no arguments (got `{}`)",
                rpc::truncate(k, 64)
            ));
        }
        let document = match self.app.call("state.get", json!({"target": "document"})) {
            Ok(v) => v,
            Err(e) => return self.failed(&e),
        };
        let mixer = match self.app.call("state.get", json!({"target": "mixer"})) {
            Ok(v) => v,
            Err(e) => return self.failed(&e),
        };
        let same = |a: &Value, b: &Value| a.get("doc") == b.get("doc");
        if !same(&document, &mixer) {
            return other_song();
        }
        let (text, structured) = tools::tracks_answer(&document, &mixer);
        with_view(tool_text(text, false), views.then_some(structured))
    }

    /// Plan 8d Ruling 5: the mixer, from one state read.
    fn show_mixer(&mut self, args: &Map<String, Value>, views: bool) -> Value {
        if let Some(k) = args.keys().next() {
            return tool_error(format!(
                "show_mixer takes no arguments (got `{}`)",
                rpc::truncate(k, 64)
            ));
        }
        match self.app.call("state.get", json!({"target": "mixer"})) {
            Ok(mixer) => {
                let (text, structured) = tools::mixer_answer(&mixer);
                with_view(tool_text(text, false), views.then_some(structured))
            }
            Err(e) => self.failed(&e),
        }
    }

    /// Plan 8d Ruling 5: the plug-ins Co-Producer may use.
    fn list_plugins(&mut self, args: &Map<String, Value>) -> Value {
        if let Err(m) = tools::check_plugin_args(tools::LIST_PLUGINS, args) {
            return tool_error(m);
        }
        match self.pass("co_producer.plugins", args, &["kind"]) {
            Ok(r) => tool_text(tools::plugins_answer(&r), false),
            Err(e) => e,
        }
    }

    /// Plan 8d Ruling 5: a loaded plug-in's parameters.
    fn plugin_params(&mut self, args: &Map<String, Value>) -> Value {
        if let Err(m) = tools::check_plugin_args(tools::PLUGIN_PARAMS, args) {
            return tool_error(m);
        }
        match self.pass("co_producer.params", args, &["slot"]) {
            Ok(r) => tool_text(tools::params_answer(&r), false),
            Err(e) => e,
        }
    }

    /// Plan 8e Ruling 7: a new song from a starter spec, checked here
    /// first so a bad spec opens nothing.
    fn starter_project(&mut self, args: &Map<String, Value>) -> Value {
        let params = match tools::starter_params(args) {
            Ok(p) => p,
            Err(m) => return tool_error(m),
        };
        match self.app.call("co_producer.starter", params) {
            Ok(r) => tool_text(tools::starter_answer(&r), false),
            Err(e) => self.failed(&e),
        }
    }

    /// Plan 8e Ruling 7: a track's notes, read only.
    fn read_midi_notes(&mut self, args: &Map<String, Value>) -> Value {
        let params = match tools::midi_notes_params(args) {
            Ok(p) => p,
            Err(m) => return tool_error(m),
        };
        match self.app.call("co_producer.midi_notes", params) {
            Ok(r) => tool_text(tools::notes_answer(&r), false),
            Err(e) => self.failed(&e),
        }
    }

    /// Plan 8e Ruling 7: the planned curve, checked here, over the one
    /// there now.
    fn preview_automation(&mut self, args: &Map<String, Value>, views: bool) -> Value {
        let (params, planned) = match tools::automation_params(args) {
            Ok(p) => p,
            Err(m) => return tool_error(m),
        };
        let mut current = match self.app.call("co_producer.automation", params.clone()) {
            Ok(r) => r,
            Err(e) => return self.failed(&e),
        };
        if let Some(o) = current.as_object_mut() {
            for k in ["start", "end"] {
                o.insert(k.into(), params.get(k).cloned().unwrap_or(Value::Null));
            }
        }
        let (text, structured) = tools::automation_answer(&current, &planned);
        with_view(tool_text(text, false), views.then_some(structured))
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
        if uri.starts_with("ui://") {
            // A view holds no song data: it is served to anyone who asks.
            return match views::read(uri) {
                Some(c) => Ok(json!({ "contents": [c] })),
                None => Err(RpcError::new(
                    rpc::RESOURCE_NOT_FOUND,
                    format!("No resource {:?}.", rpc::truncate(uri, 128)),
                )),
            };
        }
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

/// The server's capabilities; with the MCP Apps extension when the
/// client asked for views.
fn capabilities(views: bool) -> Value {
    let prompts = json!({ "listChanged": false });
    if views {
        json!({ "tools": {}, "resources": {}, "prompts": prompts, "extensions": { views::EXTENSION: {} } })
    } else {
        json!({ "tools": {}, "resources": {}, "prompts": prompts })
    }
}

/// A "no" answer: `text`, with the change view's `said_no` when `views`.
fn said_no(text: &str, pending: &Pending, views: bool) -> Value {
    with_view(
        tool_text(text, false),
        views.then(|| tools::change_view("said_no", &pending.items, None)),
    )
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

/// The refusal when a note or an export folder would land in another song.
/// Window `w` holds another song than the open request's.
fn another_song(w: i64) -> Value {
    tool_error(format!(
        "Window {w} is not the song this request began in. Work in that song, or end this request and begin one in window {w}."
    ))
}

fn other_song() -> Value {
    tool_error(
        "The front song is not the one this request began in. Bring that song's window forward, or name its `window`.",
    )
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
