//! The Select and Modify panel window over the scripted session: one per
//! document, its title, what it keeps when closed, the transport keys,
//! Return, the Templates menu and the tree.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use core_control::ControlError;
use core_control::protocol::{RawRequest, RequestId};
use core_control::server::Call;
use gpui::{AnyWindowHandle, TestAppContext, WindowHandle};
use serde_json::{Map, Value, json};
use vizdaw_app::select_modify::PanelTask;
use vizdaw_app::testing::FakeSession;
use vizdaw_app::{DocId, DocumentStore};

use super::{ModifyPanel, open_modify_panel};
use crate::AppState;
use crate::control::{self, CallSource};
use crate::menus::RunCommand;
use crate::shell::DocumentWindow;

#[derive(Default)]
struct FakeCalls {
    queue: RefCell<VecDeque<Call>>,
    replies: RefCell<Vec<(u64, Result<Value, ControlError>)>>,
    next: RefCell<u64>,
}

impl CallSource for FakeCalls {
    fn next_call(&self) -> Option<Call> {
        self.queue.borrow_mut().pop_front()
    }

    fn reply(&self, token: u64, outcome: Result<Value, ControlError>) -> bool {
        self.replies.borrow_mut().push((token, outcome));
        true
    }

    fn screenshot_path(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn is_waiting(&self, _: u64) -> bool {
        true
    }
}

/// Every session the store made, in order.
type Made = Arc<Mutex<Vec<Arc<FakeSession>>>>;

struct Setup {
    calls: FakeCalls,
    made: Made,
}

fn setup(cx: &mut TestAppContext) -> Setup {
    let made: Made = Arc::default();
    let m = made.clone();
    let store = DocumentStore::new(
        Box::new(move || {
            let s = Arc::new(FakeSession::new(vec![]));
            m.lock().unwrap().push(s.clone());
            Ok(s)
        }),
        Box::new(|| Ok(Arc::new(FakeSession::new(vec![])))),
        Box::new(|_| Ok(Arc::new(FakeSession::new(vec![])))),
    );
    cx.update(|cx| {
        crate::install(store, cx);
    });
    Setup {
        calls: FakeCalls::default(),
        made,
    }
}

fn call(
    s: &Setup,
    cx: &mut TestAppContext,
    cmd: &str,
    params: Value,
) -> Result<Value, ControlError> {
    let params: Map<String, Value> = match params {
        Value::Object(m) => m,
        _ => Map::new(),
    };
    let raw = RawRequest {
        id: RequestId::Num(1),
        cmd: cmd.into(),
        params,
    };
    let command = core_control::validate(&raw)?;
    let token = {
        let mut n = s.calls.next.borrow_mut();
        *n += 1;
        *n
    };
    s.calls.queue.borrow_mut().push_back(Call {
        token,
        command,
        conn: 1,
        audit_token: None,
    });
    cx.update(|cx| control::pump(&s.calls, cx));
    cx.run_until_parked();
    s.calls
        .replies
        .borrow()
        .iter()
        .find(|(t, _)| *t == token)
        .map(|(_, r)| r.clone())
        .expect("the call was answered")
}

fn ok(r: Result<Value, ControlError>) -> Value {
    r.unwrap_or_else(|e| panic!("call failed: {e:?}"))
}

fn node(tree: &Value, role: &str, label: &str) -> Value {
    tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["role"] == role && n["label"] == label)
        .cloned()
        .unwrap_or_else(|| panic!("no {role} {label:?} in {tree}"))
}

