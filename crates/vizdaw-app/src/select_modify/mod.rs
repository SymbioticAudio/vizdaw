//! The Select and Modify panel's vocabulary: its tasks, the two halves of
//! its config, the presets, templates, pickers and titles (Swift
//! `SelectModify.swift`, `ModifyPanel.swift:244-273`). The FFI rows are the
//! data; this module only names, defaults and assembles them.

use vizdaw_ffi::edit::EditCommand;
use vizdaw_ffi::events::{AutomationTargetRow, EventKindRow};
use vizdaw_ffi::modify::{
    AmountRow, ChangeOpRow, ConditionRow, ConditionTest, CurveRow, ModifyRow, ModifyTemplateRow,
    RuleRow, SelectModeRow, SubstituteRow, ValueTargetRow, WaveRow,
};
use vizdaw_ffi::strip::{LineOpRow, LineRow, LineShapeRow, LineTargetRow, SeriesRow};

use crate::arrangement::PPQ;
use crate::editing_prefs::ScaleKind;

mod fields;
pub mod model;

pub use fields::{
    Edge, Field, FieldKind, GROOVE_HELP, GROOVE_LABEL, NOTE_MAP_PLACEHOLDER, condition_fields,
    condition_numbers, condition_with, field_text, modify_fields, modify_numbers, modify_with,
    note_map_text, offsets_text, parse_field, parse_note_map, parse_offsets,
};

/// The Modify Again help text (`M/SelectModifyModel.swift:59`).
pub const MODIFY_AGAIN_HELP: &str = "Modify Again changes the selection in place.";

/// The panel window's title before the document's (`S/ModifyPanel.swift:42`).
pub const PANEL_TITLE_PREFIX: &str = "Select and Modify \u{2014} ";

/// What a Modify menu command opens the panel for (`M/SelectModify.swift:4-17`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelTask {
    SelectByRule,
    Transpose,
    Quantize,
    Change,
    Move,
}

impl PanelTask {
    pub const ALL: [PanelTask; 5] = [
        PanelTask::SelectByRule,
        PanelTask::Transpose,
        PanelTask::Quantize,
        PanelTask::Change,
        PanelTask::Move,
    ];

    pub fn title(self) -> &'static str {
        match self {
            PanelTask::SelectByRule => "Select by Rule",
            PanelTask::Transpose => "Transpose",
            PanelTask::Quantize => "Quantize",
            PanelTask::Change => "Change Values",
            PanelTask::Move => "Move",
        }
    }

    /// The Modify menu's command for this task.
    pub fn command_id(self) -> &'static str {
        match self {
            PanelTask::SelectByRule => "modify.select_by_rule",
            PanelTask::Transpose => "modify.transpose",
            PanelTask::Quantize => "modify.quantize",
            PanelTask::Change => "modify.change",
            PanelTask::Move => "modify.move",
        }
    }
}

/// The select half (`M/SelectModify.swift:20-32`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectHalf {
    pub enabled: bool,
    pub match_all: bool,
    pub mode: SelectModeRow,
    pub conditions: Vec<ConditionRow>,
}

impl Default for SelectHalf {
    fn default() -> Self {
        SelectHalf {
            enabled: false,
            match_all: true,
            mode: SelectModeRow::Select,
            conditions: Vec::new(),
        }
    }
}

/// The modify half (`M/SelectModify.swift:35-46`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifyHalf {
    pub enabled: bool,
    pub op: ModifyRow,
    /// To a copy (a harmonizing copy for transposes).
    pub to_copy: bool,
}

impl Default for ModifyHalf {
    fn default() -> Self {
        ModifyHalf {
            enabled: true,
            op: ModifyRow::TransposeInterval { semitones: 0 },
            to_copy: false,
        }
    }
}

/// Everything the panel holds; a template stores exactly this
/// (`M/SelectModify.swift:49-104`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifyConfig {
    pub select: SelectHalf,
    pub modify: ModifyHalf,
}

impl Default for ModifyConfig {
    fn default() -> Self {
        ModifyConfig::preset(PanelTask::Change, None)
    }
}

