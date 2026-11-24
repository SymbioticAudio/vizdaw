//! The app's preferences: one JSON file under the launch home (so a
//! throwaway e2e home keeps its own), or only in memory for tests. Only
//! what the app has needed so far: the `transportBar` and `list` keys.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};
use vizdaw_ffi::{BitDepthRow, ProjectFormatRow};

use crate::list::{ListColumns, ListFilter, ListPrefs};
use crate::transport_bar::{TransportItem, TransportLayout};

/// The preference's key.
pub const TRANSPORT_BAR_KEY: &str = "transportBar";
/// The list editors' default filter and columns (Swift "VizDAW.list").
pub const LIST_KEY: &str = "list";
/// The format File › New makes (Swift "defaultProjectFormat",
/// `<rate>/<16|24|32f>`).
pub const DEFAULT_FORMAT_KEY: &str = "defaultProjectFormat";
/// The stored value of the default bar: it follows the default when a
/// later version changes it.
const STANDARD: &str = "standard";
const VERSION: i64 = 1;

/// A preferences store: a JSON object, written through to `path`.
#[derive(Debug, Default)]
pub struct Prefs {
    path: Option<PathBuf>,
    map: Map<String, Value>,
}

impl Prefs {
    /// A store that never touches the disk.
    pub fn in_memory() -> Self {
        Self::default()
    }

    /// The store in `path`; a missing or unreadable file is an empty store
    /// (and is left as it is until something is set).
    pub fn at(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let map = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default();
        Self {
            path: Some(path),
            map,
        }
    }

    /// The file under `home`: `Library/Preferences/VizDAW/preferences.json`
    /// on macOS, `.config/vizdaw/preferences.json` elsewhere.
    pub fn file_in(home: &Path) -> PathBuf {
        if cfg!(target_os = "macos") {
            home.join("Library/Preferences/VizDAW/preferences.json")
        } else {
            home.join(".config/vizdaw/preferences.json")
        }
    }

    /// The store under the launch home (`$HOME`); in memory when there is
    /// none.
    pub fn open() -> Self {
        match std::env::var_os("HOME") {
            Some(h) => Self::at(Self::file_in(Path::new(&h))),
            None => Self::in_memory(),
        }
    }

    /// The raw stored value of `key`.
    pub fn raw(&self, key: &str) -> Option<&Value> {
        self.map.get(key)
    }

    /// The stored bar; a damaged or newer value gives the standard layout
    /// and is kept as it is until `set_transport_bar`.
    pub fn transport_bar(&self) -> TransportLayout {
        self.map
            .get(TRANSPORT_BAR_KEY)
            .and_then(decode)
            .unwrap_or_else(TransportLayout::standard)
    }

    /// Shows `layout` and writes the preference (the default as
    /// `"standard"`); a layout that breaks the rules is refused before
    /// anything is written.
    pub fn set_transport_bar(&mut self, layout: &TransportLayout) -> Result<(), String> {
        TransportLayout::checked(layout.items.clone())?;
        let value = if *layout == TransportLayout::standard() {
            Value::String(STANDARD.into())
        } else {
            json!({
                "version": VERSION,
                "items": layout.items.iter().map(TransportItem::token).collect::<Vec<_>>(),
            })
        };
        let old = self.map.insert(TRANSPORT_BAR_KEY.into(), value);
        if let Err(e) = self.save() {
            match old {
                Some(o) => self.map.insert(TRANSPORT_BAR_KEY.into(), o),
                None => self.map.remove(TRANSPORT_BAR_KEY),
            };
            return Err(e);
        }
        Ok(())
    }

    /// The default bar, written.
    pub fn reset_transport_bar(&mut self) -> Result<(), String> {
        self.set_transport_bar(&TransportLayout::standard())
    }

    /// The list preferences; a missing or damaged value is the defaults
    /// (and is kept as it is until one is set).
    pub fn list(&self) -> ListPrefs {
        self.map
            .get(LIST_KEY)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }

