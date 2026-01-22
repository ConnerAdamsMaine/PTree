// ptree-driver: Windows service driver for real-time file system change tracking
// Monitors NTFS USN Journal for incremental cache updates

#[cfg(windows)]
pub mod usn_journal;
pub mod error;
pub mod service;
#[cfg(windows)]
pub mod registration;

pub use error::{DriverError, DriverResult};

#[cfg(windows)]
pub use usn_journal::{USNTracker, UsnRecord, USNJournalState, ChangeType};

pub use service::{PtreeService, ServiceConfig, ServiceStatus};

/// Driver version
pub const DRIVER_VERSION: &str = env!("CARGO_PKG_VERSION");
