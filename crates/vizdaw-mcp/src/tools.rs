//! The tools and resources (plan 8a Ruling 12): one tool per read, add or
//! outward command of the linked Co-Producer list, Co-Producer's own
//! tools, and the state resources. Arguments are checked against the
//! command's params here, and VizDAW's own gate checks them again.

use core_control::co_producer::MAX_PLAN;
use core_control::params::{MAX_PATH, MAX_STRING};
use core_control::request::{MAX_REQUEST_NAME, TOKEN_LEN};
use serde_json::{Map, Value, json};

/// The server's instructions (Ruling 12).
pub const INSTRUCTIONS: &str = "VizDAW's Co-Producer. Adds are free; anything that changes or deletes the composer's own work goes through the change tool and their yes. Start every request with begin_request and finish it with end_request.";

/// Starts a request.
pub const BEGIN_REQUEST: &str = "begin_request";
/// Ends it.
pub const END_REQUEST: &str = "end_request";
/// Previews a change and asks for the composer's yes (Task 6).
pub const CHANGE: &str = "change";
/// Applies a change the composer agreed to (Task 6).
pub const APPLY_CHANGE: &str = "apply_change";
/// A picture of a window.
pub const SCREENSHOT: &str = "screenshot";

/// The `state.get` targets offered as resources: all but `input`, because
/// Co-Producer does not listen.
pub const STATE_TARGETS: &[&str] = &[
    "transport",
    "document",
    "editor",
    "selection",
    "windows",
    "media",
    "project",
    "record",
    "automation",
    "surfaces",
    "layout",
    "mixer",
    "audio_editor",
];

/// The resource of the Co-Producer command list.
pub const COMMANDS_URI: &str = "vizdaw://commands";
/// What starts a state resource's URI.
pub const STATE_URI: &str = "vizdaw://state/";

const OUTWARD_PATH: &str = "An absolute path. A file there is never replaced: give a new file, or for stems and folder exports an existing folder that VizDAW puts a new numbered folder in.";
const IMPORT_PATH: &str = "An absolute path to the file to read.";

/// A tool's name: its registry id with `.` turned into `_`.
pub fn tool_name(id: &str) -> String {
    id.replace('.', "_")
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// Whether a command of the list gets a tool: read, add and outward do;
/// change and remove go through `change`.
fn has_tool(c: &Value) -> bool {
    matches!(str_of(c, "class"), "read" | "add" | "outward")
}

fn params_of(spec: &Value) -> &[Value] {
    spec.get("params")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn window_schema() -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "description": "The VizDAW window to act in, by its number from the windows state (default: the front one).",
    })
}

/// One param's schema; None for `replace`, which Co-Producer refuses.
fn param_schema(p: &Value, outward: bool) -> Option<Value> {
    let name = str_of(p, "name");
    if name == "replace" {
        return None;
    }
    let help = str_of(p, "help").to_string();
    let min = p.get("min").cloned().unwrap_or(Value::Null);
    let max = p.get("max").cloned().unwrap_or(Value::Null);
    let schema = match str_of(p, "type") {
        "string" => json!({"type": "string", "maxLength": MAX_STRING, "description": help}),
        "path" => {
            let extra = if outward { OUTWARD_PATH } else { IMPORT_PATH };
            json!({"type": "string", "maxLength": MAX_PATH, "description": format!("{help} {extra}")})
        }
        "bool" => json!({"type": "boolean", "description": help}),
        "choice" => match p.get("or_prefixed").and_then(Value::as_str) {
            Some(prefix) => json!({
                "type": "string",
                "maxLength": MAX_STRING,
                "description": format!(
                    "{help} One of {}, or {prefix} and a name.",
                    p.get("values").map_or(String::new(), Value::to_string)
                ),
            }),
            None => json!({
                "type": "string",
                "enum": p.get("values").cloned().unwrap_or(json!([])),
                "description": help,
            }),
        },
        "int" => json!({"type": "integer", "minimum": min, "maximum": max, "description": help}),
        "int_or_null" => json!({
            "type": ["integer", "null"], "minimum": min, "maximum": max, "description": help,
        }),
        _ => json!({"description": help}),
    };
    Some(schema)
}

fn object_schema(properties: Map<String, Value>, required: Vec<Value>) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

