//! File › Bounce… (Swift `BounceModel`, `BounceSheet`): what to bounce
//! (the mix or chosen strips as stems; the song or the time selection), in
//! which format (the Format, Bits, Rate and Dither rows), and whether to
//! add the result to the project and write the files. The core's check of
//! the choices (the external-MIDI warning and the length) runs off the UI
//! thread: [`DocumentModel::take_bounce_check`] hands the UI each check
//! to run ([`BounceCheck::run`]) and only the newest one's answer lands
//! ([`DocumentModel::land_bounce_check`]). The bounce is a `Bounce` job
//! ("Bounce Failed" when it fails; a failure after some files were
//! renamed into place names them); a refusal to start is shown in the
//! sheet. A finished bounce says what it wrote in the status line and the
//! sheet closes.

use std::collections::BTreeSet;

use vizdaw_ffi::{
    BitDepthRow, BounceCheckRow, BounceRangeRow, BounceRequestRow, BounceSourceRow, ContainerRow,
    JobKindRow, StripKindRow, StripRow, VizError,
};

use crate::alerts::error_message;
use crate::audio_format::{AudioFormat, AudioFormatChoices};
use crate::document::DocumentModel;
use crate::formats::FALLBACK_FORMAT;
use crate::session::DocSession;

/// The sheet's title (also the source picker's label).
pub const TITLE: &str = "Bounce";
/// The source picker's segments.
pub const MIX: &str = "Mix";
pub const STEMS: &str = "Stems";
/// The caption under the source.
pub const CAPTION: &str = "Files ignore mute and solo. Add to project bounces as heard: muted and solo-silenced strips, and strips routed into a muted bus, are silent.";
/// The caption for stems.
pub const STEMS_CAPTION: &str =
    "Files already in the folder are kept; new stems take the next free number.";
/// The toggles.
pub const TIME_SELECTION_ONLY: &str = "Time selection only";
pub const ADD_TO_PROJECT: &str = "Add to project";
pub const ALSO_WRITE_FILES: &str = "Also write the files";
/// The format rows, shared with Export for Another DAW.
pub use crate::audio_format::{
    BITS, CONTAINERS, DEPTHS, DITHER, FORMAT, OTHER, OTHER_LABEL, OTHER_PLACEHOLDER, OTHER_RATES,
    RATE, RATES, rate_title,
};
/// The buttons.
pub const CANCEL: &str = "Cancel";
pub const BOUNCE: &str = "Bounce";
/// The alert title of a failed bounce (a refusal shows in the sheet).
pub const FAILED: &str = "Bounce Failed";
/// The sheet's error with no sequence shown.
pub const NO_SEQUENCE: &str = "No sequence is shown.";

/// The file extension a mix in `c` is written with (an AIFF writes
/// `.aif`).
pub fn extension(c: ContainerRow) -> &'static str {
    match c {
        ContainerRow::Wav => "wav",
        ContainerRow::Aiff => "aif",
        ContainerRow::Caf => "caf",
    }
}

/// `frames` at `rate` as `m:ss.t` (or `h:mm:ss.t`; Swift
/// `MediaModel.duration`).
pub fn duration(frames: u64, rate: u32) -> String {
    let tenths = (frames as f64 / f64::from(rate.max(1)) * 10.).round() as u64;
    let (h, rest) = (tenths / 36_000, tenths % 36_000);
    let (m, t) = (rest / 600, rest % 600);
    let s = format!("{:02}.{}", t / 10, t % 10);
    if h > 0 {
        format!("{h}:{m:02}:{s}")
    } else {
        format!("{m}:{s}")
    }
}

/// What the sheet bounces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Mix,
    Stems,
}

/// The sheet's choices (what its controls edit through
/// [`DocumentModel::edit_bounce`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BounceChoices {
    pub source: Source,
    /// The strips bounced as stems.
    pub stems: BTreeSet<u64>,
    pub use_selection: bool,
    pub container: ContainerRow,
    pub depth: BitDepthRow,
    pub rate: u32,
    /// "Other…" is the chosen rate (a field from 8,000 to 384,000).
    pub other_rate: bool,
    /// Kept as set; on by default at 16 bits; ignored for float.
    pub dither: bool,
    pub add_to_project: bool,
    /// Write the files. Always so unless "Add to project" is on, which
    /// may be used alone (no panel, no files).
    pub write_files: bool,
}

impl Default for BounceChoices {
    fn default() -> Self {
        BounceChoices {
            source: Source::Mix,
            stems: BTreeSet::new(),
            use_selection: false,
            container: ContainerRow::Wav,
            depth: FALLBACK_FORMAT.depth,
            rate: FALLBACK_FORMAT.sample_rate,
            other_rate: false,
            dither: false,
            add_to_project: false,
            write_files: true,
        }
    }
}

impl BounceChoices {
    /// The Rate picker's value: a listed rate, or 0 for "Other…".
    pub fn rate_choice(&self) -> u32 {
        self.audio_format().rate_choice()
    }

