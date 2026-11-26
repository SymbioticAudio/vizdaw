//! The transport bar as items on one row (Swift: `TransportItems.swift`):
//! the item tokens and layout, and how the row folds into the » menu.

/// A readout's size: large is the theme's title digits (today's bar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Small,
    Large,
}

impl Size {
    fn name(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Large => "large",
        }
    }
}

/// One item of the bar: a built-in item, or a command button
/// (`command:<id>[:<label>]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportItem {
    Cluster,
    Loop,
    Click,
    ReplaceOverdub,
    RecordOptions,
    CountIn,
    Summary,
    Bar(Size),
    Time(Size),
    Tempo(Size),
    Signature,
    PositionField,
    EditFields,
    PunchFields,
    DspLoad,
    Device,
    Faults,
    MainView,
    Panes,
    Space,
    Separator,
    Command { id: String, label: Option<String> },
}

/// What the row does with an item when it folds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Content,
    Space,
    Separator,
}

/// The longest label a command button takes, in characters.
pub const MAX_LABEL: usize = 32;
/// The longest command id a command button takes.
pub const MAX_COMMAND_ID: usize = 80;
/// The most items a bar holds.
pub const MAX_ITEMS: usize = 64;
/// The longest piece of hostile text an error quotes.
const QUOTED: usize = 40;

fn quote(s: &str) -> String {
    s.chars().take(QUOTED).collect()
}

impl TransportItem {
    /// The item's snake_case name (without size or command).
    pub fn name(&self) -> &'static str {
        match self {
            Self::Cluster => "cluster",
            Self::Loop => "loop",
            Self::Click => "click",
            Self::ReplaceOverdub => "replace_overdub",
            Self::RecordOptions => "record_options",
            Self::CountIn => "count_in",
            Self::Summary => "summary",
            Self::Bar(_) => "bar",
            Self::Time(_) => "time",
            Self::Tempo(_) => "tempo",
            Self::Signature => "signature",
            Self::PositionField => "position_field",
            Self::EditFields => "edit_fields",
            Self::PunchFields => "punch_fields",
            Self::DspLoad => "dsp_load",
            Self::Device => "device",
            Self::Faults => "faults",
            Self::MainView => "main_view",
            Self::Panes => "panes",
            Self::Space => "space",
            Self::Separator => "separator",
            Self::Command { .. } => "command",
        }
    }

    /// Whether the bar may hold the item more than once.
    pub fn repeats(&self) -> bool {
        matches!(self, Self::Space | Self::Separator | Self::Command { .. })
    }

    /// Items only the document window shows.
    pub fn document_only(&self) -> bool {
        matches!(self, Self::Faults | Self::MainView | Self::Panes)
    }

    /// How the row treats it: the cluster is fixed (the caller marks it).
    pub fn kind(&self) -> ItemKind {
        match self {
            Self::Space => ItemKind::Space,
            Self::Separator => ItemKind::Separator,
            _ => ItemKind::Content,
        }
    }

    /// Whether the item never folds into ».
    pub fn fixed(&self) -> bool {
        matches!(self, Self::Cluster)
    }

    /// `<id>[:large|:small]` or `command:<id>[:<label>]`, as the preference
    /// and the channel spell it. A readout's size is always spelled
    /// (`bar:large`); a label's "%" and "," are `%25` and `%2C`.
    pub fn token(&self) -> String {
        match self {
            Self::Bar(s) | Self::Time(s) | Self::Tempo(s) => {
                format!("{}:{}", self.name(), s.name())
            }
            Self::Command { id, label } => match label {
                Some(l) => format!("command:{id}:{}", l.replace('%', "%25").replace(',', "%2C")),
                None => format!("command:{id}"),
            },
            _ => self.name().to_string(),
        }
    }

    /// The item `raw` names, or why not (Swift's error text).
    pub fn parse(raw: &str) -> Result<Self, String> {
        let token = raw.trim_matches([' ', '\t']);
        let (name, rest) = match token.split_once(':') {
            Some((n, r)) => (n, Some(r)),
            None => (token, None),
        };
        if name.is_empty() {
            return Err("an empty item".into());
        }
        let sized = |f: fn(Size) -> Self| -> Result<Self, String> {
            match rest {
                None => Ok(f(Size::Large)),
                Some("large") => Ok(f(Size::Large)),
                Some("small") => Ok(f(Size::Small)),
                Some(other) => Err(format!("{name} has no size {}", quote(other))),
            }
        };
        let plain = |item: Self| -> Result<Self, String> {
            match rest {
                None => Ok(item),
                Some(_) => Err(format!("{name} has no size")),
            }
        };
        match name {
            "bar" => sized(Self::Bar),
            "time" => sized(Self::Time),
            "tempo" => sized(Self::Tempo),
            "command" => match rest {
                None => Err("command needs a command id: command:<id>".into()),
                Some(r) => Self::parse_command(r),
            },
            "cluster" => plain(Self::Cluster),
            "loop" => plain(Self::Loop),
            "click" => plain(Self::Click),
            "replace_overdub" => plain(Self::ReplaceOverdub),
            "record_options" => plain(Self::RecordOptions),
            "count_in" => plain(Self::CountIn),
            "summary" => plain(Self::Summary),
            "signature" => plain(Self::Signature),
            "position_field" => plain(Self::PositionField),
            "edit_fields" => plain(Self::EditFields),
            "punch_fields" => plain(Self::PunchFields),
            "dsp_load" => plain(Self::DspLoad),
            "device" => plain(Self::Device),
            "faults" => plain(Self::Faults),
            "main_view" => plain(Self::MainView),
            "panes" => plain(Self::Panes),
            "space" => plain(Self::Space),
            "separator" => plain(Self::Separator),
            _ => Err(format!("unknown item {}", quote(name))),
        }
    }

    fn parse_command(rest: &str) -> Result<Self, String> {
        let (id, label) = match rest.split_once(':') {
            Some((i, l)) => (i, Some(l)),
            None => (rest, None),
        };
        if !is_command_id(id) {
            return Err(format!("command has no command id {}", quote(id)));
        }
        let Some(label) = label else {
            return Ok(Self::Command {
                id: id.into(),
                label: None,
            });
        };
        let raw = label
            .replace("%2C", ",")
            .replace("%2c", ",")
            .replace("%25", "%");
        let trimmed = raw.trim();
        if trimmed.chars().count() > MAX_LABEL {
            return Err(format!(
                "command:{id}: a label is at most {MAX_LABEL} characters"
            ));
        }
        if trimmed.chars().any(char::is_control) {
            return Err(format!("command:{id}: a label has no control characters"));
        }
        if trimmed.is_empty() || trimmed != raw {
            return Err(format!(
                "command:{id}: a label is 1 to {MAX_LABEL} characters, not blank or padded"
            ));
        }
        Ok(Self::Command {
            id: id.into(),
            label: Some(raw),
        })
    }
}