impl ModifyConfig {
    /// The panel set to `task`. Select by Rule starts with Kind is Note and,
    /// when the selection has a range, a Position condition over it.
    pub fn preset(task: PanelTask, range: Option<(i64, i64)>) -> Self {
        let mut c = ModifyConfig {
            select: SelectHalf::default(),
            modify: ModifyHalf::default(),
        };
        match task {
            PanelTask::SelectByRule => {
                c.select.enabled = true;
                c.select.conditions = vec![ConditionRow {
                    test: ConditionTest::Kinds {
                        kinds: vec![EventKindRow::Note],
                    },
                    negate: false,
                }];
                if let Some((start, end)) = range {
                    c.select.conditions.push(ConditionRow {
                        test: ConditionTest::TimeRange { start, end },
                        negate: false,
                    });
                }
                c.modify.enabled = false;
            }
            PanelTask::Transpose => c.modify.op = ModifyRow::TransposeInterval { semitones: 0 },
            PanelTask::Quantize => {
                c.modify.op = ModifyRow::Quantize {
                    grid: PPQ / 4,
                    strength: 100,
                    offsets: Vec::new(),
                }
            }
            PanelTask::Change => c.modify.op = ModifyKind::Change.default_op(),
            PanelTask::Move => c.modify.op = ModifyRow::Move { delta: 0 },
        }
        c
    }

    /// The rule, when Select is on and has conditions.
    pub fn rule(&self) -> Option<RuleRow> {
        (self.select.enabled && !self.select.conditions.is_empty()).then(|| RuleRow {
            conditions: self.select.conditions.clone(),
            match_all: self.select.match_all,
        })
    }

    pub fn modification(&self) -> Option<ModifyRow> {
        self.modify.enabled.then(|| self.modify.op.clone())
    }

    /// What Apply sends; `None` when neither half has anything to do. A
    /// Strip Chart line is never applied to a copy.
    pub fn command(&self) -> Option<EditCommand> {
        let rule = self.rule();
        let modify = self.modification();
        if rule.is_none() && modify.is_none() {
            return None;
        }
        let to_copy = self.modify.to_copy
            && modify
                .as_ref()
                .is_some_and(|m| !matches!(m, ModifyRow::Line { .. }));
        Some(EditCommand::SelectAndModify {
            rule,
            mode: self.select.mode,
            modify,
            to_copy,
        })
    }

    pub fn from_template(t: &ModifyTemplateRow) -> Self {
        ModifyConfig {
            select: SelectHalf {
                enabled: t.rule.is_some(),
                match_all: t.rule.as_ref().is_none_or(|r| r.match_all),
                mode: t.mode,
                conditions: t
                    .rule
                    .as_ref()
                    .map(|r| r.conditions.clone())
                    .unwrap_or_default(),
            },
            modify: ModifyHalf {
                enabled: t.modify.is_some(),
                op: t
                    .modify
                    .clone()
                    .unwrap_or(ModifyRow::TransposeInterval { semitones: 0 }),
                to_copy: t.to_copy,
            },
        }
    }

    pub fn template(&self, name: &str) -> ModifyTemplateRow {
        ModifyTemplateRow {
            name: name.to_string(),
            rule: self.rule(),
            mode: self.select.mode,
            modify: self.modification(),
            to_copy: self.modify.to_copy,
        }
    }
}

/// The attributes a condition tests (`M/SelectModify.swift:107-169`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConditionKind {
    Time,
    Kind,
    Pitch,
    Velocity,
    ReleaseVelocity,
    Duration,
    Value,
    Controller,
    Destination,
    Metric,
    EveryNth,
    Bracketed,
    Automation,
}

impl ConditionKind {
    pub const ALL: [ConditionKind; 13] = [
        ConditionKind::Time,
        ConditionKind::Kind,
        ConditionKind::Pitch,
        ConditionKind::Velocity,
        ConditionKind::ReleaseVelocity,
        ConditionKind::Duration,
        ConditionKind::Value,
        ConditionKind::Controller,
        ConditionKind::Destination,
        ConditionKind::Metric,
        ConditionKind::EveryNth,
        ConditionKind::Bracketed,
        ConditionKind::Automation,
    ];

