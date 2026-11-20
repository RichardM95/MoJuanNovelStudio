pub mod core;
pub use core::*;
mod draft_recovery;
pub use draft_recovery::*;
#[cfg(all(feature = "desktop", target_os = "windows"))]
pub mod desktop;
pub mod files;

pub mod workflow;
pub use workflow::*;
pub mod agent;
pub mod agent_stdio;
pub mod ipc;

pub mod materials;
pub use materials::*;

pub mod search;
pub mod retrieval;
pub use search::{SearchHit, SearchResults};
pub mod material_workflow;
pub use material_workflow::{MaterialProposal, MaterialProposalInput};

pub mod context_jobs;
pub use context_jobs::{Task, TaskDetail, TaskItem};
pub mod task_runner;
pub use task_runner::TaskRunner;

pub mod history_browser;
pub use history_browser::{VersionDetail, VersionPage, VersionSummary};

pub mod backup_restore;
pub use backup_restore::{
    prepare_restore, prepare_restore_with_cancel, BackupProgress, PreparedRestore, RestoreGuard,
    RestoreOutcome, RestoreSummary,
};

pub mod backup_jobs;
pub use backup_jobs::{BackupJob, BackupJobManager, BackupResult};

pub mod book_replace;
pub use book_replace::{ReplacePreview, ReplaceChapter, ReplaceDetail, ReplaceOutcome};

pub mod txt_import;
pub use txt_import::{ImportPreview,ImportVolume,ImportChapter,ImportDetail,ImportOutcome,ImportSourceHeading};
pub mod project_import;
pub use project_import::{
    ProjectImportOptions, ProjectImportOutcome, ProjectImportPreview,
    ProjectImportRoles, ProjectImportSelection, ProjectLink,
};

pub mod txt_export;
pub use txt_export::{ExportOptions,ExportPreview};

pub mod chapter_split;
pub use chapter_split::{SplitPreview,SplitOutcome};

pub mod material_history;
pub use material_history::{MaterialContentVersion,MaterialVersionSummary,MaterialVersionPage};

pub mod constrained_writing;
pub use constrained_writing::*;

mod foreshadow_proposals;
pub use foreshadow_proposals::{DecideForeshadowProposal, ForeshadowProposal, ForeshadowProposalAction, ForeshadowProposalInput, ForeshadowProposalPage, ForeshadowProposalReceipt, ForeshadowProposalStatus, ForeshadowProposalSummary};
pub mod foreshadow;
pub use foreshadow::{CreateForeshadowOccurrence, ForeshadowChapterExcerpt, ForeshadowOccurrence, ForeshadowOccurrencePage, ForeshadowOccurrenceView, ForeshadowPosition, ForeshadowRole, ForeshadowSourceStatus, ForeshadowSourceWindow, ForeshadowSummary};
