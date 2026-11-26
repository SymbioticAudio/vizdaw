//! Application logic for the VizDAW UI. Never depends on GPUI.
#![recursion_limit = "256"]

pub mod aaf;
pub mod alerts;
pub mod arrangement;
pub mod audio_format;
pub mod audio_prefs;
pub mod audition;
pub mod automation_prefs;
pub mod banners;
pub mod bar_items;
pub mod bounce;
pub mod bounce_commands;
pub mod builtin_synth_window;
pub mod close_prompts;
pub mod commands;
pub mod dawproject;
pub mod document;
pub mod editor;
pub mod for_daw;
pub mod formats;
pub mod growing_take;
pub mod interchange;
pub mod interchange_commands;
pub mod jobs;
pub mod lane_inputs;
pub mod list;
pub mod marker_list;
pub mod merge;
pub mod mixer;
pub mod panels;
pub mod piano_roll;
pub mod plugin_window;
pub mod position;
pub mod prefs;
pub mod project_settings;
pub mod record;
pub mod region_waves;
pub mod relink;
pub mod report_text;
pub mod session;
pub mod step_entry;
pub mod store;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod transport;
pub mod transport_bar;
pub mod transport_fields;
pub mod view;

pub use alerts::AlertItem;
pub use banners::{BannerAction, ProjectBanner};
pub use document::{DocumentModel, SaveAction, SaveChoice};
pub use jobs::JobAnswer;
pub use prefs::Prefs;
pub use session::{CoreCalls, DocSession, LiveCore};
pub use store::{DocId, DocumentStore, WindowToken};
