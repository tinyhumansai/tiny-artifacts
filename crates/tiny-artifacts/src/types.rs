//! Versioned, author-tracked artifacts: the shared model for deliverables
//! an agent creates and later revises.
//!
//! An artifact is a record with an ordered [`ArtifactVersion`] list. Nothing
//! is ever overwritten: a revision appends a new version by some [`ArtifactAuthor`],
//! which is what makes [`ArtifactRecord::human_edit_diff`] answerable at any
//! later point.

use serde::{Deserialize, Serialize};

use crate::file_ref::FileRef;

/// What an artifact holds. Drives the consumer's rendering choice; the body
/// is always text on the wire (a binary artifact carries a reference, not
/// bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    /// Plain text — the default for an agent reply.
    Text,
    /// Markdown source (a draft, a post).
    Markdown,
    /// HTML source (a page, a report).
    Html,
    /// A reference to a generated image (URL or workspace path, not bytes).
    Image,
    /// A reference to a generated file (workspace path, not bytes).
    File,
}

impl ArtifactKind {
    /// The stable wire word, for logs and route bodies.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactKind::Text => "text",
            ArtifactKind::Markdown => "markdown",
            ArtifactKind::Html => "html",
            ArtifactKind::Image => "image",
            ArtifactKind::File => "file",
        }
    }

    /// Whether this kind's content is a file reference rather than text.
    #[must_use]
    pub fn holds_file(self) -> bool {
        matches!(self, Self::Image | Self::File)
    }

    /// Parse a wire word (case-insensitive). `None` for anything unknown, so
    /// a caller can reject rather than silently remap a misspelled kind.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "text" => Some(Self::Text),
            "markdown" | "md" => Some(Self::Markdown),
            "html" => Some(Self::Html),
            "image" => Some(Self::Image),
            "file" => Some(Self::File),
            _ => None,
        }
    }
}

/// Who produced a version.
///
/// The point of the port: this makes a human's pre-approval edit distinguishable
/// from the agent's own re-run, and therefore makes the quality signal
/// (`human_edit_diff`) computable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactAuthor {
    /// An agent turn produced it.
    Agent,
    /// A human edited it (typically just before approving).
    Operator,
}

/// One immutable revision of an artifact.
///
/// Versions are append-only and 1-based. A version is never edited in place —
/// a change appends a new one — so the full provenance chain survives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactVersion {
    /// 1-based revision number, contiguous within the artifact.
    pub version: u32,
    /// The revision's content.
    pub body: String,
    /// Whether an agent or a human wrote this revision.
    pub author: ArtifactAuthor,
    /// The agent id, or the operator's label. Free-form and display-only.
    pub author_id: String,
    /// Epoch-millis the revision was recorded.
    pub created_at_millis: u64,
    /// Why this revision exists (e.g. `"operator edit before approval"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// For an `image` or `file` artifact, the checked file this revision
    /// points at (its `body` holds the same path, for older readers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<FileRef>,
    /// Where this revision was shared, when it was. The one field set after a
    /// version is recorded: sharing does not change what the revision says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<PublishedLink>,
}

/// A link a revision was shared under (see [`crate::ArtifactPublisher`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedLink {
    /// The shareable URL.
    pub url: String,
    /// Epoch-millis the link stops working, when it expires.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_millis: Option<u64>,
    /// Epoch-millis it was shared.
    pub published_at_millis: u64,
    /// Whether the link is public (anyone can open it) rather than private
    /// and time-limited.
    #[serde(default)]
    pub public: bool,
}

impl PublishedLink {
    /// Whether the link still works at `now_millis`.
    #[must_use]
    pub fn is_live(&self, now_millis: u64) -> bool {
        self.expires_at_millis.is_none_or(|at| at > now_millis)
    }
}

/// Where an artifact is in its life.
///
/// A producer that takes time (rendering a deck, generating an image) opens
/// the artifact `Pending` so the host can show it in progress, then completes
/// or fails it. An artifact made in one step is `Ready` from the start, and a
/// record written before this field existed reads as `Ready`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactStatus {
    /// Being produced; may have no version yet.
    Pending,
    /// Has at least one finished version.
    #[default]
    Ready,
    /// Production failed; see [`ArtifactRecord::error`].
    Failed,
}

