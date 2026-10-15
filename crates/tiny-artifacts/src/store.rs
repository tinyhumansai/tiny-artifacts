//! The [`ArtifactStore`] port: durable storage for versioned artifacts.
//!
//! The trait is the contract; [`crate::FsArtifactStore`] is a ready-made
//! backend on the local file system. A host (`OpenHuman`, `OpenCompany`) may
//! implement it against its own storage instead; the service and the tools
//! drive it and know nothing about where bytes land.

use anyhow::Result;
use async_trait::async_trait;

use crate::types::ArtifactRecord;

/// Durable storage for artifacts. Writes replace whole records — a caller
/// mutates the record (e.g. `push_version`) and upserts the result.
#[async_trait]
pub trait ArtifactStore: Send + Sync {
    /// Lists artifacts, most-recently-updated first.
    async fn list(&self) -> Result<Vec<ArtifactRecord>>;
    /// Fetches one artifact by id, with its full version history.
    async fn get(&self, id: &str) -> Result<Option<ArtifactRecord>>;
    /// Inserts or replaces an artifact by id.
    async fn upsert(&self, artifact: &ArtifactRecord) -> Result<()>;
    /// Deletes an artifact by id; returns whether one was removed.
    async fn delete(&self, id: &str) -> Result<bool>;
}
