//! Share a version of an artifact (opt-in: only offered when the host can
//! share, see [`crate::Artifacts::can_publish`]).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tinytools::{PermissionLevel, Tool, ToolResult};

use crate::publish::PublishOptions;
use crate::service::Artifacts;
use crate::tools::{read_required_str, tool_error};

/// Share a version of an artifact and return its link.
pub struct ShareArtifactTool {
    artifacts: Arc<Artifacts>,
}

impl std::fmt::Debug for ShareArtifactTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShareArtifactTool").finish_non_exhaustive()
    }
}

impl ShareArtifactTool {
    /// Constructs the tool bound to one service.
    #[must_use]
    pub fn new(artifacts: Arc<Artifacts>) -> Self {
        Self { artifacts }
    }
}

#[async_trait]
impl Tool for ShareArtifactTool {
    fn name(&self) -> &str {
        "share_artifact"
    }

    fn description(&self) -> &str {
        "Share an artifact with people outside this app and get a link. By \
         default the newest version is shared through a private link that \
         expires; `public: true` asks for a link anyone can open instead. How \
         long links last is up to the host. The link is remembered on that \
         version, so sharing it again returns the same link while it works. \
         Only when the user asks to share or send something: sharing may \
         upload the artifact and can cost the user."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "artifact_id": {
                    "type": "string",
                    "description": "The artifact to share."
                },
                "version": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Which version to share. Default: the newest."
                },
                "public": {
                    "type": "boolean",
                    "description": "A stable public link instead of a private, expiring one. Default false."
                },
                "expires_in_seconds": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "How long a private link should work. The host may shorten it to what it supports; default: the host's."
                },
                "fresh": {
                    "type": "boolean",
                    "description": "Make a new link even if this version already has one that works."
                }
            },
            "required": ["artifact_id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Execute
    }

    fn external_effect(&self) -> bool {
        true
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let id = read_required_str(&args, "artifact_id")?;
        let version = match args.get("version") {
            None | Some(Value::Null) => None,
            Some(v) => Some(
                v.as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| anyhow::anyhow!("`version` must be a positive integer"))?,
            ),
        };
        let options = PublishOptions {
            public: args.get("public").and_then(Value::as_bool).unwrap_or(false),
            expires_in_seconds: args
                .get("expires_in_seconds")
                .and_then(Value::as_u64)
                .filter(|s| *s >= 1),
            fresh: args.get("fresh").and_then(Value::as_bool).unwrap_or(false),
        };
        match self.artifacts.publish(&id, version, &options).await {
            Ok(published) => Ok(ToolResult::success(
                json!({
                    "artifact_id": id,
                    "version": published.version,
                    "url": published.link.url,
                    "public": published.link.public,
                    "expires_at_millis": published.link.expires_at_millis,
                    "reused": published.reused,
                })
                .to_string(),
            )),
            Err(err) => tool_error(err),
        }
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        false
    }
}
