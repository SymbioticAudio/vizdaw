//! `wait` conditions (spec §5.5) and the facts the app reports for them.
//! The app evaluates [`met`] on every pump tick until it holds or the
//! wait's timeout passes; a timeout reports the last facts seen.

use serde_json::{Value, json};

/// The longest wait.
pub const MAX_TIMEOUT_MS: u64 = 600_000;
/// The wait when `timeout_ms` is absent.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// A detail editor kind as the protocol names it.
pub const DETAIL_KINDS: [&str; 5] = ["none", "piano_roll", "list", "strip_chart", "drum_grid"];

/// What a `wait` waits for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum WaitCondition {
    /// The target document's transport plays.
    Playing,
    /// It is stopped.
    Stopped,
    /// It is at or past `tick` (playing or not).
    TickAtLeast(i64),
    /// At least this many documents are open.
    DocumentsAtLeast(u32),
    /// A visible VizDAW window's title contains this text.
    WindowTitled(String),
    /// No visible window's title contains this text.
    WindowGone(String),
    /// The target document's detail pane shows this kind (`none`: empty).
    DetailKind(String),
}

impl WaitCondition {
    /// The condition's wire name.
    pub fn name(&self) -> &'static str {
        match self {
            WaitCondition::Playing => "playing",
            WaitCondition::Stopped => "stopped",
            WaitCondition::TickAtLeast(_) => "tick_at_least",
            WaitCondition::DocumentsAtLeast(_) => "documents_at_least",
            WaitCondition::WindowTitled(_) => "window_titled",
            WaitCondition::WindowGone(_) => "window_gone",
            WaitCondition::DetailKind(_) => "detail_kind",
        }
    }

    /// Whether it needs a target document.
    pub fn needs_document(&self) -> bool {
        matches!(
            self,
            WaitCondition::Playing
                | WaitCondition::Stopped
                | WaitCondition::TickAtLeast(_)
                | WaitCondition::DetailKind(_)
        )
    }

    /// One phrase for messages: `playing`, `tick ≥ 15360`, ...
    pub fn describe(&self) -> String {
        match self {
            WaitCondition::Playing => "playing".into(),
            WaitCondition::Stopped => "stopped".into(),
            WaitCondition::TickAtLeast(t) => format!("tick >= {t}"),
            WaitCondition::DocumentsAtLeast(n) => format!("at least {n} open documents"),
            WaitCondition::WindowTitled(t) => format!("a window titled {t:?}"),
            WaitCondition::WindowGone(t) => format!("no window titled {t:?}"),
            WaitCondition::DetailKind(k) => format!("detail pane {k}"),
        }
    }
}

/// The target document's state, as one pump tick saw it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocFacts {
    /// Its transport plays.
    pub playing: bool,
    /// Its transport's tick.
    pub tick: i64,
    /// Its detail pane's kind (`none` when empty), as in [`DETAIL_KINDS`].
    pub detail_kind: String,
}

/// What a pump tick saw.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// The target document; None when there is none.
    pub document: Option<DocFacts>,
    /// Open documents.
    pub documents: u32,
    /// Titles of visible VizDAW windows.
    pub window_titles: Vec<String>,
}

impl Facts {
    /// The facts as JSON (a timeout's `details.facts`).
    pub fn to_json(&self) -> Value {
        json!({
            "document": self.document.as_ref().map(|d| json!({
                "playing": d.playing, "tick": d.tick, "detail_kind": d.detail_kind,
            })),
            "documents": self.documents,
            "window_titles": self.window_titles,
        })
    }
}

/// Whether `c` holds for `f`.
pub fn met(c: &WaitCondition, f: &Facts) -> bool {
    let titled = |t: &str| f.window_titles.iter().any(|w| w.contains(t));
    match c {
        WaitCondition::Playing => f.document.as_ref().is_some_and(|d| d.playing),
        WaitCondition::Stopped => f.document.as_ref().is_some_and(|d| !d.playing),
        WaitCondition::TickAtLeast(t) => f.document.as_ref().is_some_and(|d| d.tick >= *t),
        WaitCondition::DocumentsAtLeast(n) => f.documents >= *n,
        WaitCondition::WindowTitled(t) => titled(t),
        WaitCondition::WindowGone(t) => !titled(t),
        WaitCondition::DetailKind(k) => f.document.as_ref().is_some_and(|d| d.detail_kind == *k),
    }
}

/// The `timeout` error's details for a wait that gave up.
pub fn timeout_details(c: &WaitCondition, timeout_ms: u64, last: &Facts) -> Value {
    json!({ "condition": c.name(), "timeout_ms": timeout_ms, "facts": last.to_json() })
}