/// Whether `id` is spelled as a registry id (lowercase words, digits, "."
/// and "_").
pub fn is_command_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_COMMAND_ID
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_')
}

/// The bar's items in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportLayout {
    pub items: Vec<TransportItem>,
}

impl TransportLayout {
    /// Today's bar, with Loop just after the cluster and Click after Loop;
    /// without Punch In and Out and Audio Device.
    pub fn standard() -> Self {
        use TransportItem::*;
        Self {
            items: vec![
                Cluster,
                Loop,
                Click,
                ReplaceOverdub,
                RecordOptions,
                Summary,
                Bar(Size::Large),
                Time(Size::Large),
                Tempo(Size::Large),
                Signature,
                PositionField,
                EditFields,
                Space,
                Faults,
                MainView,
                Panes,
            ],
        }
    }

    /// `ui.set_transport_bar`'s text: tokens split by ",", each
    /// `<id in snake_case>[:large|:small]`; exactly one `cluster`, at most
    /// 64 tokens; the error names the first bad token.
    pub fn parse(text: &str) -> Result<Self, String> {
        let tokens: Vec<&str> = text.split(',').collect();
        if tokens.len() > MAX_ITEMS {
            return Err(format!("at most {MAX_ITEMS} items"));
        }
        let items = tokens
            .into_iter()
            .map(TransportItem::parse)
            .collect::<Result<Vec<_>, _>>()?;
        Self::checked(items)
    }

