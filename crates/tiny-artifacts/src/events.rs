//! The [`ArtifactObserver`] port: how a host hears about artifact changes.
//!
//! The crate decides *what* happened to an artifact; the host decides what
//! that means on its side — a card in a chat, a row on a board, a copy of the
//! file in a folder the user can open. [`crate::Artifacts`] calls the observer
//! after every successful write, with the record as stored.
//!
//! An observer failure never undoes the write: the record is the source of
//! truth, and whatever the host derives from it can be rebuilt from it. The
//! error is reported back to the caller as a notice instead.

use async_trait::async_trait;

use crate::types::ArtifactRecord;

/// What just happened to an artifact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactEventKind {
    /// Opened and still being produced.
    Pending,
    /// A new version is the newest one (a creation, a revision, or a pending
    /// artifact completing).
    Ready,
    /// Production failed.
    Failed,
    /// Deleted.
    Deleted,
}

/// One change, with the record as it now stands (for `Deleted`, as it stood).
#[derive(Clone, Debug)]
pub struct ArtifactEvent {
    /// What happened.
    pub kind: ArtifactEventKind,
    /// The record.
    pub record: ArtifactRecord,
}

/// Receives artifact changes. Implemented by the host.
#[async_trait]
pub trait ArtifactObserver: Send + Sync {
    /// Called after each successful write.
    ///
    /// # Errors
    ///
    /// An error is reported to the caller as a notice; the write stands.
    async fn on_event(&self, event: &ArtifactEvent) -> anyhow::Result<()>;
}
