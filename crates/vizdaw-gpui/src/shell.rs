//! The document window: one window per open document.

use gpui::{Context, IntoElement, Render, Window, div};

pub struct DocumentWindow {
    title: String,
}

impl DocumentWindow {
    pub fn empty(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            title: String::new(),
        }
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

impl Render for DocumentWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