/// The tool of one command of the list.
fn command_tool(c: &Value) -> Value {
    let class = str_of(c, "class");
    let outward = class == "outward";
    let mut properties = Map::new();
    let mut required = Vec::new();
    for p in params_of(c) {
        let Some(schema) = param_schema(p, outward) else {
            continue;
        };
        let name = str_of(p, "name").to_string();
        if p.get("required").and_then(Value::as_bool) == Some(true) {
            required.push(json!(name));
        }
        properties.insert(name, schema);
    }
    properties.insert("window".into(), window_schema());
    let (line, annotations) = match class {
        "read" => (
            "Reads or plays; changes nothing.",
            json!({"readOnlyHint": true}),
        ),
        "add" => (
            "Adds something new; needs an open request (begin_request).",
            json!({"destructiveHint": false, "openWorldHint": false}),
        ),
        _ => (
            "Writes new files only; refuses a path that exists. Needs an open request (begin_request).",
            json!({"destructiveHint": false, "openWorldHint": false}),
        ),
    };
    json!({
        "name": tool_name(str_of(c, "id")),
        "description": format!("{} ({})\n{line}", str_of(c, "title"), str_of(c, "menu")),
        "inputSchema": object_schema(properties, required),
        "annotations": annotations,
    })
}

fn plan_schema() -> Value {
    json!({
        "type": "array",
        "minItems": 1,
        "maxItems": MAX_PLAN,
        "description": "The commands, in order: each an id from the command list and its params.",
        "items": {
            "type": "object",
            "properties": {
                "id": {"type": "string", "maxLength": 64},
                "params": {"type": "object"},
            },
            "required": ["id"],
            "additionalProperties": false,
        },
    })
}

fn own_tool(
    name: &str,
    description: &str,
    props: &[(&str, Value)],
    required: &[&str],
    annotations: Value,
) -> Value {
    let mut properties: Map<String, Value> = props
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect();
    properties.insert("window".into(), window_schema());
    json!({
        "name": name,
        "description": description,
        "inputSchema": object_schema(properties, required.iter().map(|r| json!(r)).collect()),
        "annotations": annotations,
    })
}

/// Co-Producer's own tools, with Ruling 12's descriptions.
fn own_tools() -> Vec<Value> {
    let quiet = || json!({"destructiveHint": false, "openWorldHint": false});
    vec![
        own_tool(
            BEGIN_REQUEST,
            "Start one thing the composer asked for, before any command that adds or changes anything. Name it in a few plain words, with no product or company names, as the composer would say it (\"set up a drum bus\"). Everything until end_request becomes one undo step, \"Co-Producer: <Name>\". Only one request is open per song.",
            &[(
                "name",
                json!({"type": "string", "minLength": 1, "maxLength": MAX_REQUEST_NAME, "description": "What the composer asked for, in a few plain words."}),
            )],
            &["name"],
            quiet(),
        ),
        own_tool(
            END_REQUEST,
            "Finish the open request. Its steps become one undo step. Call it as soon as the request is done.",
            &[],
            &[],
            quiet(),
        ),
        own_tool(
            CHANGE,
            "Change or delete something that already exists, such as renaming or deleting a track, Undo, Redo, or Save. Needs an open request (begin_request). VizDAW first works out exactly what will change. If the composer must agree, VizDAW asks them itself when it can. Otherwise this returns the list and a token: show the composer the whole list, word for word, and call apply_change only after they say yes in this chat. Never call apply_change on your own judgement.",
            &[("commands", plan_schema())],
            &["commands"],
            json!({"destructiveHint": true}),
        ),
        own_tool(
            APPLY_CHANGE,
            "Apply a change the composer has agreed to in this chat, with the token and the same commands change returned. Fails if the song changed since, or the token was used.",
            &[
                (
                    "token",
                    json!({"type": "string", "minLength": TOKEN_LEN, "maxLength": TOKEN_LEN, "pattern": "^[0-9a-f]{32}$"}),
                ),
                ("commands", plan_schema()),
            ],
            &["token", "commands"],
            json!({"destructiveHint": true}),
        ),
        own_tool(
            SCREENSHOT,
            "A picture of a VizDAW window, for reading what is on screen. It cannot click or type.",
            &[],
            &[],
            json!({"readOnlyHint": true}),
        ),
    ]
}