    /// Chooses a listed rate, or "Other…" (0), which keeps the rate.
    pub fn set_rate_choice(&mut self, choice: u32) {
        self.edit_audio_format(&|f| f.set_rate_choice(choice));
    }

    /// The "Other…" field's value, clamped to 8,000–384,000.
    pub fn set_other_rate(&mut self, hz: i64) {
        self.edit_audio_format(&|f| f.set_other_rate(hz));
    }

    /// What the sheet asks for: a file or folder, or nothing (add only).
    pub fn needs_destination(&self) -> bool {
        !self.add_to_project || self.write_files
    }
}

impl AudioFormatChoices for BounceChoices {
    fn audio_format(&self) -> AudioFormat {
        AudioFormat {
            container: self.container,
            depth: self.depth,
            rate: self.rate,
            other_rate: self.other_rate,
            dither: self.dither,
        }
    }

    fn set_audio_format(&mut self, f: AudioFormat) {
        self.container = f.container;
        self.depth = f.depth;
        self.rate = f.rate;
        self.other_rate = f.other_rate;
        self.dither = f.dither;
    }
}

/// A check of the sheet's choices to run off the UI thread.
#[derive(Debug, Clone, PartialEq)]
pub struct BounceCheck {
    pub generation: u64,
    pub request: BounceRequestRow,
}

/// A check's answer, to land on the document.
#[derive(Debug, Clone, PartialEq)]
pub struct BounceChecked {
    pub generation: u64,
    pub rate: u32,
    pub answer: Result<BounceCheckRow, VizError>,
}

impl BounceCheck {
    /// Asks the core (slow: call it off the UI thread).
    pub fn run(self, s: &dyn DocSession) -> BounceChecked {
        BounceChecked {
            generation: self.generation,
            rate: self.request.rate,
            answer: s.bounce_check(self.request),
        }
    }
}

/// The Bounce sheet's state on a document (Swift `DocumentModel.bounce`).
#[derive(Debug, Default)]
pub struct Bounce {
    pub(crate) choices: BounceChoices,
    shown: bool,
    /// The destination the check was last asked about.
    last_path: Option<String>,
    warning: Option<String>,
    error: Option<String>,
    length: String,
    /// The strips the stems checklist offers (every strip but master).
    strips: Vec<StripRow>,
    job: Option<u64>,
    /// The newest check asked for.
    generation: u64,
    /// The newest check, until the UI takes it.
    pending: Option<BounceCheck>,
}

impl Bounce {
    pub fn choices(&self) -> &BounceChoices {
        &self.choices
    }

    /// The sheet is asked for.
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// The core's warning (Ruling 8's sentences), as last checked.
    pub fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }

    /// Why the last start or check was refused.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// "1:32.4 + tail" (empty before a check answers, or after a refusal).
    pub fn length(&self) -> &str {
        &self.length
    }

    pub fn strips(&self) -> &[StripRow] {
        &self.strips
    }

    /// The running bounce job.
    pub fn job(&self) -> Option<u64> {
        self.job
    }
}

impl DocumentModel {
    /// The Bounce sheet's state.
    pub fn bounce(&self) -> &Bounce {
        &self.bounce
    }

    /// File › Bounce…: the sheet, its choices reset to the project's
    /// format (Ruling 11) and checked. While a bounce runs the sheet shows
    /// it again as it is.
    pub fn show_bounce(&mut self) {
        if self.bounce.job.is_none() {
            self.reset_bounce();
        }
        self.bounce.shown = true;
    }

    /// The sheet's Cancel (no job running): the sheet goes.
    pub fn hide_bounce(&mut self) {
        self.bounce.shown = false;
    }

