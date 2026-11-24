//! The left pane's Channel Strip (Swift `LeftStripPane`): the strip the
//! focused track plays through (`StripFocus`: the selected tracks of the
//! shown sequence, else the armed ones; the first decides): its audio
//! path's, its plug-in instrument's, the Built-in Synth's, or its MIDI
//! strip (an external destination's, or "No Destination"). The strip is
//! the mixer's placement `Strip` ([`MixerView`]): one column, as tall as
//! the pane less 8 points above and below, scrolling down when the pane
//! is shorter than the strip's least height; its controls, menus and
//! drags are the mixer's own. A track with no strip shows the inspector's
//! placeholder; nothing focused (or a strip the mixer lacks), the Swift
//! note. The placement exists only while a strip shows: when the focus
//! moves off one it goes, ending its drag.

use gpui::{AnyElement, App, Entity, Window};
use vizdaw_app::editor::PaneView;

use crate::mixer_view::{self, MixerView};
use crate::panes::{self, LeftStrip};
use crate::registry::RegistryScope;

/// Swift `StripFocus.nothingMessage`.
const NOTHING: &str = "Select a track to see its channel strip.";

/// The pane's content for `shows`, the strip drawn by placement `view`
/// (made while `shows` is placed). `scope` is the pane content's own
/// container.
pub(crate) fn render(
    shows: LeftStrip,
    view: Option<&Entity<MixerView>>,
    scope: &mut RegistryScope,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    match (shows, view) {
        (LeftStrip::Inspector, _) => {
            panes::note_view(&panes::note(PaneView::Inspector, false), scope)
        }
        // A strip, or no mixer yet (asked for, or Mixer Unavailable).
        (LeftStrip::Strip | LeftStrip::Unread, Some(view)) => {
            mixer_view::render(view, scope, window, cx)
        }
        _ => panes::note_view(NOTHING, scope),
    }
}
