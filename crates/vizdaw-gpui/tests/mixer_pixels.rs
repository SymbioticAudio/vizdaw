//! The Mix view drawn for real: the first frame after switching to it
//! already holds the strips (the channel's `ui.screenshot` draws one frame
//! and captures it). Where the platform has an offscreen renderer (Metal
//! on macOS) the frame is rendered and its pixels checked: the master's
//! fader cap shows in its colour, its panel over the mixer's background.

use std::sync::Arc;

use gpui::{AnyWindowHandle, App, HeadlessAppContext, WindowHandle};
use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeSession, mixer_fake};
use vizdaw_gpui::registry::Element;
use vizdaw_gpui::strip_palette::tokens;
use vizdaw_gpui::{AppState, DocumentWindow};

fn app() -> (HeadlessAppContext, WindowHandle<DocumentWindow>) {
    let text = Arc::new(gpui::NoopTextSystem::new());
    let mut cx = HeadlessAppContext::with_platform(text, Arc::new(()), || {
        Ok(gpui_platform::current_headless_renderer().ok().flatten())
    });
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(mixer_fake()))),
        Box::new(|_| Ok(Arc::new(mixer_fake()))),
    );
    let window = cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
        vizdaw_gpui::new_demo(cx).expect("a demo window")
    });
    cx.run_until_parked();
    (cx, window)
}

/// Switches to the Mix view, then draws one whole frame as `ui.screenshot`
/// does, and answers what it captures and records, all in one update: no
/// effect runs between the switch and the frame (the real app draws on
/// its display's next refresh, the test platform at once).
fn switch_and_draw(
    window: WindowHandle<DocumentWindow>,
    cx: &mut App,
) -> (Option<image::RgbaImage>, Vec<Element>) {
    let any: AnyWindowHandle = window.into();
    let (doc, registry) = window
        .read_with(cx, |v, _| (v.doc(), v.registry().clone()))
        .unwrap();
    any.update(cx, |_, w, cx| {
        AppState::global(cx).update(cx, |s, cx| {
            s.store.with_document(doc, |d, _| {
                vizdaw_app::commands::perform_from_menu("ui.view_mix", d).expect("the Mix view")
            });
            cx.notify();
        });
        w.refresh();
        w.draw(cx).clear(cx);
        let shot = w.render_to_image().ok();
        let all = registry.borrow().elements().to_vec();
        (shot, all)
    })
    .unwrap()
}

#[test]
fn the_first_frame_of_the_mix_view_paints_its_strips() {
    let (mut cx, window) = app();
    let (shot, all) = cx.update(|cx| switch_and_draw(window, cx));
    let fader = all
        .iter()
        .find(|e| e.role == "slider" && e.label == "Fader of Master")
        .unwrap_or_else(|| panic!("the first frame has no master fader: {all:#?}"))
        .bounds;
    let Some(image) = shot else {
        // No offscreen renderer here: the frame's records must do.
        return;
    };
    let scale = image.width() as f32 / 1440.;
    let px = |x: f32, y: f32| image.get_pixel((x * scale) as u32, (y * scale) as u32).0;
    let t = tokens();
    let rgb = |c: u32| [(c >> 16) as u8, (c >> 8) as u8, c as u8, 255];
    // Down the middle of the cap column: the cap is somewhere there.
    let x = f32::from(fader.origin.x) + 24. + 18.;
    let top = f32::from(fader.origin.y);
    let h = f32::from(fader.size.height);
    let cap = rgb(t.strip_master);
    let seen: Vec<[u8; 4]> = (0..h as i32).map(|dy| px(x, top + dy as f32)).collect();
    assert!(
        seen.contains(&cap),
        "the master's cap is not painted in its fader: {:?}",
        &seen[..seen.len().min(8)]
    );
    // The strip's panel shows over the mixer's background.
    let panel = px(f32::from(fader.origin.x) - 4., top - 4.);
    assert_eq!(panel, rgb(t.surface_raised), "the master's panel");
}
