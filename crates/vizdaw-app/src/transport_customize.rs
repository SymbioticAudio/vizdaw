//! Customize Transport Bar's model (Swift `TransportCustomize.swift`): a
//! draft of the bar the sheet's palette and bar edit, taken as the bar
//! only by `finish`. Every edit keeps the draft a layout the preference
//! takes: the cluster stays, a built-in item is on the bar once, at most
//! 64 items.

use core_control::registry;

use crate::bar_items::STALE_TOOLTIP;
use crate::transport_bar::{MAX_LABEL, Size, TransportItem, TransportLayout, is_command_id};

/// The command a new command button from the palette runs until one is
/// chosen.
pub const NEW_COMMAND: &str = "window.mixer";
/// What a label over the limit says.
pub const LABEL_TOO_LONG: &str = "A label is at most 32 characters.";

/// The built-in items the palette offers, in the bar's order (a readout
/// comes large).
fn built_ins() -> [TransportItem; 19] {
    use TransportItem::*;
    [
        Cluster,
        Loop,
        Click,
        ReplaceOverdub,
        RecordOptions,
        CountIn,
        Summary,
        Bar(Size::Large),
        Time(Size::Large),
        Tempo(Size::Large),
        Signature,
        PositionField,
        EditFields,
        PunchFields,
        DspLoad,
        Device,
        Faults,
        MainView,
        Panes,
    ]
}

/// One command of the picker: its id, its menu title and its menu.
pub type CommandRow = (&'static str, &'static str, &'static str);

/// Where a dragged chip came from: bar item `n` or palette item `n`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragSource {
    Bar(usize),
    Palette(usize),
}

/// The draft of the bar and the item the inspector shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Customizer {
    items: Vec<TransportItem>,
    selected: Option<usize>,
}

impl Customizer {
    pub fn new(layout: &TransportLayout) -> Self {
        Self {
            items: layout.items.clone(),
            selected: None,
        }
    }

    /// The draft's items.
    pub fn items(&self) -> &[TransportItem] {
        &self.items
    }

    /// The item the inspector shows.
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// Shows item `i` in the inspector (none, or one the bar has not:
    /// nothing).
    pub fn select(&mut self, i: Option<usize>) {
        self.selected = i.filter(|n| *n < self.items.len());
    }

    /// The draft with `items`, when they are a layout; false (the draft
    /// unchanged) when not.
    fn take(&mut self, items: Vec<TransportItem>) -> bool {
        match TransportLayout::checked(items) {
            Ok(l) => {
                if l.items.len() != self.items.len() {
                    self.selected = None;
                }
                self.items = l.items;
                true
            }
            Err(_) => false,
        }
    }

    /// Every built-in item not on the bar, in the bar's order, then
    /// Flexible Space, Separator and a Command Button (always offered).
    pub fn palette(&self) -> Vec<TransportItem> {
        let mut out: Vec<TransportItem> = built_ins()
            .into_iter()
            .filter(|b| !self.items.iter().any(|i| i.name() == b.name()))
            .collect();
        out.push(TransportItem::Space);
        out.push(TransportItem::Separator);
        out.push(TransportItem::Command {
            id: NEW_COMMAND.into(),
            label: None,
        });
        out
    }

    /// Puts `item` at `at` (past the end appends); false when the bar
    /// would break a rule.
    pub fn insert(&mut self, at: usize, item: TransportItem) -> bool {
        let mut items = self.items.clone();
        let at = at.min(items.len());
        items.insert(at, item);
        // The inspector stays on its item when one goes in before it.
        let selected = self.selected.map(|s| if at <= s { s + 1 } else { s });
        let took = self.take(items);
        if took {
            self.selected = selected;
        }
        took
    }

    /// Takes item `i` off the bar; false for the cluster or an index the
    /// bar has not.
    pub fn remove(&mut self, i: usize) -> bool {
        if i >= self.items.len() || self.items[i] == TransportItem::Cluster {
            return false;
        }
        let mut items = self.items.clone();
        items.remove(i);
        self.take(items)
    }

    pub fn move_left(&mut self, i: usize) {
        if i > 0 {
            self.move_to(i, i - 1);
        }
    }

    pub fn move_right(&mut self, i: usize) {
        if i + 1 < self.items.len() {
            self.move_to(i, i + 1);
        }
    }

