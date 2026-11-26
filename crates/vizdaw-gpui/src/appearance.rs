//! The current theme (slice 9b). One slot per UI thread holds the theme
//! every window draws in: `strip_palette::tokens()` and [`current_theme`]
//! read it, so the ~60 call sites that have no `cx` stay as they were.
//! The slot is set at launch (`AppState::set_prefs`) and by each theme
//! change ([`apply`]), which also redraws every window. GPUI draws on one
//! thread, so a thread-local is the whole story; it also keeps tests,
//! each on its own thread, apart.

use std::cell::Cell;

use gpui::App;
use vizdaw_app::theme::{self, Theme, themes};

thread_local! {
    /// The index into `themes()` of the theme drawn.
    static CURRENT: Cell<usize> = const { Cell::new(0) };
}

/// The theme every window draws in.
pub fn current_theme(_cx: &App) -> &'static Theme {
    current()
}

/// [`current_theme`] where there is no `cx` (token accessors).
pub fn current() -> &'static Theme {
    &themes()[CURRENT.with(Cell::get)]
}

/// The index of the current theme in `themes()`.
pub(crate) fn current_index() -> usize {
    CURRENT.with(Cell::get)
}

/// Makes `id` (Classic Light when it names no theme) the one drawn,
/// without redrawing anything.
pub(crate) fn set_current(id: &str) {
    let t = theme::theme_or_default(id);
    let i = themes().iter().position(|x| x.id == t.id).unwrap_or(0);
    CURRENT.with(|c| c.set(i));
}

/// [`set_current`], then every window redraws.
pub(crate) fn apply(id: &str, cx: &mut App) {
    set_current(id);
    cx.refresh_windows();
}
