//! The slice of the core session the document model uses.

use vizdaw_ffi::VizError;
use vizdaw_ffi::session::Session;
use vizdaw_ffi::snapshot::{DocSnapshot, TransportState};

/// What the app logic needs from a core session. The real `Session`
/// implements it; tests use a scripted fake.
pub trait DocSession: Send + Sync {
    /// The current package path, `None` when untitled.
    fn path(&self) -> Result<Option<String>, VizError>;
    fn save(&self) -> Result<(), VizError>;
    fn save_as(&self, path: String) -> Result<(), VizError>;
    fn snapshot(&self) -> Result<DocSnapshot, VizError>;
    fn undo(&self) -> Result<DocSnapshot, VizError>;
    fn redo(&self) -> Result<DocSnapshot, VizError>;
    fn transport(&self) -> Result<TransportState, VizError>;
    fn play(&self, seq: u64) -> Result<(), VizError>;
    fn play_from(&self, seq: u64, from: i64, to: Option<i64>) -> Result<(), VizError>;
    fn stop(&self) -> Result<(), VizError>;
    fn locate(&self, tick: i64) -> Result<(), VizError>;
    fn close(&self) -> Result<(), VizError>;
}

impl DocSession for Session {
    fn path(&self) -> Result<Option<String>, VizError> {
        Session::path(self)
    }
    fn save(&self) -> Result<(), VizError> {
        Session::save(self)
    }
    fn save_as(&self, path: String) -> Result<(), VizError> {
        Session::save_as(self, path)
    }
    fn snapshot(&self) -> Result<DocSnapshot, VizError> {
        Session::snapshot(self)
    }
    fn undo(&self) -> Result<DocSnapshot, VizError> {
        Session::undo(self)
    }
    fn redo(&self) -> Result<DocSnapshot, VizError> {
        Session::redo(self)
    }
    fn transport(&self) -> Result<TransportState, VizError> {
        Session::transport(self)
    }
    fn play(&self, seq: u64) -> Result<(), VizError> {
        Session::play(self, seq)
    }
    fn play_from(&self, seq: u64, from: i64, to: Option<i64>) -> Result<(), VizError> {
        Session::play_from(self, seq, from, to)
    }
    fn stop(&self) -> Result<(), VizError> {
        Session::stop(self)
    }
    fn locate(&self, tick: i64) -> Result<(), VizError> {
        Session::locate(self, tick)
    }
    fn close(&self) -> Result<(), VizError> {
        Session::close(self)
    }
}
