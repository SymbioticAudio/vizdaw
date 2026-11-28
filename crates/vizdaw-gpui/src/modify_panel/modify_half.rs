//! The panel's Modify half (`S/ModifyPanel.swift:249-392`): the
//! operation, In Place or To a Copy, the operation's own pickers and its
//! numeric fields. Every change goes through the document's
//! [`SelectModifyModel`]; this file only lays out. A Strip Chart line has
//! its target, operation and shape pickers (`S/LineModifyEditor.swift`).
//!
//! [`SelectModifyModel`]: vizdaw_app::select_modify::model::SelectModifyModel

use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, Window, div, px, rgb};
use vizdaw_app::editing_prefs::{ROOT_NAMES, ScaleKind};
use vizdaw_app::select_modify::{
    AmountKind, ChangeOpKind, ChangeTarget, CurveKind, GROOVE_HELP, GROOVE_LABEL, LINE_OPS,
    LINE_TARGET_BASICS, ModifyHalf, ModifyKind, NOTE_MAP_PLACEHOLDER, SubstituteChoice, WaveKind,
    line_target_controllers, line_target_octaves, modify_fields,
};
use vizdaw_app::strip_chart::{AutomationTargets, LineCurve, PeriodKind, ShapeKind, StripLane};
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::modify::{AmountRow, ChangeOpRow, ModifyRow};
use vizdaw_ffi::strip::{LineOpRow, LineRow, LineShapeRow, LineTargetRow};

use super::select_half::{DEST_WIDTH, DESTINATION, FIELD_WIDTH, dest_rows, dest_text};
use super::{ModifyPanel, PanelMenu};
use crate::cascade_menu::CascadeEntry;
use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// The labels (`S/ModifyPanel.swift:254-386`); each is also its control's
/// accessibility label.
pub const OPERATION: &str = "Operation";
pub const APPLY_MODE: &str = "Apply";
pub const IN_PLACE: &str = "In Place";
pub const TO_A_COPY: &str = "To a Copy";
pub const ROOT: &str = "Root";
pub const SCALE: &str = "Scale";
pub const NOTE_MAP: &str = "Note Map";
pub const VALUE_TO_CHANGE: &str = "Value to Change";
pub const AMOUNT: &str = "Amount";
pub const CURVE: &str = "Curve";
pub const WAVE: &str = "Wave";
pub const SUBSTITUTE_WITH: &str = "Substitute With";
/// The Strip Chart line's (`S/LineModifyEditor.swift:22-102`).
pub const LINE_TARGET: &str = "Line Target";
pub const SHAPE: &str = "Shape";
pub const PERIOD_KIND: &str = "Period Kind";
/// The Line Target's face for a plain value (`:19`).
pub const ANY_VALUE: &str = "Any Value";
/// Its submenus (`:28,33,42`).
pub const CONTROLLER: &str = "Controller";
pub const POLY_PRESSURE: &str = "Poly Pressure";
pub const AUTOMATION: &str = "Automation";

/// A Free line's caption (`S/LineModifyEditor.swift:98`).
pub fn drawn_line(points: usize) -> String {
    format!("Drawn line ({points} points)")
}

/// The widths (`S/ModifyPanel.swift:254-386`).
const OPERATION_WIDTH: f32 = 260.;
const APPLY_WIDTH: f32 = 200.;
const ROOT_WIDTH: f32 = 110.;
const SCALE_WIDTH: f32 = 190.;
const NOTE_MAP_WIDTH: f32 = 260.;
const GROOVE_WIDTH: f32 = 140.;
const TARGET_WIDTH: f32 = 140.;
const CHANGE_OP_WIDTH: f32 = 120.;
const AMOUNT_WIDTH: f32 = 130.;
const CURVE_WIDTH: f32 = 110.;
const WAVE_WIDTH: f32 = 100.;
const SUBSTITUTE_WIDTH: f32 = 150.;
/// The line's (`S/LineModifyEditor.swift:52-95`).
const LINE_TARGET_WIDTH: f32 = 210.;
const LINE_OP_WIDTH: f32 = 110.;
const SHAPE_WIDTH: f32 = 110.;
const LINE_CURVE_WIDTH: f32 = 100.;
const LINE_WAVE_WIDTH: f32 = 90.;
const PERIOD_KIND_WIDTH: f32 = 150.;