    /// Moves item `from` so it ends at `to` (clamped); the selection
    /// follows it.
    pub fn move_to(&mut self, from: usize, to: usize) {
        if from >= self.items.len() {
            return;
        }
        let mut items = self.items.clone();
        let item = items.remove(from);
        let at = to.min(items.len());
        items.insert(at, item);
        // The selection stays on its item: it follows a move of its own,
        // and shifts by one when the move crosses it.
        let selected = self.selected.map(|s| {
            if s == from {
                at
            } else {
                let s = if from < s { s - 1 } else { s };
                if at <= s { s + 1 } else { s }
            }
        });
        if self.take(items) {
            self.selected = selected;
        }
    }

    /// A drop of `source` on the bar before slot `index` (the end appends):
    /// a palette item is added, a bar item moved. False when nothing
    /// changed.
    pub fn drop_on_bar(&mut self, source: DragSource, index: usize) -> bool {
        match source {
            DragSource::Palette(n) => match self.palette().get(n).cloned() {
                Some(item) => self.insert(index, item),
                None => false,
            },
            DragSource::Bar(n) => {
                if n >= self.items.len() {
                    return false;
                }
                let before = self.items.clone();
                self.move_to(n, if n < index { index - 1 } else { index });
                self.items != before
            }
        }
    }

    /// A drop of `source` on the palette: a bar item is removed (never the
    /// cluster).
    pub fn drop_on_palette(&mut self, source: DragSource) -> bool {
        match source {
            DragSource::Bar(n) => self.remove(n),
            DragSource::Palette(_) => false,
        }
    }

    /// Bar, Time and Tempo come small or large; other items ignore it.
    pub fn set_size(&mut self, i: usize, size: Size) {
        let Some(item) = self.items.get(i) else {
            return;
        };
        let new = match item {
            TransportItem::Bar(_) => TransportItem::Bar(size),
            TransportItem::Time(_) => TransportItem::Time(size),
            TransportItem::Tempo(_) => TransportItem::Tempo(size),
            _ => return,
        };
        let mut items = self.items.clone();
        items[i] = new;
        self.take(items);
    }

    /// Command button `i` runs `id`, keeping its label; false for an item
    /// that is not a command button or an id not spelled as one.
    pub fn set_command(&mut self, i: usize, id: &str) -> bool {
        let Some(TransportItem::Command { label, .. }) = self.items.get(i) else {
            return false;
        };
        if !is_command_id(id) {
            return false;
        }
        let mut items = self.items.clone();
        items[i] = TransportItem::Command {
            id: id.into(),
            label: label.clone(),
        };
        self.take(items)
    }

    /// Command button `i` shows `label`, trimmed (blank: its command's
    /// title); an error for more than 32 characters or an item that is
    /// not a command button.
    pub fn set_label(&mut self, i: usize, label: &str) -> Result<(), String> {
        let Some(TransportItem::Command { id, .. }) = self.items.get(i) else {
            return Err("Only a command button has a label.".into());
        };
        let label = label.trim();
        if label.chars().count() > MAX_LABEL {
            return Err(LABEL_TOO_LONG.into());
        }
        let mut items = self.items.clone();
        items[i] = TransportItem::Command {
            id: id.clone(),
            label: (!label.is_empty()).then(|| label.to_owned()),
        };
        if self.take(items) {
            Ok(())
        } else {
            Err("That label is not allowed.".into())
        }
    }

    /// The picker's commands, in registry order, whose title, menu or id
    /// contains `query` (trimmed, any case); a blank query gives all.
    pub fn commands(&self, query: &str) -> Vec<CommandRow> {
        let q = query.trim().to_lowercase();
        registry::COMMANDS
            .iter()
            .filter(|c| {
                q.is_empty()
                    || c.title.to_lowercase().contains(&q)
                    || c.menu.to_lowercase().contains(&q)
                    || c.id.to_lowercase().contains(&q)
            })
            .map(|c| (c.id, c.title, c.menu))
            .collect()
    }

    /// Why command button `i` is greyed: its command is no longer in
    /// VizDAW (the button stays, never dropped).
    pub fn stale(&self, i: usize) -> Option<String> {
        match self.items.get(i)? {
            TransportItem::Command { id, .. } if registry::find(id).is_none() => {
                Some(STALE_TOOLTIP.into())
            }
            _ => None,
        }
    }

    /// Reset to the standard bar.
    pub fn reset(&mut self) {
        self.items = TransportLayout::standard().items;
        self.selected = None;
    }

    /// Done: the draft as the layout to store, or why it is not one.
    pub fn finish(&self) -> Result<TransportLayout, String> {
        TransportLayout::checked(self.items.clone())
    }
}
