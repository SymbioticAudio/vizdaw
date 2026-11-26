//! The bars above a document window's editors (Swift `ProjectBanners`):
//! one per running job (its progress, 120 wide, and Cancel), media not in
//! the project format (Convert), a device at another rate (Change Project
//! Rate), disk underruns and a finished job's problems (Dismiss). The
//! model ([`DocumentModel::banners`]) gives each bar's text, button and
//! action; this draws them, top first, as plain rows in the window's
//! column, so they take their height from this frame's layout and the
//! pane host below gets what is left.
//!
//! While a merge is held, Convert and the rate change are off with the
//! help `Finish or cancel the DAWproject merge waiting to be confirmed
//! first.` (Swift `MergeModel.heldHelp`), as their tooltip and their
//! `value` in the tree.

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div, px, relative, rgb,
};
use vizdaw_app::DocumentModel;
use vizdaw_app::banners::ProjectBanner;

use crate::registry::RegistryScope;
use crate::shell::DocumentWindow;
use crate::transport_bar::{self, colors};

/// The job bar's progress width (Swift `.frame(width: 120)`).
pub const PROGRESS_WIDTH: f32 = 120.;
/// The problems bar shows at most this many lines (Swift `.lineLimit(3)`).
pub const PROBLEM_LINES: usize = 3;

const BAR_BACKGROUND: u32 = 0x2a2a33;
const TRACK: u32 = 0x15151a;
const FILL: u32 = 0x3f5f8f;

/// One bar: what it shows, whether its button is enabled, its help.
pub(crate) struct Bar {
    banner: ProjectBanner,
    enabled: bool,
    help: Option<&'static str>,
}

/// `d`'s bars, top first, with their buttons' state.
pub(crate) fn bars(d: &DocumentModel) -> Vec<Bar> {
    d.banners()
        .into_iter()
        .map(|banner| Bar {
            enabled: d.banner_enabled(&banner),
            help: d.banner_help(&banner),
            banner,
        })
        .collect()
}

/// The bars, or `None` when there are none (nothing is drawn or recorded
/// then, so the registry paths after them stay as they were).
pub(crate) fn render(
    banners: Vec<Bar>,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> Option<impl IntoElement + use<>> {
    if banners.is_empty() {
        return None;
    }
    let mut column = scope.container();
    let rows: Vec<_> = banners
        .into_iter()
        .enumerate()
        .map(|(i, b)| row(i, &b, &mut column.container(), cx))
        .collect();
    Some(div().flex().flex_col().flex_none().children(rows))
}

fn row(
    i: usize,
    bar: &Bar,
    scope: &mut RegistryScope,
    cx: &mut Context<DocumentWindow>,
) -> impl IntoElement + use<> {
    let b = &bar.banner;
    let text = b.text();
    let lead = match b {
        ProjectBanner::Job { progress, .. } => progress_bar(*progress, &text, scope),
        other => glyph(other).into_any_element(),
    };
    let problems = matches!(b, ProjectBanner::Problems(_));
    let label = div()
        .relative()
        .flex_1()
        .min_w_0()
        .when(problems, |d| d.line_clamp(PROBLEM_LINES).text_ellipsis())
        .when(!problems, |d| d.truncate())
        .child(if problems {
            clamped(&text)
        } else {
            text.clone()
        })
        .child(scope.control("text", &text, true));
    let action = b.action();
    let title = b.button();
    let button = transport_bar::button_with_help(
        ("banner-button", i),
        &title,
        bar.enabled,
        bar.help,
        colors::BUTTON,
        scope,
    )
    .when(bar.enabled, |d| {
        d.on_click(cx.listener(move |this, _, _, cx| {
            this.with_model(cx, |d: &mut DocumentModel| d.run_banner(action));
        }))
    });
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .bg(rgb(BAR_BACKGROUND))
        .border_b_1()
        .border_color(rgb(colors::BORDER))
        .child(lead)
        .child(label)
        .child(button)
}

/// `text`'s first [`PROBLEM_LINES`] lines, the last ending in "…" when
/// more were cut (the bar wraps a long line within the same limit).
pub fn clamped(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= PROBLEM_LINES {
        return text.to_owned();
    }
    format!("{}\u{2026}", lines[..PROBLEM_LINES].join("\n"))
}

/// A determinate progress bar, labelled with the job's text; its value
/// is the progress (0–1).
fn progress_bar(progress: f32, label: &str, scope: &mut RegistryScope) -> gpui::AnyElement {
    let p = progress.clamp(0., 1.);
    let value = format!("{p:.2}");
    div()
        .relative()
        .flex_none()
        .w(px(PROGRESS_WIDTH))
        .h(px(6.))
        .rounded_sm()
        .bg(rgb(TRACK))
        .child(div().h_full().w(relative(p)).rounded_sm().bg(rgb(FILL)))
        .child(scope.control_with("progress", label, true, Some(&value), false))
        .into_any_element()
}

/// The bar's icon (Swift `AppIcon.warning|muted|diskSlow|problem`), a
/// glyph until the icon set lands.
fn glyph(b: &ProjectBanner) -> impl IntoElement {
    let g = match b {
        ProjectBanner::RateMismatch { .. } => "\u{2298}",
        ProjectBanner::DiskTooSlow => "\u{23F1}",
        _ => "\u{26A0}",
    };
    div().flex_none().text_color(rgb(colors::DIM)).child(g)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_show_at_most_three_lines() {
        assert_eq!(clamped("a\nb\nc"), "a\nb\nc");
        assert_eq!(clamped("a\nb\nc\nd"), "a\nb\nc\u{2026}");
        assert_eq!(clamped("one"), "one");
    }
}