    /// The Add Condition menu: every kind but Automation Target, which needs
    /// the unported Strip Chart's target menus.
    pub const OFFERED: [ConditionKind; 12] = [
        ConditionKind::Time,
        ConditionKind::Kind,
        ConditionKind::Pitch,
        ConditionKind::Velocity,
        ConditionKind::ReleaseVelocity,
        ConditionKind::Duration,
        ConditionKind::Value,
        ConditionKind::Controller,
        ConditionKind::Destination,
        ConditionKind::Metric,
        ConditionKind::EveryNth,
        ConditionKind::Bracketed,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ConditionKind::Time => "Position",
            ConditionKind::Kind => "Kind",
            ConditionKind::Pitch => "Pitch",
            ConditionKind::Velocity => "Velocity",
            ConditionKind::ReleaseVelocity => "Release Velocity",
            ConditionKind::Duration => "Length",
            ConditionKind::Value => "Value",
            ConditionKind::Controller => "Controller",
            ConditionKind::Destination => "Destination",
            ConditionKind::Metric => "Metric Position",
            ConditionKind::EveryNth => "Every Nth",
            ConditionKind::Bracketed => "Inside a Note",
            ConditionKind::Automation => "Automation Target",
        }
    }

    pub fn of(t: &ConditionTest) -> Self {
        match t {
            ConditionTest::TimeRange { .. } => ConditionKind::Time,
            ConditionTest::Kinds { .. } => ConditionKind::Kind,
            ConditionTest::Pitch { .. } => ConditionKind::Pitch,
            ConditionTest::Velocity { .. } => ConditionKind::Velocity,
            ConditionTest::ReleaseVelocity { .. } => ConditionKind::ReleaseVelocity,
            ConditionTest::Duration { .. } => ConditionKind::Duration,
            ConditionTest::Value { .. } => ConditionKind::Value,
            ConditionTest::Controller { .. } => ConditionKind::Controller,
            ConditionTest::Dest { .. } => ConditionKind::Destination,
            ConditionTest::Metric { .. } => ConditionKind::Metric,
            ConditionTest::EveryNth { .. } => ConditionKind::EveryNth,
            ConditionTest::BracketedBy { .. } => ConditionKind::Bracketed,
            ConditionTest::Automation { .. } => ConditionKind::Automation,
        }
    }

    /// A new condition of this kind; Position covers `range` (else the first
    /// four 4/4 bars).
    pub fn default_test(self, range: Option<(i64, i64)>) -> ConditionTest {
        let q = PPQ;
        match self {
            ConditionKind::Time => {
                let (start, end) = range.unwrap_or((0, 16 * q));
                ConditionTest::TimeRange { start, end }
            }
            ConditionKind::Kind => ConditionTest::Kinds {
                kinds: vec![EventKindRow::Note],
            },
            ConditionKind::Pitch => ConditionTest::Pitch { low: 60, high: 72 },
            ConditionKind::Velocity => ConditionTest::Velocity {
                low: 0,
                high: 0xFFFF,
            },
            ConditionKind::ReleaseVelocity => ConditionTest::ReleaseVelocity {
                low: 0,
                high: 0xFFFF,
            },
            ConditionKind::Duration => ConditionTest::Duration { min: 0, max: q },
            ConditionKind::Value => ConditionTest::Value {
                low: 0,
                high: u32::MAX,
            },
            ConditionKind::Controller => ConditionTest::Controller { low: 7, high: 7 },
            ConditionKind::Destination => ConditionTest::Dest { dest: None },
            ConditionKind::Metric => ConditionTest::Metric {
                grid: q,
                offset: 0,
                tolerance: q / 16,
            },
            ConditionKind::EveryNth => ConditionTest::EveryNth { n: 2, phase: 0 },
            ConditionKind::Bracketed => ConditionTest::BracketedBy { low: 0, high: 127 },
            ConditionKind::Automation => ConditionTest::Automation {
                target: AutomationTargetRow::StripFader { strip: 0 },
            },
        }
    }
}

/// How a condition compares; "not" is the condition's `negate`
/// (`M/SelectModify.swift:173-224`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Comparison {
    Is,
    IsNot,
    Inside,
    Outside,
}

