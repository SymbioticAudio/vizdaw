//! The report of what an import or export changed, left out or noted.

use crate::limits::MAX_DETAILS;

/// How an item affected the result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    /// Both sides of a merge changed it; the project's side was kept (plan
    /// 6e Ruling 15).
    Conflict,
    /// The result differs from the input.
    Changed,
    /// Something in the input is not in the result.
    LeftOut,
    /// Worth knowing; nothing was lost.
    Note,
}

/// One kind of thing that happened, with how often and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// How it affected the result.
    pub kind: ItemKind,
    /// What it is, in a sentence fragment.
    pub what: String,
    /// How many times it happened.
    pub count: u64,
    /// The first [`MAX_DETAILS`] places it happened.
    pub details: Vec<String>,
}

/// What the result holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    /// Tracks in the result.
    pub tracks: u64,
    /// Audio files in the result.
    pub audio_files: u64,
    /// Regions in the result.
    pub regions: u64,
    /// Notes in the result.
    pub notes: u64,
    /// Markers in the result.
    pub markers: u64,
    /// Automation points in the result.
    pub automation_points: u64,
    /// Plug-ins in the result.
    pub plugins: u64,
    /// Whether a tempo map is in the result.
    pub tempo_map: bool,
}

/// What happened, for the dialog after an import or export. Counts
/// saturate rather than wrap: they are for display, not for sizes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// What the result holds.
    pub summary: Summary,
    /// What was in conflict, changed, left out or noted.
    pub items: Vec<Item>,
}

impl Report {
    /// Counts one occurrence of `what` under `kind`, keeping `detail` while
    /// the item has fewer than [`MAX_DETAILS`].
    pub fn add(&mut self, kind: ItemKind, what: &str, detail: Option<String>) {
        let item = match self
            .items
            .iter()
            .position(|i| i.kind == kind && i.what == what)
        {
            Some(i) => self.items.get_mut(i),
            None => {
                self.items.push(Item {
                    kind,
                    what: what.to_string(),
                    count: 0,
                    details: Vec::new(),
                });
                self.items.last_mut()
            }
        };
        let Some(item) = item else { return };
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
        s.audio_files = s.audio_files.saturating_add(o.audio_files);
        s.regions = s.regions.saturating_add(o.regions);
        s.notes = s.notes.saturating_add(o.notes);
        s.markers = s.markers.saturating_add(o.markers);
        s.automation_points = s.automation_points.saturating_add(o.automation_points);
        s.plugins = s.plugins.saturating_add(o.plugins);
        s.tempo_map |= o.tempo_map;
        for mut it in other.items {
            match self
                .items
                .iter_mut()
                .find(|i| i.kind == it.kind && i.what == it.what)
            {
                Some(mine) => {
                    mine.count = mine.count.saturating_add(it.count);
                    let room = MAX_DETAILS.saturating_sub(mine.details.len());
                    mine.details.extend(it.details.into_iter().take(room));
                }
                None => {
                    it.details.truncate(MAX_DETAILS);
                    self.items.push(it);
                }
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
            count(s.audio_files, "audio file", "audio files"),
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
        match parts.split_last() {
            None => format!("{verb} nothing"),
            Some((only, [])) => format!("{verb} {only}"),
            Some((last, rest)) => format!("{verb} {} and {last}", rest.join(", ")),
        }
    }

    /// The report as text, as the report sheet's Copy gives it: the summary
    /// line, then the groups "Conflicts", "Changed", "Left out" and "Notes"
    /// (empty ones omitted), each item as `"  <what>"` or
    /// `"  <what> (<count>)"` with its details as `"    <detail>"` and,
    /// after them, `"    and N more"` when it happened more often than it
    /// has details. With no
    /// items: the summary, then "Everything transferred.". No newline at
    /// the end.
    pub fn plain(&self, verb: &str) -> String {
        let mut lines = vec![self.summary_line(verb)];
        if self.is_empty() {
            lines.push("Everything transferred.".to_string());
            return lines.join("\n");
        }
        for (title, kind) in [
            ("Conflicts", ItemKind::Conflict),
            ("Changed", ItemKind::Changed),
            ("Left out", ItemKind::LeftOut),
            ("Notes", ItemKind::Note),
        ] {
            let mut items = self.items.iter().filter(|i| i.kind == kind).peekable();
            if items.peek().is_none() {
                continue;
            }
            lines.push(title.to_string());
            for i in items {
                lines.push(if i.count > 1 {
                    format!("  {} ({})", i.what, i.count)
                } else {
                    format!("  {}", i.what)
                });
                lines.extend(i.details.iter().map(|d| format!("    {d}")));
                let given = u64::try_from(i.details.len()).unwrap_or(u64::MAX);
                // "and N more" follows given details; an item kept without
                // any shows only its count.
                if given > 0 && i.count > given {
                    lines.push(format!("    and {} more", i.count - given));
                }
            }
        }
        lines.join("\n")
    }
}

/// `n` with thousands separated by a space.
pub(crate) fn group(n: u64) -> String {
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
