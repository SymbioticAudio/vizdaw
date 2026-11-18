//! Co-Producer's recipes (plan 8e Ruling 6): written-down steps Claude
//! follows with the Co-Producer commands, served from one table both as
//! the `recipes` tool and as MCP prompts. No recipe writes notes (owner,
//! 2026-11-16), and no recipe names a plug-in, maker or product.

use crate::rpc::{self, RpcError};
use serde_json::{Map, Value, json};

/// One recipe: its name, its title, and its steps as plain text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recipe {
    /// The name the tool and the prompt take.
    pub name: &'static str,
    /// What it is for, in the composer's words.
    pub title: &'static str,
    /// Its steps, in order.
    pub steps: &'static [&'static str],
}

const BEGIN: &str = "Start a request with begin_request.";
const END: &str =
    "End the request with end_request, and tell the composer what was added and what changed.";
const NO_FITTING: &str = "If list_plugins has no fitting effect, say so, set up the bus with multi-q only, and suggest the kind of plug-in to install.";

/// Ruling 6's six recipes, in its order.
pub const RECIPES: &[Recipe] = &[
    Recipe {
        name: "starter_project",
        title: "Set up a starter project in a style",
        steps: &[
            BEGIN,
            "Read list_plugins for instruments and for effects.",
            "Choose roles that suit the style (for 80s synthwave: drums, bass, pad, lead and arpeggio parts, a gated reverb and a delay bus).",
            "Pick installed plug-ins by name; search the web with your own tools if unsure which installed plug-ins fit.",
            "Choose a typical tempo and meter, and markers for the sections (Intro, Verse, Chorus, Bridge, Outro).",
            "Call starter_project once.",
            "Never write notes or beats.",
            "Read its lines aloud: they say which roles fell back to the Built-in Synth or Multi-Q.",
            END,
        ],
    },
    Recipe {
        name: "mix_drums",
        title: "Help me mix my drums",
        steps: &[
            BEGIN,
            "Find the drum tracks with show_tracks and show_mixer, and ask the composer which they are if unsure.",
            "Make a bus \"Drums\" with mixer_new_bus.",
            "Plan routing each drum strip to it in one change: the composer sees the list and says yes.",
            "Gain-stage from the fader levels show_mixer reports: aim the loudest drum near \u{2212}6 dB on its fader, and ask the composer to play the song and say what the meters show.",
            "Put an EQ (multi-q) and a compressor from list_plugins on the bus; on its own bus this applies without asking.",
            "Offer parallel compression next (parallel_compression).",
            NO_FITTING,
            END,
        ],
    },
    Recipe {
        name: "shared_reverb",
        title: "A shared reverb bus",
        steps: &[
            BEGIN,
            "Make a bus \"Reverb\", put a reverb from list_plugins on it, set its mix parameter fully wet with plugin_params and mixer.set_param, and plan one send from each chosen track at about \u{2212}12 dB, post-fader, in one change.",
            NO_FITTING,
            END,
        ],
    },
    Recipe {
        name: "parallel_compression",
        title: "Parallel compression",
        steps: &[
            BEGIN,
            "Make a bus \"Crush\", put a compressor on it set hard (fast attack, high ratio), and plan a send from the source track or bus at 0 dB, post-fader.",
            "Bring the bus's fader up from \u{2212}\u{221E} until it thickens the sound; start near \u{2212}10 dB.",
            NO_FITTING,
            END,
        ],
    },
    Recipe {
        name: "delay_into_reverb",
        title: "Delay into reverb",
        steps: &[
            BEGIN,
            "Make or reuse the \"Reverb\" bus, make a bus \"Delay\" with a delay plug-in fully wet, route \"Delay\" to \"Reverb\" with mixer.set_output (both are Co-Producer's, so no question), and plan sends to \"Delay\" from the chosen tracks.",
            NO_FITTING,
            END,
        ],
    },
    Recipe {
        name: "automation_moves",
        title: "Fades, rides and filter sweeps",
        steps: &[
            BEGIN,
            "Read the curve first with preview_automation, and show it.",
            "Then write it with change and automation.write.",
            "A fade is fader points from the level to \u{2212}144 dB; a ride is a few fader points across a phrase, a dB or two up or down; a sweep is points on a filter's cutoff parameter from plugin_params.",
            "Use the target's own units.",
            END,
        ],
    },
];