impl Comparison {
    pub const ALL: [Comparison; 4] = [
        Comparison::Is,
        Comparison::IsNot,
        Comparison::Inside,
        Comparison::Outside,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Comparison::Is => "is",
            Comparison::IsNot => "is not",
            Comparison::Inside => "inside",
            Comparison::Outside => "outside",
        }
    }

    pub fn negates(self) -> bool {
        matches!(self, Comparison::IsNot | Comparison::Outside)
    }

    /// Position offers inside and outside, the ranged kinds all four, the
    /// rest is and is not.
    pub fn choices(t: &ConditionTest) -> &'static [Comparison] {
        match ConditionKind::of(t) {
            ConditionKind::Time => &[Comparison::Inside, Comparison::Outside],
            ConditionKind::Pitch
            | ConditionKind::Velocity
            | ConditionKind::ReleaseVelocity
            | ConditionKind::Duration
            | ConditionKind::Value
            | ConditionKind::Controller
            | ConditionKind::Bracketed => &Self::ALL,
            _ => &[Comparison::Is, Comparison::IsNot],
        }
    }

    /// A range reads as "is" when both ends show the same text.
    pub fn of(c: &ConditionRow) -> Self {
        let choices = Self::choices(&c.test);
        if !choices.contains(&Comparison::Inside) {
            return if c.negate {
                Comparison::IsNot
            } else {
                Comparison::Is
            };
        }
        let f = condition_fields(&c.test);
        let n = condition_numbers(&c.test);
        let one_step = choices.contains(&Comparison::Is)
            && f.len() == 2
            && n.len() == 2
            && field_text(n[0], f[0].kind, &[]) == field_text(n[1], f[1].kind, &[]);
        match (one_step, c.negate) {
            (true, false) => Comparison::Is,
            (true, true) => Comparison::IsNot,
            (false, false) => Comparison::Inside,
            (false, true) => Comparison::Outside,
        }
    }

    /// The condition compared this way; "is" and "is not" on a range make
    /// its high end the same step as its low end.
    pub fn applied(self, c: &ConditionRow) -> ConditionRow {
        let mut out = c.clone();
        out.negate = self.negates();
        let f = condition_fields(&c.test);
        let n = condition_numbers(&c.test);
        if matches!(self, Comparison::Is | Comparison::IsNot)
            && Self::choices(&c.test).contains(&Comparison::Inside)
            && f.len() == 2
            && n.len() == 2
        {
            // Swift reads both ends with an empty meter; ranges never show
            // positions.
            let low = field_text(n[0], f[0].kind, &[]);
            let high = parse_field(&low, f[1].kind, &[]).unwrap_or(n[0]);
            if let Some(t) = condition_with(&c.test, &[n[0], high]) {
                out.test = t;
            }
        }
        out
    }
}

/// The panel's modify choices (`M/SelectModify.swift:228-298`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifyKind {
    Transpose,
    Diatonic,
    ToScale,
    Invert,
    NoteMap,
    Quantize,
    Change,
    Move,
    ScaleTime,
    TrimStart,
    SetDest,
    Substitute,
    Thin,
    Thicken,
    Delete,
    Line,
}

impl ModifyKind {
    pub const ALL: [ModifyKind; 16] = [
        ModifyKind::Transpose,
        ModifyKind::Diatonic,
        ModifyKind::ToScale,
        ModifyKind::Invert,
        ModifyKind::NoteMap,
        ModifyKind::Quantize,
        ModifyKind::Change,
        ModifyKind::Move,
        ModifyKind::ScaleTime,
        ModifyKind::TrimStart,
        ModifyKind::SetDest,
        ModifyKind::Substitute,
        ModifyKind::Thin,
        ModifyKind::Thicken,
        ModifyKind::Delete,
        ModifyKind::Line,
    ];