/// The commands of `commands` (a Co-Producer `commands.list` result) that
/// get a tool, by tool name.
pub fn command_tools(commands: &Value) -> Vec<(String, Value)> {
    commands
        .get("commands")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|c| has_tool(c))
        .map(|c| (tool_name(str_of(c, "id")), c.clone()))
        .collect()
}

/// Every tool: one per read, add or outward command of `commands` (a
/// Co-Producer `commands.list` result), then Co-Producer's own.
pub fn tools_json(commands: &Value) -> Vec<Value> {
    let mut tools: Vec<Value> = command_tools(commands)
        .iter()
        .map(|(_, c)| command_tool(c))
        .collect();
    tools.extend(own_tools());
    tools
}

fn in_range(n: i64, p: &Value) -> bool {
    let min = p.get("min").and_then(Value::as_i64).unwrap_or(i64::MIN);
    let max = p.get("max").and_then(Value::as_i64).unwrap_or(i64::MAX);
    (min..=max).contains(&n)
}

/// A tool's `window` argument: a positive window number.
pub fn window_arg(v: &Value) -> Result<Option<i64>, String> {
    match v {
        Value::Null => Ok(None),
        Value::Number(n) => match n.as_i64() {
            Some(i) if i > 0 => Ok(Some(i)),
            _ => Err("`window` must be a positive window number".into()),
        },
        _ => Err("`window` must be a window number".into()),
    }
}

/// Checks one argument against its param.
fn check_arg(p: &Value, name: &str, v: &Value) -> Result<(), String> {
    let bad = |what: &str| Err(format!("`{name}` must be {what}"));
    match (str_of(p, "type"), v) {
        ("string", Value::String(s)) if s.chars().count() <= MAX_STRING => Ok(()),
        ("string", Value::String(_)) => bad(&format!("at most {MAX_STRING} characters")),
        ("path", Value::String(s)) if s.len() <= MAX_PATH => Ok(()),
        ("path", Value::String(_)) => bad(&format!("a path of at most {MAX_PATH} bytes")),
        ("string" | "path", _) => bad("a string"),
        ("bool", Value::Bool(_)) => Ok(()),
        ("bool", _) => bad("true or false"),
        ("choice", Value::String(s)) => {
            let values = p.get("values").and_then(Value::as_array);
            let listed = values.is_some_and(|vs| vs.iter().any(|x| x.as_str() == Some(s)));
            let prefixed = p
                .get("or_prefixed")
                .and_then(Value::as_str)
                .is_some_and(|pre| s.starts_with(pre) && s.chars().count() <= MAX_STRING);
            if listed || prefixed {
                Ok(())
            } else {
                bad(&format!(
                    "one of {}",
                    values.map_or(String::new(), |vs| Value::Array(vs.clone()).to_string())
                ))
            }
        }
        ("choice", _) => bad("a string"),
        ("int" | "int_or_null", Value::Number(n)) => match n.as_i64() {
            Some(i) if in_range(i, p) => Ok(()),
            _ => bad(&format!(
                "an integer from {} to {}",
                p.get("min").unwrap_or(&Value::Null),
                p.get("max").unwrap_or(&Value::Null)
            )),
        },
        ("int_or_null", Value::Null) => Ok(()),
        ("int" | "int_or_null", _) => bad("an integer"),
        _ => Ok(()),
    }
}

/// A command tool's `arguments` as `command.run` params, and its window.
/// Err names the argument that is wrong: unknown (`replace` included),
/// of the wrong type, out of range or missing. A null for a param that
/// takes no null counts as absent, as VizDAW reads it.
pub fn arguments_to_params(
    spec: &Value,
    args: &Map<String, Value>,
) -> Result<(Map<String, Value>, Option<i64>), String> {
    let specs = params_of(spec);
    let mut params = Map::new();
    let mut window = None;
    for (name, v) in args {
        if name == "window" {
            window = window_arg(v)?;
            continue;
        }
        let shown = || crate::rpc::truncate(name, 64);
        let Some(p) = specs
            .iter()
            .find(|p| str_of(p, "name") == name && name != "replace")
        else {
            return Err(format!("unknown argument `{}`", shown()));
        };
        if v.is_null() && str_of(p, "type") != "int_or_null" {
            continue;
        }
        check_arg(p, name, v)?;
        params.insert(name.clone(), v.clone());
    }
    for p in specs {
        let name = str_of(p, "name");
        if p.get("required").and_then(Value::as_bool) == Some(true) && !params.contains_key(name) {
            return Err(format!("missing `{name}`"));
        }
    }
    Ok((params, window))
}