    /// Writes the list preferences; on a failure nothing changes.
    pub fn set_list(&mut self, prefs: &ListPrefs) -> Result<(), String> {
        let value = serde_json::to_value(prefs).map_err(|e| e.to_string())?;
        let old = self.map.insert(LIST_KEY.into(), value);
        if let Err(e) = self.save() {
            match old {
                Some(o) => self.map.insert(LIST_KEY.into(), o),
                None => self.map.remove(LIST_KEY),
            };
            return Err(e);
        }
        Ok(())
    }

    /// The optional list columns and the SMPTE rate.
    pub fn set_list_columns(&mut self, columns: ListColumns) -> Result<(), String> {
        let mut p = self.list();
        p.columns = columns;
        self.set_list(&p)
    }

    /// "Save as Default": only the hidden kinds are kept (destinations
    /// belong to one project).
    pub fn set_list_default_filter(&mut self, filter: &ListFilter) -> Result<(), String> {
        let mut p = self.list();
        p.default_filter = filter.kinds_only();
        self.set_list(&p)
    }

    /// The default project format; a missing or damaged value is 48 kHz,
    /// 24-bit.
    pub fn default_format(&self) -> ProjectFormatRow {
        self.map
            .get(DEFAULT_FORMAT_KEY)
            .and_then(Value::as_str)
            .and_then(parse_format)
            .unwrap_or(ProjectFormatRow {
                sample_rate: 48_000,
                depth: BitDepthRow::Int24,
            })
    }

    /// Writes the default project format; on a failure nothing changes.
    pub fn set_default_format(&mut self, f: ProjectFormatRow) -> Result<(), String> {
        let depth = match f.depth {
            BitDepthRow::Int16 => "16",
            BitDepthRow::Int24 => "24",
            BitDepthRow::Float32 => "32f",
        };
        self.set_raw(
            DEFAULT_FORMAT_KEY,
            Value::String(format!("{}/{depth}", f.sample_rate)),
        )
    }

    /// Sets `key` and writes; on a failure the old value stays.
    pub fn set_raw(&mut self, key: &str, value: Value) -> Result<(), String> {
        let old = self.map.insert(key.into(), value);
        if let Err(e) = self.save() {
            match old {
                Some(o) => self.map.insert(key.into(), o),
                None => self.map.remove(key),
            };
            return Err(e);
        }
        Ok(())
    }

    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let fail = |e: std::io::Error| format!("the preferences could not be written: {e}");
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(fail)?;
        }
        let text = serde_json::to_vec_pretty(&self.map).map_err(|e| e.to_string())?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(fail)?;
        std::fs::rename(&tmp, path).map_err(fail)
    }
}

/// `<rate>/<16|24|32f>` with a project rate.
fn parse_format(s: &str) -> Option<ProjectFormatRow> {
    let (rate, depth) = s.split_once('/')?;
    let sample_rate: u32 = rate.parse().ok()?;
    if !vizdaw_ffi::project_rates().contains(&sample_rate) {
        return None;
    }
    let depth = match depth {
        "16" => BitDepthRow::Int16,
        "24" => BitDepthRow::Int24,
        "32f" => BitDepthRow::Float32,
        _ => return None,
    };
    Some(ProjectFormatRow { sample_rate, depth })
}

fn decode(v: &Value) -> Option<TransportLayout> {
    match v {
        Value::String(s) if s == STANDARD => Some(TransportLayout::standard()),
        Value::Object(o) => {
            if o.get("version")?.as_i64()? != VERSION {
                return None;
            }
            let items = o
                .get("items")?
                .as_array()?
                .iter()
                .map(|t| TransportItem::parse(t.as_str()?).ok())
                .collect::<Option<Vec<_>>>()?;
            TransportLayout::checked(items).ok()
        }
        _ => None,
    }
}
