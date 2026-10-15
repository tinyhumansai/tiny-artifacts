//! Delete an artifact (opt-in: not part of the default tool set).

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};
use tinytools::{PermissionLevel, Tool, ToolResult};

use crate::service::Artifacts;
use crate::tools::{read_required_str, tool_error};

/// Delete an artifact and its history.
pub struct DeleteArtifactTool {
    artifacts: Arc<Artifacts>,
}

impl std::fmt::Debug for DeleteArtifactTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeleteArtifactTool").finish_non_exhaustive()
    }
}

impl DeleteArtifactTool {
    /// Constructs the tool bound to one service.
    #[must_use]
    pub fn new(artifacts: Arc<Artifacts>) -> Self {
        Self { artifacts }
    }
}

#[async_trait]
impl Tool for DeleteArtifactTool {
    fn name(&self) -> &str {
        "delete_artifact"
    }

    fn description(&self) -> &str {
        "Permanently delete an artifact and every version of it. Only when the \
         user asks for it to be removed."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "artifact_id": {
                    "type": "string",
                    "description": "The artifact id to delete."
                }
            },
            "required": ["artifact_id"]
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Dangerous
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let id = read_required_str(&args, "artifact_id")?;
        match self.artifacts.delete(&id).await {
            Ok((true, notice)) => Ok(ToolResult::success(
                json!({ "artifact_id": id, "deleted": true, "notice": notice }).to_string(),
            )),
            Ok((false, _)) => Ok(ToolResult::error(format!("artifact `{id}` not found"))),
            Err(err) => tool_error(err),
        }
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        false
    }
}