    fn reset_bounce(&mut self) {
        let f = self
            .session
            .project_info()
            .map_or(FALLBACK_FORMAT, |i| i.format);
        let strips: Vec<StripRow> = self
            .session
            .mixer()
            .map(|m| m.strips)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.kind != StripKindRow::Master)
            .collect();
        let b = &mut self.bounce;
        b.choices = BounceChoices {
            source: Source::Mix,
            stems: strips.iter().map(|s| s.id).collect(),
            ..BounceChoices::default()
        };
        b.choices.set_audio_format(AudioFormat::for_project(f));
        b.strips = strips;
        b.job = None;
        b.warning = None;
        b.length.clear();
        self.check_bounce(None);
    }

    /// Edits the sheet's choices; a change is checked again.
    pub fn edit_bounce(&mut self, edit: impl FnOnce(&mut BounceChoices)) {
        let before = self.bounce.choices.clone();
        edit(&mut self.bounce.choices);
        if self.bounce.shown && self.bounce.choices != before {
            let path = self.bounce.last_path.clone();
            self.check_bounce(path);
        }
    }

    /// A time selection exists to bounce.
    pub fn bounce_has_selection(&self) -> bool {
        self.selection().time_range.is_some_and(|t| t.start < t.end)
    }

    /// The bounce job is still listed as running.
    pub fn bounce_running(&self) -> bool {
        self.bounce
            .job
            .is_some_and(|id| self.jobs.iter().any(|j| j.id == id))
    }

    /// The running bounce's progress (0–1).
    pub fn bounce_progress(&self) -> Option<f32> {
        let id = self.bounce.job?;
        self.jobs.iter().find(|j| j.id == id).map(|j| j.progress)
    }

    /// The request for `path` (a file, a folder for stems, `None`: add
    /// only); `replace` only for a mix (the save panel's confirmation).
    /// `None` with no sequence shown.
    pub fn bounce_request(&self, path: Option<&str>, replace: bool) -> Option<BounceRequestRow> {
        let seq = self.selected_seq()?;
        let c = &self.bounce.choices;
        let range = match self.selection().time_range {
            Some(t) if c.use_selection && t.start < t.end => BounceRangeRow::Ticks {
                start: t.start,
                end: t.end,
            },
            _ => BounceRangeRow::Song,
        };
        let source = match c.source {
            Source::Mix => BounceSourceRow::Mix,
            Source::Stems => BounceSourceRow::Stems {
                strips: c.stems.iter().copied().collect(),
            },
        };
        Some(BounceRequestRow {
            seq,
            source,
            range,
            container: c.container,
            depth: c.depth,
            rate: c.rate,
            dither: c.audio_format().sent_dither(),
            path: path.map(str::to_owned),
            add_to_project: c.add_to_project,
            replace: c.source == Source::Mix && replace,
        })
    }

    /// Asks for a check of the choices for `path` (the UI runs it, see
    /// [`DocumentModel::take_bounce_check`]); with no sequence shown the
    /// sheet's error says so at once.
    fn check_bounce(&mut self, path: Option<String>) {
        self.bounce.error = None;
        let request = self.bounce_request(path.as_deref(), false);
        self.bounce.last_path = path;
        self.bounce.generation += 1;
        match request {
            Some(request) => {
                self.bounce.pending = Some(BounceCheck {
                    generation: self.bounce.generation,
                    request,
                });
            }
            None => {
                self.bounce.pending = None;
                self.bounce.warning = None;
                self.bounce.length.clear();
                self.bounce.error = Some(NO_SEQUENCE.into());
            }
        }
    }

    /// The newest check waiting to run, taken (the UI runs it off its
    /// thread and lands it).
    pub fn take_bounce_check(&mut self) -> Option<BounceCheck> {
        self.bounce.pending.take()
    }

    /// A check's answer: the warning and length, or the refusal as the
    /// sheet's error. An answer older than the newest check is dropped.
    pub fn land_bounce_check(&mut self, c: BounceChecked) {
        let b = &mut self.bounce;
        if c.generation != b.generation {
            return;
        }
        match c.answer {
            Ok(row) => {
                b.warning = row.warning;
                b.length = format!("{} + tail", duration(row.frames, c.rate));
            }
            Err(e) => {
                b.warning = None;
                b.length.clear();
                b.error = Some(error_message(&e));
            }
        }
    }

    /// Runs the waiting check here and lands it (tests; the UI runs it off
    /// its thread).
    pub fn run_bounce_check(&mut self) {
        if let Some(c) = self.take_bounce_check() {
            let checked = c.run(self.session.as_ref());
            self.land_bounce_check(checked);
        }
    }

    /// The sheet's Bounce (after its panel): starts the job. A refusal is
    /// the sheet's error, not an alert; a job done at once ends here.
    pub fn start_bounce(&mut self, path: Option<&str>, replace: bool) {
        self.bounce.error = None;
        let Some(req) = self.bounce_request(path, replace) else {
            self.bounce.error = Some(NO_SEQUENCE.into());
            return;
        };
        match self.try_start_job(JobKindRow::Bounce, |s| s.bounce(req)) {
            Ok(id) => {
                self.bounce.job = Some(id);
                if !self.bounce_running() {
                    self.bounce_finished(id);
                }
            }
            Err(e) => self.bounce.error = Some(error_message(&e)),
        }
    }

    /// The sheet's Cancel while the bounce runs: its end (quiet) arrives
    /// with the next poll and closes the sheet.
    pub fn cancel_bounce(&mut self) {
        if let Some(id) = self.bounce.job {
            self.cancel_job(id);
        }
    }

    /// After a poll: the bounce job ended (it is no longer listed).
    pub(crate) fn bounce_poll(&mut self) {
        if let Some(id) = self.bounce.job
            && !self.bounce_running()
        {
            self.bounce_finished(id);
        }
    }

    /// The job ended: the sheet closes. A failure is the document's alert
    /// (polling made it), warnings its Problems banner; a bounce that wrote
    /// or added something says so in the status, each file under its
    /// final name (Ruling 10).
    fn bounce_finished(&mut self, id: u64) {
        if let Some(r) = self.session.bounce_result(id).ok().flatten()
            && let Some(text) = crate::jobs::bounce_status(&r, &[])
        {
            self.status = Some(text);
        }
        self.bounce.job = None;
        self.bounce.shown = false;
    }
}
