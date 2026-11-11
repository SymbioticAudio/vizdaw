//! The report of what an import or export changed, left out or noted.

use crate::limits::MAX_DETAILS;

/// How an item affected the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Changed,
    LeftOut,
    Note,
}

/// One kind of thing that happened, with how often and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub kind: ItemKind,
    pub what: String,
    pub count: u64,
    /// The first [`MAX_DETAILS`] places it happened.
    pub details: Vec<String>,
}

/// What the result holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub tracks: u64,
    pub regions: u64,
    pub notes: u64,
    pub markers: u64,
    pub automation_points: u64,
    pub plugins: u64,
    pub tempo_map: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    pub summary: Summary,
    pub items: Vec<Item>,
}

impl Report {
    /// Counts one occurrence of `what` under `kind`, keeping `detail` while
    /// the item has fewer than [`MAX_DETAILS`].
    pub fn add(&mut self, kind: ItemKind, what: &str, detail: Option<String>) {
        let i = match self
            .items
            .iter()
            .position(|i| i.kind == kind && i.what == what)
        {
            Some(i) => i,
            None => {
                self.items.push(Item {
                    kind,
                    what: what.to_string(),
                    count: 0,
                    details: Vec::new(),
                });
                self.items.len() - 1
            }
        };
        let item = &mut self.items[i];
        item.count = item.count.saturating_add(1);
        if let Some(d) = detail
            && item.details.len() < MAX_DETAILS
        {
            item.details.push(d);
        }
    }

    /// Adds `other`'s summary counts and items (same grouping and caps).
    pub fn merge(&mut self, other: Report) {
        let (s, o) = (&mut self.summary, other.summary);
        s.tracks = s.tracks.saturating_add(o.tracks);
        s.regions = s.regions.saturating_add(o.regions);
        s.notes = s.notes.saturating_add(o.notes);
        s.markers = s.markers.saturating_add(o.markers);
        s.automation_points = s.automation_points.saturating_add(o.automation_points);
        s.plugins = s.plugins.saturating_add(o.plugins);
        s.tempo_map |= o.tempo_map;
        for it in other.items {
            match self
                .items
                .iter_mut()
                .find(|i| i.kind == it.kind && i.what == it.what)
            {
                Some(mine) => {
                    mine.count = mine.count.saturating_add(it.count);
                    let room = MAX_DETAILS - mine.details.len();
                    mine.details.extend(it.details.into_iter().take(room));
                }
                None => self.items.push(it),
            }
        }
    }

    /// True if there are no items to show.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// "`verb` 12 tracks, 4 210 notes, the tempo map and 8 markers".
    pub fn summary_line(&self, verb: &str) -> String {
        let s = &self.summary;
        let count = |n: u64, one: &str, many: &str| {
            (n > 0).then(|| format!("{} {}", group(n), if n == 1 { one } else { many }))
        };
        let parts: Vec<String> = [
            count(s.tracks, "track", "tracks"),
            count(s.regions, "region", "regions"),
            count(s.notes, "note", "notes"),
            s.tempo_map.then(|| "the tempo map".to_string()),
            count(s.markers, "marker", "markers"),
            count(s.automation_points, "automation point", "automation points"),
            count(s.plugins, "plug-in", "plug-ins"),
        ]
        .into_iter()
        .flatten()
        .collect();
        match parts.len() {
            0 => format!("{verb} nothing"),
            1 => format!("{verb} {}", parts[0]),
            n => format!("{verb} {} and {}", parts[..n - 1].join(", "), parts[n - 1]),
        }
    }
}

/// `n` with thousands separated by a space.
fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}
