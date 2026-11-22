#[gpui::test]
fn opens_an_empty_document_window(cx: &mut gpui::TestAppContext) {
    let w = cx.add_window(vizdaw_gpui::shell::DocumentWindow::empty);
    cx.run_until_parked();
    assert!(w.read_with(cx, |v, _| v.title().is_empty()).unwrap());
}
