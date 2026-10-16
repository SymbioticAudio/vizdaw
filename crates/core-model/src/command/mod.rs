//! Commands: every edit is a `Command` applied by the pure [`apply`].

pub mod lane;
pub mod sequence;

pub use lane::LaneCmd;
pub use sequence::SequenceCmd;

use crate::document::Document;
use crate::error::Result;
use crate::ids::AnyId;
use crate::selection::{Selection, ViewFilter};

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// IDs created, in creation order.
    pub created: Vec<AnyId>,
    /// IDs removed.
    pub removed: Vec<AnyId>,
    /// Non-fatal notes for the user.
    pub warnings: Vec<String>,
}

/// The result of applying a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// The new document.
    pub doc: Document,
    /// What changed.
    pub report: Report,
    /// The new selection, for commands that change it.
    pub selection: Option<Selection>,
}

/// The current selection and view filter, for commands that act on them.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// The document's selection.
    pub selection: &'a Selection,
    /// The view filter of the view issuing the command.
    pub filter: &'a ViewFilter,
}

/// Every edit the model supports.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Sequence-level edits.
    Sequence(SequenceCmd),
    /// Lane and take edits.
    Lane(LaneCmd),
}

impl Command {
    /// The stable name, used for key bindings and undo labels.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Sequence(c) => c.name(),
            Command::Lane(c) => c.name(),
        }
    }
}

/// Applies a command with an empty selection and no view filter.
pub fn apply(doc: &Document, cmd: &Command) -> Result<Applied> {
    let selection = Selection::default();
    let filter = ViewFilter::default();
    apply_in(
        doc,
        Context {
            selection: &selection,
            filter: &filter,
        },
        cmd,
    )
}

/// Applies a command in the context of a selection and view filter. Pure:
/// `doc` is never changed; on error nothing is returned but the error.
pub fn apply_in(doc: &Document, ctx: Context<'_>, cmd: &Command) -> Result<Applied> {
    let mut d = doc.clone();
    let mut report = Report::default();
    let _ = ctx;
    let selection = match cmd {
        Command::Sequence(c) => {
            sequence::apply(&mut d, c, &mut report)?;
            None
        }
        Command::Lane(c) => {
            lane::apply(&mut d, c, &mut report)?;
            None
        }
    };
    Ok(Applied {
        doc: d,
        report,
        selection,
    })
}
