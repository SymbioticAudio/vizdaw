//! The tools and resources (plan 8a Ruling 12): one tool per read, add or
//! outward command of the linked Co-Producer list, Co-Producer's own
//! tools, and the state resources. Arguments are checked against the
//! command's params here, and VizDAW's own gate checks them again.

use core_control::co_producer::MAX_PLAN;
use core_control::params::{MAX_PATH, MAX_STRING};
use core_control::request::{ExportWhat, MAX_ASKED, MAX_NOTE, MAX_REQUEST_NAME, TOKEN_LEN};
use serde_json::{Map, Value, json};

/// The server's instructions (Ruling 12).
pub const INSTRUCTIONS: &str = "VizDAW's Co-Producer. Adds are free; anything that changes or deletes the composer's own work goes through the change tool and their yes. Read the song's notes first (read_notes) and follow what the composer wrote there. Start every request with begin_request and finish it with end_request.";

/// Starts a request.
pub const BEGIN_REQUEST: &str = "begin_request";
/// Ends it.
pub const END_REQUEST: &str = "end_request";
/// Previews a change and asks for the composer's yes (Task 6).
pub const CHANGE: &str = "change";
/// Applies a change the composer agreed to (Task 6).
pub const APPLY_CHANGE: &str = "apply_change";
/// Reads the song's notes.
pub const READ_NOTES: &str = "read_notes";
/// Adds a note to the open request's entry.
pub const ADD_NOTE: &str = "add_note";
/// Makes a dated export folder.
pub const EXPORT_FOLDER: &str = "export_folder";
/// Exports the song in one call into a new dated folder (plan 8c Task 5).
pub const EXPORT: &str = "export";
/// Lists the song's tracks and mixer strips.
pub const SHOW_TRACKS: &str = "show_tracks";
/// Lists the installed plug-ins Co-Producer may use (plan 8d Ruling 5).
pub const LIST_PLUGINS: &str = "list_plugins";
/// Lists a loaded plug-in's parameters.
pub const PLUGIN_PARAMS: &str = "plugin_params";
/// Shows the mixer.
pub const SHOW_MIXER: &str = "show_mixer";
/// The most tracks, strips or files a chore answer lists (Ruling 8).
pub const MAX_SHOWN: usize = 200;
/// The longest `strips` list `export` takes.
const MAX_STRIPS_ARG: usize = 512;
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

/// The longest a string param may be: a plug-in reference has its own
/// limit (plan 8d Ruling 3).
fn string_limit(name: &str) -> usize {
    if name == "plugin" {
        core_control::registry::PLUGIN_REF_MAX
    } else {
        MAX_STRING
    }
}

/// A param Co-Producer's tool does not offer: `mixer.new_bus`'s `strip`
/// form changes the composer's routing, so it goes through `change`.
fn barred(spec: &Value, param: &str) -> bool {
    str_of(spec, "id") == "mixer.new_bus" && param == "strip"
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
        "string" => json!({"type": "string", "maxLength": string_limit(name), "description": help}),
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
        if barred(c, str_of(p, "name")) {
            continue;
        }
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

/// The export names a chore takes: all but `selection`.
fn export_names() -> Vec<&'static str> {
    ExportWhat::ALL
        .iter()
        .filter(|w| **w != ExportWhat::Selection)
        .map(|w| w.name())
        .collect()
}

/// `show_tracks`: it takes no argument at all, not even `window`.
fn show_tracks_tool() -> Value {
    json!({
        "name": SHOW_TRACKS,
        "description": "List the song's tracks and mixer strips, with their kinds and colours and which ones Co-Producer added. Changes nothing.",
        "inputSchema": object_schema(Map::new(), Vec::new()),
        "annotations": {"readOnlyHint": true},
    })
}

