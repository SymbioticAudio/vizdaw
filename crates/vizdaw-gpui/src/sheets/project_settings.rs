//! File › Project Settings… (Swift `ProjectSettingsSheet`): the bold title
//! "Project Settings"; the `Format` section with the `Sample rate` and
//! `Bit depth` pop-ups and the note on what a format change converts; the
//! `Song info` section with its 12 single-line fields (the label is also
//! the placeholder) and `Comment`; `Cancel` and `OK`. OK sends only what
//! changed ([`ProjectSettings::apply`]): a song-info change is one undo
//! step, a format change the format job (its banner shows on the window).
//! While a held merge blocks a format change OK is off, with Swift's help.
//! Typing in a field turns the document's keys off (the sheet host's
//! per-window owner).

use gpui::{
    AnyElement, App, AppContext, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement,
    ParentElement, StatefulInteractiveElement, Styled, Window, div, px, rgb,
};
use vizdaw_app::DocId;
use vizdaw_app::formats::FormatChoice;
use vizdaw_app::project_settings::{
    BIT_DEPTH, CANCEL, COMMENT_LABEL, FIELDS, FORMAT_SECTION, HELD_HELP, OK, ProjectSettings,
    SAMPLE_RATE, SONG_SECTION, SongText, TITLE,
};

use crate::app_state::AppState;
use crate::format_panel::{FormatState, Popup, popup};
use crate::registry::RegistryScope;
use crate::sheet_host::{ButtonRole, Sheet, SheetButton};
use crate::text_input::TextInput;
use crate::transport_bar::colors;

/// The width of a field's label column.
const LABEL_WIDTH: f32 = 110.;
/// How tall the song-info list may grow before it scrolls.
const SONG_HEIGHT: f32 = 330.;

/// The Project Settings sheet of document `doc`.
pub struct ProjectSettingsSheet {
    doc: DocId,
    settings: ProjectSettings,
    /// The pop-ups' choice (the format panel's pop-ups).
    format: Entity<FormatState>,
    /// The 12 single-line fields, then Comment.
    fields: Vec<Entity<TextInput>>,
}

impl ProjectSettingsSheet {
    /// Loads document `doc`'s settings (a failed read is its alert
    /// "Project Settings Failed"; the sheet still shows, as Swift's).
    /// `None`: the document is gone.
    pub fn load(doc: DocId, cx: &mut App) -> Option<Self> {
        let app = AppState::global(cx);
        let rates = app.read(cx).store.core().project_rates();
        let settings = app.update(cx, |s, cx| {
            cx.notify();
            s.store.with_document(doc, |d, _| ProjectSettings::load(d))
        })?;
        // A project rate the core does not list stays choosable (a
        // fallback would change the format on OK).
        let mut rates = rates;
        if !rates.contains(&settings.rate) {
            rates.push(settings.rate);
            rates.sort_unstable();
        }
        let format = cx.new(|_| FormatState {
            choice: FormatChoice::new(rates, settings.format()),
            open: None,
        });
        let song = &settings.song;
        let mut fields: Vec<Entity<TextInput>> = FIELDS
            .iter()
            .map(|f| {
                let text = (f.get)(song).clone();
                cx.new(|cx| TextInput::new(f.label, cx).compact().with_text(&text))
            })
            .collect();
        let comment = song.comment.clone();
        fields.push(cx.new(|cx| {
            TextInput::new(COMMENT_LABEL, cx)
                .compact()
                .with_text(&comment)
        }));
        Some(ProjectSettingsSheet {
            doc,
            settings,
            format,
            fields,
        })
    }

    /// The settings with what the sheet shows now.
    fn current(&self, cx: &App) -> ProjectSettings {
        let mut s = self.settings.clone();
        let f = self.format.read(cx).choice.format;
        s.rate = f.sample_rate;
        s.depth = f.depth;
        let mut song = SongText::default();
        for (f, field) in FIELDS.iter().zip(&self.fields) {
            *(f.get_mut)(&mut song) = field.read(cx).text().to_owned();
        }
        song.comment = self.fields[FIELDS.len()].read(cx).text().to_owned();
        s.song = song;
        s
    }

    fn blocked(&self, cx: &App) -> bool {
        let app = AppState::global(cx).read(cx);
        app.store
            .document(self.doc)
            .is_some_and(|d| self.current(cx).format_blocked(d))
    }
}