    /// The Operation picker: every kind but Strip Chart Line, which needs the
    /// unported Strip Chart.
    pub const OFFERED: [ModifyKind; 15] = [
        ModifyKind::Transpose,
        ModifyKind::Diatonic,
        ModifyKind::ToScale,
        ModifyKind::Invert,
        ModifyKind::NoteMap,
        ModifyKind::Quantize,
        ModifyKind::Change,
        ModifyKind::Move,
        ModifyKind::ScaleTime,
        ModifyKind::TrimStart,
        ModifyKind::SetDest,
        ModifyKind::Substitute,
        ModifyKind::Thin,
        ModifyKind::Thicken,
        ModifyKind::Delete,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ModifyKind::Transpose => "Transpose",
            ModifyKind::Diatonic => "Transpose in Scale",
            ModifyKind::ToScale => "Snap to Scale",
            ModifyKind::Invert => "Invert",
            ModifyKind::NoteMap => "Note Map",
            ModifyKind::Quantize => "Quantize",
            ModifyKind::Change => "Change Values",
            ModifyKind::Move => "Move",
            ModifyKind::ScaleTime => "Scale Time",
            ModifyKind::TrimStart => "Trim Start",
            ModifyKind::SetDest => "Set Destination",
            ModifyKind::Substitute => "Substitute",
            ModifyKind::Thin => "Thin Out",
            ModifyKind::Thicken => "Thicken",
            ModifyKind::Delete => "Delete",
            ModifyKind::Line => "Strip Chart Line",
        }
    }

    pub fn of(r: &ModifyRow) -> Self {
        match r {
            ModifyRow::TransposeInterval { .. } => ModifyKind::Transpose,
            ModifyRow::TransposeDiatonic { .. } => ModifyKind::Diatonic,
            ModifyRow::TransposeToScale { .. } => ModifyKind::ToScale,
            ModifyRow::TransposeInvert { .. } => ModifyKind::Invert,
            ModifyRow::NoteMap { .. } => ModifyKind::NoteMap,
            ModifyRow::Quantize { .. } => ModifyKind::Quantize,
            ModifyRow::Change { .. } => ModifyKind::Change,
            ModifyRow::Move { .. } => ModifyKind::Move,
            ModifyRow::ScaleTime { .. } => ModifyKind::ScaleTime,
            ModifyRow::TrimStart { .. } => ModifyKind::TrimStart,
            ModifyRow::SetDest { .. } => ModifyKind::SetDest,
            ModifyRow::Substitute { .. } => ModifyKind::Substitute,
            ModifyRow::Thin { .. } => ModifyKind::Thin,
            ModifyRow::Thicken { .. } => ModifyKind::Thicken,
            ModifyRow::Delete => ModifyKind::Delete,
            ModifyRow::Line { .. } => ModifyKind::Line,
        }
    }

    /// A new modification of this kind. Line is Swift's default line; the
    /// panel never offers it in this slice.
    pub fn default_op(self) -> ModifyRow {
        let q = PPQ;
        let major = ScaleKind::Major.mask();
        match self {
            ModifyKind::Transpose => ModifyRow::TransposeInterval { semitones: 0 },
            ModifyKind::Diatonic => ModifyRow::TransposeDiatonic {
                steps: 1,
                root: 0,
                mask: major,
            },
            ModifyKind::ToScale => ModifyRow::TransposeToScale {
                root: 0,
                mask: major,
            },
            ModifyKind::Invert => ModifyRow::TransposeInvert { axis: 60 },
            ModifyKind::NoteMap => ModifyRow::NoteMap { map: Vec::new() },
            ModifyKind::Quantize => ModifyRow::Quantize {
                grid: q / 4,
                strength: 100,
                offsets: Vec::new(),
            },
            ModifyKind::Change => ModifyRow::Change {
                target: ValueTargetRow::Velocity,
                op: ChangeOpRow::Add,
                amount: AmountRow::Constant { value: 0 },
            },
            ModifyKind::Move => ModifyRow::Move { delta: 0 },
            ModifyKind::ScaleTime => ModifyRow::ScaleTime {
                num: 1,
                den: 1,
                anchor: 0,
            },
            ModifyKind::TrimStart => ModifyRow::TrimStart { delta: 0 },
            ModifyKind::SetDest => ModifyRow::SetDest { dest: None },
            ModifyKind::Substitute => ModifyRow::Substitute {
                to: SubstituteChoice::Controller.row(),
            },
            ModifyKind::Thin => ModifyRow::Thin { spacing: q / 16 },
            ModifyKind::Thicken => ModifyRow::Thicken { spacing: q / 16 },
            ModifyKind::Delete => ModifyRow::Delete,
            ModifyKind::Line => ModifyRow::Line {
                target: LineTargetRow::Value {
                    target: ValueTargetRow::Velocity,
                },
                op: LineOpRow::Set,
                line: LineRow {
                    from: 0,
                    to: 4 * q,
                    start: 0,
                    end: 0xFFFF,
                    step: q / 8,
                    shape: LineShapeRow::Straight,
                },
            },
        }
    }
}