/// `show_mixer`: it takes no argument at all, not even `window`.
fn show_mixer_tool() -> Value {
    json!({
        "name": SHOW_MIXER,
        "description": "Show the mixer: every strip with its ID, volume, pan, mute and solo, where it goes, its EQ, inserts (with slot IDs) and sends, and which strips Co-Producer added. Changes nothing.",
        "inputSchema": object_schema(Map::new(), Vec::new()),
        "annotations": {"readOnlyHint": true},
    })
}

/// `list_plugins`: only `kind`; no window, the catalogue is not a song's.
fn list_plugins_tool() -> Value {
    let mut props = Map::new();
    props.insert(
        "kind".into(),
        json!({"type": "string", "enum": ["instrument", "effect"], "description": "Only instruments or only effects (default: both)."}),
    );
    json!({
        "name": LIST_PLUGINS,
        "description": "List the installed plug-ins Co-Producer may use: instruments and effects the composer has not turned off in the Plugin Manager, and VizDAW's built-ins (multi-q, the EQ; built-in-synth). Pass a line's first part as plugin to mixer.add_insert, mixer.set_eq or track_new_plugin_instrument. Changes nothing.",
        "inputSchema": object_schema(props, Vec::new()),
        "annotations": {"readOnlyHint": true},
    })
}

