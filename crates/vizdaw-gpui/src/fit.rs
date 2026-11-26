//! An element whose content is made once its own bounds are known, in the
//! same frame (the way GPUI's lists make their rows): a layout that
//! depends on the area it fills never uses an area measured a frame late.

use std::panic;

use gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, LayoutId, Pixels, Style, Window, relative,
};

/// Fills its parent; `make` builds what it shows from its bounds this
/// frame, laid out at their origin and as large.
pub fn fitted(
    make: impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> AnyElement + 'static,
) -> Fitted {
    Fitted {
        make: Some(Box::new(make)),
        child: None,
    }
}

type Make = Box<dyn FnOnce(Bounds<Pixels>, &mut Window, &mut App) -> AnyElement>;

pub struct Fitted {
    make: Option<Make>,
    child: Option<AnyElement>,
}

impl Element for Fitted {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style {
            position: gpui::Position::Absolute,
            ..Style::default()
        };
        style.inset.top = gpui::px(0.).into();
        style.inset.left = gpui::px(0.).into();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(make) = self.make.take() else {
            return;
        };
        let mut child = make(bounds, window, cx);
        let space = gpui::size(
            AvailableSpace::Definite(bounds.size.width),
            AvailableSpace::Definite(bounds.size.height),
        );
        child.prepaint_as_root(bounds.origin, space, window, cx);
        self.child = Some(child);
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(child) = &mut self.child {
            child.paint(window, cx);
        }
    }
}
