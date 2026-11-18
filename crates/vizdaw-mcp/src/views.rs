//! MCP Apps views (plan 8c Ruling 7): the change preview, the track list,
//! the export result and the mixer. Each page is static HTML built into the binary;
//! it holds no song data and loads nothing. Data reaches a view only as
//! the `structuredContent` of a tool result, which the page draws with
//! `textContent`. Views are offered only to a client that asks for them.

use serde_json::{Value, json};

/// The MCP Apps extension's capability key.
pub const EXTENSION: &str = "io.modelcontextprotocol/ui";
/// The MIME type of a view.
pub const MIME: &str = "text/html;profile=mcp-app";
/// Each view's resource URI and the tool whose results it draws.
pub const VIEWS: &[(&str, &str)] = &[
    ("ui://vizdaw/change", "change"),
    ("ui://vizdaw/tracks", "show_tracks"),
    ("ui://vizdaw/export", "export"),
    ("ui://vizdaw/mixer", "show_mixer"),
];

/// Where each page takes the bridge.
const MARK: &str = "<!--BRIDGE-->";
const BRIDGE: &str = include_str!("../views/bridge.js");
const CHANGE: &str = include_str!("../views/change.html");
const TRACKS: &str = include_str!("../views/tracks.html");
const EXPORT: &str = include_str!("../views/export.html");
const MIXER: &str = include_str!("../views/mixer.html");

/// True when `caps` (a client's capabilities) lists MIME under EXTENSION's
/// mimeTypes.
pub fn wants_views(caps: &Value) -> bool {
    caps.get("extensions")
        .and_then(|e| e.get(EXTENSION))
        .and_then(|u| u.get("mimeTypes"))
        .and_then(Value::as_array)
        .is_some_and(|m| m.iter().any(|t| t.as_str() == Some(MIME)))
}

/// The HTML for `uri`, with bridge.js put in place of `<!--BRIDGE-->`;
/// None for another uri.
pub fn html(uri: &str) -> Option<String> {
    let page = match uri {
        "ui://vizdaw/change" => CHANGE,
        "ui://vizdaw/tracks" => TRACKS,
        "ui://vizdaw/export" => EXPORT,
        "ui://vizdaw/mixer" => MIXER,
        _ => return None,
    };
    Some(page.replacen(MARK, BRIDGE, 1))
}

/// `resources/read`'s one content item for `uri`; None for another uri.
/// No `csp`, so the host applies its restrictive default.
pub fn read(uri: &str) -> Option<Value> {
    let text = html(uri)?;
    Some(json!({
        "uri": uri, "mimeType": MIME, "text": text,
        "_meta": { "ui": { "prefersBorder": true } },
    }))
}

/// The view resources, for `resources/list` when views are on.
pub fn resources_json() -> Vec<Value> {
    let about = |tool: &str| match tool {
        "change" => "The list of changes Co-Producer wants to make, and the composer's answer.",
        "show_tracks" => "The song's tracks and mixer strips.",
        "show_mixer" => {
            "The mixer: each strip with its volume, pan, output, EQ, inserts and sends."
        }
        _ => "What an export wrote, and where.",
    };
    VIEWS
        .iter()
        .map(|(uri, tool)| {
            json!({
                "uri": uri,
                "name": format!("view/{tool}"),
                "description": about(tool),
                "mimeType": MIME,
            })
        })
        .collect()
}

/// `tools` with `_meta.ui.resourceUri` on each tool a view draws.
pub fn with_tool_meta(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            let name = t.get("name").and_then(Value::as_str);
            let mut t = t.clone();
            if let (Some((uri, _)), Some(o)) = (
                VIEWS.iter().find(|(_, tool)| Some(*tool) == name),
                t.as_object_mut(),
            ) {
                o.insert("_meta".into(), json!({ "ui": { "resourceUri": uri } }));
            }
            t
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiation() {
        let caps = |m: Value| json!({"extensions": {EXTENSION: {"mimeTypes": m}}});
        assert!(wants_views(&caps(json!(["text/html", MIME]))));
        assert!(!wants_views(&caps(json!(["text/html"]))));
        assert!(!wants_views(&caps(json!(MIME))));
        assert!(!wants_views(&caps(json!([]))));
        assert!(!wants_views(&json!({"extensions": {EXTENSION: {}}})));
        assert!(!wants_views(&json!({"extensions": [EXTENSION]})));
        assert!(!wants_views(&json!(null)));
    }

    #[test]
    fn pages_hold_the_bridge_and_load_nothing() {
        for s in [BRIDGE, CHANGE, TRACKS, EXPORT, MIXER] {
            for bad in [
                "fetch(",
                "XMLHttpRequest",
                "WebSocket",
                "EventSource",
                "import(",
                "url(",
                "@import",
                "sendBeacon",
                "localStorage",
                "srcdoc",
                "<iframe",
                "<img",
                "<object",
                "<embed",
            ] {
                assert!(!s.contains(bad), "{bad}");
            }
        }
        for bad in ["<!--", "<script", "</script", "-->"] {
            assert!(!BRIDGE.contains(bad), "bridge.js holds {bad}");
        }
        for page in [CHANGE, TRACKS, EXPORT, MIXER] {
            assert_eq!(page.matches(MARK).count(), 1);
        }
        for (uri, _) in VIEWS {
            let h = html(uri).unwrap();
            assert!(h.contains("\"ui/initialize\""), "{uri}");
            assert!(!h.contains("<!--"), "{uri}");
        }
    }
}
