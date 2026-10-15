//! List artifacts in the store, newest first.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tinytools::{Tool, ToolResult};

use crate::service::Artifacts;

/// List all artifacts, newest first, returning their records.
pub struct ListArtifactsTool {
    artifacts: Arc<Artifacts>,
}

impl std::fmt::Debug for ListArtifactsTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListArtifactsTool").finish_non_exhaustive()
    }
}

impl ListArtifactsTool {
    /// Constructs the tool bound to one service.
    #[must_use]
    pub fn new(artifacts: Arc<Artifacts>) -> Self {
        Self { artifacts }
    }
}

#[async_trait]
impl Tool for ListArtifactsTool {
    fn name(&self) -> &'static str {
        "list_artifacts"
    }

    fn description(&self) -> &'static str {
        "List every artifact — documents, pages, images and files — most \
         recently updated first, each with its version history and status. \
         Use this to find artifact ids for `read_artifact` or `update_artifact`."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        let records = self.artifacts.store().list().await?;
        Ok(ToolResult::success(serde_json::to_string(&records)?))
    }

    fn is_concurrency_safe(&self, _args: &Value) -> bool {
        true
    }
}