/// The step's name as the undo menu shows it (Swift `ModifyKind.title(_:)`
/// and `lineTitle`, `M/SelectModify.swift:300-340`).
pub fn modify_title(op: &ModifyRow) -> &'static str {
    match op {
        ModifyRow::TransposeInterval { .. }
        | ModifyRow::TransposeDiatonic { .. }
        | ModifyRow::TransposeToScale { .. }
        | ModifyRow::TransposeInvert { .. }
        | ModifyRow::NoteMap { .. } => "Transpose",
        ModifyRow::Quantize { .. } => "Quantize",
        ModifyRow::Change { target, .. } => match target {
            ValueTargetRow::Velocity => "Change Velocity",
            ValueTargetRow::ReleaseVelocity => "Change Release Velocity",
            ValueTargetRow::Duration => "Change Duration",
            ValueTargetRow::Value => "Change Values",
        },
        ModifyRow::Move { .. } => "Move",
        ModifyRow::ScaleTime { .. } => "Scale Time",
        ModifyRow::TrimStart { .. } => "Trim Start",
        ModifyRow::SetDest { .. } => "Reassign Destination",
        ModifyRow::Substitute { .. } => "Substitute",
        ModifyRow::Thin { .. } => "Thin Out",
        ModifyRow::Thicken { .. } => "Thicken",
        ModifyRow::Delete => "Delete Events",
        ModifyRow::Line { target, op, .. } => line_title(target, *op),
    }
}

fn line_title(t: &LineTargetRow, op: LineOpRow) -> &'static str {
    match (op, t) {
        (LineOpRow::Draw, LineTargetRow::Tempo) => "Draw Tempo",
        (
            LineOpRow::Draw,
            LineTargetRow::Series {
                series: SeriesRow::Automation { .. },
            },
        ) => "Draw Automation",
        (
            LineOpRow::Draw,
            LineTargetRow::Series {
                series: SeriesRow::Program,
            },
        ) => "Draw Program Changes",
        (LineOpRow::Draw, LineTargetRow::Series { .. }) => "Draw Controllers",
        (LineOpRow::Thin, _) => "Thin Out",
        (LineOpRow::Thicken, _) => "Thicken",
        (LineOpRow::Compress, _) => "Compress",
        (LineOpRow::Legato, _) => "Legato",
        (_, LineTargetRow::Tempo) => "Change Tempo",
        (_, LineTargetRow::Value { target }) => match target {
            ValueTargetRow::Velocity => "Change Velocity",
            ValueTargetRow::ReleaseVelocity => "Change Release Velocity",
            ValueTargetRow::Duration => "Change Duration",
            ValueTargetRow::Value => "Change Values",
        },
        _ => "Change Values",
    }
}

/// "Modify Again: ‹title›", else "Modify Again" (`M/SelectModifyModel.swift:84-86`).
pub fn modify_again_title(last: Option<&ModifyRow>) -> String {
    match last {
        Some(op) => format!("Modify Again: {}", modify_title(op)),
        None => "Modify Again".to_string(),
    }
}

/// `cmd` with every Random amount given `seed`; any other command as it was
/// (Swift `reseeded`, `M/SelectModifyModel.swift:214-219`).
pub fn reseeded(cmd: EditCommand, seed: u64) -> EditCommand {
    match cmd {
        EditCommand::SelectAndModify {
            rule,
            mode,
            modify:
                Some(ModifyRow::Change {
                    target,
                    op,
                    amount: AmountRow::Random { low, high, .. },
                }),
            to_copy,
        } => EditCommand::SelectAndModify {
            rule,
            mode,
            modify: Some(ModifyRow::Change {
                target,
                op,
                amount: AmountRow::Random { low, high, seed },
            }),
            to_copy,
        },
        other => other,
    }
}

/// Which value Change edits (`S/ModifyPanel.swift:335-343`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeTarget {
    Velocity,
    ReleaseVelocity,
    Length,
    Value,
}

