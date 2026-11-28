//! The panel's Select half (`S/ModifyPanel.swift:122-214`): what the
//! matches do to the selection, whether all or any condition must hold,
//! one row per condition and the Add Condition menu. Every change goes
//! through the document's [`SelectModifyModel`]; this file only lays out.
//!
//! [`SelectModifyModel`]: vizdaw_app::select_modify::model::SelectModifyModel

use gpui::{AnyElement, Context, IntoElement, ParentElement, Styled, Window, div, px, rgb};
use vizdaw_app::list::ListKind;
use vizdaw_app::select_modify::{Comparison, ConditionKind, SelectHalf, condition_fields};
use vizdaw_ffi::events::EventKindRow;
use vizdaw_ffi::mixer::DestRow;
use vizdaw_ffi::modify::{ConditionRow, ConditionTest, SelectModeRow};

use super::{MenuRow, ModifyPanel, PanelMenu};
use crate::registry::RegistryScope;
use crate::strip_palette::tokens;

/// The pickers' labels (`S/ModifyPanel.swift:128-160`).
pub const RESULT: &str = "Result";
pub const MATCH: &str = "Match";
pub const ADD_CONDITION: &str = "Add Condition";
pub const COMPARISON: &str = "Comparison";
pub const KINDS: &str = "Kinds";
pub const DESTINATION: &str = "Destination";
pub const NO_KIND: &str = "No Kind";
pub const NO_DESTINATION: &str = "No Destination";

/// The Result picker's items (`:129-131`).
const MODES: [(SelectModeRow, &str); 3] = [
    (SelectModeRow::Select, "Replace the Selection"),
    (SelectModeRow::Add, "Add to the Selection"),
    (SelectModeRow::Refine, "Refine the Selection"),
];

/// The Match picker's items (`:135-136`).
const MATCHES: [(bool, &str); 2] = [(true, "All Conditions"), (false, "Any Condition")];

/// The kinds the Kind condition offers (Swift `PanelKinds.all`,
/// `M/SelectModify.swift:435-440`), by their list titles.
const PANEL_KINDS: [ListKind; 11] = [
    ListKind::Note,
    ListKind::Cc,
    ListKind::PitchBend,
    ListKind::ChannelPressure,
    ListKind::PolyPressure,
    ListKind::Program,
    ListKind::SysEx,
    ListKind::Text,
    ListKind::Reference,
    ListKind::AudioRegion,
    ListKind::Automation,
];

/// The widths (`S/ModifyPanel.swift:122-200`).
const RESULT_WIDTH: f32 = 280.;
const MATCH_WIDTH: f32 = 220.;
const ADD_WIDTH: f32 = 160.;
const KIND_TITLE_WIDTH: f32 = 120.;
const COMPARISON_WIDTH: f32 = 90.;
const KINDS_WIDTH: f32 = 220.;
pub(super) const DEST_WIDTH: f32 = 180.;
pub(super) const FIELD_WIDTH: f32 = 84.;

/// The list title of event kind `k`.
fn kind_title(k: EventKindRow) -> &'static str {
    ListKind::ALL
        .into_iter()
        .find(|l| l.event_kind() == Some(k))
        .map_or("", ListKind::title)
}

/// The Kinds menu's face: "No Kind" or the kinds' titles.
pub fn kinds_text(kinds: &[EventKindRow]) -> String {
    if kinds.is_empty() {
        return NO_KIND.into();
    }
    kinds
        .iter()
        .map(|&k| kind_title(k))
        .collect::<Vec<_>>()
        .join(", ")
}

/// A Destination picker's face: "No Destination" or the destination's name.
pub(super) fn dest_text(dest: Option<u64>, dests: &[DestRow]) -> String {
    match dest {
        None => NO_DESTINATION.into(),
        Some(id) => dests
            .iter()
            .find(|d| d.id == id)
            .map_or_else(String::new, |d| d.name.clone()),
    }
}

/// A Destination picker's rows: "No Destination", then the session's.
pub(super) fn dest_rows(
    current: Option<u64>,
    dests: &[DestRow],
    pick: impl Fn(&mut vizdaw_app::DocumentModel, Option<u64>) + 'static,
) -> Vec<MenuRow> {
    let items: Vec<Option<u64>> = std::iter::once(None)
        .chain(dests.iter().map(|d| Some(d.id)))
        .collect();
    let names = dests.to_vec();
    ModifyPanel::choice_rows(&items, Some(current), move |d| dest_text(d, &names), pick)
}

