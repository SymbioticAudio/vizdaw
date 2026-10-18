//! Typed reads of a request's `params` object. Every failure is an
//! `invalid_params` error naming the command and the parameter.

use crate::error::ControlError;
use crate::protocol::truncate;
use serde_json::{Map, Value};
use std::path::PathBuf;

/// The longest string parameter (characters).
pub const MAX_STRING: usize = 4096;
/// The longest path parameter (bytes).
pub const MAX_PATH: usize = 1024;
/// Window coordinates are within ±this many points.
pub const MAX_COORD: f64 = 100_000.0;

/// A window a request addresses (spec §5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowRef {
    /// `NSWindow.windowNumber`, as `state.get windows` lists it.
    Number(i64),
    /// The key window.
    Key,
    /// The main window.
    Main,
}

/// The `params` of command `cmd`.
#[derive(Debug, Clone, Copy)]
pub struct Params<'a> {
    cmd: &'a str,
    map: &'a Map<String, Value>,
}

impl<'a> Params<'a> {
    /// Wraps `map`, the params of `cmd`.
    pub fn new(cmd: &'a str, map: &'a Map<String, Value>) -> Params<'a> {
        Params { cmd, map }
    }

    /// The command these params belong to.
    pub fn cmd(&self) -> &str {
        self.cmd
    }

    /// An `invalid_params` error for this command.
    pub fn error(&self, message: impl std::fmt::Display) -> ControlError {
        ControlError::params(self.cmd, message)
    }

    /// Refuses any parameter not in `allowed`.
    pub fn only(&self, allowed: &[&str]) -> Result<(), ControlError> {
        match self.map.keys().find(|k| !allowed.contains(&k.as_str())) {
            None => Ok(()),
            Some(k) => Err(self.error(format!(
                "unknown parameter {:?} (allowed: {})",
                truncate(k, 64),
                if allowed.is_empty() {
                    "none".to_string()
                } else {
                    allowed.join(", ")
                }
            ))),
        }
    }

    /// The raw value of `name`; null counts as absent.
    pub fn raw(&self, name: &str) -> Option<&'a Value> {
        self.map.get(name).filter(|v| !v.is_null())
    }

    /// Whether `name` is present (and not null).
    pub fn has(&self, name: &str) -> bool {
        self.raw(name).is_some()
    }

    /// An optional string of at most `max` characters.
    pub fn opt_str(&self, name: &str, max: usize) -> Result<Option<String>, ControlError> {
        match self.raw(name) {
            None => Ok(None),
            Some(Value::String(s)) if s.chars().count() <= max => Ok(Some(s.clone())),
            Some(Value::String(_)) => {
                Err(self.error(format!("`{name}` is longer than {max} characters")))
            }
            Some(_) => Err(self.error(format!("`{name}` must be a string"))),
        }
    }

    /// A required string of at most `max` characters.
    pub fn str(&self, name: &str, max: usize) -> Result<String, ControlError> {
        self.opt_str(name, max)?
            .ok_or_else(|| self.error(format!("missing `{name}`")))
    }

    /// An optional boolean.
    pub fn opt_bool(&self, name: &str) -> Result<Option<bool>, ControlError> {
        match self.raw(name) {
            None => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(*b)),
            Some(_) => Err(self.error(format!("`{name}` must be true or false"))),
        }
    }

    /// An optional integer in `min..=max`.
    pub fn opt_int(&self, name: &str, min: i64, max: i64) -> Result<Option<i64>, ControlError> {
        match self.raw(name) {
            None => Ok(None),
            Some(Value::Number(n)) => match n.as_i64() {
                Some(i) if (min..=max).contains(&i) => Ok(Some(i)),
                _ => Err(self.error(format!("`{name}` must be an integer from {min} to {max}"))),
            },
            Some(_) => Err(self.error(format!("`{name}` must be an integer"))),
        }
    }

    /// A required integer in `min..=max`.
    pub fn int(&self, name: &str, min: i64, max: i64) -> Result<i64, ControlError> {
        self.opt_int(name, min, max)?
            .ok_or_else(|| self.error(format!("missing `{name}`")))
    }

    /// An optional finite number in `min..=max`.
    pub fn opt_num(&self, name: &str, min: f64, max: f64) -> Result<Option<f64>, ControlError> {
        match self.raw(name) {
            None => Ok(None),
            Some(Value::Number(n)) => match n.as_f64() {
                Some(x) if x.is_finite() && (min..=max).contains(&x) => Ok(Some(x)),
                _ => Err(self.error(format!("`{name}` must be a number from {min} to {max}"))),
            },
            Some(_) => Err(self.error(format!("`{name}` must be a number"))),
        }
    }

    /// An optional absolute path of at most [`MAX_PATH`] bytes ending in
    /// `.{extension}`.
    pub fn opt_path(&self, name: &str, extension: &str) -> Result<Option<PathBuf>, ControlError> {
        let Some(s) = self.opt_str(name, MAX_PATH)? else {
            return Ok(None);
        };
        let p = PathBuf::from(&s);
        if s.len() > MAX_PATH || s.contains('\0') || !p.is_absolute() {
            return Err(self.error(format!("`{name}` must be an absolute path")));
        }
        if p.extension().and_then(|e| e.to_str()) != Some(extension) {
            return Err(self.error(format!("`{name}` must end in .{extension}")));
        }
        Ok(Some(p))
    }

    /// The optional `window`: a window number, `"key"` or `"main"`.
    pub fn window(&self) -> Result<Option<WindowRef>, ControlError> {
        match self.raw("window") {
            None => Ok(None),
            Some(Value::Number(n)) => match n.as_i64() {
                Some(i) if i > 0 => Ok(Some(WindowRef::Number(i))),
                _ => Err(self.error("`window` must be a positive window number")),
            },
            Some(Value::String(s)) if s == "key" => Ok(Some(WindowRef::Key)),
            Some(Value::String(s)) if s == "main" => Ok(Some(WindowRef::Main)),
            Some(_) => Err(self.error(
                "`window` must be a window number from `state.get windows`, \"key\" or \"main\"",
            )),
        }
    }

    /// A point object `{x, y}` in window points.
    pub fn point_of(&self, name: &str, v: &Value) -> Result<(f64, f64), ControlError> {
        let coord = |k: &str| {
            v.get(k)
                .and_then(Value::as_f64)
                .filter(|x| x.is_finite() && x.abs() <= MAX_COORD)
        };
        match (v.as_object().map(Map::len), coord("x"), coord("y")) {
            (Some(2), Some(x), Some(y)) => Ok((x, y)),
            _ => Err(self.error(format!(
                "`{name}` must be {{\"x\": number, \"y\": number}} within ±{MAX_COORD}"
            ))),
        }
    }
}
