//! Versioned artifacts: the deliverables an agent creates and revises.
//!
//! An artifact is a record with an ordered version list. Nothing is ever
//! overwritten: a revision appends a new version by an agent or a human, which
//! is what makes [`ArtifactRecord::human_edit_diff`] answerable at any later
//! point. Text artifacts carry their content; `image` and `file` artifacts
//! carry a checked [`FileRef`] to a file on the host's disk.
//!
//! # How a host uses it
//!
//! [`Artifacts`] is the single entry point — the agent's tools
//! ([`registered_tools`]) and the host's own producers both go through it. The
//! host supplies the ports:
//!
//! * [`ArtifactStore`] — where records are kept ([`FsArtifactStore`] is a
//!   ready-made one: a JSON file per artifact in a directory);
//! * [`ArtifactObserver`] — told after every write, to show the artifact
//!   (a card, a board row) and place its file where the user can open it;
//! * [`ArtifactPublisher`] — how a version is shared, when the host can;
//! * [`FileRoots`] — the folders file artifacts may point into.
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use tiny_artifacts::{registered_tools, ArtifactStore, Artifacts, Author, FileRoots};
//!
//! fn tools(store: Arc<dyn ArtifactStore>, workdir: std::path::PathBuf) {
//!     let artifacts = Arc::new(Artifacts::new(store).with_file_roots(FileRoots::new([workdir])));
//!     let _tools = registered_tools(artifacts, Author::agent("my-agent"));
//! }
//! ```

mod diff;
pub mod events;
pub mod file_ref;
pub mod fs_store;
pub mod publish;
pub mod service;
pub mod store;
pub mod tools;
pub mod types;

pub use diff::{ArtifactDiff, DiffLine, DiffOp, MAX_DIFF_LINES};
pub use events::{ArtifactEvent, ArtifactEventKind, ArtifactObserver};
pub use file_ref::{media_type_for, FileRef, FileRefError, FileRoots};
pub use fs_store::FsArtifactStore;
pub use publish::{ArtifactPublisher, PublishOptions};
pub use service::{Artifacts, ArtifactsError, Author, Content, Published, Saved};
pub use store::ArtifactStore;
pub use tools::{
    registered_tools, CreateArtifactTool, DeleteArtifactTool, ListArtifactsTool, ReadArtifactTool,
    ShareArtifactTool, UpdateArtifactTool,
};
pub use types::{
    ArtifactAuthor, ArtifactKind, ArtifactRecord, ArtifactStatus, ArtifactVersion, PublishedLink,
};

/// Helper: current unix epoch millis.
#[must_use]
pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
