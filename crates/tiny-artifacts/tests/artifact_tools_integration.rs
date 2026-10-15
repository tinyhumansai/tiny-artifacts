//! Integration test for the artifact tools: create → read → update → list,
//! verifying the versioned record model and tool responses.

// Unwrapping is fine in tests (AGENTS.md); the workspace lints keep it out of
// library code.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;

use tiny_artifacts::{
    ArtifactAuthor, ArtifactRecord, ArtifactStore, ArtifactVersion, Artifacts, Author,
    CreateArtifactTool, ListArtifactsTool, ReadArtifactTool, UpdateArtifactTool,
};
use tinytools::Tool;

/// A trivial in-memory store for testing tools without filesystem I/O.
struct MemStore(std::sync::Mutex<Vec<ArtifactRecord>>);

#[async_trait::async_trait]
impl ArtifactStore for MemStore {
    async fn list(&self) -> anyhow::Result<Vec<ArtifactRecord>> {
        let mut records = self.0.lock().unwrap().clone();
        records.sort_by_key(|r| std::cmp::Reverse(r.updated_at_millis));
        Ok(records)
    }
    async fn get(&self, id: &str) -> anyhow::Result<Option<ArtifactRecord>> {
        Ok(self.0.lock().unwrap().iter().find(|r| r.id == id).cloned())
    }
    async fn upsert(&self, artifact: &ArtifactRecord) -> anyhow::Result<()> {
        let mut store = self.0.lock().unwrap();
        if let Some(existing) = store.iter_mut().find(|r| r.id == artifact.id) {
            *existing = artifact.clone();
        } else {
            store.push(artifact.clone());
        }
        Ok(())
    }
    async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let mut store = self.0.lock().unwrap();
        let len = store.len();
        store.retain(|r| r.id != id);
        Ok(store.len() < len)
    }
}

