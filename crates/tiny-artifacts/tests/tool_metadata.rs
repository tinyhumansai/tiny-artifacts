//! The tools' contract with the model: names, schemas, permissions, and how a
//! malformed call or a failing store surfaces.

// Unwrapping is fine in tests (AGENTS.md); the workspace lints keep it out of
// library code.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use serde_json::{json, Value};
use tiny_artifacts::{
    registered_tools, ArtifactRecord, ArtifactStore, Artifacts, Author, CreateArtifactTool,
    DeleteArtifactTool, ListArtifactsTool, ReadArtifactTool, ShareArtifactTool, UpdateArtifactTool,
};
use tinytools::{PermissionLevel, Tool};

/// A store that fails every call, as a backend outage would.
struct DownStore;

#[async_trait::async_trait]
impl ArtifactStore for DownStore {
    async fn list(&self) -> anyhow::Result<Vec<ArtifactRecord>> {
        anyhow::bail!("store down")
    }
    async fn get(&self, _id: &str) -> anyhow::Result<Option<ArtifactRecord>> {
        anyhow::bail!("store down")
    }
    async fn upsert(&self, _artifact: &ArtifactRecord) -> anyhow::Result<()> {
        anyhow::bail!("store down")
    }
    async fn delete(&self, _id: &str) -> anyhow::Result<bool> {
        anyhow::bail!("store down")
    }
}

fn down() -> Arc<Artifacts> {
    Arc::new(Artifacts::new(Arc::new(DownStore)))
}

fn author() -> Author {
    Author::agent("agent")
}

fn all_tools(artifacts: &Arc<Artifacts>) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(CreateArtifactTool::new(artifacts.clone(), author())),
        Box::new(ReadArtifactTool::new(artifacts.clone())),
        Box::new(UpdateArtifactTool::new(artifacts.clone(), author())),
        Box::new(ListArtifactsTool::new(artifacts.clone())),
        Box::new(DeleteArtifactTool::new(artifacts.clone())),
        Box::new(ShareArtifactTool::new(artifacts.clone())),
    ]
}

#[test]
fn the_default_set_is_the_four_safe_tools_in_catalogue_order() {
    let names: Vec<String> = registered_tools(down(), author())
        .iter()
        .map(|t| t.name().to_string())
        .collect();
    assert_eq!(
        names,
        [
            "create_artifact",
            "read_artifact",
            "update_artifact",
            "list_artifacts"
        ]
    );
}

#[test]
fn every_tool_describes_itself_and_declares_an_object_schema() {
    for tool in all_tools(&down()) {
        let name = tool.name().to_string();
        assert!(!tool.description().trim().is_empty(), "{name}");
        let schema: Value = tool.parameters_schema();
        assert_eq!(schema["type"], "object", "{name}");
        assert!(schema["properties"].is_object(), "{name}");
        // Whatever a tool requires is declared among its properties.
        for required in schema["required"].as_array().into_iter().flatten() {
            let key = required.as_str().unwrap();
            assert!(schema["properties"].get(key).is_some(), "{name}: {key}");
        }
    }
}

#[test]
fn tools_debug_print_their_type_only() {
    let artifacts = down();
    let shown = [
        format!("{:?}", CreateArtifactTool::new(artifacts.clone(), author())),
        format!("{:?}", ReadArtifactTool::new(artifacts.clone())),
        format!("{:?}", UpdateArtifactTool::new(artifacts.clone(), author())),
        format!("{:?}", ListArtifactsTool::new(artifacts.clone())),
        format!("{:?}", DeleteArtifactTool::new(artifacts.clone())),
        format!("{:?}", ShareArtifactTool::new(artifacts)),
    ];
    for text in shown {
        assert!(
            text.contains("Artifact") && text.ends_with("Tool { .. }"),
            "{text}"
        );
    }
}

#[test]
fn permissions_and_concurrency_match_what_each_tool_does() {
    let artifacts = down();
    let expected = [
        ("create_artifact", PermissionLevel::Write, false),
        ("read_artifact", PermissionLevel::ReadOnly, true),
        ("update_artifact", PermissionLevel::Write, false),
        ("list_artifacts", PermissionLevel::ReadOnly, true),
        ("delete_artifact", PermissionLevel::Dangerous, false),
        ("share_artifact", PermissionLevel::Execute, false),
    ];
    let tools = all_tools(&artifacts);
    for (name, permission, concurrent) in expected {
        let tool = tools.iter().find(|t| t.name() == name).unwrap();
        assert_eq!(tool.permission_level(), permission, "{name}");
        assert_eq!(tool.is_concurrency_safe(&json!({})), concurrent, "{name}");
    }
    let share = tools.iter().find(|t| t.name() == "share_artifact").unwrap();
    assert!(share.external_effect(), "sharing leaves the host");
}

#[tokio::test]
async fn a_failing_store_is_an_error_not_a_tool_result() {
    for tool in all_tools(&down()) {
        let args = match tool.name() {
            "create_artifact" => json!({ "kind": "text", "title": "T", "body": "x" }),
            "list_artifacts" => json!({}),
            _ => json!({ "artifact_id": "a-1", "body": "x" }),
        };
        let name = tool.name().to_string();
        if name == "share_artifact" {
            // No publisher: refused before the store is touched.
            assert!(tool.execute(args).await.unwrap().is_error, "{name}");
        } else {
            assert!(
                tool.execute(args).await.is_err(),
                "{name} must surface the outage"
            );
        }
    }
}

#[tokio::test]
async fn a_file_kind_without_a_path_is_a_malformed_call() {
    let artifacts = Arc::new(Artifacts::new(Arc::new(DownStore)));
    let create = CreateArtifactTool::new(artifacts, author());
    assert!(create
        .execute(json!({ "kind": "image", "title": "Logo" }))
        .await
        .is_err());
}