impl ModifyPanel {
    /// The Modify half, while Modify is on.
    pub(super) fn modify_half(
        &mut self,
        h: &ModifyHalf,
        dests: &[DestRow],
        targets: &AutomationTargets,
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut sc = scope.container();
        let kind = ModifyKind::of(&h.op);
        let operation = self.picker(
            PanelMenu::Operation,
            OPERATION,
            OPERATION_WIDTH,
            kind.title(),
            Self::choice_rows(
                &ModifyKind::OFFERED,
                Some(kind),
                |k| k.title().to_owned(),
                |d, k| d.set_modify_kind(k),
            ),
            &mut sc,
            window,
            cx,
        );
        let apply = Self::segments(
            APPLY_MODE,
            APPLY_WIDTH,
            &[IN_PLACE, TO_A_COPY],
            usize::from(h.to_copy),
            &mut sc,
            cx,
            |d, n| d.select_modify_mut().config.modify.to_copy = n == 1,
        );
        let mut line = div().flex().flex_row().flex_wrap().items_center().gap_1();
        for el in self.specials(&h.op, dests, targets, &mut sc, window, cx) {
            line = line.child(el);
        }
        for (j, f) in modify_fields(&h.op).iter().enumerate() {
            let field = self.text_field(
                &format!("m{j}"),
                f.label,
                f.label,
                FIELD_WIDTH,
                None,
                &mut sc,
                window,
                cx,
            );
            line = line.child(field);
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .items_center()
                    .gap_2()
                    .child(operation)
                    .child(apply),
            )
            .child(line)
            .into_any_element()
    }

