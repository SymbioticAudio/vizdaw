//! One list editor (Swift `ListEditorModel`): its rows, cursor, filter,
//! inserts and edits. The state lives in the document's editor model (one
//! list: the detail pane's); the operations are `DocumentModel`'s, since
//! they read and edit the document. Rows are built from the document's
//! cached reads, once per snapshot version, target, filter and columns.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use vizdaw_ffi::edit::LaneRef;
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::selection::SelectionRow;

use super::edit::{ListChange, ListDelete, ListEdit, ListInsert};
use super::rows::{
    ListColumn, ListColumns, ListContext, ListFilter, ListInput, ListKind, ListLane, ListRow,
    ListSource, build_rows,
};
use crate::document::DocumentModel;
use crate::editor::EditorTarget;

/// The list preferences (Swift `ListPreferences`, key "VizDAW.list"):
/// the default filter and the optional columns. App-wide; each document
/// holds the app's current copy.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ListPrefs {
    pub default_filter: ListFilter,
    pub columns: ListColumns,
}

/// The statuses Insert and Delete leave (Swift's exact strings).
pub const INSERT_REFERENCE: &str = "References and regions are made in the arrangement.";
pub const INSERT_MAP_NEEDS_SEQUENCE: &str =
    "Open the whole sequence to add tempo, meter or marker rows.";
pub const INSERT_METER_TAKEN: &str = "There is already a meter change at the next barline.";
pub const INSERT_NO_TRACK: &str = "There is no track to insert into.";
pub const FIRST_TEMPO_STAYS: &str = "The first tempo change stays.";
pub const FIRST_METER_STAYS: &str = "The first meter change stays.";

#[derive(Clone, PartialEq)]
struct RowsKey {
    version: u64,
    shown: Option<u64>,
    target: EditorTarget,
    filter: ListFilter,
    columns: ListColumns,
}

/// What one build produced.
struct Built {
    key: RowsKey,
    rows: Arc<[ListRow]>,
    /// Each row's index in `rows`, by ID.
    index: HashMap<String, usize>,
    context: Arc<ListContext>,
    lanes: usize,
    /// Every track read was this version's (not a busy core's older one).
    fresh: bool,
}

/// The list's own state: the cursor, the last inserted kind and the
/// cached rows.
pub struct ListModel {
    /// The row the keyboard cursor is on.
    pub(crate) cursor: Option<String>,
    pub(crate) last_insert_kind: ListKind,
    built: Mutex<Option<Arc<Built>>>,
    dests: Mutex<Option<(u64, Arc<[DestRow]>)>>,
    builds: Mutex<u64>,
}

impl Default for ListModel {
    fn default() -> Self {
        ListModel {
            cursor: None,
            last_insert_kind: ListKind::Note,
            built: Mutex::new(None),
            dests: Mutex::new(None),
            builds: Mutex::new(0),
        }
    }
}

impl Clone for ListModel {
    /// The cursor and last kind; the caches start empty.
    fn clone(&self) -> Self {
        ListModel {
            cursor: self.cursor.clone(),
            last_insert_kind: self.last_insert_kind,
            ..ListModel::default()
        }
    }
}

impl fmt::Debug for ListModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ListModel")
            .field("cursor", &self.cursor)
            .field("last_insert_kind", &self.last_insert_kind)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl ListModel {
    pub fn cursor(&self) -> Option<&str> {
        self.cursor.as_deref()
    }

    pub fn last_insert_kind(&self) -> ListKind {
        self.last_insert_kind
    }

    /// How many times rows were built (the cache's tests).
    #[doc(hidden)]
    pub fn builds(&self) -> u64 {
        *lock(&self.builds)
    }

    /// A new target: the cursor's row ids belonged to the old one.
    pub(crate) fn retarget(&mut self) {
        self.cursor = None;
    }
}

impl DocumentModel {
    /// The list's state (cursor, last inserted kind).
    pub fn list(&self) -> &ListModel {
        &self.editor.list
    }

    /// The app's list preferences as this document holds them.
    pub fn list_prefs(&self) -> &ListPrefs {
        &self.list_prefs
    }

    /// Takes the app's list preferences (after they changed).
    pub fn set_list_prefs(&mut self, prefs: ListPrefs) {
        self.list_prefs = prefs;
    }

    /// The list's target: the detail's.
    pub fn list_target(&self) -> Option<EditorTarget> {
        self.layout().detail.as_ref().map(|d| d.target)
    }

    /// A list editor shows in the detail pane.
    pub fn list_open(&self) -> bool {
        self.layout()
            .detail
            .as_ref()
            .is_some_and(|d| d.kind == crate::editor::DetailEditorKind::List)
    }