/// A section's heading.
fn heading(text: &str, scope: &mut RegistryScope) -> AnyElement {
    div()
        .relative()
        .pt_1()
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(colors::DIM))
        .child(text.to_owned())
        .child(scope.control("text", text, true))
        .into_any_element()
}

/// A labelled row: the label column, then `control`.
fn row(label: &str, control: impl IntoElement) -> gpui::Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(LABEL_WIDTH))
                .flex_none()
                .text_color(rgb(colors::TEXT))
                .child(label.to_owned()),
        )
        .child(control)
}

impl Sheet for ProjectSettingsSheet {
    fn label(&self) -> String {
        TITLE.to_owned()
    }

    fn render(
        &mut self,
        scope: &mut RegistryScope,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let mut format_scope = scope.container();
        let rate = popup(Popup::Rate, &self.format, &mut format_scope, cx);
        let depth = popup(Popup::Depth, &self.format, &mut format_scope, cx);
        let note = self.settings.media_note.clone();
        let mut song_scope = scope.container();
        let clip = song_scope.clip();
        let labels = FIELDS
            .iter()
            .map(|f| (f.label, f.accessibility))
            .chain(std::iter::once((COMMENT_LABEL, COMMENT_LABEL)));
        let readable = self.settings.song_read();
        let rows: Vec<_> =
            labels
                .zip(&self.fields)
                .map(|((label, a11y), field)| {
                    let text = field.read(cx).text().to_owned();
                    let focused = field.focus_handle(cx).is_focused(window);
                    row(
                        label,
                        div().relative().flex_1().child(field.clone()).child(
                            song_scope.control_with(
                                "textfield",
                                a11y,
                                readable,
                                Some(&text),
                                focused,
                            ),
                        ),
                    )
                })
                .collect();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .relative()
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(TITLE)
                    .child(scope.control("text", TITLE, true)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(heading(FORMAT_SECTION, &mut format_scope))
                    .child(row(SAMPLE_RATE, rate))
                    .child(row(BIT_DEPTH, depth))
                    .child(
                        div()
                            .relative()
                            .text_xs()
                            .text_color(rgb(colors::DIM))
                            .child(note.clone())
                            .child(format_scope.control("text", &note, true)),
                    ),
            )
            .child(heading(SONG_SECTION, scope))
            .child(
                div()
                    .id("song-info")
                    .relative()
                    .max_h(px(SONG_HEIGHT))
                    .overflow_y_scroll()
                    .child(div().flex().flex_col().gap_1().children(rows))
                    .child(clip),
            )
            .into_any_element()
    }

    fn buttons(&self, cx: &App) -> Vec<SheetButton> {
        let ok = SheetButton::new(OK, ButtonRole::Default);
        vec![
            SheetButton::new(CANCEL, ButtonRole::Cancel),
            // A held merge holds a format change back.
            if self.blocked(cx) {
                ok.disabled().with_help(HELD_HELP)
            } else {
                ok
            },
        ]
    }

    fn focus_order(&self, cx: &App) -> Vec<FocusHandle> {
        // Song info that could not be read is not edited.
        if !self.settings.song_read() {
            return vec![];
        }
        self.fields.iter().map(|f| f.focus_handle(cx)).collect()
    }

    fn press(&mut self, index: usize, _: &mut Window, cx: &mut App) -> bool {
        if index == 1 {
            let mut settings = self.current(cx);
            let doc = self.doc;
            AppState::global(cx).update(cx, |s, cx| {
                s.store.with_document(doc, |d, _| settings.apply(d));
                cx.notify();
            });
            self.settings = settings;
        }
        true
    }
}

/// File › Project Settings…: the sheet on `view`'s window (none while
/// another sheet is up, or under the control channel).
pub fn show(
    view: &mut crate::shell::DocumentWindow,
    window: &mut Window,
    cx: &mut gpui::Context<crate::shell::DocumentWindow>,
) {
    if crate::control::channel_on(cx) || view.sheet_label().is_some() {
        return;
    }
    let Some(sheet) = ProjectSettingsSheet::load(view.doc(), cx) else {
        return;
    };
    let _ = view.show_sheet(Box::new(sheet), window, cx);
}