fn has_node(tree: &Value, role: &str, label: &str) -> bool {
    tree["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n["role"] == role && n["label"] == label)
}

fn new_document(cx: &mut TestAppContext) -> (WindowHandle<DocumentWindow>, DocId) {
    let h = cx.update(|cx| crate::new_document(cx).expect("a window"));
    cx.run_until_parked();
    let doc = cx.update(|cx| h.read(cx).unwrap().doc());
    (h, doc)
}

/// The `modify_panel` rows of `state.get windows`.
fn panels(s: &Setup, cx: &mut TestAppContext) -> Vec<Value> {
    let w = ok(call(s, cx, "state.get", json!({"target": "windows"})));
    w["windows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|w| w["kind"] == "modify_panel")
        .cloned()
        .collect()
}

fn panel_of(cx: &mut TestAppContext, doc: DocId) -> Option<AnyWindowHandle> {
    cx.update(|cx| {
        AppState::global(cx)
            .read(cx)
            .modify_panel_of(doc)
            .map(|w| w.handle)
    })
}

fn run_in(s: &Setup, cx: &mut TestAppContext, id: &str, window: impl Into<AnyWindowHandle>) {
    let n = control::number(window);
    ok(call(s, cx, "command.run", json!({"id": id, "window": n})));
}

fn with_doc<R>(
    cx: &mut TestAppContext,
    doc: DocId,
    f: impl FnOnce(&mut vizdaw_app::DocumentModel) -> R,
) -> R {
    cx.update(|cx| AppState::global(cx).update(cx, |s, _| s.store.with_document(doc, |d, _| f(d))))
        .expect("the document is open")
}

#[gpui::test]
fn modify_transpose_opens_one_panel_titled_for_the_document(cx: &mut TestAppContext) {
    let s = setup(cx);
    let (h, doc) = new_document(cx);
    run_in(&s, cx, "modify.transpose", h);
    run_in(&s, cx, "modify.transpose", h);
    let listed = panels(&s, cx);
    assert_eq!(listed.len(), 1, "one panel: {listed:?}");
    assert_eq!(listed[0]["title"], "Select and Modify \u{2014} Untitled");
    assert_eq!(listed[0]["doc"], control::doc_id(doc));
    assert_eq!(cx.update(|cx| cx.windows().len()), 2);
    let task = with_doc(cx, doc, |d| d.select_modify().task);
    assert_eq!(task, PanelTask::Transpose);
}

#[gpui::test]
fn one_panel_per_document_and_it_closes_with_its_document(cx: &mut TestAppContext) {
    let s = setup(cx);
    let (ha, a) = new_document(cx);
    let (hb, b) = new_document(cx);
    run_in(&s, cx, "modify.transpose", ha);
    run_in(&s, cx, "modify.transpose", hb);
    run_in(&s, cx, "modify.transpose", ha);
    let listed = panels(&s, cx);
    assert_eq!(listed.len(), 2, "one panel each: {listed:?}");
    let docs: Vec<&Value> = listed.iter().map(|w| &w["doc"]).collect();
    assert!(docs.contains(&&json!(control::doc_id(a))));
    assert!(docs.contains(&&json!(control::doc_id(b))));
    let pa = panel_of(cx, a).expect("a's panel");
    let pb = panel_of(cx, b).expect("b's panel");
    assert_ne!(pa, pb);
    // A menu command in b's panel acts on b.
    cx.update(|cx| {
        pb.update(cx, |_, window, cx| {
            window.dispatch_action(Box::new(RunCommand::new("modify.quantize")), cx)
        })
        .unwrap()
    });
    cx.run_until_parked();
    assert_eq!(
        with_doc(cx, b, |d| d.select_modify().task),
        PanelTask::Quantize
    );
    assert_eq!(
        with_doc(cx, a, |d| d.select_modify().task),
        PanelTask::Transpose
    );
    // Closing a closes its panel, and only its.
    cx.update(|cx| ha.update(cx, |_, w, _| w.remove_window()).unwrap());
    cx.run_until_parked();
    let listed = panels(&s, cx);
    assert_eq!(listed.len(), 1, "{listed:?}");
    assert_eq!(listed[0]["doc"], control::doc_id(b));
    assert!(
        cx.update(|cx| pa.update(cx, |_, _, _| ()).is_err()),
        "a's panel is gone"
    );
    assert_eq!(cx.update(|cx| cx.windows().len()), 2);
}

#[gpui::test]
fn a_reopened_panel_keeps_its_config(cx: &mut TestAppContext) {
    let s = setup(cx);
    let (h, doc) = new_document(cx);
    run_in(&s, cx, "modify.transpose", h);
    let panel = panel_of(cx, doc).unwrap();
    let n = control::number(panel);
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    let field = node(&tree, "textfield", "Template name")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.key",
        json!({"window": n, "element": field, "text": "Bass"}),
    ));
    assert_eq!(
        with_doc(cx, doc, |d| d.select_modify().template_name.clone()),
        "Bass"
    );
    // The Task picker: Quantize.
    let task = node(&tree, "menu", "Task")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"window": n, "element": task}),
    ));
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    let quantize = node(&tree, "menuitem", "Quantize")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"window": n, "element": quantize}),
    ));
    assert_eq!(
        with_doc(cx, doc, |d| d.select_modify().task),
        PanelTask::Quantize
    );
    let config = with_doc(cx, doc, |d| d.select_modify().config.clone());
    // Closed: the window goes, the model stays.
    cx.update(|cx| panel.update(cx, |_, w, _| w.remove_window()).unwrap());
    cx.run_until_parked();
    assert!(panels(&s, cx).is_empty());
    assert!(!with_doc(cx, doc, |d| d.select_modify().is_open()));
    // Reopened: rebuilt from the model.
    cx.update(|cx| open_modify_panel(doc, cx));
    cx.run_until_parked();
    let panel = panel_of(cx, doc).expect("reopened");
    let tree = ok(call(
        &s,
        cx,
        "ui.tree",
        json!({"window": control::number(panel)}),
    ));
    assert_eq!(node(&tree, "menu", "Task")["value"], "Quantize");
    assert_eq!(node(&tree, "textfield", "Template name")["value"], "Bass");
    assert_eq!(
        with_doc(cx, doc, |d| d.select_modify().config.clone()),
        config
    );
}