    /// The operation's own pickers and fields, before its numbers.
    fn specials(
        &mut self,
        op: &ModifyRow,
        dests: &[DestRow],
        targets: &AutomationTargets,
        sc: &mut RegistryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut out = Vec::new();
        match op {
            ModifyRow::TransposeDiatonic { root, mask, .. }
            | ModifyRow::TransposeToScale { root, mask } => {
                let (root, scale) = (*root, ScaleKind::of_mask(*mask).unwrap_or_default());
                let roots: Vec<u8> = (0..12).collect();
                out.push(self.picker(
                    PanelMenu::Root,
                    ROOT,
                    ROOT_WIDTH,
                    ROOT_NAMES.get(usize::from(root)).copied().unwrap_or(""),
                    Self::choice_rows(
                        &roots,
                        Some(root),
                        |r| ROOT_NAMES[usize::from(r)].to_owned(),
                        move |d, r| d.set_modify_scale(r, scale),
                    ),
                    sc,
                    window,
                    cx,
                ));
                out.push(self.picker(
                    PanelMenu::Scale,
                    SCALE,
                    SCALE_WIDTH,
                    scale.title(),
                    Self::choice_rows(
                        &ScaleKind::ALL,
                        Some(scale),
                        |s| s.title().to_owned(),
                        move |d, s| d.set_modify_scale(root, s),
                    ),
                    sc,
                    window,
                    cx,
                ));
            }
            ModifyRow::NoteMap { .. } => {
                out.push(self.text_field(
                    "map",
                    NOTE_MAP_PLACEHOLDER,
                    NOTE_MAP,
                    NOTE_MAP_WIDTH,
                    None,
                    sc,
                    window,
                    cx,
                ));
            }
            ModifyRow::Quantize { .. } => {
                out.push(self.text_field(
                    "offsets",
                    GROOVE_LABEL,
                    GROOVE_LABEL,
                    GROOVE_WIDTH,
                    Some(GROOVE_HELP),
                    sc,
                    window,
                    cx,
                ));
            }
            ModifyRow::Change { target, op, amount } => {
                let target = ChangeTarget::of(*target);
                out.push(self.picker(
                    PanelMenu::ValueToChange,
                    VALUE_TO_CHANGE,
                    TARGET_WIDTH,
                    target.title(),
                    Self::choice_rows(
                        &ChangeTarget::ALL,
                        Some(target),
                        |t| t.title().to_owned(),
                        |d, t| d.set_modify_target(t.row()),
                    ),
                    sc,
                    window,
                    cx,
                ));
                let change = ChangeOpKind::of(op);
                out.push(self.picker(
                    PanelMenu::ChangeOp,
                    OPERATION,
                    CHANGE_OP_WIDTH,
                    change.title(),
                    Self::choice_rows(
                        &ChangeOpKind::ALL,
                        Some(change),
                        |k| k.title().to_owned(),
                        |d, k| d.set_modify_op(k),
                    ),
                    sc,
                    window,
                    cx,
                ));
                if matches!(op, ChangeOpRow::Set | ChangeOpRow::Scale | ChangeOpRow::Add) {
                    let a = AmountKind::of(amount);
                    out.push(self.picker(
                        PanelMenu::Amount,
                        AMOUNT,
                        AMOUNT_WIDTH,
                        a.title(),
                        Self::choice_rows(
                            &AmountKind::ALL,
                            Some(a),
                            |k| k.title().to_owned(),
                            |d, k| d.set_modify_amount(k),
                        ),
                        sc,
                        window,
                        cx,
                    ));
                }
                match amount {
                    AmountRow::Line { curve, .. } => {
                        let c = CurveKind::of(*curve);
                        out.push(self.picker(
                            PanelMenu::Curve,
                            CURVE,
                            CURVE_WIDTH,
                            c.title(),
                            Self::choice_rows(
                                &CurveKind::ALL,
                                Some(c),
                                |k| k.title().to_owned(),
                                |d, k| d.set_modify_curve(k.row()),
                            ),
                            sc,
                            window,
                            cx,
                        ));
                    }
                    AmountRow::Oscillator { wave, .. } => {
                        let w = WaveKind::of(*wave);
                        out.push(self.picker(
                            PanelMenu::Wave,
                            WAVE,
                            WAVE_WIDTH,
                            w.title(),
                            Self::choice_rows(
                                &WaveKind::ALL,
                                Some(w),
                                |k| k.title().to_owned(),
                                |d, k| d.set_modify_wave(k.row()),
                            ),
                            sc,
                            window,
                            cx,
                        ));
                    }
                    AmountRow::Constant { .. } | AmountRow::Random { .. } => {}
                }
            }
            ModifyRow::SetDest { dest } => {
                out.push(self.picker(
                    PanelMenu::Destination,
                    DESTINATION,
                    DEST_WIDTH,
                    &dest_text(*dest, dests),
                    dest_rows(*dest, dests, |d, x| d.set_modify_dest(x)),
                    sc,
                    window,
                    cx,
                ));
            }
            ModifyRow::Substitute { to } => {
                let s = SubstituteChoice::of(to);
                out.push(self.picker(
                    PanelMenu::Substitute,
                    SUBSTITUTE_WITH,
                    SUBSTITUTE_WIDTH,
                    s.title(),
                    Self::choice_rows(
                        &SubstituteChoice::ALL,
                        Some(s),
                        |k| k.title().to_owned(),
                        |d, k| d.set_modify_substitute(k.row()),
                    ),
                    sc,
                    window,
                    cx,
                ));
            }
            ModifyRow::Line { target, op, line } => {
                out.extend(self.line_editor(*target, *op, line, targets, sc, window, cx));
            }
            // The rest have only their numbers.
            _ => {}
        }
        out
    }

