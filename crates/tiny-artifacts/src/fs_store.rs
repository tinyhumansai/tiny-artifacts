//! [`FsArtifactStore`]: a ready-made [`ArtifactStore`] on the local file system.
//!
//! One JSON file per artifact, `<dir>/<id>.json`, holding the whole record.
//! The host picks the directory; the store creates it on first write.
//!
//! * Writes are atomic: the record goes to a temporary file in the same
//!   directory and is renamed over the old one, so a crash mid-write never
//!   leaves a half-written record.
//! * Ids are checked before they become file names: no separators, no `.` or
//!   `..`, nothing empty.
//! * A record that cannot be parsed is skipped by [`ArtifactStore::list`] (with
//!   a warning) so one bad file does not hide every other artifact, but
//!   reported as an error by [`ArtifactStore::get`].

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;

use crate::store::ArtifactStore;
use crate::types::ArtifactRecord;

/// Artifacts stored as one JSON file each in a directory.
#[derive(Clone, Debug)]
pub struct FsArtifactStore {
    dir: PathBuf,
}

impl FsArtifactStore {
    /// A store keeping its records in `dir`.
    #[must_use]
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The directory records are kept in.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn record_path(&self, id: &str) -> Result<PathBuf> {
        validate_id(id)?;
        Ok(self.dir.join(format!("{id}.json")))
    }
}

/// Reject ids that could escape the directory or collide with its own files.
fn validate_id(id: &str) -> Result<()> {
    if id.is_empty() || id == "." || id == ".." {
        bail!("invalid artifact id {id:?}");
    }
    if id.contains(['/', '\\', '\0']) {
        bail!("artifact id must not contain path separators: {id:?}");
    }
    if id.starts_with('.') {
        bail!("artifact id must not start with '.': {id:?}");
    }
    Ok(())
}

#[async_trait]
impl ArtifactStore for FsArtifactStore {
    async fn list(&self) -> Result<Vec<ArtifactRecord>> {
        let mut entries = match tokio::fs::read_dir(&self.dir).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(err).with_context(|| format!("cannot list {}", self.dir.display()))
            }
        };
        let mut records = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let bytes = tokio::fs::read(&path).await?;
            match serde_json::from_slice::<ArtifactRecord>(&bytes) {
                Ok(record) => records.push(record),
                Err(err) => tracing::warn!(
                    target: "tiny-artifacts",
                    file = ?path.file_name(),
                    err = %err,
                    "skipping an unreadable artifact record"
                ),
            }
        }
        records.sort_by(|a, b| b.updated_at_millis.cmp(&a.updated_at_millis));
        Ok(records)
    }

    async fn get(&self, id: &str) -> Result<Option<ArtifactRecord>> {
        let path = self.record_path(id)?;
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err).with_context(|| format!("cannot read artifact {id}")),
        };
        let record = serde_json::from_slice(&bytes)
            .with_context(|| format!("artifact {id} is not a valid record"))?;
        Ok(Some(record))
    }

    async fn upsert(&self, artifact: &ArtifactRecord) -> Result<()> {
        let path = self.record_path(&artifact.id)?;
        tokio::fs::create_dir_all(&self.dir)
            .await
            .with_context(|| format!("cannot create {}", self.dir.display()))?;
        let bytes = serde_json::to_vec(artifact)?;
        // Same directory, so the rename is atomic on one file system.
        let staging = self
            .dir
            .join(format!(".{}.{}.tmp", artifact.id, uuid::Uuid::new_v4()));
        tokio::fs::write(&staging, bytes)
            .await
            .with_context(|| format!("cannot write artifact {}", artifact.id))?;
        if let Err(err) = tokio::fs::rename(&staging, &path).await {
            let _ = tokio::fs::remove_file(&staging).await;
            return Err(err).with_context(|| format!("cannot save artifact {}", artifact.id));
        }
        Ok(())
    }

    async fn delete(&self, id: &str) -> Result<bool> {
        let path = self.record_path(id)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(err) => Err(err).with_context(|| format!("cannot delete artifact {id}")),
        }
    }
}
