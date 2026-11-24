//! The left pane's Channel Strip (Swift `LeftStripPane`): the strip the
//! focused track plays through (`StripFocus`: the selected tracks of the
//! shown sequence, else the armed ones; the first decides): its audio
//! path's, its plug-in instrument's, the Built-in Synth's, or its MIDI
//! strip (an external destination's, or "No Destination"). The strip is
//! the mixer's placement `Strip` ([`MixerView`]): one column, as tall as
//! the pane less 8 points above and below, scrolling down when the pane
//! is shorter than the strip's least height; its controls, menus and
//! drags are the mixer's own. A track with no strip shows the inspector's
//! placeholder; nothing focused, the Swift note (the placement is not
//! made then).

use gpui::{AnyElement, App, Entity, Window};
use vizdaw_app::editor::PaneView;
use vizdaw_app::view::StripFocus;

use crate::mixer_view::{self, MixerView};
use crate::panes;
use crate::registry::RegistryScope;

/// The pane's content, drawn by placement `view` (`None` while nothing
/// is focused). `scope` is the pane content's own container.
pub(crate) fn render(
    view: Option<&Entity<MixerView>>,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(view) = view else {
        return panes::note_view(&panes::note(PaneView::Strip, true), scope);
    };
    let mixer = view.read(cx).mixer.clone();
    let focus = mixer.read(cx, |m, d| {
        d.left_strip(&m.snapshot().strips, m.destinations())
    });
    match focus {
        Some(StripFocus::Inspector) => {
            panes::note_view(&panes::note(PaneView::Inspector, false), scope)
        }
        Some(StripFocus::Nothing) => panes::note_view(&panes::note(PaneView::Strip, true), scope),
        // A strip, or no mixer yet (asked for, or Mixer Unavailable).
        _ => mixer_view::render(view, scope, window, cx),
    }
}