    /// The Strip Chart line's pickers (`S/LineModifyEditor.swift:22-102`):
    /// Line Target, Operation and Shape, then the shape's own; its numbers
    /// are the half's fields.
    #[allow(clippy::too_many_arguments)]
    fn line_editor(
        &mut self,
        target: LineTargetRow,
        op: LineOpRow,
        line: &LineRow,
        targets: &AutomationTargets,
        sc: &mut RegistryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let names = &targets.names;
        let face = StripLane::from_line_target(target)
            .map_or_else(|| ANY_VALUE.to_owned(), |l| l.title(names));
        let entries = if self.menu_open(PanelMenu::LineTarget) {
            self.line_target_entries(targets, cx)
        } else {
            Vec::new()
        };
        let mut out = vec![self.cascade_picker(
            PanelMenu::LineTarget,
            LINE_TARGET,
            LINE_TARGET_WIDTH,
            &face,
            entries,
            sc,
            cx,
        )];
        let op_title = |o: LineOpRow| {
            LINE_OPS
                .iter()
                .find(|(_, x)| *x == o)
                .map_or("", |(t, _)| t)
                .to_owned()
        };
        let ops: Vec<LineOpRow> = LINE_OPS.iter().map(|(_, o)| *o).collect();
        out.push(self.picker(
            PanelMenu::LineOp,
            OPERATION,
            LINE_OP_WIDTH,
            &op_title(op),
            Self::choice_rows(&ops, Some(op), op_title, |d, o| d.set_modify_line_op(o)),
            sc,
            window,
            cx,
        ));
        let shape = ShapeKind::of(&line.shape);
        out.push(self.picker(
            PanelMenu::LineShape,
            SHAPE,
            SHAPE_WIDTH,
            shape.title(),
            Self::choice_rows(
                &ShapeKind::ALL,
                Some(shape),
                |k| k.title().to_owned(),
                |d, k| d.set_modify_line_shape(k),
            ),
            sc,
            window,
            cx,
        ));
        match &line.shape {
            LineShapeRow::Curved { curve } => {
                let c = LineCurve::ALL.into_iter().find(|c| c.row() == *curve);
                out.push(self.picker(
                    PanelMenu::LineCurve,
                    CURVE,
                    LINE_CURVE_WIDTH,
                    c.map_or("", LineCurve::title),
                    Self::choice_rows(
                        &LineCurve::ALL,
                        c,
                        |k| k.title().to_owned(),
                        |d, k| d.set_modify_line_curve(k.row()),
                    ),
                    sc,
                    window,
                    cx,
                ));
            }
            LineShapeRow::Oscillator { wave, period, .. } => {
                let w = WaveKind::of(*wave);
                out.push(self.picker(
                    PanelMenu::LineWave,
                    WAVE,
                    LINE_WAVE_WIDTH,
                    w.title(),
                    Self::choice_rows(
                        &WaveKind::ALL,
                        Some(w),
                        |k| k.title().to_owned(),
                        |d, k| d.set_modify_line_wave(k.row()),
                    ),
                    sc,
                    window,
                    cx,
                ));
                let k = PeriodKind::of(period);
                out.push(self.picker(
                    PanelMenu::LinePeriodKind,
                    PERIOD_KIND,
                    PERIOD_KIND_WIDTH,
                    k.title(),
                    Self::choice_rows(
                        &PeriodKind::ALL,
                        Some(k),
                        |k| k.title().to_owned(),
                        |d, k| d.set_modify_line_period_kind(k),
                    ),
                    sc,
                    window,
                    cx,
                ));
            }
            LineShapeRow::Free { points } => {
                let t = tokens();
                let text = drawn_line(points.len());
                out.push(
                    div()
                        .relative()
                        .flex_none()
                        .px_1()
                        .text_size(px(11.))
                        .text_color(rgb(t.text_secondary))
                        .child(text.clone())
                        .child(sc.control("text", &text, true))
                        .into_any_element(),
                );
            }
            LineShapeRow::Straight | LineShapeRow::Flat | LineShapeRow::Random { .. } => {}
        }
        out
    }