/// Ruling 8: the most items a question lists before "- and N more".
pub const MAX_LISTED: usize = 40;
/// The longest item part shown; the app already cuts names to 60 (P11).
const MAX_ITEM_PART: usize = 200;

/// A no (Ruling 8).
pub const SAID_NO: &str = "The composer said no. Nothing changed.";
/// No answer within the wait (Ruling 8, P6).
pub const NO_ANSWER: &str = "The composer did not answer. Nothing changed.";
/// An answer that is not the asked-for form.
pub const UNREADABLE: &str = "The composer's answer could not be read. Nothing changed.";
/// An unknown, used, expired or altered `requestState` (P5).
pub const EXPIRED: &str = "This question expired; call change again.";
/// A token `change` did not hand out in this conversation.
pub const NOT_OUR_TOKEN: &str = "Use the token change returned in this conversation.";

/// `change` and `apply_change`'s checked arguments.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangeArgs {
    /// The plan, as given: 1 to `MAX_PLAN` `{id, params?}`.
    pub commands: Value,
    /// The window, if named.
    pub window: Option<i64>,
    /// `apply_change`'s token.
    pub token: Option<String>,
}

/// Checks `change`'s arguments, or `apply_change`'s when `with_token`.
/// Err names what is wrong. VizDAW checks each command again.
pub fn change_args(args: &Map<String, Value>, with_token: bool) -> Result<ChangeArgs, String> {
    let shape =
        || format!("`commands` must be an array of 1 to {MAX_PLAN} {{\"id\", \"params\"}} objects");
    let mut out = ChangeArgs {
        commands: Value::Null,
        window: None,
        token: None,
    };
    for (k, v) in args {
        match k.as_str() {
            "window" => out.window = window_arg(v)?,
            "commands" => out.commands = v.clone(),
            "token" if with_token => match v {
                Value::String(t)
                    if t.len() == TOKEN_LEN
                        && t.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) =>
                {
                    out.token = Some(t.clone());
                }
                _ => {
                    return Err(format!(
                        "`token` must be the {TOKEN_LEN} lowercase hex characters change gave"
                    ));
                }
            },
            _ => {
                return Err(format!(
                    "unknown argument `{}`",
                    crate::rpc::truncate(k, 64)
                ));
            }
        }
    }
    let list = match &out.commands {
        Value::Array(a) if (1..=MAX_PLAN).contains(&a.len()) => a,
        Value::Null => return Err("missing `commands`".into()),
        _ => return Err(shape()),
    };
    for (i, c) in list.iter().enumerate() {
        let Value::Object(m) = c else {
            return Err(shape());
        };
        if let Some(k) = m.keys().find(|k| *k != "id" && *k != "params") {
            return Err(format!(
                "`commands[{i}]`: unknown key `{}` (allowed: id, params)",
                crate::rpc::truncate(k, 64)
            ));
        }
        match m.get("id") {
            Some(Value::String(s)) if !s.is_empty() && s.chars().count() <= 64 => {}
            _ => return Err(format!("`commands[{i}]`: `id` must be a command id")),
        }
        if !matches!(m.get("params"), None | Some(Value::Object(_))) {
            return Err(format!("`commands[{i}]`: `params` must be an object"));
        }
    }
    if with_token && out.token.is_none() {
        return Err("missing `token`".into());
    }
    Ok(out)
}

/// Whether `c` could disguise a line of the list: a control, a bidi
/// control or mark, or a line or paragraph separator.
fn disguises(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{2028}' | '\u{2029}'
        )
        || ('\u{202A}'..='\u{202E}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
}

fn item_part(v: Option<&Value>) -> String {
    let s = v.and_then(Value::as_str).unwrap_or("?");
    s.chars()
        .map(|c| if disguises(c) { ' ' } else { c })
        .take(MAX_ITEM_PART)
        .collect()
}

