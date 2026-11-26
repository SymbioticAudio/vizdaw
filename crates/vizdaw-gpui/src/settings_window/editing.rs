//! Settings › Editing (Swift `EditingSettingsView.swift`, 460 wide): the
//! two nudge steps, the default crossfade, zero-crossing snapping and the
//! external sample editor. A change goes to the app's [`EditingPrefs`]
//! and is saved in the preferences; a failed write shows on the tab's
//! problem line. Nothing reads these settings yet.
//!
//! [`EditingPrefs`]: vizdaw_app::editing_prefs::EditingPrefs

use std::path::PathBuf;
use std::rc::Rc;

use gpui::{AnyElement, App, Context, IntoElement, ParentElement, Styled, Window, div, px};
use vizdaw_app::Prefs;
use vizdaw_app::editing_prefs::{
    AMOUNT_MAX, AMOUNT_MIN, CROSSFADE_MAX, CROSSFADE_MIN, EDITOR_PANEL_MESSAGE, EditingPrefs,
    NudgeSettings, NudgeTime, ROOT_NAMES, ScaleKind, TICKS_DEFAULT, TICKS_MAX, TICKS_MIN,
    TICKS_STEP, ZERO_CROSSINGS_HELP, ZERO_CROSSINGS_TITLE, editor_display_name, ms_text,
    ticks_text,
};
use vizdaw_app::editor::Division;
use vizdaw_app::panels::{FileType, PanelKind, PanelSpec, StartFolder};

use super::keys::button;
use super::{AudioMenu, Choice, SettingsWindow, caption, form_row, section, switch, text};
use crate::app_state::AppState;
use crate::controls::stepper::{Range, stepper_buttons};
use crate::file_panels::{self, PanelPlace};
use crate::registry::RegistryScope;

/// The Editing tab's width (Swift's Form frame).
pub const EDITING_WIDTH: f32 = 460.;

/// Swift's `ExternalEditorPanel` (`ExternalEditing.swift:99-112`): one
/// application, chosen in `/Applications`. On Linux any file is
/// accepted and the panel starts where the system does.
pub const EDITOR_PANEL: PanelSpec = PanelSpec {
    row: "editing-external-editor",
    kind: PanelKind::Open,
    prompt: "Choose",
    message: Some(EDITOR_PANEL_MESSAGE),
    types: if cfg!(target_os = "macos") {
        &[FileType::Application]
    } else {
        &[]
    },
    multiple: false,
    can_create_folders: false,
    start: if cfg!(target_os = "macos") {
        StartFolder::Given
    } else {
        StartFolder::System
    },
    default_name: None,
    save_extension: None,
};

/// Changes the editing settings and saves them (a failed write is the
/// tab's problem line).
fn edit(cx: &mut App, f: impl FnOnce(&mut EditingPrefs, &mut Prefs)) {
    AppState::global(cx).update(cx, |s, cx| {
        f(&mut s.editing, &mut s.prefs);
        cx.notify();
    });
}

/// Changes the nudge settings with `f`.
fn nudge(cx: &mut App, f: impl FnOnce(&mut NudgeSettings)) {
    edit(cx, |e, p| {
        let mut n = e.nudge().clone();
        f(&mut n);
        let _ = e.set_nudge(n, p);
    });
}

/// The three time steps' titles.
const TIME_STEPS: [&str; 3] = ["Edit Grid", "Note Value", "Ticks"];

fn choice(title: &str, checked: bool, pick: impl Fn(&mut Window, &mut App) + 'static) -> Choice {
    Choice {
        title: title.to_owned(),
        checked,
        pick: Rc::new(pick),
    }
}