    /// The Line Target menu (`S/LineModifyEditor.swift:23-51`): the basic
    /// values, then Controller ▸, Poly Pressure ▸ octaves ▸ and
    /// Automation ▸ groups ▸.
    fn line_target_entries(
        &self,
        targets: &AutomationTargets,
        cx: &Context<Self>,
    ) -> Vec<CascadeEntry> {
        let names = &targets.names;
        let item = |lane: StripLane| {
            self.cascade_item(
                lane.title(names),
                move |d| d.set_modify_line_target(lane),
                cx,
            )
        };
        let mut menu: Vec<CascadeEntry> = LINE_TARGET_BASICS.iter().map(|&l| item(l)).collect();
        menu.push(CascadeEntry::submenu(
            CONTROLLER,
            line_target_controllers().into_iter().map(item).collect(),
        ));
        menu.push(CascadeEntry::submenu(
            POLY_PRESSURE,
            line_target_octaves()
                .into_iter()
                .map(|(title, lanes)| {
                    CascadeEntry::submenu(title, lanes.into_iter().map(item).collect())
                })
                .collect(),
        ));
        let groups: Vec<CascadeEntry> = targets
            .menus
            .iter()
            .map(|g| {
                CascadeEntry::submenu(g.title.clone(), g.lanes.iter().map(|&l| item(l)).collect())
            })
            .collect();
        // An empty menu, as Swift's, cannot open.
        menu.push(CascadeEntry::Submenu {
            title: AUTOMATION.into(),
            enabled: !groups.is_empty(),
            help: None,
            items: groups,
        });
        menu
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use vizdaw_app::select_modify::model::FIX_FIELDS;
    use vizdaw_app::select_modify::{LINE_OPS, ModifyKind};
    use vizdaw_app::testing::mixer_fake;
    use vizdaw_ffi::edit::EditCommand;
    use vizdaw_ffi::events::AutomationTargetRow;
    use vizdaw_ffi::modify::{AmountRow, CurveRow, ModifyRow};
    use vizdaw_ffi::strip::{LineOpRow, LineTargetRow, SeriesRow};

    use super::super::tests::{
        Setup, click, click_node, edit_doc, has_node, labels, node, nodes, open_panel, panel_of,
        setup, setup_with, tree, type_into, with_doc,
    };
    use super::{AMOUNT, CURVE, LINE_TARGET, OPERATION, PERIOD_KIND, SHAPE, TO_A_COPY, WAVE};
    use crate::modify_panel::ModifyPanel;

    /// The SelectAndModify commands Apply sent so far.
    fn applied(s: &Setup) -> Vec<(Option<ModifyRow>, bool)> {
        let fake = s.made.lock().unwrap()[0].clone();
        fake.filtered_edits()
            .into_iter()
            .filter_map(|(c, _)| match c {
                EditCommand::SelectAndModify {
                    modify, to_copy, ..
                } => Some((modify, to_copy)),
                _ => None,
            })
            .collect()
    }

    #[gpui::test]
    fn typing_semitones_and_apply_sends_one_transpose(cx: &mut TestAppContext) {
        let s = setup(cx);
        let (n, doc) = open_panel(&s, cx, "modify.transpose");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", OPERATION)["value"], "Transpose");
        assert_eq!(node(&t, "textfield", "semitones")["value"], "0");
        type_into(&s, cx, n, "semitones", "12");
        // The field owns the keyboard while focused.
        let owners = with_doc(cx, doc, |d| d.text_owners().to_vec());
        assert!(
            owners.iter().any(|o| o == "selectModify.m0"),
            "owners: {owners:?}"
        );
        assert_eq!(
            node(&tree(&s, cx, n), "textfield", "semitones")["value"],
            "12"
        );
        click_node(&s, cx, n, "button", "Apply");
        assert_eq!(
            applied(&s),
            [(Some(ModifyRow::TransposeInterval { semitones: 12 }), false)]
        );
    }

    #[gpui::test]
    fn bad_text_turns_the_field_red_and_apply_says_fix_the_fields(cx: &mut TestAppContext) {
        let s = setup(cx);
        let (n, doc) = open_panel(&s, cx, "modify.transpose");
        type_into(&s, cx, n, "semitones", "up");
        assert!(with_doc(cx, doc, |d| d.modify_field_invalid("m0")));
        let panel = panel_of(cx, doc)
            .unwrap()
            .downcast::<ModifyPanel>()
            .unwrap();
        let red = cx.update(|cx| {
            panel
                .read(cx)
                .unwrap()
                .field_for_test("m0")
                .map(|f| f.read(cx).is_error())
        });
        assert_eq!(red, Some(true), "the field is red");
        click_node(&s, cx, n, "button", "Apply");
        assert!(applied(&s).is_empty(), "nothing sent");
        node(&tree(&s, cx, n), "text", FIX_FIELDS);
        // Good text again: no longer red, and Apply sends.
        type_into(&s, cx, n, "semitones", "-5");
        let red = cx.update(|cx| {
            panel
                .read(cx)
                .unwrap()
                .field_for_test("m0")
                .map(|f| f.read(cx).is_error())
        });
        assert_eq!(red, Some(false));
        click_node(&s, cx, n, "button", "Apply");
        assert_eq!(
            applied(&s),
            [(Some(ModifyRow::TransposeInterval { semitones: -5 }), false)]
        );
    }

    /// The panel's operation (the line, once picked).
    fn line_of(cx: &mut TestAppContext, doc: vizdaw_app::DocId) -> ModifyRow {
        with_doc(cx, doc, |d| d.select_modify().config.modify.op.clone())
    }

    #[gpui::test]
    fn the_operation_picker_offers_strip_chart_line_and_its_editor_edits_the_line(
        cx: &mut TestAppContext,
    ) {
        let s = setup(cx);
        let (n, doc) = open_panel(&s, cx, "modify.transpose");
        click_node(&s, cx, n, "menu", OPERATION);
        let t = tree(&s, cx, n);
        let offered: Vec<String> = ModifyKind::OFFERED
            .iter()
            .map(|k| k.title().to_owned())
            .collect();
        assert_eq!(labels(&t, "menuitem"), offered);
        click_node(&s, cx, n, "menuitem", "Strip Chart Line");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", OPERATION)["value"], "Strip Chart Line");
        assert_eq!(node(&t, "menu", LINE_TARGET)["value"], "Velocity");
        // The half's Operation, then the line's.
        let ops = nodes(&t, "menu", OPERATION);
        assert_eq!(ops.len(), 2);
        assert_eq!(ops[1]["value"], "Set");
        assert_eq!(node(&t, "menu", SHAPE)["value"], "Straight");
        for label in ["from", "to", "start", "end", "step"] {
            node(&t, "textfield", label);
        }
        // Line Target › Tempo.
        click_node(&s, cx, n, "menu", LINE_TARGET);
        click_node(&s, cx, n, "menuitem", "Tempo");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", LINE_TARGET)["value"], "Tempo");
        assert!(labels(&t, "menuitem").is_empty(), "the menu closed");
        // The line's Operation is its own picker.
        click(&s, cx, n, &nodes(&t, "menu", OPERATION)[1]["id"]);
        let t = tree(&s, cx, n);
        let ops: Vec<String> = LINE_OPS.iter().map(|(t, _)| (*t).to_owned()).collect();
        assert_eq!(labels(&t, "menuitem"), ops);
        click_node(&s, cx, n, "menuitem", "Add");
        assert!(
            matches!(
                line_of(cx, doc),
                ModifyRow::Line {
                    target: LineTargetRow::Tempo,
                    op: LineOpRow::Add,
                    ..
                }
            ),
            "{:?}",
            line_of(cx, doc)
        );
        // Shape › Oscillator: its wave, period kind and fields.
        click_node(&s, cx, n, "menu", SHAPE);
        click_node(&s, cx, n, "menuitem", "Oscillator");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", SHAPE)["value"], "Oscillator");
        node(&t, "textfield", "period");
        node(&t, "textfield", "phase (\u{2030})");
        assert_eq!(node(&t, "menu", WAVE)["value"], "Sine");
        assert_eq!(node(&t, "menu", PERIOD_KIND)["value"], "Musical");
        assert!(!has_node(&t, "menu", CURVE));
        click_node(&s, cx, n, "menu", WAVE);
        click_node(&s, cx, n, "menuitem", "Saw");
        click_node(&s, cx, n, "menu", PERIOD_KIND);
        click_node(&s, cx, n, "menuitem", "Clock");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", WAVE)["value"], "Saw");
        assert_eq!(node(&t, "menu", PERIOD_KIND)["value"], "Clock");
        node(&t, "textfield", "period (ms)");
        // Shape › Curved: its curve.
        click_node(&s, cx, n, "menu", SHAPE);
        click_node(&s, cx, n, "menuitem", "Curved");
        click_node(&s, cx, n, "menu", CURVE);
        let t = tree(&s, cx, n);
        assert_eq!(labels(&t, "menuitem"), ["Ease In", "Ease Out", "S-Curve"]);
        click_node(&s, cx, n, "menuitem", "S-Curve");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", CURVE)["value"], "S-Curve");
        assert!(!has_node(&t, "menu", WAVE));
        // Every picker is its own element.
        let panel = panel_of(cx, doc)
            .unwrap()
            .downcast::<ModifyPanel>()
            .unwrap();
        let ids = cx.update(|cx| panel.read(cx).unwrap().picker_ids.clone());
        let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len(), "duplicate picker ids: {ids:?}");
    }