/// Co-Producer's own tools, with Ruling 12's descriptions.
fn own_tools() -> Vec<Value> {
    let quiet = || json!({"destructiveHint": false, "openWorldHint": false});
    vec![
        own_tool(
            BEGIN_REQUEST,
            "Start one thing the composer asked for, before any command that adds or changes anything. Name it in a few plain words, with no product or company names, as the composer would say it (\"set up a drum bus\"). Everything until end_request becomes one undo step, \"Co-Producer: <Name>\". Only one request is open per song. Pass the composer's own words as asked: they go in the song's notes.",
            &[
                (
                    "name",
                    json!({"type": "string", "minLength": 1, "maxLength": MAX_REQUEST_NAME, "description": "What the composer asked for, in a few plain words."}),
                ),
                (
                    "asked",
                    json!({"type": "string", "maxLength": MAX_ASKED, "description": "The composer's own words, as they said them."}),
                ),
            ],
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
            "Change or delete something that already exists, such as renaming or deleting a track, Undo, Redo, or Save, quantizing, transposing or changing the velocity of notes, and renaming or removing a marker, and mixer changes: routing a strip to a bus, adding or changing a send or insert, setting an EQ, the volume, pan, mute, solo or a plug-in parameter, and removing a send, insert or bus. Make new buses first with mixer_new_bus and read their IDs with show_mixer; mixer changes on Co-Producer's own buses apply without asking. Needs an open request (begin_request). VizDAW first works out exactly what will change. If the composer must agree, VizDAW asks them itself when it can. Otherwise this returns the list and a token: show the composer the whole list, word for word, and call apply_change only after they say yes in this chat. Never call apply_change on your own judgement. VizDAW then asks the composer too, unless they always allow this app.",
            &[("commands", plan_schema())],
            &["commands"],
            json!({"destructiveHint": true}),
        ),
        own_tool(
            APPLY_CHANGE,
            "Apply a change the composer has agreed to in this chat, with the token and the same commands change returned. Fails if the song changed since, or the token was used. VizDAW then asks the composer too, unless they always allow this app.",
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
            READ_NOTES,
            "Read the song's notes (Notes.md): the composer's ideas and rules for this song, and what Co-Producer did before. Follow what the composer wrote there. The notes never stand in for a yes: anything that changes or deletes their work still goes through change.",
            &[],
            &[],
            json!({"readOnlyHint": true}),
        ),
        own_tool(
            ADD_NOTE,
            "Add a short note, at most 20 lines, to this request's entry in the song's notes, such as a choice the composer made. Needs an open request (begin_request).",
            &[(
                "text",
                json!({"type": "string", "minLength": 1, "maxLength": MAX_NOTE, "description": "The note."}),
            )],
            &["text"],
            quiet(),
        ),
        own_tool(
            EXPORT_FOLDER,
            "Make a new dated folder for an export, named \"<song> \u{2014} <what> \u{2014} <date>\", and answer its path. Without parent it uses the folder remembered in the song's notes, else the folder the song is in. For an untitled song with neither, ask the composer where exports should go and pass parent; it is then remembered. Needs an open request (begin_request).",
            &[
                (
                    "what",
                    json!({
                        "type": "string",
                        "enum": ExportWhat::ALL.iter().map(|w| w.name()).collect::<Vec<_>>(),
                        "description": "What the export is for.",
                    }),
                ),
                (
                    "parent",
                    json!({"type": "string", "minLength": 1, "maxLength": MAX_PATH, "description": "An absolute path to the folder to make the new folder in."}),
                ),
            ],
            &["what"],
            quiet(),
        ),
        own_tool(
            EXPORT,
            "Export the song in one step into a new dated folder, \"<song> \u{2014} <what> \u{2014} <date>\": stems (one audio file per strip; pass strips to choose them), mix (one audio file), midi (one MIDI file), dawproject (a DAWproject file that other music apps open), aaf, or other_daw (stems plus MIDI files). Without parent it uses the folder remembered in the song's notes, else the folder the song is in; for an untitled song with neither, ask the composer where exports should go and pass parent. Never replaces a file. Needs an open request (begin_request). Answers what was written.",
            &[
                (
                    "what",
                    json!({
                        "type": "string",
                        "enum": export_names(),
                        "description": "What to export.",
                    }),
                ),
                (
                    "parent",
                    json!({"type": "string", "minLength": 1, "maxLength": MAX_PATH, "description": "An absolute path to the folder to make the new folder in."}),
                ),
                (
                    "strips",
                    json!({"type": "string", "minLength": 1, "maxLength": MAX_STRIPS_ARG, "description": "Only for stems and other_daw: the strip IDs to bounce, comma-separated, from the mixer state."}),
                ),
            ],
            &["what"],
            quiet(),
        ),
        show_tracks_tool(),
        show_mixer_tool(),
        list_plugins_tool(),
        own_tool(
            PLUGIN_PARAMS,
            "List a loaded plug-in's parameters with their current values and ranges, for mixer.set_param. Slots are in show_mixer. Changes nothing.",
            &[(
                "slot",
                json!({"type": "integer", "minimum": 1, "maximum": core_control::registry::MAX_EXACT_ID, "description": "The slot's ID, from show_mixer."}),
            )],
            &["slot"],
            json!({"readOnlyHint": true}),
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
        ("string", Value::String(s)) if s.chars().count() <= string_limit(name) => Ok(()),
        ("string", Value::String(_)) => bad(&format!("at most {} characters", string_limit(name))),
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
            .find(|p| str_of(p, "name") == name && name != "replace" && !barred(spec, name))
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

/// `begin_request`'s `asked`: a string of at most [`MAX_ASKED`] characters.
pub fn check_asked(v: &Value) -> Result<(), String> {
    match v {
        Value::String(s) if s.chars().count() <= MAX_ASKED => Ok(()),
        Value::String(_) => Err(format!("`asked` must be at most {MAX_ASKED} characters")),
        _ => Err("`asked` must be a string".into()),
    }
}

/// Checks `add_note`'s and `export_folder`'s arguments (`window` aside):
/// only `allowed` names, each of its kind, the required ones present.
/// Err names what is wrong. VizDAW checks them again.
pub fn check_notes_args(tool: &str, args: &Map<String, Value>) -> Result<(), String> {
    for (k, v) in args {
        match (tool, k.as_str()) {
            (_, "window") => {
                window_arg(v)?;
            }
            (ADD_NOTE, "text") => match v {
                Value::String(s) if (1..=MAX_NOTE).contains(&s.chars().count()) => {}
                Value::String(_) => {
                    return Err(format!("`text` must be 1 to {MAX_NOTE} characters"));
                }
                _ => return Err("`text` must be a string".into()),
            },
            (EXPORT_FOLDER, "what") => {
                if !v.as_str().is_some_and(|s| ExportWhat::parse(s).is_some()) {
                    let names: Vec<&str> = ExportWhat::ALL.iter().map(|w| w.name()).collect();
                    return Err(format!("`what` must be one of {}", names.join(", ")));
                }
            }
            (EXPORT_FOLDER, "parent") => match v {
                Value::String(s) if !s.is_empty() && s.len() <= MAX_PATH && !s.contains('\0') => {}
                _ => {
                    return Err(format!(
                        "`parent` must be a folder's path of 1 to {MAX_PATH} bytes"
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
    let needed = match tool {
        ADD_NOTE => "text",
        _ => "what",
    };
    if args.contains_key(needed) {
        Ok(())
    } else {
        Err(format!("missing `{needed}`"))
    }
}

/// Checks `export`'s arguments (`window` aside): `what` one of the six,
/// `parent` a path, `strips` a list of IDs, no other name. Err names what
/// is wrong. VizDAW checks them again.
pub fn check_export_args(args: &Map<String, Value>) -> Result<(), String> {
    for (k, v) in args {
        match k.as_str() {
            "window" => {
                window_arg(v)?;
            }
            "what" => {
                if !v.as_str().is_some_and(|s| export_names().contains(&s)) {
                    return Err(format!(
                        "`what` must be one of {}",
                        export_names().join(", ")
                    ));
                }
            }
            "parent" => match v {
                Value::String(s) if !s.is_empty() && s.len() <= MAX_PATH && !s.contains('\0') => {}
                _ => {
                    return Err(format!(
                        "`parent` must be a folder's path of 1 to {MAX_PATH} bytes"
                    ));
                }
            },
            "strips" => match v {
                Value::String(s) if s.len() <= MAX_STRIPS_ARG => {
                    core_control::co_producer::id_list(s).map_err(|m| format!("`strips`: {m}"))?;
                }
                _ => {
                    return Err(format!(
                        "`strips` must be strip IDs, comma-separated, of at most {MAX_STRIPS_ARG} characters"
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
    if args.contains_key("what") {
        Ok(())
    } else {
        Err("missing `what`".into())
    }
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

/// The longest label `co_producer.apply` takes (plan 8b Ruling 11).
pub const MAX_CLIENT_LABEL: usize = core_control::request::MAX_CLIENT_LABEL;

/// The client's `clientInfo.name` as `co_producer.apply`'s `client` label
/// (plan 8b Ruling 11): trimmed and cut to [`MAX_CLIENT_LABEL`] characters
/// at a character boundary. None for a name that is not a string, is
/// blank, or holds a control, bidi or separator character, so a hostile
/// name is never sent. VizDAW only shows it; it never proves who asks.
pub fn client_label(v: Option<&Value>) -> Option<String> {
    let name = v?.as_str()?;
    if name.chars().any(disguises) {
        return None;
    }
    let cut: String = name.trim().chars().take(MAX_CLIENT_LABEL).collect();
    let cut = cut.trim_end();
    (!cut.is_empty()).then(|| cut.to_string())
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

/// `s` as one safe line: each disguising character becomes U+FFFD (and,
/// when `flat`, a newline or tab a space), cut to `max` characters.
fn safe(s: &str, max: usize, flat: bool) -> String {
    s.chars()
        .map(|c| match c {
            '\n' | '\t' if flat => ' ',
            c if disguises(c) => '\u{FFFD}',
            c => c,
        })
        .take(max)
        .collect()
}

fn shown(v: Option<&Value>, fallback: &str) -> String {
    safe(
        v.and_then(Value::as_str).unwrap_or(fallback),
        MAX_ITEM_PART,
        false,
    )
}

fn path_of(v: &Value) -> String {
    safe(
        v.as_str().unwrap_or("?"),
        core_control::params::MAX_PATH,
        false,
    )
}

/// Ruling 8: `export`'s text, and its `structuredContent` minus `what`
/// (the caller adds the name it sent). A result with no folder is Err.
pub fn export_answer(what: &str, result: &Value) -> Result<(String, Value), String> {
    let folder = match result.get("folder").and_then(Value::as_str) {
        Some(f) => safe(f, core_control::params::MAX_PATH, false),
        None => return Err("VizDAW's export answer named no folder.".into()),
    };
    let files: &[Value] = result
        .get("written")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let summary = result
        .get("summary")
        .and_then(Value::as_str)
        .map(|s| safe(s, MAX_SUMMARY, true));
    let noun = if files.len() == 1 { "file" } else { "files" };
    let mut text = format!("Wrote {} {noun} in {folder}:", files.len());
    for f in files.iter().take(MAX_LISTED) {
        text.push_str("\n- ");
        text.push_str(&path_of(f));
    }
    if files.len() > MAX_LISTED {
        text.push_str(&format!("\n- and {} more", files.len() - MAX_LISTED));
    }
    if let Some(s) = &summary {
        text.push_str("\nReport: ");
        text.push_str(s);
    }
    let written: Vec<String> = files.iter().take(MAX_SHOWN).map(path_of).collect();
    let structured = json!({
        "view": "export", "what": what, "folder": folder, "written": written, "summary": summary,
    });
    Ok((text, structured))
}

/// The longest report summary shown.
const MAX_SUMMARY: usize = 2000;

fn flag(v: &Value) -> bool {
    v.get("co_producer").and_then(Value::as_bool) == Some(true)
}

const ADDED: &str = " \u{2014} added by Co-Producer";

/// Ruling 8: `show_tracks`' text and `structuredContent`, from
/// `state.get document` and `state.get mixer`. Each list stops at
/// [`MAX_SHOWN`] lines, then "- and N more".
pub fn tracks_answer(document: &Value, mixer: &Value) -> (String, Value) {
    let title = shown(document.get("title"), "Untitled");
    let seq = document.get("selected_seq").cloned().unwrap_or(Value::Null);
    let lanes: &[Value] = document
        .get("lanes")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let strips: &[Value] = mixer
        .get("strips")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let mut text = format!("Song: {title}\nTracks:");
    let mut tracks = Vec::new();
    for (i, l) in lanes.iter().enumerate().take(MAX_SHOWN) {
        let (name, kind) = (shown(l.get("name"), "?"), shown(l.get("kind"), "?"));
        let mark = if flag(l) { ADDED } else { "" };
        text.push_str(&format!("\n- Track {}, {name}: {kind}{mark}", i + 1));
        tracks.push(json!({
            "seq": seq, "lane": l.get("id").cloned().unwrap_or(Value::Null), "n": i + 1,
            "name": name, "kind": kind, "co_producer": flag(l),
        }));
    }
    if lanes.len() > MAX_SHOWN {
        text.push_str(&format!("\n- and {} more", lanes.len() - MAX_SHOWN));
    }
    text.push_str("\nStrips:");
    let mut rows = Vec::new();
    for s in strips.iter().take(MAX_SHOWN) {
        let (name, kind) = (shown(s.get("name"), "?"), shown(s.get("kind_label"), "?"));
        let colour = s
            .get("colour")
            .and_then(Value::as_str)
            .map(|c| safe(c, 32, false));
        let mark = if flag(s) { ADDED } else { "" };
        text.push_str(&format!(
            "\n- {name}: {kind}, colour {}{mark}",
            colour.as_deref().unwrap_or("none")
        ));
        rows.push(json!({
            "id": s.get("id").cloned().unwrap_or(Value::Null),
            "name": name, "kind": kind, "colour": colour, "co_producer": flag(s),
        }));
    }
    if strips.len() > MAX_SHOWN {
        text.push_str(&format!("\n- and {} more", strips.len() - MAX_SHOWN));
    }
    let structured = json!({"view": "tracks", "title": title, "tracks": tracks, "strips": rows});
    (text, structured)
}

/// Checks `list_plugins`' or `plugin_params`' arguments: only the names
/// the tool has, each of its kind, `slot` present. Err names what is wrong.
pub fn check_plugin_args(tool: &str, args: &Map<String, Value>) -> Result<(), String> {
    for (k, v) in args {
        match (tool, k.as_str()) {
            (LIST_PLUGINS, "kind") if matches!(v.as_str(), Some("instrument" | "effect")) => {}
            (LIST_PLUGINS, "kind") => return Err("`kind` must be instrument or effect".into()),
            (PLUGIN_PARAMS, "window") => {
                window_arg(v)?;
            }
            (PLUGIN_PARAMS, "slot")
                if v.as_i64()
                    .is_some_and(|n| (1..=core_control::registry::MAX_EXACT_ID).contains(&n)) => {}
            (PLUGIN_PARAMS, "slot") => {
                return Err("`slot` must be a slot ID, an integer from 1".into());
            }
            _ => {
                return Err(format!(
                    "unknown argument `{}`",
                    crate::rpc::truncate(k, 64)
                ));
            }
        }
    }
    if tool == PLUGIN_PARAMS && !args.contains_key("slot") {
        return Err("missing `slot`".into());
    }
    Ok(())
}

/// The head of `list_plugins`' text.
const PLUGINS_HEAD: &str = "Plug-ins you may use (pass the first part as plugin):";

/// Ruling 5: `list_plugins`' text, from `co_producer.plugins`' result.
pub fn plugins_answer(result: &Value) -> String {
    let rows: &[Value] = result
        .get("plugins")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let mut text = String::from(PLUGINS_HEAD);
    for r in rows.iter().take(MAX_SHOWN) {
        let kind = shown(r.get("kind"), "?");
        let name = shown(r.get("name"), "?");
        let maker = shown(r.get("maker"), "");
        let who = if str_of(r, "format") == "built_in" {
            format!("{name}, built in")
        } else if maker.is_empty() {
            name
        } else {
            format!("{name} by {maker}")
        };
        text.push_str(&format!(
            "\n- {}: {who}, {kind}",
            shown(r.get("plugin"), "?")
        ));
    }
    let more = rows.len().saturating_sub(MAX_SHOWN);
    if more > 0 {
        text.push_str(&format!(
            "\n- and {more} more; pass kind to narrow the list"
        ));
    } else if result.get("truncated").and_then(Value::as_bool) == Some(true) {
        text.push_str("\n- and more; pass kind to narrow the list");
    } else if rows.is_empty() {
        text.push_str("\n- none");
    }
    text
}

/// A number as the views and the text show it: at most 3 decimals,
/// trailing zeros dropped.
fn number(v: f64) -> String {
    if !v.is_finite() || v.abs() >= 1e12 {
        return format!("{v:.3e}");
    }
    let s = format!("{:.3}", v.abs());
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if v < 0.0 && s != "0" {
        format!("\u{2212}{s}")
    } else {
        s.to_string()
    }
}

/// Ruling 5: `plugin_params`' text, from `co_producer.params`' result.
pub fn params_answer(result: &Value) -> String {
    let slot = result.get("slot").and_then(Value::as_u64).unwrap_or(0);
    if result.get("running").and_then(Value::as_bool) != Some(true) {
        return format!(
            "The plug-in in slot {slot} is not running yet, so its parameters are not known. Try again in a moment."
        );
    }
    let rows: &[Value] = result
        .get("params")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let mut text = format!("Slot {slot}, {}:", shown(result.get("plugin"), "?"));
    let n = |p: &Value, k: &str| p.get(k).and_then(Value::as_f64).map_or("?".into(), number);
    for p in rows.iter().take(MAX_SHOWN) {
        let steps = if p.get("stepped").and_then(Value::as_bool) == Some(true) {
            ", whole values"
        } else {
            ""
        };
        text.push_str(&format!(
            "\n- {}: {} = {} ({} to {}{steps})",
            p.get("id").and_then(Value::as_u64).unwrap_or(0),
            shown(p.get("name"), "?"),
            n(p, "value"),
            n(p, "min"),
            n(p, "max"),
        ));
    }
    if rows.len() > MAX_SHOWN {
        text.push_str(&format!("\n- and {} more", rows.len() - MAX_SHOWN));
    }
    text
}

/// A level: one decimal, "\u{2212}\u{221E} dB" at the floor.
fn level(v: Option<f64>) -> String {
    match v {
        Some(v) if v > -144.0 => format!(
            "{}{:.1} dB",
            if v < -0.05 { "\u{2212}" } else { "" },
            v.abs()
        ),
        Some(_) => "\u{2212}\u{221E} dB".into(),
        None => "? dB".into(),
    }
}

/// A pan: "centre", "30% left" or "30% right".
fn pan_text(p: Option<f64>) -> String {
    let n = (p.unwrap_or(0.0) * 100.0).round() as i64;
    match n {
        0 => "centre".into(),
        n if n < 0 => format!("{}% left", n.unsigned_abs()),
        n => format!("{n}% right"),
    }
}

fn finite(v: Option<&Value>) -> Value {
    v.and_then(Value::as_f64)
        .filter(|f| f.is_finite())
        .map_or(Value::Null, |f| json!(f))
}

fn flag_of(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool) == Some(true)
}

/// A strip's slot (EQ or insert) as the view has it.
fn slot_view(s: &Value) -> Value {
    json!({
        "slot": s.get("slot").cloned().filter(Value::is_u64).unwrap_or(Value::Null),
        "name": shown(s.get("name"), "?"),
        "bypass": flag_of(s, "bypass"),
    })
}

fn slot_text(s: &Value) -> String {
    let bypassed = if s["bypass"] == json!(true) {
        " (bypassed)"
    } else {
        ""
    };
    format!("{}{bypassed}", s["name"].as_str().unwrap_or("?"))
}

/// `items` joined by ", ", or "none"; at most [`MAX_SHOWN`], then "and N more".
fn joined(items: &[String], total: usize) -> String {
    if items.is_empty() {
        return "none".into();
    }
    let mut out = items.join(", ");
    if total > items.len() {
        out.push_str(&format!(", and {} more", total - items.len()));
    }
    out
}

/// Ruling 5: `show_mixer`'s text and `structuredContent`, from
/// `state.get mixer`. Both stop at [`MAX_SHOWN`] strips, then "- and N
/// more". Master's `output_name` is null and is shown as the main output.
pub fn mixer_answer(mixer: &Value) -> (String, Value) {
    let strips: &[Value] = mixer
        .get("strips")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice);
    let name_of = |id: Option<&Value>| -> String {
        let found = strips.iter().find(|s| s.get("id") == id);
        match (found, id.and_then(Value::as_u64)) {
            (Some(s), _) => shown(s.get("name"), "?"),
            (None, Some(n)) => format!("strip {n}"),
            (None, None) => "?".into(),
        }
    };
    let mut text = String::from("Mixer:");
    let mut rows = Vec::new();
    for s in strips.iter().take(MAX_SHOWN) {
        let name = shown(s.get("name"), "?");
        let kind = shown(s.get("kind_label"), "?");
        let id = s
            .get("id")
            .cloned()
            .filter(Value::is_u64)
            .unwrap_or(Value::Null);
        let output_name = s
            .get("output_name")
            .and_then(Value::as_str)
            .map(|o| safe(o, MAX_ITEM_PART, false));
        let eq = s.get("eq").filter(|e| e.is_object()).map(slot_view);
        let list = |key: &str| -> &[Value] {
            s.get(key)
                .and_then(Value::as_array)
                .map_or(&[], Vec::as_slice)
        };
        let inserts: Vec<Value> = list("inserts")
            .iter()
            .take(MAX_SHOWN)
            .map(slot_view)
            .collect();
        let sends: Vec<Value> = list("sends")
            .iter()
            .take(MAX_SHOWN)
            .map(|x| {
                json!({
                    "target": x.get("target").cloned().filter(Value::is_u64).unwrap_or(Value::Null),
                    "target_name": name_of(x.get("target")),
                    "level_db": finite(x.get("level_db")),
                    "pre_fader": flag_of(x, "pre_fader"),
                    "enabled": flag_of(x, "enabled"),
                })
            })
            .collect();
        let colour = s
            .get("colour")
            .and_then(Value::as_str)
            .map(|c| safe(c, 32, false));
        let (gain, pan) = (finite(s.get("gain_db")), finite(s.get("pan")));
        let (muted, soloed) = (flag_of(s, "mute"), flag_of(s, "solo"));
        let mut line = format!(
            "\n- [{}] {name} ({kind}): volume {}, pan {}",
            s.get("id").and_then(Value::as_u64).unwrap_or(0),
            level(gain.as_f64()),
            pan_text(pan.as_f64()),
        );
        if muted {
            line.push_str(", muted");
        }
        if soloed {
            line.push_str(", soloed");
        }
        line.push_str(&format!(
            ", to {}",
            output_name.as_deref().unwrap_or("the main output")
        ));
        let ins: Vec<String> = inserts
            .iter()
            .map(|i| format!("slot {} {}", i["slot"], slot_text(i)))
            .collect();
        let snd: Vec<String> = sends
            .iter()
            .map(|x| {
                format!(
                    "{} {}{}{}",
                    x["target_name"].as_str().unwrap_or("?"),
                    level(x["level_db"].as_f64()),
                    if x["pre_fader"] == json!(true) {
                        " pre"
                    } else {
                        ""
                    },
                    if x["enabled"] == json!(true) {
                        ""
                    } else {
                        " off"
                    },
                )
            })
            .collect();
        line.push_str(&format!(
            "; EQ {}; inserts {}; sends {}",
            eq.as_ref().map_or("none".into(), slot_text),
            joined(&ins, list("inserts").len()),
            joined(&snd, list("sends").len()),
        ));
        if flag(s) {
            line.push_str(ADDED);
        }
        text.push_str(&line);
        rows.push(json!({
            "id": id, "name": name, "kind": kind, "colour": colour,
            "gain_db": gain, "pan": pan, "mute": muted, "solo": soloed,
            "output": s.get("output").cloned().filter(Value::is_u64).unwrap_or(Value::Null),
            "output_name": output_name, "eq": eq, "inserts": inserts, "sends": sends,
            "co_producer": flag(s),
        }));
    }
    if strips.len() > MAX_SHOWN {
        text.push_str(&format!("\n- and {} more", strips.len() - MAX_SHOWN));
    }
    (text, json!({"view": "mixer", "strips": rows}))
}

/// Ruling 8: `change`'s `structuredContent`. `state` is `ask_in_chat`,
/// `applied`, `said_no` or `applied_without_asking`. Items are cut to the
/// lines the question lists, with the same safe text. The token is never
/// in it.
pub fn change_view(state: &str, items: &[Value], undo_label: Option<&str>) -> Value {
    let items: Vec<Value> = items
        .iter()
        .take(MAX_LISTED)
        .map(|i| {
            json!({
                "thing": item_part(i.get("thing")),
                "change": item_part(i.get("change")),
                "composers_work": i.get("composers_work").and_then(Value::as_bool) == Some(true),
            })
        })
        .collect();
    json!({
        "view": "change", "state": state, "items": items,
        "undo_label": undo_label.map(|l| safe(l, MAX_ITEM_PART, false)),
    })
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