#[gpui::test]
fn deactivating_the_panel_gives_the_transport_keys_back(cx: &mut TestAppContext) {
    let s = setup(cx);
    let (h, doc) = new_document(cx);
    run_in(&s, cx, "modify.transpose", h);
    let panel = panel_of(cx, doc).unwrap();
    cx.update(|cx| panel.update(cx, |_, w, _| w.activate_window()).unwrap());
    cx.run_until_parked();
    assert!(with_doc(cx, doc, |d| d.select_modify().is_key()));
    assert!(
        !with_doc(cx, doc, |d| d.transport_keys_enabled()),
        "off while key"
    );
    // A field takes the keyboard.
    let n = control::number(panel);
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    let field = node(&tree, "textfield", "Template name")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.key",
        json!({"window": n, "element": field, "text": "x"}),
    ));
    // The document window takes key: the panel resigns.
    cx.update(|cx| h.update(cx, |_, w, _| w.activate_window()).unwrap());
    cx.run_until_parked();
    assert!(!with_doc(cx, doc, |d| d.select_modify().is_key()));
    assert!(with_doc(cx, doc, |d| d.transport_keys_enabled()), "back");
    let owners = with_doc(cx, doc, |d| d.text_owners().len());
    assert_eq!(owners, 0, "no text owner is left behind");
    // Key again, then closed: the keys come back too.
    cx.update(|cx| panel.update(cx, |_, w, _| w.activate_window()).unwrap());
    cx.run_until_parked();
    assert!(!with_doc(cx, doc, |d| d.transport_keys_enabled()));
    cx.update(|cx| panel.update(cx, |_, w, _| w.remove_window()).unwrap());
    cx.run_until_parked();
    assert!(with_doc(cx, doc, |d| d.transport_keys_enabled()));
}

#[gpui::test]
fn return_in_the_panel_applies_and_does_not_locate(cx: &mut TestAppContext) {
    let s = setup(cx);
    let (h, doc) = new_document(cx);
    run_in(&s, cx, "modify.transpose", h);
    let panel = panel_of(cx, doc).unwrap();
    cx.update(|cx| panel.update(cx, |_, w, _| w.activate_window()).unwrap());
    cx.run_until_parked();
    let fake = s.made.lock().unwrap()[0].clone();
    let before = fake.filtered_edits().len();
    let n = control::number(panel);
    ok(call(
        &s,
        cx,
        "ui.key",
        json!({"window": n, "key": "return"}),
    ));
    assert_eq!(fake.filtered_edits().len(), before + 1, "Return ran Apply");
    assert!(
        fake.calls().iter().all(|c| !c.starts_with("locate(")),
        "nothing located: {:?}",
        fake.calls()
    );
    // Return in the template name field applies too.
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    let field = node(&tree, "textfield", "Template name")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.key",
        json!({"window": n, "element": field, "text": "a\n"}),
    ));
    assert_eq!(fake.filtered_edits().len(), before + 2);
    assert!(fake.calls().iter().all(|c| !c.starts_with("locate(")));
}