    #[gpui::test]
    fn the_line_target_menu_reaches_a_poly_pressure_pitch_and_an_automation_target(
        cx: &mut TestAppContext,
    ) {
        let s = setup_with(cx, mixer_fake);
        let (n, doc) = open_panel(&s, cx, "modify.transpose");
        edit_doc(cx, doc, |d| {
            d.select_modify_mut().config.modify.op = ModifyKind::Line.default_op();
        });
        click_node(&s, cx, n, "menu", LINE_TARGET);
        let t = tree(&s, cx, n);
        assert_eq!(
            labels(&t, "menuitem"),
            [
                "Velocity",
                "Release Velocity",
                "Length",
                "Pitch Bend",
                "Channel Pressure",
                "Program",
                "Tempo",
                "Controller",
                "Poly Pressure",
                "Automation",
            ]
        );
        click_node(&s, cx, n, "menuitem", "Poly Pressure");
        click_node(&s, cx, n, "menuitem", "C4\u{2013}B4");
        click_node(&s, cx, n, "menuitem", "Poly Pressure C4");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", LINE_TARGET)["value"], "Poly Pressure C4");
        assert!(labels(&t, "menuitem").is_empty(), "the menu closed");
        click_node(&s, cx, n, "menu", LINE_TARGET);
        click_node(&s, cx, n, "menuitem", "Automation");
        click_node(&s, cx, n, "menuitem", "Synth");
        click_node(&s, cx, n, "menuitem", "Mute \u{b7} Synth");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", LINE_TARGET)["value"], "Mute \u{b7} Synth");
        assert!(
            matches!(
                line_of(cx, doc),
                ModifyRow::Line {
                    target: LineTargetRow::Series {
                        series: SeriesRow::Automation {
                            target: AutomationTargetRow::StripMute { strip: 1 }
                        }
                    },
                    ..
                }
            ),
            "{:?}",
            line_of(cx, doc)
        );
    }

