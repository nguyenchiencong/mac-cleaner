pub mod apply;
pub mod config;
pub mod journal;
pub mod model;
pub mod plan;
pub mod platform;
pub mod restore;
pub mod scan;
pub mod scanners;

pub use apply::{ApplyEngine, ApplyError, ApplySummary};
pub use config::{ConfigError, ConfigProfile, WorkspaceRoot};
pub use journal::{JournalReader, JournalWriter};
pub use model::*;
pub use plan::PlanBuilder;
pub use platform::*;
pub use restore::{RestorationError, RestoreEngine, RestoreSummary};
pub use scan::{calculate_path_size, walk_directory_safely};
pub use scanners::{ScanContext, ScanResult, Scanner, ScannerError, ScannerRegistry};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