/// A preview's items as "- <thing>: <change>" lines: at most
/// [`MAX_LISTED`], then "- and N more" (Ruling 8).
pub fn item_lines(items: &[Value]) -> Vec<String> {
    let mut lines: Vec<String> = items
        .iter()
        .take(MAX_LISTED)
        .map(|i| {
            format!(
                "- {}: {}",
                item_part(i.get("thing")),
                item_part(i.get("change"))
            )
        })
        .collect();
    if items.len() > MAX_LISTED {
        lines.push(format!("- and {} more", items.len() - MAX_LISTED));
    }
    lines
}

/// The `elicitation/create` params that ask the composer (Ruling 8).
pub fn question(items: &[Value]) -> Value {
    let mut message = String::from("Co-Producer wants to change your work:\n");
    for l in item_lines(items) {
        message.push_str(&l);
        message.push('\n');
    }
    message.push_str("\nAllow these changes?");
    json!({
        "message": message,
        "requestedSchema": {
            "type": "object",
            "properties": {"allow": {"type": "boolean", "title": "Allow these changes", "default": false}},
            "required": ["allow"],
        },
    })
}

/// What the model gets without elicitation: the list, then how to ask.
pub fn list_for_the_model(items: &[Value], token: &str) -> String {
    let mut text = item_lines(items).join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(&format!(
        "Show this list to the composer and ask for a yes. Then call apply_change with token {token} and the same commands."
    ));
    text
}

/// The resources: a state resource per target, and the command list.
pub fn resources_json() -> Vec<Value> {
    let mut out: Vec<Value> = STATE_TARGETS
        .iter()
        .map(|t| {
            json!({
                "uri": format!("{STATE_URI}{t}"),
                "name": format!("state/{t}"),
                "description": format!("VizDAW's {} state, as JSON.", t.replace('_', " ")),
                "mimeType": "application/json",
            })
        })
        .collect();
    out.push(json!({
        "uri": COMMANDS_URI,
        "name": "commands",
        "description": "The commands Co-Producer may use, each with its class and whether it can be previewed.",
        "mimeType": "application/json",
    }));
    out
}

/// The `state.get` target a resource URI names, if it is one offered.
pub fn state_target(uri: &str) -> Option<&'static str> {
    let t = uri.strip_prefix(STATE_URI)?;
    STATE_TARGETS.iter().find(|x| **x == t).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_control::co_producer::list_json;

    #[test]
    fn every_param_kind_has_a_schema() {
        for (_, c) in command_tools(&list_json()) {
            for p in params_of(&c) {
                let known = ["string", "path", "bool", "choice", "int", "int_or_null"];
                assert!(known.contains(&str_of(p, "type")), "{p}");
            }
        }
    }

    #[test]
    fn arguments_are_checked() {
        let list = list_json();
        let tools = command_tools(&list);
        let spec = &tools
            .iter()
            .find(|(n, _)| n == "file_export_midi")
            .unwrap()
            .1;
        let args = |v: Value| v.as_object().unwrap().clone();
        let (p, w) =
            arguments_to_params(spec, &args(json!({"path": "/tmp/a.mid", "window": 3}))).unwrap();
        assert_eq!(Value::Object(p), json!({"path": "/tmp/a.mid"}));
        assert_eq!(w, Some(3));
        for (bad, word) in [
            (json!({"replace": true}), "replace"),
            (json!({"path": 1}), "path"),
            (json!({"path": "x".repeat(MAX_PATH + 1)}), "path"),
            (json!({"seq": -1}), "seq"),
            (json!({"window": 0}), "window"),
            (json!({"window": "key"}), "window"),
            (json!({"nope": 1}), "nope"),
        ] {
            let e = arguments_to_params(spec, &args(bad.clone())).unwrap_err();
            assert!(e.contains(word), "{bad}: {e}");
        }
    }

    #[test]
    fn items_cannot_forge_lines() {
        let items = vec![
            json!({"thing": "A\n- B: kept", "change": "x\u{2028}y\u{202E}"}),
            json!({}),
        ];
        assert_eq!(item_lines(&items), vec!["- A - B: kept: x y ", "- ?: ?"]);
        let long = vec![json!({"thing": "t".repeat(70_000), "change": "c"})];
        assert_eq!(item_lines(&long)[0].chars().count(), 2 + MAX_ITEM_PART + 3);
    }

    #[test]
    fn state_uris() {
        assert_eq!(state_target("vizdaw://state/mixer"), Some("mixer"));
        assert_eq!(state_target("vizdaw://state/input"), None);
        assert_eq!(state_target("vizdaw://state/"), None);
    }
}
