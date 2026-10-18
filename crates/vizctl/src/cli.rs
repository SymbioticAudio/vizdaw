//! Arguments to requests, and the exit codes: 0 done, 1 VizDAW answered
//! with an error, 2 bad usage, 3 no connection (control spec §10).

use crate::launch::{LaunchOptions, launch};
use core_control::client::{CallError, Client};
use serde_json::{Map, Value, json};
use std::path::PathBuf;
use std::time::Duration;

/// `vizctl help`.
pub const USAGE: &str = "\
usage: vizctl [--socket PATH] [--timeout SECONDS] COMMAND [ARGS]

The socket defaults to $VIZDAW_CONTROL. Results print as JSON on stdout.

  launch [--app PATH] [--home DIR] [--demo] [--env K=V]...
                               start VizDAW in a throwaway home; prints pid, socket, home, log
  ping                         check the channel
  commands                     the menu-command registry
  run ID [PARAMS-JSON]         run a menu command (e.g. run transport.play_stop)
  state TARGET                 transport | document | editor | selection | windows
  tree [--depth N]             the window's accessibility tree with element IDs
  find TEXT                    tree elements whose label, title or value contains TEXT
  click ID | click X Y         click an element or a point (window points, top-left origin)
        [--right] [--count N] [--mod shift,control,option,command]
  drag FROM TO [--steps N] [--mod ...]   FROM and TO: an element ID or X,Y
  key KEY [--mod ...]          one key: space, return, escape, left, a, 1, ...
  type TEXT                    type characters
  screenshot [--out PATH] [--scale S]
  wait CONDITION [--tick N | --count N | --title T | --kind K] [--timeout-ms N]
                               playing | stopped | tick_at_least | documents_at_least |
                               window_titled | window_gone | detail_kind
  new-demo [--audio]           the demo project in a new window
  open PATH [--plugins normal|off|disable] [--restore-autosave true|false]
  quit                         quit VizDAW
  call CMD [PARAMS-JSON]       send any request

Every command except launch takes --window N (a number from `state windows`).
Exit codes: 0 done, 1 VizDAW answered with an error, 2 bad usage, 3 no connection.";

/// A request to send.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// Command name.
    pub cmd: String,
    /// Its params.
    pub params: Value,
    /// Client-side reply timeout.
    pub timeout: Duration,
    /// `find`: filter the tree for this text.
    pub find: Option<String>,
}

/// What the arguments ask for.
#[derive(Debug, Clone)]
pub enum Plan {
    /// Print usage.
    Help,
    /// Launch VizDAW.
    Launch(LaunchOptions),
    /// Send a request to `socket`.
    Send {
        /// The socket.
        socket: PathBuf,
        /// The request.
        request: Request,
    },
}

/// The default client timeout: longer than the server's 60 s app timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);

struct Args {
    items: Vec<String>,
}

impl Args {
    /// Removes `--name VALUE`; Err when the value is missing.
    fn value(&mut self, name: &str) -> Result<Option<String>, String> {
        let Some(i) = self.items.iter().position(|a| a == name) else {
            return Ok(None);
        };
        if i + 1 >= self.items.len() {
            return Err(format!("{name} needs a value"));
        }
        let v = self.items.remove(i + 1);
        self.items.remove(i);
        Ok(Some(v))
    }

    /// Removes every `--name VALUE`.
    fn values(&mut self, name: &str) -> Result<Vec<String>, String> {
        let mut out = Vec::new();
        while let Some(v) = self.value(name)? {
            out.push(v);
        }
        Ok(out)
    }

    /// Removes `--name`; whether it was there.
    fn flag(&mut self, name: &str) -> bool {
        match self.items.iter().position(|a| a == name) {
            Some(i) => {
                self.items.remove(i);
                true
            }
            None => false,
        }
    }

    fn positional(&self) -> &[String] {
        &self.items
    }
}

fn number(s: &str, what: &str) -> Result<i64, String> {
    s.parse::<i64>()
        .map_err(|_| format!("{what} must be an integer, not {s:?}"))
}

fn float(s: &str, what: &str) -> Result<f64, String> {
    s.parse::<f64>()
        .map_err(|_| format!("{what} must be a number, not {s:?}"))
}

fn json_arg(s: Option<&String>) -> Result<Value, String> {
    match s {
        None => Ok(json!({})),
        Some(t) => serde_json::from_str(t).map_err(|e| format!("not JSON: {e}")),
    }
}

/// `X,Y` as a point, anything else as an element ID.
fn target(s: &str) -> Result<Value, String> {
    match s.split_once(',') {
        Some((x, y)) if !s.contains(':') => Ok(json!({"x": float(x, "x")?, "y": float(y, "y")?})),
        _ => Ok(json!(s)),
    }
}

/// Sets `key` in the object `p`.
fn set(p: &mut Value, key: &str, v: Value) {
    if let Some(o) = p.as_object_mut() {
        o.insert(key.into(), v);
    }
}