/// The longest prompt `style`, in characters.
pub const MAX_STYLE: usize = 200;

/// The refusal of a `style` that is not plain text of at most
/// [`MAX_STYLE`] characters.
const BAD_STYLE: &str = "style is up to 200 characters of plain text.";

/// The recipe named `name`.
fn find(name: &str) -> Option<&'static Recipe> {
    RECIPES.iter().find(|r| r.name == name)
}

/// A name from the input, as a message may quote it: cut short, with each
/// disguising character replaced.
fn quoted(name: &str) -> String {
    crate::tools::safe_text(&rpc::truncate(name, 64))
}

/// The `recipes` tool's text: the list for no name, else the recipe's
/// title and steps. Err is the tool error for an unknown name.
pub fn recipes_answer(name: Option<&str>) -> Result<String, String> {
    let Some(name) = name else {
        let mut text = String::from("Recipes:");
        for r in RECIPES {
            text.push_str(&format!("\n- {}: {}", r.name, r.title));
        }
        return Ok(text);
    };
    let Some(r) = find(name) else {
        return Err(format!(
            "No recipe {}. Call recipes with no name for the list.",
            quoted(name)
        ));
    };
    let mut text = format!("{}:", r.title);
    for s in r.steps {
        text.push_str("\n- ");
        text.push_str(s);
    }
    Ok(text)
}

/// `prompts/list`'s result: one prompt per recipe; only `starter_project`
/// takes an argument, the optional `style`.
pub fn prompts_list() -> Value {
    let prompts: Vec<Value> = RECIPES
        .iter()
        .map(|r| {
            let mut p = json!({ "name": r.name, "title": r.title, "description": r.title });
            if let (true, Some(o)) = (r.name == "starter_project", p.as_object_mut()) {
                o.insert(
                    "arguments".into(),
                    json!([{
                        "name": "style",
                        "description": "The style of the song, such as 80s synthwave.",
                        "required": false,
                    }]),
                );
            }
            p
        })
        .collect();
    json!({ "prompts": prompts })
}

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::new(rpc::INVALID_PARAMS, message)
}

/// `prompts/get`'s result: one `user` message, "<title>." and the steps,
/// and for `starter_project` with a style, "The style: <style>.". Every
/// refusal is `-32602`.
pub fn prompt_get(params: &Map<String, Value>) -> Result<Value, RpcError> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err(invalid("prompts/get needs the prompt's `name`."));
    };
    let Some(r) = find(name) else {
        return Err(invalid(format!("No prompt {}.", quoted(name))));
    };
    let empty = Map::new();
    let args = match params.get("arguments") {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(a)) => a,
        Some(_) => return Err(invalid("`arguments` must be an object.")),
    };
    let mut style = None;
    for (k, v) in args {
        match (r.name, k.as_str()) {
            ("starter_project", "style") => match v {
                Value::String(s)
                    if s.chars().count() <= MAX_STYLE
                        && !s.chars().any(crate::tools::disguises) =>
                {
                    let s = s.trim();
                    if !s.is_empty() {
                        style = Some(s.to_string());
                    }
                }
                _ => return Err(invalid(BAD_STYLE)),
            },
            _ => {
                return Err(invalid(format!(
                    "The prompt {} takes no argument {}.",
                    r.name,
                    quoted(k)
                )));
            }
        }
    }
    let mut text = format!("{}.", r.title);
    for s in r.steps {
        text.push_str("\n- ");
        text.push_str(s);
    }
    if let Some(s) = style {
        text.push_str(&format!("\nThe style: {s}."));
    }
    Ok(json!({
        "description": r.title,
        "messages": [{ "role": "user", "content": { "type": "text", "text": text } }],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_recipe_begins_and_ends_a_request() {
        assert_eq!(RECIPES.len(), 6);
        for r in RECIPES {
            assert_eq!(r.steps.first(), Some(&BEGIN), "{}", r.name);
            assert_eq!(r.steps.last(), Some(&END), "{}", r.name);
        }
    }
}