impl SettingsWindow {
    pub(super) fn editing_tab(
        &mut self,
        scope: &mut RegistryScope,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (n, crossfade, zero, editor, problem) = {
            let e = &AppState::global(cx).read(cx).editing;
            (
                e.nudge().clone(),
                e.crossfade_ms(),
                e.zero_crossings(),
                e.external_editor().map(|p| p.to_owned()),
                e.problem().map(str::to_owned),
            )
        };

        // Nudge in Time.
        let time_face = match n.time {
            NudgeTime::Grid => TIME_STEPS[0],
            NudgeTime::Division(..) => TIME_STEPS[1],
            NudgeTime::Ticks(_) => TIME_STEPS[2],
        };
        let time_items = TIME_STEPS
            .iter()
            .enumerate()
            .map(|(i, title)| {
                choice(title, title == &time_face, move |_, cx| {
                    nudge(cx, |n| {
                        // Switching sets 1/16, or 240 ticks (Swift :21-27).
                        let same = matches!(
                            (i, n.time),
                            (0, NudgeTime::Grid)
                                | (1, NudgeTime::Division(..))
                                | (2, NudgeTime::Ticks(_))
                        );
                        if !same {
                            n.time = match i {
                                0 => NudgeTime::Grid,
                                1 => NudgeTime::Division(Division::Sixteenth, false),
                                _ => NudgeTime::Ticks(TICKS_DEFAULT),
                            };
                        }
                    });
                })
            })
            .collect();
        let time_step = self.picker(
            "Nudge in time step",
            time_face,
            AudioMenu::TimeStep,
            time_items,
            scope,
            window,
            cx,
        );
        let mut body = div().flex().flex_col().gap(px(6.)).p_4();
        body = body
            .child(section("Nudge in Time", scope))
            .child(form_row("Step", time_step, scope));
        match n.time {
            NudgeTime::Division(d, triplet) => {
                let items = Division::ALL
                    .iter()
                    .map(|&v| {
                        choice(v.title(), v == d, move |_, cx| {
                            nudge(cx, |n| n.time = NudgeTime::Division(v, triplet));
                        })
                    })
                    .collect();
                let face = self.picker(
                    "Note Value",
                    d.title(),
                    AudioMenu::NoteValue,
                    items,
                    scope,
                    window,
                    cx,
                );
                let tri = switch(
                    "settings-nudge-triplet",
                    "Triplet",
                    triplet,
                    true,
                    scope,
                    move |_, cx| nudge(cx, |n| n.time = NudgeTime::Division(d, !triplet)),
                );
                body = body.child(form_row("Note Value", face, scope)).child(tri);
            }
            NudgeTime::Ticks(t) => {
                let buttons = stepper_buttons(
                    "Ticks",
                    Range {
                        min: i64::from(TICKS_MIN),
                        max: i64::from(TICKS_MAX),
                        step: i64::from(TICKS_STEP),
                    },
                    i64::from(t),
                    scope,
                    |v, _, cx| nudge(cx, |n| n.time = NudgeTime::Ticks(v as u32)),
                );
                body = body.child(Self::stepper_row(&ticks_text(t), buttons, scope));
            }
            NudgeTime::Grid => {}
        }

        // Nudge in Pitch.
        let steps = n.pitch_scale_steps;
        let pitch_face = if steps { "Scale Steps" } else { "Semitones" };
        let pitch_items = [("Semitones", false), ("Scale Steps", true)]
            .into_iter()
            .map(|(title, on)| {
                choice(title, on == steps, move |_, cx| {
                    nudge(cx, |n| n.pitch_scale_steps = on);
                })
            })
            .collect();
        let pitch_step = self.picker(
            "Nudge in pitch step",
            pitch_face,
            AudioMenu::PitchStep,
            pitch_items,
            scope,
            window,
            cx,
        );
        let amount = stepper_buttons(
            "Amount",
            Range {
                min: i64::from(AMOUNT_MIN),
                max: i64::from(AMOUNT_MAX),
                step: 1,
            },
            i64::from(n.amount),
            scope,
            |v, _, cx| nudge(cx, |n| n.amount = v as u8),
        );
        let follow = n.follow_key_signature;
        let follow_row = switch(
            "settings-nudge-follow",
            "Follow the sequence's key signature",
            follow,
            steps,
            scope,
            move |_, cx| nudge(cx, |n| n.follow_key_signature = !follow),
        );
        let own_key = steps && !follow;
        let root_items = ROOT_NAMES
            .iter()
            .enumerate()
            .map(|(i, name)| {
                choice(name, i == usize::from(n.root), move |_, cx| {
                    nudge(cx, |n| n.root = i as u8);
                })
            })
            .collect();
        let root = self.picker_enabled(
            "Root",
            ROOT_NAMES[usize::from(n.root % 12)],
            AudioMenu::Root,
            root_items,
            own_key,
            scope,
            window,
            cx,
        );
        let scale_items = ScaleKind::ALL
            .iter()
            .map(|&s| {
                choice(s.title(), s == n.scale, move |_, cx| {
                    nudge(cx, |n| n.scale = s);
                })
            })
            .collect();
        let scale = self.picker_enabled(
            "Scale",
            n.scale.title(),
            AudioMenu::Scale,
            scale_items,
            own_key,
            scope,
            window,
            cx,
        );
        body = body
            .child(section("Nudge in Pitch", scope))
            .child(form_row("Step", pitch_step, scope))
            .child(Self::stepper_row(
                &format!("Amount: {}", n.amount),
                amount,
                scope,
            ))
            .child(follow_row)
            .child(form_row("Root", root, scope))
            .child(form_row("Scale", scale, scope));

        // Crossfades.
        let fade = stepper_buttons(
            "Default crossfade",
            Range {
                min: CROSSFADE_MIN as i64,
                max: CROSSFADE_MAX as i64,
                step: 1,
            },
            crossfade as i64,
            scope,
            |v, _, cx| {
                edit(cx, move |e, p| {
                    let _ = e.set_crossfade_ms(v as f64, p);
                });
            },
        );
        let zero_row = switch(
            "settings-zero-crossings",
            ZERO_CROSSINGS_TITLE,
            zero,
            true,
            scope,
            move |_, cx| {
                edit(cx, move |e, p| {
                    let _ = e.set_zero_crossings(!zero, p);
                });
            },
        );
        body = body
            .child(section("Crossfades", scope))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().child(text("Default crossfade", scope)))
                    .child(text(&ms_text(crossfade), scope))
                    .child(fade),
            )
            .child(zero_row)
            .child(caption(ZERO_CROSSINGS_HELP, scope));

        // External Sample Editor.
        let name = editor
            .as_deref()
            .map_or_else(|| "None".to_owned(), editor_display_name);
        let me = cx.entity().downgrade();
        let choose = button("Choose…", true, scope, move |window, cx| {
            me.update(cx, |t, cx| t.choose_editor(window, cx)).ok();
        });
        let clear = button("Clear", editor.is_some(), scope, |_, cx| {
            edit(cx, |e, p| {
                let _ = e.set_external_editor(None, p);
            });
        });
        body = body
            .child(section("External Sample Editor", scope))
            .child(form_row("Application", text(&name, scope), scope))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .child(choose)
                    .child(clear),
            );
        if let Some(p) = Self::problem_line(problem, scope) {
            body = body.child(p);
        }
        body.into_any_element()
    }

    /// Choose…: the application the panel gives becomes the editor.
    fn choose_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let place = PanelPlace {
            directory: cfg!(target_os = "macos").then(|| PathBuf::from("/Applications")),
            ..Default::default()
        };
        let chosen = file_panels::choose(&EDITOR_PANEL, place, Some(window.window_handle()), cx);
        cx.spawn(async move |_, cx| {
            if let Ok(Some(paths)) = chosen.await
                && let Some(path) = paths.into_iter().next()
            {
                cx.update(|cx| {
                    edit(cx, move |e, p| {
                        let _ = e.set_external_editor(Some(path), p);
                    })
                });
            }
        })
        .detach();
    }
}