    /// `items` as a layout, or why not: exactly one cluster, at most 64
    /// items, no built-in item twice but spaces, separators and command
    /// buttons, and every item one its own token spells.
    pub fn checked(items: Vec<TransportItem>) -> Result<Self, String> {
        if items.len() > MAX_ITEMS {
            return Err(format!("at most {MAX_ITEMS} items"));
        }
        for item in &items {
            let back = TransportItem::parse(&item.token())?;
            if &back != item {
                return Err(format!("{} is not an item", quote(&item.token())));
            }
        }
        if items
            .iter()
            .filter(|i| **i == TransportItem::Cluster)
            .count()
            != 1
        {
            return Err("the bar needs cluster once".into());
        }
        for (n, item) in items.iter().enumerate() {
            if !item.repeats() && items[..n].iter().any(|o| o.name() == item.name()) {
                return Err(format!("{} is in the bar twice", item.name()));
            }
        }
        Ok(Self { items })
    }

    /// Its text for `ui.set_transport_bar`.
    pub fn tokens(&self) -> String {
        self.items
            .iter()
            .map(TransportItem::token)
            .collect::<Vec<_>>()
            .join(",")
    }

    /// The items a window shows: the mixer window skips the document-only
    /// ones.
    pub fn items_for(&self, document_window: bool) -> Vec<&TransportItem> {
        self.items
            .iter()
            .filter(|i| document_window || !i.document_only())
            .collect()
    }
}

/// Which pieces of the row show and which fold into ».
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fit {
    /// Indexes of the pieces drawn, in bar order (hidden separators left out).
    pub shown: Vec<usize>,
    /// Content pieces folded into », in bar order.
    pub folded: Vec<usize>,
    /// Separators not drawn: by an edge, the », or another separator.
    pub hidden_separators: Vec<usize>,
}

fn clean(w: f32) -> f32 {
    if w.is_finite() && w > 0.0 { w } else { 0.0 }
}

/// Content folds last first until the shown pieces, their spacing and (when
/// anything folds) the » fit. `fixed` pieces never fold; spaces and
/// separators are never in `folded`. NaN or <= 0 width: everything but
/// fixed folds; +inf: nothing folds.
pub fn fit(widths: &[(f32, bool, ItemKind)], width: f32, spacing: f32, chevron: f32) -> Fit {
    let fixed_at = |i: usize| widths[i].1;
    let content: Vec<usize> = (0..widths.len())
        .filter(|&i| !fixed_at(i) && widths[i].2 == ItemKind::Content)
        .collect();
    let fixed: Vec<usize> = (0..widths.len()).filter(|&i| fixed_at(i)).collect();
    let finish = |shown: Vec<usize>, folded: Vec<usize>| {
        let hidden_separators = (0..widths.len())
            .filter(|i| widths[*i].2 == ItemKind::Separator && !fixed_at(*i) && !shown.contains(i))
            .collect();
        Fit {
            shown,
            folded,
            hidden_separators,
        }
    };
    if width.is_nan() || width <= 0.0 {
        return finish(fixed, content);
    }
    let all: Vec<usize> = (0..widths.len()).collect();
    if width == f32::INFINITY {
        return finish(tidied(&all, widths), vec![]);
    }
    let gap = clean(spacing);
    for k in 0..=content.len() {
        let folded: Vec<usize> = content[content.len() - k..].to_vec();
        let kept: Vec<usize> = all
            .iter()
            .copied()
            .filter(|i| !folded.contains(i))
            .collect();
        let shown = tidied(&kept, widths);
        let used: f32 = shown.iter().map(|&i| clean(widths[i].0)).sum::<f32>()
            + gap * shown.len().saturating_sub(1) as f32;
        let need = used + if k > 0 { gap + clean(chevron) } else { 0.0 };
        if need <= width {
            return finish(shown, folded);
        }
    }
    finish(fixed, content)
}

/// `shown` without the separators next to an edge, the » or another
/// separator.
fn tidied(shown: &[usize], widths: &[(f32, bool, ItemKind)]) -> Vec<usize> {
    let sep = |i: usize| widths[i].2 == ItemKind::Separator && !widths[i].1;
    shown
        .iter()
        .enumerate()
        .filter_map(|(n, &i)| {
            if !sep(i) {
                return Some(i);
            }
            let before = n.checked_sub(1).map(|b| shown[b]);
            let after = shown.get(n + 1).copied();
            match (before, after) {
                (Some(b), Some(a)) if !sep(b) && !sep(a) => Some(i),
                _ => None,
            }
        })
        .collect()
}