    #[gpui::test]
    fn a_free_line_shows_its_point_count(cx: &mut TestAppContext) {
        let s = setup(cx);
        let (n, doc) = open_panel(&s, cx, "modify.transpose");
        edit_doc(cx, doc, |d| {
            d.select_modify_mut().config.modify.op = ModifyKind::Line.default_op();
        });
        click_node(&s, cx, n, "menu", SHAPE);
        click_node(&s, cx, n, "menuitem", "Free");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", SHAPE)["value"], "Free");
        node(&t, "text", "Drawn line (2 points)");
    }

    #[gpui::test]
    fn apply_sends_the_line_from_the_panel(cx: &mut TestAppContext) {
        let s = setup(cx);
        let (n, _) = open_panel(&s, cx, "modify.transpose");
        click_node(&s, cx, n, "menu", OPERATION);
        click_node(&s, cx, n, "menuitem", "Strip Chart Line");
        click_node(&s, cx, n, "menu", LINE_TARGET);
        click_node(&s, cx, n, "menuitem", "Tempo");
        click_node(&s, cx, n, "button", "Apply");
        let sent = applied(&s);
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert!(
            matches!(
                &sent[0],
                (
                    Some(ModifyRow::Line {
                        target: LineTargetRow::Tempo,
                        ..
                    }),
                    false
                )
            ),
            "{sent:?}"
        );
    }