impl ModifyPanel {
    /// The Select half, while Select is on.
    pub(super) fn select_half(
        &mut self,
        h: &SelectHalf,
        dests: &[DestRow],
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut sc = scope.container();
        let mode = MODES
            .iter()
            .find(|(m, _)| *m == h.mode)
            .map_or("", |(_, t)| t);
        let result = self.picker(
            PanelMenu::Result,
            RESULT,
            RESULT_WIDTH,
            mode,
            Self::choice_rows(
                &MODES.map(|(m, _)| m),
                Some(h.mode),
                |m| {
                    MODES
                        .iter()
                        .find(|(x, _)| *x == m)
                        .map_or("", |(_, t)| t)
                        .to_owned()
                },
                |d, m| d.select_modify_mut().config.select.mode = m,
            ),
            &mut sc,
            window,
            cx,
        );
        let matching = MATCHES
            .iter()
            .find(|(m, _)| *m == h.match_all)
            .map_or("", |(_, t)| t);
        let match_picker = self.picker(
            PanelMenu::Match,
            MATCH,
            MATCH_WIDTH,
            matching,
            Self::choice_rows(
                &MATCHES.map(|(m, _)| m),
                Some(h.match_all),
                |m| {
                    MATCHES
                        .iter()
                        .find(|(x, _)| *x == m)
                        .map_or("", |(_, t)| t)
                        .to_owned()
                },
                |d, m| d.select_modify_mut().config.select.match_all = m,
            ),
            &mut sc,
            window,
            cx,
        );
        let mut col = div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_center()
                .gap_2()
                .child(result)
                .child(match_picker),
        );
        for (i, c) in h.conditions.iter().enumerate() {
            let row = self.condition_row(i, c, dests, &mut sc, window, cx);
            col = col.child(row);
        }
        let add = self.picker(
            PanelMenu::AddCondition,
            ADD_CONDITION,
            ADD_WIDTH,
            ADD_CONDITION,
            Self::choice_rows(
                &ConditionKind::OFFERED,
                None,
                |k| k.title().to_owned(),
                |d, k| d.add_modify_condition(k),
            ),
            &mut sc,
            window,
            cx,
        );
        col.child(add).into_any_element()
    }

    /// Condition `i`'s row: its kind, how it compares, its value and the
    /// remove button. An Automation Target condition (out of scope here)
    /// shows only its title and the remove button.
    fn condition_row(
        &mut self,
        i: usize,
        c: &ConditionRow,
        dests: &[DestRow],
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let t = tokens();
        let kind = ConditionKind::of(&c.test);
        let mut row = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .child(
                div()
                    .relative()
                    .flex_none()
                    .w(px(KIND_TITLE_WIDTH))
                    .text_color(rgb(t.text_primary))
                    .child(kind.title())
                    .child(scope.control("text", kind.title(), true)),
            );
        if kind != ConditionKind::Automation {
            let current = Comparison::of(c);
            let comparison = self.picker(
                PanelMenu::Comparison(i),
                COMPARISON,
                COMPARISON_WIDTH,
                current.title(),
                Self::choice_rows(
                    Comparison::choices(&c.test),
                    Some(current),
                    |x| x.title().to_owned(),
                    move |d, x| d.set_modify_comparison(i, x),
                ),
                scope,
                window,
                cx,
            );
            row = row.child(comparison);
        }
        match &c.test {
            ConditionTest::Kinds { kinds } => {
                let rows = PANEL_KINDS
                    .iter()
                    .filter_map(|l| l.event_kind().map(|k| (*l, k)))
                    .map(|(l, k)| {
                        let on = kinds.contains(&k);
                        let next: Vec<EventKindRow> = if on {
                            kinds.iter().copied().filter(|&x| x != k).collect()
                        } else {
                            kinds.iter().copied().chain([k]).collect()
                        };
                        let mut item = MenuRow::item(l.title(), move |this, _, cx| {
                            let next = next.clone();
                            this.with_doc(cx, |d| d.set_modify_kinds(i, next));
                        });
                        if let MenuRow::Item { checked, .. } = &mut item {
                            *checked = on;
                        }
                        item
                    })
                    .collect();
                let menu = self.picker(
                    PanelMenu::Kinds(i),
                    KINDS,
                    KINDS_WIDTH,
                    &kinds_text(kinds),
                    rows,
                    scope,
                    window,
                    cx,
                );
                row = row.child(menu);
            }
            ConditionTest::Dest { dest } => {
                let picker = self.picker(
                    PanelMenu::ConditionDest(i),
                    DESTINATION,
                    DEST_WIDTH,
                    &dest_text(*dest, dests),
                    dest_rows(*dest, dests, move |d, x| d.set_modify_condition_dest(i, x)),
                    scope,
                    window,
                    cx,
                );
                row = row.child(picker);
            }
            ConditionTest::Automation { .. } => {}
            test => {
                for (j, f) in condition_fields(test).iter().enumerate() {
                    let label = format!("{} {}", kind.title(), f.label);
                    let field = self.text_field(
                        &format!("c{i}.{j}"),
                        f.label,
                        &label,
                        FIELD_WIDTH,
                        None,
                        scope,
                        window,
                        cx,
                    );
                    row = row.child(field);
                }
            }
        }
        row.child(div().flex_1())
            .child(Self::remove_button(i, scope, cx))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use vizdaw_app::select_modify::ConditionKind;
    use vizdaw_ffi::events::AutomationTargetRow;
    use vizdaw_ffi::modify::{ConditionRow, ConditionTest};

    use super::super::tests::{
        click, click_node, edit_doc, has_node, labels, node, nodes, open_panel, setup, tree,
        with_doc,
    };
    use super::{ADD_CONDITION, KINDS};

    #[gpui::test]
    fn add_condition_offers_no_automation_target_and_remove_drops_the_row(cx: &mut TestAppContext) {
        let s = setup(cx);
        let (n, doc) = open_panel(&s, cx, "modify.select_by_rule");
        // Select by Rule: Select on, Kind is Note.
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "checkbox", "Select")["selected"], true);
        assert_eq!(node(&t, "menu", KINDS)["value"], "Note");
        assert_eq!(node(&t, "menu", "Result")["value"], "Replace the Selection");
        assert_eq!(node(&t, "menu", "Match")["value"], "All Conditions");
        assert_eq!(nodes(&t, "button", "Remove Condition").len(), 1);
        // The menu offers every kind but Automation Target.
        click_node(&s, cx, n, "menu", ADD_CONDITION);
        let t = tree(&s, cx, n);
        let offered: Vec<String> = ConditionKind::OFFERED
            .iter()
            .map(|k| k.title().to_owned())
            .collect();
        assert_eq!(labels(&t, "menuitem"), offered);
        assert!(!has_node(&t, "menuitem", "Automation Target"));
        let pitch = node(&t, "menuitem", "Pitch")["id"].clone();
        click(&s, cx, n, &pitch);
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "textfield", "Pitch from")["value"], "C4");
        assert_eq!(node(&t, "textfield", "Pitch to")["value"], "C5");
        assert_eq!(nodes(&t, "menu", "Comparison").len(), 2);
        // Removing the Pitch row drops it and its fields.
        let removes = nodes(&t, "button", "Remove Condition");
        assert_eq!(removes.len(), 2);
        click(&s, cx, n, &removes[1]["id"]);
        let t = tree(&s, cx, n);
        assert!(!has_node(&t, "textfield", "Pitch from"));
        assert_eq!(nodes(&t, "button", "Remove Condition").len(), 1);
        let kinds = with_doc(cx, doc, |d| {
            d.select_modify()
                .config
                .select
                .conditions
                .iter()
                .map(|c| ConditionKind::of(&c.test))
                .collect::<Vec<_>>()
        });
        assert_eq!(kinds, [ConditionKind::Kind]);
        // A loaded Automation Target condition shows its title and the
        // remove button only.
        edit_doc(cx, doc, |d| {
            d.select_modify_mut().config.select.conditions = vec![ConditionRow {
                test: ConditionTest::Automation {
                    target: AutomationTargetRow::StripFader { strip: 0 },
                },
                negate: false,
            }];
        });
        let t = tree(&s, cx, n);
        node(&t, "text", "Automation Target");
        assert!(!has_node(&t, "menu", "Comparison"));
        assert_eq!(nodes(&t, "button", "Remove Condition").len(), 1);
    }

    #[gpui::test]
    fn the_kinds_menu_toggles_kinds_and_the_result_picker_sets_the_mode(cx: &mut TestAppContext) {
        let s = setup(cx);
        let (n, doc) = open_panel(&s, cx, "modify.select_by_rule");
        click_node(&s, cx, n, "menu", KINDS);
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menuitem", "Note")["selected"], true);
        let cc = node(&t, "menuitem", "Controller")["id"].clone();
        click(&s, cx, n, &cc);
        let t = tree(&s, cx, n);
        assert_eq!(node(&t, "menu", KINDS)["value"], "Note, Controller");
        click_node(&s, cx, n, "menu", "Result");
        click_node(&s, cx, n, "menuitem", "Refine the Selection");
        let mode = with_doc(cx, doc, |d| d.select_modify().config.select.mode);
        assert_eq!(mode, vizdaw_ffi::modify::SelectModeRow::Refine);
        // Select off hides the half.
        click_node(&s, cx, n, "checkbox", "Select");
        let t = tree(&s, cx, n);
        assert!(!has_node(&t, "menu", "Result"));
        assert!(!with_doc(cx, doc, |d| d
            .select_modify()
            .config
            .select
            .enabled));
    }
}
