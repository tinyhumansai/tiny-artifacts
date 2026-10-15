//! The [`ArtifactPublisher`] port: sharing a version outside the host.
//!
//! The crate knows that a version can be shared and remembers the link on that
//! version ([`crate::PublishedLink`]), so a later edit can tell the shared copy
//! is behind, and sharing the same version again reuses a link that still
//! works. *How* it is shared — an upload to the host's cloud storage, a post
//! to a board — is the host's, behind this trait.

use async_trait::async_trait;

use crate::types::{ArtifactRecord, ArtifactVersion, PublishedLink};

/// How a version should be shared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PublishOptions {
    /// A stable public link anyone can open, instead of a private,
    /// time-limited one.
    pub public: bool,
    /// How long a private link should work. The host may shorten it to what
    /// it supports. `None`: the host's default.
    pub expires_in_seconds: Option<u64>,
    /// Make a new link even when the version already has one that works.
    pub fresh: bool,
}

/// Shares one version of an artifact. Implemented by the host.
#[async_trait]
pub trait ArtifactPublisher: Send + Sync {
    /// Share `version` of `record` and return where it can be reached.
    ///
    /// # Errors
    ///
    /// Returns an error when the host cannot share it (not allowed, not signed
    /// in, quota, network); nothing is recorded on the version then.
    async fn publish(
        &self,
        record: &ArtifactRecord,
        version: &ArtifactVersion,
        options: &PublishOptions,
    ) -> anyhow::Result<PublishedLink>;
}