    #[gpui::test]
    fn change_values_along_a_line_shows_from_to_and_curve(cx: &mut TestAppContext) {
        let s = setup(cx);
        let (n, doc) = open_panel(&s, cx, "modify.change");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", "Value to Change")["value"], "Velocity");
        assert_eq!(node(&t, "menu", AMOUNT)["value"], "Constant");
        node(&t, "textfield", "amount");
        assert!(!has_node(&t, "menu", CURVE));
        click_node(&s, cx, n, "menu", AMOUNT);
        click_node(&s, cx, n, "menuitem", "Along a Line");
        let t = tree(&s, cx, n);
        node(&t, "textfield", "from");
        node(&t, "textfield", "to");
        assert!(!has_node(&t, "textfield", "amount"));
        assert_eq!(node(&t, "menu", CURVE)["value"], "Straight");
        click_node(&s, cx, n, "menu", CURVE);
        click_node(&s, cx, n, "menuitem", "Ease In");
        let op = with_doc(cx, doc, |d| d.select_modify().config.modify.op.clone());
        assert!(
            matches!(
                op,
                ModifyRow::Change {
                    amount: AmountRow::Line {
                        curve: CurveRow::EaseIn,
                        ..
                    },
                    ..
                }
            ),
            "{op:?}"
        );
        assert_eq!(node(&tree(&s, cx, n), "menu", CURVE)["value"], "Ease In");
        // Oscillating swaps the Curve for a Wave.
        click_node(&s, cx, n, "menu", AMOUNT);
        click_node(&s, cx, n, "menuitem", "Oscillating");
        let t = tree(&s, cx, n);
        assert!(!has_node(&t, "menu", CURVE));
        assert_eq!(node(&t, "menu", "Wave")["value"], "Sine");
    }

    #[gpui::test]
    fn to_a_copy_sends_to_copy(cx: &mut TestAppContext) {
        let s = setup(cx);
        let (n, doc) = open_panel(&s, cx, "modify.transpose");
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "button", "In Place")["selected"], true);
        node(&t, "group", "Apply");
        click_node(&s, cx, n, "button", TO_A_COPY);
        assert!(with_doc(cx, doc, |d| d
            .select_modify()
            .config
            .modify
            .to_copy));
        assert_eq!(
            node(&tree(&s, cx, n), "button", TO_A_COPY)["selected"],
            true
        );
        click_node(&s, cx, n, "button", "Apply");
        assert_eq!(
            applied(&s),
            [(Some(ModifyRow::TransposeInterval { semitones: 0 }), true)]
        );
    }
}