impl ChangeTarget {
    pub const ALL: [ChangeTarget; 4] = [
        ChangeTarget::Velocity,
        ChangeTarget::ReleaseVelocity,
        ChangeTarget::Length,
        ChangeTarget::Value,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ChangeTarget::Velocity => "Velocity",
            ChangeTarget::ReleaseVelocity => "Release Velocity",
            ChangeTarget::Length => "Length",
            ChangeTarget::Value => "Value",
        }
    }

    pub fn of(t: ValueTargetRow) -> Self {
        match t {
            ValueTargetRow::Velocity => ChangeTarget::Velocity,
            ValueTargetRow::ReleaseVelocity => ChangeTarget::ReleaseVelocity,
            ValueTargetRow::Duration => ChangeTarget::Length,
            ValueTargetRow::Value => ChangeTarget::Value,
        }
    }

    pub fn row(self) -> ValueTargetRow {
        match self {
            ChangeTarget::Velocity => ValueTargetRow::Velocity,
            ChangeTarget::ReleaseVelocity => ValueTargetRow::ReleaseVelocity,
            ChangeTarget::Length => ValueTargetRow::Duration,
            ChangeTarget::Value => ValueTargetRow::Value,
        }
    }
}

/// The change operations (`M/SelectModify.swift:352-394`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeOpKind {
    Set,
    Scale,
    Add,
    Limit,
    Compress,
    Legato,
}

impl ChangeOpKind {
    pub const ALL: [ChangeOpKind; 6] = [
        ChangeOpKind::Set,
        ChangeOpKind::Scale,
        ChangeOpKind::Add,
        ChangeOpKind::Limit,
        ChangeOpKind::Compress,
        ChangeOpKind::Legato,
    ];

    pub fn title(self) -> &'static str {
        match self {
            ChangeOpKind::Set => "Set to",
            ChangeOpKind::Scale => "Scale by",
            ChangeOpKind::Add => "Add",
            ChangeOpKind::Limit => "Limit to",
            ChangeOpKind::Compress => "Compress to",
            ChangeOpKind::Legato => "Legato",
        }
    }

    pub fn of(op: &ChangeOpRow) -> Self {
        match op {
            ChangeOpRow::Set => ChangeOpKind::Set,
            ChangeOpRow::Scale => ChangeOpKind::Scale,
            ChangeOpRow::Add => ChangeOpKind::Add,
            ChangeOpRow::Limit { .. } => ChangeOpKind::Limit,
            ChangeOpRow::Compress { .. } => ChangeOpKind::Compress,
            ChangeOpRow::Legato => ChangeOpKind::Legato,
        }
    }

    /// The operation, with the target's full range for limits.
    pub fn row(self, target: ValueTargetRow) -> ChangeOpRow {
        let top: i64 = match target {
            ValueTargetRow::Velocity | ValueTargetRow::ReleaseVelocity => 0xFFFF,
            ValueTargetRow::Duration => 4 * PPQ,
            ValueTargetRow::Value => i64::from(u32::MAX),
        };
        match self {
            ChangeOpKind::Set => ChangeOpRow::Set,
            ChangeOpKind::Scale => ChangeOpRow::Scale,
            ChangeOpKind::Add => ChangeOpRow::Add,
            ChangeOpKind::Limit => ChangeOpRow::Limit { min: 0, max: top },
            ChangeOpKind::Compress => ChangeOpRow::Compress { low: 0, high: top },
            ChangeOpKind::Legato => ChangeOpRow::Legato,
        }
    }
}

/// The amount shapes (`M/SelectModify.swift:397-427`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmountKind {
    Constant,
    Line,
    Oscillator,
    Random,
}

impl AmountKind {
    pub const ALL: [AmountKind; 4] = [
        AmountKind::Constant,
        AmountKind::Line,
        AmountKind::Oscillator,
        AmountKind::Random,
    ];

    pub fn title(self) -> &'static str {
        match self {
            AmountKind::Constant => "Constant",
            AmountKind::Line => "Along a Line",
            AmountKind::Oscillator => "Oscillating",
            AmountKind::Random => "Random",
        }
    }

    pub fn of(a: &AmountRow) -> Self {
        match a {
            AmountRow::Constant { .. } => AmountKind::Constant,
            AmountRow::Line { .. } => AmountKind::Line,
            AmountRow::Oscillator { .. } => AmountKind::Oscillator,
            AmountRow::Random { .. } => AmountKind::Random,
        }
    }

    pub fn row(self, seed: u64) -> AmountRow {
        match self {
            AmountKind::Constant => AmountRow::Constant { value: 0 },
            AmountKind::Line => AmountRow::Line {
                from: 0,
                to: 0,
                curve: CurveRow::Straight,
            },
            AmountKind::Oscillator => AmountRow::Oscillator {
                wave: WaveRow::Sine,
                low: 0,
                high: 0,
                period: PPQ,
                phase: 0,
            },
            AmountKind::Random => AmountRow::Random {
                low: 0,
                high: 0,
                seed,
            },
        }
    }
}