impl ArtifactStatus {
    /// The stable wire word.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

impl ArtifactVersion {
    /// A new revision with defaults for the optional fields.
    pub fn new(
        version: u32,
        body: impl Into<String>,
        author: ArtifactAuthor,
        author_id: impl Into<String>,
        created_at_millis: u64,
    ) -> Self {
        Self {
            version,
            body: body.into(),
            author,
            author_id: author_id.into(),
            created_at_millis,
            note: None,
            file: None,
            published: None,
        }
    }
}

/// A versioned output: the durable deliverable an agent produces and revises.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRecord {
    /// Stable id within the store.
    pub id: String,
    /// A short display title.
    pub title: String,
    /// What it holds.
    pub kind: ArtifactKind,
    /// Every revision, oldest first. Never empty in practice — an artifact is
    /// created with its first version — but the accessors tolerate empty so a
    /// hand-written or truncated record cannot panic a read route.
    pub versions: Vec<ArtifactVersion>,
    /// Epoch-millis the artifact was first created.
    pub created_at_millis: u64,
    /// Epoch-millis of the newest revision.
    pub updated_at_millis: u64,
    /// Where the artifact is in its life. Absent on older records: `Ready`.
    #[serde(default)]
    pub status: ArtifactStatus,
    /// Why production failed, when `status` is `Failed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ArtifactRecord {
    /// Creates an artifact with its first (agent-authored) version.
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        kind: ArtifactKind,
        body: impl Into<String>,
        author_id: impl Into<String>,
        at_millis: u64,
    ) -> Self {
        let id = id.into();
        Self {
            id,
            title: title.into(),
            kind,
            versions: vec![ArtifactVersion::new(
                1,
                body,
                ArtifactAuthor::Agent,
                author_id,
                at_millis,
            )],
            created_at_millis: at_millis,
            updated_at_millis: at_millis,
            status: ArtifactStatus::Ready,
            error: None,
        }
    }

    /// Opens an artifact that is still being produced: no versions yet,
    /// status `Pending`.
    pub fn pending(
        id: impl Into<String>,
        title: impl Into<String>,
        kind: ArtifactKind,
        at_millis: u64,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            kind,
            versions: Vec::new(),
            created_at_millis: at_millis,
            updated_at_millis: at_millis,
            status: ArtifactStatus::Pending,
            error: None,
        }
    }

    /// Whether the artifact's content is a file (`image` / `file` kinds).
    #[must_use]
    pub fn holds_file(&self) -> bool {
        self.kind.holds_file()
    }

    /// The newest revision, or `None` on an empty record.
    #[must_use]
    pub fn latest(&self) -> Option<&ArtifactVersion> {
        self.versions.last()
    }

    /// A revision by its 1-based number.
    #[must_use]
    pub fn version(&self, version: u32) -> Option<&ArtifactVersion> {
        self.versions.iter().find(|v| v.version == version)
    }

    /// Appends a revision, numbering it after the current newest and advancing
    /// `updated_at_millis`. Returns the new version number.
    ///
    /// Numbering is derived from the existing max rather than `len()`, so a
    /// record that ever lost a row mid-history still numbers monotonically
    /// instead of silently reusing a number that already means something else.
    pub fn push_version(&mut self, version: ArtifactVersion) -> u32 {
        let next = self.versions.iter().map(|v| v.version).max().unwrap_or(0) + 1;
        self.versions.push(ArtifactVersion {
            version: next,
            ..version
        });
        self.updated_at_millis = self
            .versions
            .last()
            .map_or(self.updated_at_millis, |v| v.created_at_millis);
        next
    }

    /// The diff of what a human changed before approving: the last
    /// agent-authored revision → the last operator-authored revision after it.
    ///
    /// `None` when no human has edited or when no agent version precedes the
    /// edit.
    #[must_use]
    pub fn human_edit_diff(&self) -> Option<crate::diff::ArtifactDiff> {
        let last_operator = self
            .versions
            .iter()
            .rev()
            .find(|v| v.author == ArtifactAuthor::Operator)?;
        let last_agent_before = self
            .versions
            .iter()
            .rfind(|v| v.author == ArtifactAuthor::Agent && v.version < last_operator.version)?;
        Some(crate::diff::ArtifactDiff::between(
            last_agent_before,
            last_operator,
        ))
    }
}
