use std::sync::Arc;

use vizdaw_app::DocumentStore;
use vizdaw_app::testing::{FakeSession, demo_fake};

#[gpui::test]
fn opens_an_untitled_document_window(cx: &mut gpui::TestAppContext) {
    let store = DocumentStore::new(
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|| Ok(Arc::new(demo_fake()))),
        Box::new(|_| Ok(Arc::new(demo_fake()))),
    );
    let w = cx.update(|cx| {
        vizdaw_gpui::install(store, cx);
        vizdaw_gpui::new_document(cx).expect("a window")
    });
    cx.run_until_parked();
    assert_eq!(
        w.read_with(cx, |v, _| v.title().to_owned()).unwrap(),
        "Untitled"
    );
}