/// A line amount's shape (`S/ModifyPanel.swift:363-367`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurveKind {
    Straight,
    EaseIn,
    EaseOut,
    SCurve,
}

impl CurveKind {
    pub const ALL: [CurveKind; 4] = [
        CurveKind::Straight,
        CurveKind::EaseIn,
        CurveKind::EaseOut,
        CurveKind::SCurve,
    ];

    pub fn title(self) -> &'static str {
        match self {
            CurveKind::Straight => "Straight",
            CurveKind::EaseIn => "Ease In",
            CurveKind::EaseOut => "Ease Out",
            CurveKind::SCurve => "S-Curve",
        }
    }

    pub fn of(c: CurveRow) -> Self {
        match c {
            CurveRow::Straight => CurveKind::Straight,
            CurveRow::EaseIn => CurveKind::EaseIn,
            CurveRow::EaseOut => CurveKind::EaseOut,
            CurveRow::SCurve => CurveKind::SCurve,
        }
    }

    pub fn row(self) -> CurveRow {
        match self {
            CurveKind::Straight => CurveRow::Straight,
            CurveKind::EaseIn => CurveRow::EaseIn,
            CurveKind::EaseOut => CurveRow::EaseOut,
            CurveKind::SCurve => CurveRow::SCurve,
        }
    }
}

/// An oscillating amount's wave (`S/ModifyPanel.swift:373-377`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WaveKind {
    Sine,
    Square,
    Triangle,
    Saw,
}

impl WaveKind {
    pub const ALL: [WaveKind; 4] = [
        WaveKind::Sine,
        WaveKind::Square,
        WaveKind::Triangle,
        WaveKind::Saw,
    ];

    pub fn title(self) -> &'static str {
        match self {
            WaveKind::Sine => "Sine",
            WaveKind::Square => "Square",
            WaveKind::Triangle => "Triangle",
            WaveKind::Saw => "Saw",
        }
    }

    pub fn of(w: WaveRow) -> Self {
        match w {
            WaveRow::Sine => WaveKind::Sine,
            WaveRow::Square => WaveKind::Square,
            WaveRow::Triangle => WaveKind::Triangle,
            WaveRow::Saw => WaveKind::Saw,
        }
    }

    pub fn row(self) -> WaveRow {
        match self {
            WaveKind::Sine => WaveRow::Sine,
            WaveKind::Square => WaveRow::Square,
            WaveKind::Triangle => WaveRow::Triangle,
            WaveKind::Saw => WaveRow::Saw,
        }
    }
}

/// Substitute's choices (`S/ModifyPanel.swift:244-273`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubstituteChoice {
    Controller,
    PitchBend,
    ChannelPressure,
    Pitch,
}

impl SubstituteChoice {
    pub const ALL: [SubstituteChoice; 4] = [
        SubstituteChoice::Controller,
        SubstituteChoice::PitchBend,
        SubstituteChoice::ChannelPressure,
        SubstituteChoice::Pitch,
    ];

    pub fn title(self) -> &'static str {
        match self {
            SubstituteChoice::Controller => "Controller",
            SubstituteChoice::PitchBend => "Pitch Bend",
            SubstituteChoice::ChannelPressure => "Channel Pressure",
            SubstituteChoice::Pitch => "One Pitch",
        }
    }

    pub fn of(s: &SubstituteRow) -> Self {
        match s {
            SubstituteRow::Controller { .. } => SubstituteChoice::Controller,
            SubstituteRow::PitchBend => SubstituteChoice::PitchBend,
            SubstituteRow::ChannelPressure => SubstituteChoice::ChannelPressure,
            SubstituteRow::Pitch { .. } => SubstituteChoice::Pitch,
        }
    }

    pub fn row(self) -> SubstituteRow {
        match self {
            SubstituteChoice::Controller => SubstituteRow::Controller { index: 1 },
            SubstituteChoice::PitchBend => SubstituteRow::PitchBend,
            SubstituteChoice::ChannelPressure => SubstituteRow::ChannelPressure,
            SubstituteChoice::Pitch => SubstituteRow::Pitch { pitch: 36 },
        }
    }
}