#[gpui::test]
fn the_templates_menu_says_no_templates_when_empty(cx: &mut TestAppContext) {
    let s = setup(cx);
    let (h, doc) = new_document(cx);
    run_in(&s, cx, "modify.transpose", h);
    let n = control::number(panel_of(cx, doc).unwrap());
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    let menu = node(&tree, "menu", "Templates")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"window": n, "element": menu}),
    ));
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    let none = node(&tree, "menuitem", "No Templates");
    assert_eq!(none["enabled"], false);
    // A click outside closes the menu.
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"window": n, "element": menu}),
    ));
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    assert!(!has_node(&tree, "menuitem", "No Templates"));
    // Saved ones: Load and Delete for each.
    let field = node(&tree, "textfield", "Template name")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.key",
        json!({"window": n, "element": field, "text": "Bass"}),
    ));
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    let save = node(&tree, "button", "Save as Template")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"window": n, "element": save}),
    ));
    let names = with_doc(cx, doc, |d| {
        d.modify_templates()
            .iter()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>()
    });
    assert_eq!(names, ["Bass"]);
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    let menu = node(&tree, "menu", "Templates")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"window": n, "element": menu}),
    ));
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    assert!(!has_node(&tree, "menuitem", "No Templates"));
    node(&tree, "menuitem", "Load \u{201c}Bass\u{201d}");
    node(&tree, "menuitem", "Delete \u{201c}Bass\u{201d}");
}

#[gpui::test]
fn ui_tree_lists_the_panels_apply_button(cx: &mut TestAppContext) {
    let s = setup(cx);
    let (h, doc) = new_document(cx);
    run_in(&s, cx, "modify.transpose", h);
    let panel: WindowHandle<ModifyPanel> = panel_of(cx, doc).unwrap().downcast().unwrap();
    let n = control::number(panel);
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    assert_eq!(tree["window"], n);
    assert_eq!(node(&tree, "button", "Apply")["enabled"], true);
    assert_eq!(node(&tree, "button", "Modify Again")["enabled"], false);
    node(&tree, "button", "Save as Template");
    node(&tree, "text", "Apply acts on the current selection.");
    // A click on Apply runs it.
    let fake = s.made.lock().unwrap()[0].clone();
    let before = fake.filtered_edits().len();
    let apply = node(&tree, "button", "Apply")["id"].clone();
    ok(call(
        &s,
        cx,
        "ui.click",
        json!({"window": n, "element": apply}),
    ));
    assert_eq!(fake.filtered_edits().len(), before + 1);
    assert_eq!(cx.update(|cx| panel.read(cx).unwrap().doc()), doc);
}

/// Modify › Templates › ‹name› loads the template into the model while
/// the panel is open: the field shows its name, and Save as Template
/// saves under the name on screen.
#[gpui::test]
fn the_template_name_field_follows_a_template_run_from_the_menu(cx: &mut TestAppContext) {
    let s = setup(cx);
    let (h, doc) = new_document(cx);
    run_in(&s, cx, "modify.transpose", h);
    let n = control::number(panel_of(cx, doc).unwrap());
    let type_name = |s: &Setup, cx: &mut TestAppContext, text: &str| {
        let tree = ok(call(s, cx, "ui.tree", json!({"window": n})));
        let field = node(&tree, "textfield", "Template name")["id"].clone();
        ok(call(
            s,
            cx,
            "ui.key",
            json!({"window": n, "element": field, "text": text}),
        ));
    };
    let save = |s: &Setup, cx: &mut TestAppContext| {
        let tree = ok(call(s, cx, "ui.tree", json!({"window": n})));
        let save = node(&tree, "button", "Save as Template")["id"].clone();
        ok(call(
            s,
            cx,
            "ui.click",
            json!({"window": n, "element": save}),
        ));
    };
    type_name(&s, cx, "Bass");
    save(&s, cx);
    // The field now says another name (the typing adds to it); the menu
    // runs Bass.
    type_name(&s, cx, "Lead");
    let _ = call(
        &s,
        cx,
        "command.run",
        json!({"id": "modify.template", "params": {"name": "Bass"}, "window": control::number(h)}),
    );
    assert_eq!(
        with_doc(cx, doc, |d| d.select_modify().template_name.clone()),
        "Bass"
    );
    let tree = ok(call(&s, cx, "ui.tree", json!({"window": n})));
    assert_eq!(node(&tree, "textfield", "Template name")["value"], "Bass");
    // Save as Template saves under the shown name.
    let fake = s.made.lock().unwrap()[0].clone();
    save(&s, cx);
    let saved = fake.edits().into_iter().rev().find_map(|e| match e {
        vizdaw_ffi::edit::EditCommand::Settings {
            edit: vizdaw_ffi::modify::SettingsEdit::SaveTemplate { template },
        } => Some(template.name),
        _ => None,
    });
    assert_eq!(saved.as_deref(), Some("Bass"));
}