    /// The list's own filter, else the default one.
    pub fn list_filter(&self) -> ListFilter {
        self.layout()
            .detail
            .as_ref()
            .and_then(|d| d.list_filter.clone())
            .unwrap_or_else(|| self.list_prefs.default_filter.clone())
    }

    pub fn list_columns(&self) -> Vec<ListColumn> {
        let lanes = self.list_built().map_or(0, |b| b.lanes);
        self.list_prefs.columns.visible(lanes)
    }

    /// The rows, built once per snapshot version, target, filter and
    /// columns.
    pub fn list_rows(&self) -> Arc<[ListRow]> {
        self.list_built()
            .map_or_else(|| Arc::from([]), |b| b.rows.clone())
    }

    pub fn list_row(&self, id: &str) -> Option<ListRow> {
        let b = self.list_built()?;
        b.index.get(id).map(|&i| b.rows[i].clone())
    }

    /// The row's index in [`list_rows`](Self::list_rows), by its ID (a
    /// map built with the rows: no scan).
    pub fn list_index(&self, id: &str) -> Option<usize> {
        self.list_built()?.index.get(id).copied()
    }

    pub fn list_cursor_row(&self) -> Option<ListRow> {
        self.list_row(self.editor.list.cursor.as_deref()?)
    }

    /// What cell text needs (names, seconds, meter), built with the rows.
    pub fn list_context(&self) -> Arc<ListContext> {
        self.list_built()
            .map_or_else(|| Arc::new(ListContext::new(0)), |b| b.context.clone())
    }

    /// The instrument list (Destination column and filter), per snapshot;
    /// the last answer while the core cannot give one.
    pub fn list_destinations(&self) -> Arc<[DestRow]> {
        let version = self.snapshot().version;
        let mut cache = lock(&self.editor.list.dests);
        if let Some((v, rows)) = &*cache
            && *v == version
        {
            return rows.clone();
        }
        let rows: Arc<[DestRow]> = match self.session.destinations() {
            Ok(rows) => rows.into(),
            Err(_) => cache
                .as_ref()
                .map_or_else(|| Arc::from([]), |(_, r)| r.clone()),
        };
        *cache = Some((version, rows.clone()));
        rows
    }

    fn list_built(&self) -> Option<Arc<Built>> {
        let target = self.list_target()?;
        let key = RowsKey {
            version: self.snapshot().version,
            shown: self.selected_seq(),
            target,
            filter: self.list_filter(),
            columns: self.list_prefs.columns,
        };
        if let Some(b) = &*lock(&self.editor.list.built)
            && b.key == key
        {
            return Some(b.clone());
        }
        let built = Arc::new(self.build_list(key));
        *lock(&self.editor.list.builds) += 1;
        // A build from a busy core's older reads is built again next time.
        if built.fresh {
            *lock(&self.editor.list.built) = Some(built.clone());
        }
        Some(built)
    }

