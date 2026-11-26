//! The piano roll's saved view settings (Swift `PianoRollSettings`, the
//! `ui.json` key `pianoRoll`), shared by every roll of the document.

use std::ops::RangeInclusive;

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::editor::EditorLayout;

/// The settings under Swift's keys; all six are always written. The
/// expression pane and the region strip are not drawn yet, so
/// `expression`, `show_regions` and `show_expression` are carried as
/// read (`expression` is Swift's enum JSON, e.g. `{"registered":{"_0":74}}`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PianoRollSettings {
    pub show_keyboard: bool,
    pub row_height: f64,
    /// The roll's own zoom (a 4/4 bar is 640 pt at 96).
    pub ticks_per_point: f64,
    pub expression: Value,
    pub show_regions: bool,
    pub show_expression: bool,
}

impl Default for PianoRollSettings {
    fn default() -> Self {
        PianoRollSettings {
            show_keyboard: true,
            row_height: 10.,
            ticks_per_point: 96.,
            expression: json!({"pitchBend": {}}),
            show_regions: true,
            show_expression: true,
        }
    }
}

impl PianoRollSettings {
    /// The row heights a layout is clamped to.
    pub const ROW_HEIGHT_RANGE: RangeInclusive<f64> = 4.0..=40.;

    /// Tolerant, field by field: one that is missing or malformed keeps
    /// its default; anything but an object is the default.
    pub(crate) fn decode(v: &Value) -> PianoRollSettings {
        let d = PianoRollSettings::default();
        let Some(obj) = v.as_object() else {
            return d;
        };
        let flag =
            |key: &str, fallback: bool| obj.get(key).and_then(Value::as_bool).unwrap_or(fallback);
        let number = |key: &str, range: &RangeInclusive<f64>, fallback: f64| {
            let v = obj.get(key).and_then(Value::as_f64).unwrap_or(fallback);
            EditorLayout::sane(v, range, fallback)
        };
        PianoRollSettings {
            show_keyboard: flag("showKeyboard", d.show_keyboard),
            row_height: number("rowHeight", &Self::ROW_HEIGHT_RANGE, d.row_height),
            ticks_per_point: number(
                "ticksPerPoint",
                &EditorLayout::TICKS_PER_POINT_RANGE,
                d.ticks_per_point,
            ),
            expression: obj
                .get("expression")
                .filter(|e| is_expression(e))
                .cloned()
                .unwrap_or(d.expression),
            show_regions: flag("showRegions", d.show_regions),
            show_expression: flag("showExpression", d.show_expression),
        }
    }
}

/// Swift's `ExpressionKind` JSON: `{"pitchBend":{}}`,
/// `{"registered":{"_0":n}}` or `{"assignable":{"_0":n}}` with `n` a byte.
fn is_expression(v: &Value) -> bool {
    let Some(obj) = v.as_object() else {
        return false;
    };
    let [(kind, payload)] = obj.iter().collect::<Vec<_>>()[..] else {
        return false;
    };
    let index = |p: &Map<String, Value>| {
        p.get("_0")
            .and_then(Value::as_u64)
            .is_some_and(|n| n <= u64::from(u8::MAX))
    };
    match (kind.as_str(), payload.as_object()) {
        ("pitchBend", Some(_)) => true,
        ("registered" | "assignable", Some(p)) => index(p),
        _ => false,
    }
}
