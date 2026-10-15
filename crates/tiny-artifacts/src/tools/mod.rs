//! LLM-callable tools over the artifact domain, all driving one
//! [`Artifacts`] service.
//!
//! The default set ([`registered_tools`]) is `create_artifact`,
//! `read_artifact`, `update_artifact` and `list_artifacts`.
//! `delete_artifact` ([`DeleteArtifactTool`]) is separate because deleting is
//! destructive, and `share_artifact` ([`ShareArtifactTool`]) because it needs
//! a publisher: a host opts in to each.

mod create;
mod delete;
mod list;
mod read;
mod share;
mod update;

pub use create::CreateArtifactTool;
pub use delete::DeleteArtifactTool;
pub use list::ListArtifactsTool;
pub use read::ReadArtifactTool;
pub use share::ShareArtifactTool;
pub use update::UpdateArtifactTool;

use std::sync::Arc;

use serde_json::json;
use tinytools::{Tool, ToolResult};

use crate::service::{Artifacts, ArtifactsError, Author, Content, Saved};
use crate::types::ArtifactKind;

/// Build the default tools in their persisted catalogue order, bound to one
/// service. Versions the tools write are attributed to `author` — the agent
/// the tools are built for (a run context carries no agent identity, so a host
/// that builds tools per agent passes each its own).
#[must_use]
pub fn registered_tools(artifacts: Arc<Artifacts>, author: Author) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(CreateArtifactTool::new(artifacts.clone(), author.clone())),
        Box::new(ReadArtifactTool::new(artifacts.clone())),
        Box::new(UpdateArtifactTool::new(artifacts.clone(), author)),
        Box::new(ListArtifactsTool::new(artifacts)),
    ]
}

/// Shared helper: read a required, non-empty string arg.
pub(crate) fn read_required_str(args: &serde_json::Value, key: &str) -> anyhow::Result<String> {
    optional_str(args, key)
        .ok_or_else(|| anyhow::anyhow!("missing required string argument `{key}`"))
}

/// Shared helper: read an optional, non-empty string arg.
pub(crate) fn optional_str(args: &serde_json::Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// The content a create/update call carries for `kind`: `body` for text
/// kinds, `path` for file kinds (`body` is accepted as the path too, for
/// callers written before `path` existed).
///
/// A missing argument is an `Err`: the call was malformed (tinytools'
/// convention — argument failures are errors, domain refusals are tool
/// results).
pub(crate) fn content_for(kind: ArtifactKind, args: &serde_json::Value) -> anyhow::Result<Content> {
    if kind.holds_file() {
        optional_str(args, "path")
            .or_else(|| optional_str(args, "body"))
            .map(Content::File)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "a `{}` artifact needs `path`: the file to file",
                    kind.as_str()
                )
            })
    } else {
        // Text is taken as written: surrounding whitespace can be content.
        args.get("body")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(|s| Content::Text(s.to_string()))
            .ok_or_else(|| {
                anyhow::anyhow!("a `{}` artifact needs `body`: its content", kind.as_str())
            })
    }
}

/// A tool result for a write.
pub(crate) fn saved_result(saved: &Saved) -> ToolResult {
    let mut out = json!({
        "artifact_id": saved.record.id,
        "title": saved.record.title,
        "kind": saved.record.kind.as_str(),
        "version": saved.version,
        "status": saved.record.status.as_str(),
        "appended": saved.appended,
    });
    if let Some(file) = saved.record.latest().and_then(|v| v.file.as_ref()) {
        out["path"] = json!(file.path);
    }
    if let Some(notice) = &saved.notice {
        out["notice"] = json!(notice);
    }
    ToolResult::success(out.to_string())
}

/// Turn a service error into what the agent sees. A malformed call (content
/// that does not fit the kind) and a backend failure are errors; a refusal
/// about the world — no such artifact, a missing or out-of-reach file — is a
/// tool result the agent can act on.
pub(crate) fn tool_error(err: ArtifactsError) -> anyhow::Result<ToolResult> {
    match err {
        ArtifactsError::Backend(err) => Err(err),
        err @ (ArtifactsError::WrongContent { .. } | ArtifactsError::EmptyText) => {
            Err(anyhow::anyhow!(err))
        }
        other => Ok(ToolResult::error(other.to_string())),
    }
}