    fn build_list(&self, key: RowsKey) -> Built {
        let t = key.target;
        let seq = t.seq();
        let rows_read = self.lane_rows_read(seq);
        let mut fresh = rows_read.fresh;
        let lane_rows = rows_read.rows;
        let keys: Vec<LaneRef> = match t.lane() {
            Some(lane) => vec![LaneRef { seq, lane }],
            None => lane_rows
                .iter()
                .map(|l| LaneRef { seq, lane: l.id })
                .collect(),
        };
        let range = match t {
            EditorTarget::Block { start, end, .. } => Some(start..end.max(start.saturating_add(1))),
            _ => None,
        };
        let info = self.sequence_info(seq).cloned();
        let input = ListInput {
            lanes: keys
                .iter()
                .map(|k| {
                    let events = self.lane_events_read(seq, k.lane);
                    fresh &= events.fresh;
                    ListLane {
                        key: *k,
                        events: events.rows,
                        dest: lane_rows
                            .iter()
                            .find(|l| l.id == k.lane)
                            .and_then(|l| l.midi_dest),
                    }
                })
                .collect(),
            range,
            info: if t.lane().is_none() {
                info.clone()
            } else {
                None
            },
        };
        let rows = build_rows(&input, &key.filter);
        let mut seconds = HashMap::new();
        if key.columns.smpte && !rows.is_empty() {
            let ticks: Vec<i64> = rows.iter().map(ListRow::tick).collect();
            if let Ok(secs) = self.session.seconds_at(seq, ticks)
                && secs.len() == rows.len()
            {
                for (r, s) in rows.iter().zip(secs) {
                    seconds.insert(r.id().to_owned(), s);
                }
            }
        }
        let session = self.session.clone();
        let context = ListContext {
            seq,
            meter: info.as_ref().map(|i| i.meter.clone()).unwrap_or_default(),
            info,
            lane_names: lane_rows
                .iter()
                .map(|l| (LaneRef { seq, lane: l.id }, l.name.clone()))
                .collect(),
            dest_names: self
                .list_destinations()
                .iter()
                .rev()
                .map(|d| (d.id, d.name.clone()))
                .collect::<BTreeMap<_, _>>(),
            seq_names: self
                .snapshot()
                .sequences
                .iter()
                .rev()
                .map(|q| (q.id, q.name.clone()))
                .collect(),
            seconds: Arc::new(seconds),
            fps: key.columns.fps,
            tick_at_seconds: Arc::new(move |s| {
                session
                    .ticks_at(seq, vec![s])
                    .ok()
                    .and_then(|t| t.first().copied())
            }),
        };
        let index = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id().to_owned(), i))
            .collect();
        Built {
            key,
            index,
            rows: rows.into(),
            context: Arc::new(context),
            lanes: keys.len(),
            fresh,
        }
    }

    /// Moves the keyboard cursor (the view's last selected row).
    pub fn list_set_cursor(&mut self, id: Option<String>) {
        self.editor.list.cursor = id;
    }

    /// Commits typed text (Return or Tab); false when refused (the status
    /// line says why) or the row is gone.
    pub fn list_commit(&mut self, text: &str, id: &str, column: ListColumn) -> bool {
        let Some(r) = self.list_row(id) else {
            return false;
        };
        let change = match ListEdit::commit(text, column, &r, &self.list_context()) {
            Ok(c) => c,
            Err(e) => {
                self.status = Some(e.0);
                return false;
            }
        };
        let ListChange::Edit(cmd, tick) = change else {
            return true;
        };
        if !self.edit(cmd) {
            return false;
        }
        self.list_follow(&r, tick);
        true
    }

    /// The cursor stays on the edited row: an event keeps its ID (and
    /// becomes the selection); a map row is found again by kind and tick.
    fn list_follow(&mut self, r: &ListRow, tick: i64) {
        if r.event().is_some() {
            self.editor.list.cursor = Some(r.id().to_owned());
            self.list_select(&[r.id().to_owned()]);
        } else if let Some(found) = self
            .list_rows()
            .iter()
            .find(|x| x.kind() == r.kind() && x.tick() == tick)
        {
            self.editor.list.cursor = Some(found.id().to_owned());
        }
    }

    /// What Insert offers here: map kinds only for a whole sequence.
    pub fn list_insert_kinds(&self) -> Vec<ListKind> {
        let whole = self.list_target().is_some_and(|t| t.lane().is_none());
        ListKind::INSERTABLE
            .into_iter()
            .filter(|k| whole || k.event_kind().is_some())
            .collect()
    }

    /// Inserts `kind`, or the last inserted kind.
    pub fn list_insert(&mut self, kind: Option<ListKind>) {
        let Some(t) = self.list_target() else { return };
        let k = kind.unwrap_or(self.editor.list.last_insert_kind);
        if matches!(k, ListKind::Reference | ListKind::AudioRegion) {
            self.status = Some(INSERT_REFERENCE.into());
            return;
        }
        if !self.list_insert_kinds().contains(&k) {
            self.status = Some(INSERT_MAP_NEEDS_SEQUENCE.into());
            return;
        }
        let tick = self.list_insert_tick();
        let seq = t.seq();
        let lane = match t.lane() {
            Some(lane) => Some(LaneRef { seq, lane }),
            None => self.list_cursor_row().and_then(|r| r.lane()).or_else(|| {
                self.lane_rows(seq)
                    .first()
                    .map(|l| LaneRef { seq, lane: l.id })
            }),
        };
        let info = self.sequence_info(seq).cloned();
        let Some(cmd) = ListInsert::command(k, tick, lane, seq, info.as_ref()) else {
            self.status = Some(
                if k == ListKind::Meter {
                    INSERT_METER_TAKEN
                } else {
                    INSERT_NO_TRACK
                }
                .into(),
            );
            return;
        };
        let before: BTreeSet<u64> = self
            .list_rows()
            .iter()
            .filter_map(|r| r.event().map(|e| e.id))
            .collect();
        if !self.edit(cmd) {
            return;
        }
        self.editor.list.last_insert_kind = k;
        let rows = self.list_rows();
        let made = if k.event_kind().is_none() {
            rows.iter().find(|r| r.kind() == k && r.tick() >= tick)
        } else {
            rows.iter()
                .find(|r| r.event().is_some_and(|e| !before.contains(&e.id)))
        };
        if let Some(made) = made {
            let id = made.id().to_owned();
            let is_event = made.event().is_some();
            self.editor.list.cursor = Some(id.clone());
            if is_event {
                self.list_select(&[id]);
            }
        }
    }

    /// The cursor row's tick, else the stopped transport's when it lies in
    /// the target and is on this list's own sequence, else the target's
    /// start.
    fn list_insert_tick(&self) -> i64 {
        if let Some(r) = self.list_cursor_row() {
            return r.tick();
        }
        let Some(t) = self.list_target() else {
            return 0;
        };
        let range = match t {
            EditorTarget::Block { start, end, .. } => Some(start..end.max(start.saturating_add(1))),
            _ => None,
        };
        let transport = self.last_transport();
        if !transport.playing
            && transport.seq == Some(t.seq())
            && range.as_ref().is_none_or(|r| r.contains(&transport.tick))
        {
            return transport.tick;
        }
        range.map_or(0, |r| r.start)
    }

    /// Deletes rows: mixed rows (events, tempo, meter, markers) are one
    /// undo step "Delete", all or nothing; the first tempo and meter
    /// changes stay, and the status says so.
    pub fn list_delete(&mut self, ids: &[String]) {
        let Some(t) = self.list_target() else { return };
        let set: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let chosen: Vec<ListRow> = self
            .list_rows()
            .iter()
            .filter(|r| set.contains(r.id()))
            .cloned()
            .collect();
        let first = chosen.iter().find_map(|r| match r.source {
            ListSource::Tempo { index: 0, .. } => Some(FIRST_TEMPO_STAYS),
            ListSource::Meter { index: 0, .. } => Some(FIRST_METER_STAYS),
            _ => None,
        });
        let info = self.sequence_info(t.seq()).cloned();
        let cmds = ListDelete::commands(&chosen, t.seq(), info.as_ref());
        if !self.edit_all(cmds, "Delete") {
            return;
        }
        if let Some(first) = first {
            self.status = Some(first.into());
        }
        self.editor.list.cursor = None;
    }

    /// Selected rows become the document selection (their events and
    /// tracks); the time range goes.
    pub fn list_select(&mut self, ids: &[String]) {
        let rows = self.list_rows();
        let set: HashSet<&str> = ids.iter().map(String::as_str).collect();
        let chosen: Vec<&ListRow> = rows.iter().filter(|r| set.contains(r.id())).collect();
        let mut lanes: Vec<LaneRef> = vec![];
        for r in &chosen {
            if let Some(l) = r.lane()
                && !lanes.contains(&l)
            {
                lanes.push(l);
            }
        }
        let events = chosen
            .iter()
            .filter_map(|r| r.event().map(|e| e.id))
            .collect();
        self.set_selection(SelectionRow {
            time_range: None,
            lanes,
            events,
        });
    }

    /// Gives this list its own filter.
    pub fn list_set_filter(&mut self, filter: ListFilter) {
        self.update_layout(|l| {
            if let Some(d) = &mut l.detail {
                d.list_filter = Some(filter);
            }
        });
    }

    pub fn list_toggle_kind(&mut self, kind: ListKind) {
        let mut f = self.list_filter();
        if !f.hidden_kinds.remove(&kind) {
            f.hidden_kinds.insert(kind);
        }
        self.list_set_filter(f);
    }

    pub fn list_shows_dest(&self, dest: u64) -> bool {
        self.list_filter()
            .shown_dests
            .is_none_or(|d| d.contains(&dest))
    }

    /// Shows or hides destination `dest`; showing them all is no
    /// destination filter.
    pub fn list_toggle_dest(&mut self, dest: u64) {
        let mut f = self.list_filter();
        let all: BTreeSet<u64> = self.list_destinations().iter().map(|d| d.id).collect();
        let mut shown = f.shown_dests.clone().unwrap_or_else(|| all.clone());
        if !shown.remove(&dest) {
            shown.insert(dest);
        }
        f.shown_dests = (shown != all).then_some(shown);
        self.list_set_filter(f);
    }

    /// "Show Everything": no kinds hidden, every destination.
    pub fn list_show_everything(&mut self) {
        self.list_set_filter(ListFilter::default());
    }

    /// The table has the keyboard: Return no longer returns to start
    /// (Space still plays).
    pub fn list_focused(&self) -> bool {
        self.list_focused
    }

    pub fn set_list_focused(&mut self, focused: bool) {
        self.list_focused = focused;
    }
}