fn modifiers(a: &mut Args) -> Result<Option<Value>, String> {
    Ok(a.value("--mod")?
        .map(|m| json!(m.split(',').map(str::trim).collect::<Vec<_>>())))
}

/// Reads the arguments (without the program name).
pub fn parse(args: &[String], env_socket: Option<String>) -> Result<Plan, String> {
    let mut a = Args {
        items: args.to_vec(),
    };
    let socket = a.value("--socket")?.or(env_socket);
    let timeout = match a.value("--timeout")? {
        Some(t) => Duration::from_secs_f64(float(&t, "--timeout")?.clamp(1.0, 3600.0)),
        None => DEFAULT_TIMEOUT,
    };
    let window = match a.value("--window")? {
        Some(w) if w == "key" || w == "main" => Some(json!(w)),
        Some(w) => Some(json!(number(&w, "--window")?)),
        None => None,
    };
    let Some(cmd) = a.items.first().cloned() else {
        return Ok(Plan::Help);
    };
    a.items.remove(0);
    if cmd == "help" || cmd == "--help" || cmd == "-h" {
        return Ok(Plan::Help);
    }
    if cmd == "launch" {
        let env = a
            .values("--env")?
            .into_iter()
            .map(|kv| {
                kv.split_once('=')
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .ok_or(format!("--env takes K=V, not {kv:?}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let app = a
            .value("--app")?
            .or_else(|| std::env::var("VIZDAW_APP").ok())
            .unwrap_or_else(|| "dist/VizDAW.app".into());
        let home = a.value("--home")?.map(PathBuf::from);
        let demo = a.flag("--demo");
        if let Some(extra) = a.positional().first() {
            return Err(format!("launch: unexpected {extra:?}"));
        }
        return Ok(Plan::Launch(LaunchOptions {
            app: PathBuf::from(app),
            home,
            demo,
            timeout: timeout.max(Duration::from_secs(120)),
            env,
        }));
    }
    let mut find = None;
    let mut wait_ms = None;
    let (name, mut params): (String, Value) = match cmd.as_str() {
        "ping" => ("ping".to_string(), json!({})),
        "commands" => ("commands.list".to_string(), json!({})),
        "call" => {
            let c = a
                .positional()
                .first()
                .cloned()
                .ok_or("call: give a command")?;
            let p = json_arg(a.positional().get(1))?;
            (c, p)
        }
        "run" => {
            let id = a
                .positional()
                .first()
                .cloned()
                .ok_or("run: give a command ID")?;
            let p = json_arg(a.positional().get(1))?;
            ("command.run".to_string(), json!({"id": id, "params": p}))
        }
        "state" => {
            let t = a
                .positional()
                .first()
                .cloned()
                .ok_or("state: give a target")?;
            ("state.get".to_string(), json!({"target": t}))
        }
        "tree" | "find" => {
            let mut p = json!({});
            if let Some(d) = a.value("--depth")? {
                set(&mut p, "max_depth", json!(number(&d, "--depth")?));
            }
            if cmd == "find" {
                find = Some(
                    a.positional()
                        .first()
                        .cloned()
                        .ok_or("find: give some text")?,
                );
            }
            ("ui.tree".to_string(), p)
        }
        "click" => {
            let right = a.flag("--right");
            let count = a.value("--count")?;
            let m = modifiers(&mut a)?;
            let mut p = match a.positional() {
                [id] => json!({"element": id}),
                [x, y] => json!({"point": {"x": float(x, "x")?, "y": float(y, "y")?}}),
                _ => return Err("click: give an element ID or X Y".into()),
            };
            if right {
                set(&mut p, "button", json!("right"));
            }
            if let Some(c) = count {
                set(&mut p, "count", json!(number(&c, "--count")?));
            }
            if let Some(m) = m {
                set(&mut p, "modifiers", m);
            }
            ("ui.click".to_string(), p)
        }
        "drag" => {
            let steps = a.value("--steps")?;
            let m = modifiers(&mut a)?;
            let [from, to] = a.positional() else {
                return Err("drag: give FROM and TO".into());
            };
            let mut p = json!({"from": target(from)?, "to": target(to)?});
            if let Some(s) = steps {
                set(&mut p, "steps", json!(number(&s, "--steps")?));
            }
            if let Some(m) = m {
                set(&mut p, "modifiers", m);
            }
            ("ui.drag".to_string(), p)
        }
        "key" => {
            let m = modifiers(&mut a)?;
            let k = a.positional().first().cloned().ok_or("key: give a key")?;
            let mut p = json!({"key": k});
            if let Some(m) = m {
                set(&mut p, "modifiers", m);
            }
            ("ui.key".to_string(), p)
        }
        "type" => {
            let t = a.positional().join(" ");
            if t.is_empty() {
                return Err("type: give some text".into());
            }
            ("ui.key".to_string(), json!({"text": t}))
        }
        "screenshot" => {
            let mut p = json!({});
            if let Some(o) = a.value("--out")? {
                let path = std::path::absolute(&o).map_err(|e| format!("--out: {e}"))?;
                set(&mut p, "path", json!(path));
            }
            if let Some(s) = a.value("--scale")? {
                set(&mut p, "scale", json!(float(&s, "--scale")?));
            }
            ("ui.screenshot".to_string(), p)
        }
        "wait" => {
            let c = a
                .positional()
                .first()
                .cloned()
                .ok_or("wait: give a condition")?;
            let mut p = json!({"condition": c});
            for (flag, key, num) in [
                ("--tick", "tick", true),
                ("--count", "count", true),
                ("--title", "title", false),
                ("--kind", "kind", false),
            ] {
                if let Some(v) = a.value(flag)? {
                    let v = if num {
                        json!(number(&v, flag)?)
                    } else {
                        json!(v)
                    };
                    set(&mut p, key, v);
                }
            }
            if let Some(t) = a.value("--timeout-ms")? {
                let ms = number(&t, "--timeout-ms")?;
                wait_ms = Some(ms.max(0) as u64);
                set(&mut p, "timeout_ms", json!(ms));
            }
            ("wait".to_string(), p)
        }
        "new-demo" => (
            "app.new_demo".to_string(),
            json!({"audio": a.flag("--audio")}),
        ),
        "open" => {
            let plugins = a.value("--plugins")?;
            let restore = a.value("--restore-autosave")?;
            let path = a
                .positional()
                .first()
                .cloned()
                .ok_or("open: give a .vizdaw path")?;
            let path = std::path::absolute(&path).map_err(|e| format!("open: {e}"))?;
            let mut p = json!({"path": path});
            if let Some(pl) = plugins {
                set(&mut p, "plugins", json!(pl));
            }
            if let Some(r) = restore {
                let b = match r.as_str() {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(format!(
                            "--restore-autosave takes true or false, not {other:?}"
                        ));
                    }
                };
                set(&mut p, "restore_autosave", json!(b));
            }
            ("app.open".to_string(), p)
        }
        "quit" => ("app.quit".to_string(), json!({})),
        other => return Err(format!("unknown command {other:?} (see vizctl help)")),
    };
    if let (Some(w), Some(o)) = (window, params.as_object_mut()) {
        if !matches!(
            name.as_str(),
            "commands.list" | "ping" | "app.new_demo" | "app.open" | "app.quit"
        ) {
            o.insert("window".into(), w);
        }
    }
    let socket = socket
        .map(PathBuf::from)
        .ok_or("no socket: pass --socket or set VIZDAW_CONTROL (vizctl launch prints it)")?;
    // A wait answers within its own timeout; leave room for the reply.
    let timeout = match wait_ms {
        Some(ms) => timeout.max(Duration::from_millis(ms) + Duration::from_secs(30)),
        None if name == "wait" => timeout.max(Duration::from_secs(60)),
        None => timeout,
    };
    Ok(Plan::Send {
        socket,
        request: Request {
            cmd: name,
            params,
            timeout,
            find,
        },
    })
}

/// Tree nodes whose label, title or value contains `text`, flattened.
pub fn find_in_tree(tree: &Value, text: &str) -> Value {
    fn walk(nodes: &Value, text: &str, out: &mut Vec<Value>) {
        for n in nodes.as_array().into_iter().flatten() {
            let hit = ["label", "title", "value"].iter().any(|k| {
                n.get(*k)
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.contains(text))
            });
            if hit {
                let mut m = Map::new();
                for k in ["id", "role", "label", "title", "value", "frame"] {
                    if let Some(v) = n.get(k) {
                        m.insert(k.into(), v.clone());
                    }
                }
                out.push(Value::Object(m));
            }
            if let Some(c) = n.get("children") {
                walk(c, text, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(tree.get("nodes").unwrap_or(&Value::Null), text, &mut out);
    json!({ "window": tree.get("window"), "matches": out })
}

/// Runs `vizctl` with `args`; returns the exit code.
pub fn run(args: &[String]) -> i32 {
    let plan = match parse(args, std::env::var("VIZDAW_CONTROL").ok()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("vizctl: {e}");
            return 2;
        }
    };
    match plan {
        Plan::Help => {
            println!("{USAGE}");
            0
        }
        Plan::Launch(o) => match launch(&o) {
            Ok(l) => {
                println!("{}", l.to_json());
                eprintln!("export VIZDAW_CONTROL={}", l.socket.display());
                0
            }
            Err(e) => {
                eprintln!("vizctl: {e}");
                3
            }
        },
        Plan::Send { socket, request } => {
            let mut client = match Client::connect(&socket) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("vizctl: cannot connect to {}: {e}", socket.display());
                    return 3;
                }
            };
            match client.call(&request.cmd, request.params, request.timeout) {
                Ok(v) => {
                    let v = match &request.find {
                        Some(t) => find_in_tree(&v, t),
                        None => v,
                    };
                    println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                    0
                }
                Err(CallError::Reply(e)) => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({ "error": e.to_json() }))
                            .unwrap_or_default()
                    );
                    eprintln!("vizctl: {e}");
                    1
                }
                Err(e) => {
                    eprintln!("vizctl: {e}");
                    3
                }
            }
        }
    }
}
