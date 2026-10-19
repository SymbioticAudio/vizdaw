//! JSON lines (spec §5.1–5.2): one request object per line in, one reply
//! object per line out. Reading never panics: any byte string becomes a
//! request or a [`LineError`].

use crate::error::{ControlError, ErrorCode};
use serde_json::{Map, Value, json};

/// The longest request line, newline excluded (1 MiB).
pub const MAX_LINE: usize = 1 << 20;
/// The longest string `id`.
pub const MAX_ID_LEN: usize = 128;
/// The longest command name.
pub const MAX_CMD_LEN: usize = 64;
/// The deepest nesting of arrays and objects in a request.
pub const MAX_DEPTH: usize = 32;

/// A request's `id`, echoed in its reply: a string of at most
/// [`MAX_ID_LEN`] characters or an integer.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RequestId {
    /// An integer id.
    Num(i64),
    /// A string id.
    Text(String),
}

impl RequestId {
    /// The JSON value.
    pub fn to_json(&self) -> Value {
        match self {
            RequestId::Num(n) => Value::from(*n),
            RequestId::Text(s) => Value::from(s.clone()),
        }
    }

    /// The id a JSON value holds, if it is a valid one.
    pub fn from_json(v: &Value) -> Option<RequestId> {
        match v {
            Value::Number(n) => n.as_i64().map(RequestId::Num),
            Value::String(s) if s.chars().count() <= MAX_ID_LEN => Some(RequestId::Text(s.clone())),
            _ => None,
        }
    }
}

/// A request as read: id, command name and its unvalidated parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct RawRequest {
    /// Echoed in the reply.
    pub id: RequestId,
    /// The command name (`state.get`, ...).
    pub cmd: String,
    /// The `params` object (empty when absent or null).
    pub params: Map<String, Value>,
}

/// A line that is not a request, with its id when one could be read.
#[derive(Debug, Clone, PartialEq)]
pub struct LineError {
    /// The request's id, if it had a valid one.
    pub id: Option<RequestId>,
    /// Why the line was refused.
    pub error: ControlError,
}

fn refuse(id: Option<RequestId>, code: ErrorCode, message: impl Into<String>) -> LineError {
    LineError {
        id,
        error: ControlError::new(code, message),
    }
}

/// How deeply `v` nests (a scalar is 0). Iterative: a hostile document
/// cannot overflow the stack here (serde_json itself stops at 128).
pub fn depth(v: &Value) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(v, 0usize)];
    while let Some((v, d)) = stack.pop() {
        deepest = deepest.max(d);
        match v {
            Value::Array(a) => stack.extend(a.iter().map(|x| (x, d + 1))),
            Value::Object(o) => stack.extend(o.values().map(|x| (x, d + 1))),
            _ => {}
        }
    }
    deepest
}

/// Reads one request line (without its newline).
pub fn read_request(line: &[u8]) -> Result<RawRequest, LineError> {
    if line.len() > MAX_LINE {
        return Err(refuse(
            None,
            ErrorCode::ParseError,
            format!("request line longer than {MAX_LINE} bytes"),
        ));
    }
    let Ok(text) = std::str::from_utf8(line) else {
        return Err(refuse(
            None,
            ErrorCode::ParseError,
            "request line is not UTF-8",
        ));
    };
    let text = text.trim();
    if text.is_empty() {
        return Err(refuse(
            None,
            ErrorCode::InvalidRequest,
            "empty request line",
        ));
    }
    let v: Value = serde_json::from_str(text)
        .map_err(|e| refuse(None, ErrorCode::ParseError, format!("not JSON: {e}")))?;
    let Value::Object(mut obj) = v else {
        return Err(refuse(
            None,
            ErrorCode::InvalidRequest,
            "a request is a JSON object",
        ));
    };
    let id = match obj.remove("id") {
        None => return Err(refuse(None, ErrorCode::InvalidRequest, "missing `id`")),
        Some(v) => RequestId::from_json(&v).ok_or_else(|| {
            refuse(
                None,
                ErrorCode::InvalidRequest,
                format!("`id` must be an integer or a string of at most {MAX_ID_LEN} characters"),
            )
        })?,
    };
    let fail = |code, message: String| Err(refuse(Some(id.clone()), code, message));
    if depth(&Value::Object(obj.clone())) > MAX_DEPTH {
        return fail(
            ErrorCode::InvalidRequest,
            format!("request nests deeper than {MAX_DEPTH} levels"),
        );
    }
    let cmd = match obj.remove("cmd") {
        Some(Value::String(c)) => c,
        Some(_) => return fail(ErrorCode::InvalidRequest, "`cmd` must be a string".into()),
        None => return fail(ErrorCode::InvalidRequest, "missing `cmd`".into()),
    };
    let well_formed = !cmd.is_empty()
        && cmd.len() <= MAX_CMD_LEN
        && cmd
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_');
    if !well_formed {
        return fail(
            ErrorCode::UnknownCommand,
            format!("no command {:?}", truncate(&cmd, MAX_CMD_LEN)),
        );
    }
    let params = match obj.remove("params") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(p)) => p,
        Some(_) => {
            return fail(
                ErrorCode::InvalidParams,
                format!("{cmd}: `params` must be an object"),
            );
        }
    };
    if let Some(k) = obj.keys().next() {
        return fail(
            ErrorCode::InvalidRequest,
            format!(
                "unknown field {:?} (a request has `id`, `cmd` and `params`)",
                truncate(k, 64)
            ),
        );
    }
    Ok(RawRequest { id, cmd, params })
}

/// `s` cut to at most `n` characters, for messages that quote input.
pub fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// The reply line for `id` (no trailing newline).
pub fn reply_line(id: Option<&RequestId>, outcome: &Result<Value, ControlError>) -> String {
    let id = id.map_or(Value::Null, RequestId::to_json);
    let v = match outcome {
        Ok(result) => json!({ "id": id, "ok": true, "result": result }),
        Err(e) => json!({ "id": id, "ok": false, "error": e.to_json() }),
    };
    v.to_string()
}

/// A request line (no trailing newline), for clients.
pub fn request_line(id: &RequestId, cmd: &str, params: &Value) -> String {
    json!({ "id": id.to_json(), "cmd": cmd, "params": params }).to_string()
}

/// A reply as a client reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    /// The request's id (null for a line the server could not read).
    pub id: Option<RequestId>,
    /// `result` or `error`.
    pub outcome: Result<Value, ControlError>,
}

/// Reads a reply line; Err says why it is not one.
pub fn read_reply(line: &str) -> Result<Reply, String> {
    let v: Value =
        serde_json::from_str(line.trim()).map_err(|e| format!("reply is not JSON: {e}"))?;
    let id = match v.get("id") {
        None | Some(Value::Null) => None,
        Some(i) => Some(RequestId::from_json(i).ok_or("reply has an invalid id")?),
    };
    let outcome = match v.get("ok").and_then(Value::as_bool) {
        Some(true) => Ok(v.get("result").cloned().unwrap_or(Value::Null)),
        Some(false) => Err(v
            .get("error")
            .and_then(ControlError::from_json)
            .ok_or("reply has ok:false without a valid error")?),
        None => return Err("reply has no `ok`".into()),
    };
    Ok(Reply { id, outcome })
}
