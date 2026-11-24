//! The bars above a document's editors (Swift `ProjectBanner`,
//! `ProjectBanners`): running jobs, media in another format, a device at
//! another rate, disk underruns and a finished job's problems. The model
//! gives each bar's text and button; the UI draws them.

use std::sync::atomic::{AtomicBool, Ordering};

use vizdaw_ffi::JobKindRow;
use vizdaw_ffi::snapshot::TransportState;

use crate::document::DocumentModel;
use crate::jobs::job_text;

/// The transport fault of a stream that read audio too late.
pub const UNDERRUN: &str = "stream-underrun";

/// One bar, top first.
#[derive(Debug, Clone, PartialEq)]
pub enum ProjectBanner {
    /// A job running, with its progress (0–1).
    Job {
        id: u64,
        kind: JobKindRow,
        progress: f32,
    },
    /// Media not in the project format: Convert.
    Mismatched { count: u32, label: String },
    /// The device would not switch to the project rate: regions are muted.
    RateMismatch { device: f64, project: u32 },
    /// Audio was not read from disk in time.
    DiskTooSlow,
    /// Files a finished job could not handle.
    Problems(Vec<String>),
}

/// What a bar's button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerAction {
    CancelJob(u64),
    Convert,
    ChangeRateToDevice,
    DismissDiskWarning,
    DismissProblems,
}

/// `44 kHz`-style rate text: whole kHz bare, else one decimal.
pub fn khz(hz: f64) -> String {
    let k = hz / 1000.0;
    if k.round() == k {
        format!("{} kHz", k as i64)
    } else {
        format!("{k:.1} kHz")
    }
}

impl ProjectBanner {
    /// The bar's text (a job's is also its progress bar's label; the
    /// problems' lines are joined by newlines, shown at most 3 lines).
    pub fn text(&self) -> String {
        match self {
            Self::Job { kind, .. } => job_text(*kind).into(),
            Self::Mismatched { count, label } => format!(
                "{count} media file{} not in the project format ({label}) and stay silent.",
                if *count == 1 { " is" } else { "s are" }
            ),
            Self::RateMismatch { device, project } => format!(
                "The audio device runs at {}; this project is {}. Audio regions are muted.",
                khz(*device),
                khz(f64::from(*project))
            ),
            Self::DiskTooSlow => "Disk too slow: some audio played as silence.".into(),
            Self::Problems(lines) => lines.join("\n"),
        }
    }

    /// The button's title.
    pub fn button(&self) -> String {
        match self {
            Self::Job { .. } => "Cancel".into(),
            Self::Mismatched { count, .. } => {
                format!("Convert {count} File{}", if *count == 1 { "" } else { "s" })
            }
            Self::RateMismatch { device, .. } => {
                format!("Change Project Rate to {}", khz(*device))
            }
            Self::DiskTooSlow | Self::Problems(_) => "Dismiss".into(),
        }
    }

    /// What the button does.
    pub fn action(&self) -> BannerAction {
        match self {
            Self::Job { id, .. } => BannerAction::CancelJob(*id),
            Self::Mismatched { .. } => BannerAction::Convert,
            Self::RateMismatch { .. } => BannerAction::ChangeRateToDevice,
            Self::DiskTooSlow => BannerAction::DismissDiskWarning,
            Self::Problems(_) => BannerAction::DismissProblems,
        }
    }
}

/// A transport read: the disk warning shows again at the next underrun
/// once a read has none (Swift `poll`).
pub(crate) fn note_transport(dismissed: &AtomicBool, t: &TransportState) {
    if !t.faults.iter().any(|f| f == UNDERRUN) {
        dismissed.store(false, Ordering::Relaxed);
    }
}

impl DocumentModel {
    /// What the bars above the editors show, top first.
    pub fn banners(&self) -> Vec<ProjectBanner> {
        let mut out: Vec<ProjectBanner> = self
            .jobs
            .iter()
            .map(|j| ProjectBanner::Job {
                id: j.id,
                kind: j.kind,
                progress: j.progress,
            })
            .collect();
        if let Some(i) = &self.project_info {
            if i.rate_mismatch {
                out.push(ProjectBanner::RateMismatch {
                    device: i.device_rate,
                    project: i.format.sample_rate,
                });
            }
            let converting = self
                .jobs
                .iter()
                .any(|j| matches!(j.kind, JobKindRow::Convert | JobKindRow::Format));
            if i.mismatched > 0 && !converting {
                out.push(ProjectBanner::Mismatched {
                    count: i.mismatched,
                    label: vizdaw_ffi::format_label(i.format),
                });
            }
        }
        let underrun = self.last_transport().faults.iter().any(|f| f == UNDERRUN);
        if underrun && !self.disk_warning_dismissed.load(Ordering::Relaxed) {
            out.push(ProjectBanner::DiskTooSlow);
        }
        if !self.job_problems.is_empty() {
            out.push(ProjectBanner::Problems(self.job_problems.clone()));
        }
        out
    }

    /// Runs a bar's button.
    pub fn run_banner(&mut self, action: BannerAction) {
        match action {
            BannerAction::CancelJob(id) => self.cancel_job(id),
            BannerAction::Convert => self.convert_media(),
            BannerAction::ChangeRateToDevice => self.change_rate_to_device(),
            BannerAction::DismissDiskWarning => self.dismiss_disk_warning(),
            BannerAction::DismissProblems => self.dismiss_problems(),
        }
    }

    pub fn dismiss_disk_warning(&mut self) {
        self.disk_warning_dismissed.store(true, Ordering::Relaxed);
    }

    pub fn dismiss_problems(&mut self) {
        self.job_problems.clear();
    }
}
