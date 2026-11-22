//! A scripted `DocSession` (Swift: `FakeSession`): scripted errors, a busy
//! flag and a call log.

use std::sync::Mutex;

use vizdaw_app::DocSession;
use vizdaw_ffi::VizError;
use vizdaw_ffi::snapshot::{DocSnapshot, SeqRow, TransportState};

struct State {
    snap: DocSnapshot,
    next_snapshot: Option<DocSnapshot>,
    transport: TransportState,
    saved_path: Option<String>,
    play_error: Option<VizError>,
    transport_error: Option<VizError>,
    step_error: Option<VizError>,
    save_error: Option<VizError>,
    calls: Vec<String>,
}

pub struct FakeSession(Mutex<State>);

fn stopped() -> TransportState {
    TransportState {
        playing: false,
        seq: None,
        tick: 0,
        position_bbu: "1.1.0".into(),
        seconds: 0.0,
        sample_rate: 0.0,
        device: String::new(),
        faults: vec![],
        report: vec![],
        host_time_ns: 0,
        ticks_per_second: 30_720.0,
        rate_mismatch: false,
        counting_in: false,
        click: None,
    }
}

impl FakeSession {
    pub fn new(sequences: Vec<SeqRow>) -> Self {
        FakeSession(Mutex::new(State {
            snap: DocSnapshot {
                version: 0,
                step: 0,
                title: "Untitled".into(),
                dirty: false,
                undo_label: None,
                redo_label: None,
                sequences,
                warnings: vec![],
                lane_revs: vec![],
                edit_notes: vec![],
            },
            next_snapshot: None,
            transport: stopped(),
            saved_path: None,
            play_error: None,
            transport_error: None,
            step_error: None,
            save_error: None,
            calls: vec![],
        }))
    }

    fn st(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap()
    }

    pub fn snap(&self) -> DocSnapshot {
        self.st().snap.clone()
    }

    /// Makes the next command return `s`.
    pub fn bump(&self, s: DocSnapshot) {
        self.st().next_snapshot = Some(s);
    }

    pub fn set_step_error(&self, e: Option<VizError>) {
        self.st().step_error = e;
    }

    pub fn set_path(&self, p: Option<&str>) {
        self.st().saved_path = p.map(String::from);
    }

    pub fn set_save_error(&self, e: Option<VizError>) {
        self.st().save_error = e;
    }

    pub fn set_play_error(&self, e: Option<VizError>) {
        self.st().play_error = e;
    }

    pub fn set_transport_error(&self, e: Option<VizError>) {
        self.st().transport_error = e;
    }

    pub fn set_playing(&self, playing: bool) {
        self.st().transport.playing = playing;
    }

    /// Every call, in order, as `name` or `name(arg)`.
    pub fn calls(&self) -> Vec<String> {
        self.st().calls.clone()
    }

    pub fn count(&self, name: &str) -> usize {
        self.calls().iter().filter(|c| c.as_str() == name).count()
    }

    fn command_result(s: &mut State) -> DocSnapshot {
        match s.next_snapshot.take() {
            Some(n) => s.snap = n,
            None => s.snap.version += 1,
        }
        s.snap.clone()
    }
}

impl DocSession for FakeSession {
    fn path(&self) -> Result<Option<String>, VizError> {
        Ok(self.st().saved_path.clone())
    }
    fn save(&self) -> Result<(), VizError> {
        let mut s = self.st();
        s.calls.push("save".into());
        if let Some(e) = s.save_error.clone() {
            return Err(e);
        }
        if s.saved_path.is_none() {
            return Err(VizError::Io {
                message: "use Save As".into(),
            });
        }
        s.snap.dirty = false;
        Ok(())
    }
    fn save_as(&self, path: String) -> Result<(), VizError> {
        let mut s = self.st();
        s.calls.push(format!("save_as({path})"));
        let name = std::path::Path::new(&path)
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        s.saved_path = Some(path);
        s.snap.dirty = false;
        s.snap.title = name;
        Ok(())
    }
    fn snapshot(&self) -> Result<DocSnapshot, VizError> {
        Ok(self.st().snap.clone())
    }
    fn undo(&self) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        if let Some(e) = s.step_error.clone() {
            return Err(e);
        }
        s.calls.push("undo".into());
        Ok(Self::command_result(&mut s))
    }
    fn redo(&self) -> Result<DocSnapshot, VizError> {
        let mut s = self.st();
        if let Some(e) = s.step_error.clone() {
            return Err(e);
        }
        s.calls.push("redo".into());
        Ok(Self::command_result(&mut s))
    }
    fn transport(&self) -> Result<TransportState, VizError> {
        let s = self.st();
        match s.transport_error.clone() {
            Some(e) => Err(e),
            None => Ok(s.transport.clone()),
        }
    }
    fn play(&self, seq: u64) -> Result<(), VizError> {
        let mut s = self.st();
        if let Some(e) = s.play_error.clone() {
            return Err(e);
        }
        s.calls.push(format!("play({seq})"));
        s.transport.playing = true;
        s.transport.seq = Some(seq);
        Ok(())
    }
    fn play_from(&self, seq: u64, from: i64, to: Option<i64>) -> Result<(), VizError> {
        let mut s = self.st();
        if let Some(e) = s.play_error.clone() {
            return Err(e);
        }
        s.calls.push(format!("play_from({seq},{from},{to:?})"));
        s.transport.playing = true;
        s.transport.seq = Some(seq);
        s.transport.tick = from;
        Ok(())
    }
    fn stop(&self) -> Result<(), VizError> {
        let mut s = self.st();
        s.calls.push("stop".into());
        s.transport.playing = false;
        Ok(())
    }
    fn close(&self) -> Result<(), VizError> {
        self.st().calls.push("close".into());
        Ok(())
    }
}

fn seq(id: u64, name: &str, top_level: bool, uses: u32) -> SeqRow {
    SeqRow {
        id,
        name: name.into(),
        use_count: uses,
        top_level,
        length_ticks: 0,
        lane_count: 0,
        comment: String::new(),
        keyboard_slot: None,
    }
}

/// Three sequences: Beat and Groove (referenced), Song (top level).
pub fn demo_fake() -> FakeSession {
    FakeSession::new(vec![
        seq(1, "Beat", false, 1),
        seq(2, "Groove", false, 2),
        seq(3, "Song", true, 0),
    ])
}
