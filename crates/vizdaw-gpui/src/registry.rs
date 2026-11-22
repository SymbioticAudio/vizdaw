//! The elements a window exposes to the control channel's `ui.tree` and
//! `ui.click` (GPUI has no accessibility tree yet). Every clickable control
//! records its role, label and bounds while drawing. IDs follow the control
//! channel's rule (`core_control::element`); the path part comes from the
//! view tree: each container pushes a [`RegistryScope`] while rendering, so
//! a control's path is its place among its container's children, and a
//! moved or reordered container changes it (the old ID then answers
//! `no_element` instead of clicking something else).

use std::cell::RefCell;
use std::rc::Rc;

use core_control::{ControlError, ErrorCode};
use gpui::{Bounds, IntoElement, Pixels, Styled, canvas};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    pub id: String,
    pub path: Vec<u32>,
    pub role: String,
    pub label: String,
    pub bounds: Bounds<Pixels>,
    pub enabled: bool,
}

/// What one window drew last frame.
#[derive(Debug, Default, Clone)]
pub struct Registry {
    elements: Vec<Element>,
}

/// A window's registry, shared with the draw callbacks that fill it.
pub type SharedRegistry = Rc<RefCell<Registry>>;

impl Registry {
    pub fn clear(&mut self) {
        self.elements.clear();
    }

    pub fn record(
        &mut self,
        path: &[u32],
        role: &str,
        label: &str,
        bounds: Bounds<Pixels>,
        enabled: bool,
    ) {
        self.elements.push(Element {
            id: core_control::element::element_id(path, role, label),
            path: path.to_vec(),
            role: role.to_owned(),
            label: label.to_owned(),
            bounds,
            enabled,
        });
    }

    pub fn elements(&self) -> &[Element] {
        &self.elements
    }

    /// The `ui.tree` reply body (flat: every node is a child of the window).
    pub fn tree(&self) -> Value {
        let nodes: Vec<Value> = self
            .elements
            .iter()
            .map(|e| {
                let mut n = json!({
                    "id": e.id,
                    "role": e.role,
                    "label": e.label,
                    "frame": {
                        "x": f32::from(e.bounds.origin.x),
                        "y": f32::from(e.bounds.origin.y),
                        "width": f32::from(e.bounds.size.width),
                        "height": f32::from(e.bounds.size.height),
                    },
                    "enabled": e.enabled,
                });
                if e.role == "button" {
                    n["actions"] = json!(["press"]);
                }
                n
            })
            .collect();
        json!({ "truncated": false, "nodes": nodes })
    }

    /// The element `id` names, if one with the same path, role and label
    /// is still drawn.
    pub fn resolve(&self, id: &str) -> Result<&Element, ControlError> {
        let r = core_control::element::parse(id)
            .map_err(|m| ControlError::new(ErrorCode::InvalidParams, m))?;
        self.elements
            .iter()
            .find(|e| {
                e.path == r.path && core_control::element::check(&e.role, &e.label) == r.check
            })
            .ok_or_else(|| ControlError::new(ErrorCode::NoElement, format!("no element {id}")))
    }
}

/// One container's place in the view tree while it renders. Children take
/// the next index in render order: [`container`](Self::container) for a
/// nested container, [`control`](Self::control) for a recorded control.
pub struct RegistryScope {
    registry: SharedRegistry,
    path: Vec<u32>,
    next: u32,
}

impl RegistryScope {
    /// The window's root scope; clears what the last frame recorded.
    pub fn root(registry: &SharedRegistry) -> Self {
        registry.borrow_mut().clear();
        RegistryScope {
            registry: registry.clone(),
            path: Vec::new(),
            next: 0,
        }
    }

    /// The path of this container.
    pub fn path(&self) -> &[u32] {
        &self.path
    }

    fn take_index(&mut self) -> Vec<u32> {
        let mut path = self.path.clone();
        path.push(self.next);
        self.next += 1;
        path
    }

    /// The scope of the next child container.
    pub fn container(&mut self) -> RegistryScope {
        RegistryScope {
            registry: self.registry.clone(),
            path: self.take_index(),
            next: 0,
        }
    }

    /// An invisible overlay that records this container itself (a
    /// `group`, say) with its bounds; its controls are its children in
    /// `ui.tree` paths. Add it as a child of the container, which must be
    /// `.relative()`. It takes no index.
    pub fn this(&self, role: &str, label: &str, enabled: bool) -> impl IntoElement {
        overlay(
            self.registry.clone(),
            self.path.clone(),
            role,
            label,
            enabled,
        )
    }

    /// An invisible overlay that records the next child control with its
    /// parent's bounds when drawn. Add it as a child of the control, which
    /// must be `.relative()`. It is pinned to the parent's corner: an
    /// absolute child without insets sits where it would flow (after the
    /// text in a block container).
    pub fn control(&mut self, role: &str, label: &str, enabled: bool) -> impl IntoElement {
        let path = self.take_index();
        overlay(self.registry.clone(), path, role, label, enabled)
    }
}

fn overlay(
    registry: SharedRegistry,
    path: Vec<u32>,
    role: &str,
    label: &str,
    enabled: bool,
) -> impl IntoElement + use<> {
    let role = role.to_owned();
    let label = label.to_owned();
    canvas(
        move |bounds, _, _| {
            registry
                .borrow_mut()
                .record(&path, &role, &label, bounds, enabled)
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}
