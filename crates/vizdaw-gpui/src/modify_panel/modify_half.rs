//! The panel's Modify half (`S/ModifyPanel.swift:249-392`): the
//! operation, In Place or To a Copy, the operation's own pickers and its
//! numeric fields. Every change goes through the document's
//! [`SelectModifyModel`]; this file only lays out. A loaded Strip Chart
//! line (out of scope here) shows its title and no editor.
//!
//! [`SelectModifyModel`]: vizdaw_app::select_modify::model::SelectModifyModel

use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, Window, div};
use vizdaw_app::editing_prefs::{ROOT_NAMES, ScaleKind};
use vizdaw_app::select_modify::{
    AmountKind, ChangeOpKind, ChangeTarget, CurveKind, GROOVE_HELP, GROOVE_LABEL, ModifyHalf,
    ModifyKind, NOTE_MAP_PLACEHOLDER, SubstituteChoice, WaveKind, modify_fields,
};
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::modify::{AmountRow, ChangeOpRow, ModifyRow};

use super::select_half::{DEST_WIDTH, DESTINATION, FIELD_WIDTH, dest_rows, dest_text};
use super::{ModifyPanel, PanelMenu};
use crate::registry::RegistryScope;

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

impl ModifyPanel {
    /// The Modify half, while Modify is on.
    pub(super) fn modify_half(
        &mut self,
        h: &ModifyHalf,
        dests: &[DestRow],
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
        for el in self.specials(&h.op, dests, &mut sc, window, cx) {
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
            // The Strip Chart line has no editor in this slice; the rest
            // have only their numbers.
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use vizdaw_app::select_modify::ModifyKind;
    use vizdaw_app::select_modify::model::FIX_FIELDS;
    use vizdaw_ffi::edit::EditCommand;
    use vizdaw_ffi::modify::{AmountRow, CurveRow, ModifyRow};

    use super::super::tests::{
        Setup, click_node, edit_doc, has_node, labels, node, open_panel, panel_of, setup, tree,
        type_into, with_doc,
    };
    use super::{AMOUNT, CURVE, OPERATION, TO_A_COPY};
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

    #[gpui::test]
    fn the_operation_picker_offers_every_kind(cx: &mut TestAppContext) {
        let s = setup(cx);
        let (n, doc) = open_panel(&s, cx, "modify.transpose");
        click_node(&s, cx, n, "menu", OPERATION);
        let t = tree(&s, cx, n);
        let offered: Vec<String> = ModifyKind::OFFERED
            .iter()
            .map(|k| k.title().to_owned())
            .collect();
        assert_eq!(labels(&t, "menuitem"), offered);
        assert!(has_node(&t, "menuitem", "Strip Chart Line"));
        // Quantize: its groove field and numbers.
        click_node(&s, cx, n, "menuitem", "Quantize");
        let t = tree(&s, cx, n);
        node(&t, "textfield", "Groove offsets");
        node(&t, "textfield", "grid");
        node(&t, "textfield", "strength");
        assert!(!has_node(&t, "textfield", "semitones"));
        // A loaded Strip Chart line: its title and its fields (slice 10c
        // Task 7; its pickers are the GPUI line editor's).
        edit_doc(cx, doc, |d| {
            d.select_modify_mut().config.modify.op = ModifyKind::Line.default_op();
        });
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", OPERATION)["value"], "Strip Chart Line");
        for label in ["from", "to", "start", "end", "step"] {
            node(&t, "textfield", label);
        }
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