#[tokio::test]
async fn create_read_update_list() {
    let store: Arc<dyn ArtifactStore> = Arc::new(MemStore(std::sync::Mutex::new(Vec::new())));
    let artifacts = Arc::new(Artifacts::new(store));
    let create = CreateArtifactTool::new(artifacts.clone(), Author::agent("agent"));
    let read = ReadArtifactTool::new(artifacts.clone());
    let update = UpdateArtifactTool::new(artifacts.clone(), Author::agent("agent"));
    let list = ListArtifactsTool::new(artifacts.clone());

    // Step 1: create
    let args = serde_json::json!({
        "kind": "markdown",
        "title": "Release Notes",
        "body": "## v1.0\nReleased on time."
    });
    let result = create.execute(args).await.unwrap();
    assert!(!result.is_error, "create should succeed: {}", result.text());
    let data: serde_json::Value = serde_json::from_str(&result.text()).unwrap();
    let id = data["artifact_id"].as_str().unwrap().to_string();
    assert_eq!(data["title"], "Release Notes");
    assert_eq!(data["version"], 1);

    // Step 2: read — verify version 1
    let result = read
        .execute(serde_json::json!({ "artifact_id": &id }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let record: ArtifactRecord = serde_json::from_str(&result.text()).unwrap();
    assert_eq!(record.title, "Release Notes");
    assert_eq!(record.versions.len(), 1);
    assert_eq!(record.versions[0].body, "## v1.0\nReleased on time.");
    assert_eq!(record.versions[0].author, ArtifactAuthor::Agent);

    // Step 3: update — push version 2
    let result = update
        .execute(serde_json::json!({
            "artifact_id": &id,
            "body": "## v1.1\nFixed the critical bug."
        }))
        .await
        .unwrap();
    assert!(!result.is_error, "update should succeed: {}", result.text());
    let data: serde_json::Value = serde_json::from_str(&result.text()).unwrap();
    assert_eq!(data["version"], 2);

    // Step 4: read again — verify v2
    let result = read
        .execute(serde_json::json!({ "artifact_id": &id }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let record: ArtifactRecord = serde_json::from_str(&result.text()).unwrap();
    assert_eq!(record.versions.len(), 2);
    assert_eq!(record.versions[0].body, "## v1.0\nReleased on time.");
    assert_eq!(record.versions[1].body, "## v1.1\nFixed the critical bug.");
    // human_edit_diff is None because no operator-authored version
    assert!(
        record.human_edit_diff().is_none(),
        "no operator version yet"
    );

    // Step 5: list — should show 1 record
    let result = list.execute(serde_json::json!({})).await.unwrap();
    assert!(!result.is_error);
    let records: Vec<ArtifactRecord> = serde_json::from_str(&result.text()).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].id, id);

    // Step 6: operator diff — lines between v2 (agent) and v3 (operator)
    let now = tiny_artifacts::now_millis();
    let mut record = record;
    let v3 = ArtifactVersion::new(
        3,
        "## v1.1\nEdited by human.",
        ArtifactAuthor::Operator,
        "alice",
        now,
    );
    record.push_version(v3);
    let diff = record.human_edit_diff();
    assert!(diff.is_some(), "operator version should produce a diff");
    let diff = diff.unwrap();
    assert_eq!(diff.from_version, 2);
    assert_eq!(diff.to_version, 3);
    assert_eq!(diff.added, 1);
    assert_eq!(diff.removed, 1);
    assert_eq!(diff.lines.len(), 3); // equal + delete + insert
}

#[tokio::test]
async fn error_paths() {
    let store: Arc<dyn ArtifactStore> = Arc::new(MemStore(std::sync::Mutex::new(Vec::new())));
    let artifacts = Arc::new(Artifacts::new(store));
    let create = CreateArtifactTool::new(artifacts.clone(), Author::agent("agent"));
    let read = ReadArtifactTool::new(artifacts.clone());
    let update = UpdateArtifactTool::new(artifacts.clone(), Author::agent("agent"));

    // Argument validation failures surface as `Err` from `execute` (the
    // tool could not run at all), while domain failures like a missing
    // artifact surface as `Ok(ToolResult::error(...))` (the tool ran and
    // said no). See tinytools `Tool::execute` docs.

    // Invalid kind is rejected.
    let result = create
        .execute(serde_json::json!({
            "kind": "pdf",
            "title": "T",
            "body": "b"
        }))
        .await;
    assert!(result.is_err(), "invalid kind must be an Err");

    // Empty title is rejected.
    let result = create
        .execute(serde_json::json!({
            "kind": "text",
            "title": "   ",
            "body": "b"
        }))
        .await;
    assert!(result.is_err(), "blank title must be an Err");

    // Empty body is rejected.
    let result = create
        .execute(serde_json::json!({
            "kind": "text",
            "title": "T",
            "body": ""
        }))
        .await;
    assert!(result.is_err(), "empty body must be an Err");

    // Missing required create args are rejected.
    let result = create
        .execute(serde_json::json!({"kind": "text", "title": "T"}))
        .await;
    assert!(result.is_err(), "missing body must be an Err");

    // read_artifact on a missing id is a reported tool error.
    let result = read
        .execute(serde_json::json!({"artifact_id": "does-not-exist"}))
        .await
        .unwrap();
    assert!(result.is_error, "read of missing artifact must be an error");

    // update_artifact on a missing id is a reported tool error.
    let result = update
        .execute(serde_json::json!({
            "artifact_id": "does-not-exist",
            "body": "new"
        }))
        .await
        .unwrap();
    assert!(
        result.is_error,
        "update of missing artifact must be an error"
    );

    // A happy-path create still works after all the rejections.
    let result = create
        .execute(serde_json::json!({
            "kind": "html",
            "title": "Page",
            "body": "<p>hi</p>"
        }))
        .await
        .unwrap();
    assert!(!result.is_error, "valid html create must succeed");
}
