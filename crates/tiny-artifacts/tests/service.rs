//! The `Artifacts` service and the file-aware tools: file references, the
//! pending lifecycle, observer events, publishing, and records written before
//! these fields existed.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::json;
use tiny_artifacts::{
    ArtifactAuthor, ArtifactEvent, ArtifactEventKind, ArtifactKind, ArtifactObserver,
    ArtifactPublisher, ArtifactRecord, ArtifactStatus, ArtifactStore, ArtifactVersion, Artifacts,
    ArtifactsError, Author, Content, CreateArtifactTool, DeleteArtifactTool, FileRefError,
    FileRoots, PublishOptions, PublishedLink, ShareArtifactTool, UpdateArtifactTool,
};
use tinytools::Tool;

#[derive(Default)]
struct MemStore(Mutex<Vec<ArtifactRecord>>);

#[async_trait::async_trait]
impl ArtifactStore for MemStore {
    async fn list(&self) -> anyhow::Result<Vec<ArtifactRecord>> {
        Ok(self.0.lock().unwrap().clone())
    }
    async fn get(&self, id: &str) -> anyhow::Result<Option<ArtifactRecord>> {
        Ok(self.0.lock().unwrap().iter().find(|r| r.id == id).cloned())
    }
    async fn upsert(&self, artifact: &ArtifactRecord) -> anyhow::Result<()> {
        let mut store = self.0.lock().unwrap();
        store.retain(|r| r.id != artifact.id);
        store.push(artifact.clone());
        Ok(())
    }
    async fn delete(&self, id: &str) -> anyhow::Result<bool> {
        let mut store = self.0.lock().unwrap();
        let before = store.len();
        store.retain(|r| r.id != id);
        Ok(store.len() < before)
    }
}

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<(ArtifactEventKind, ArtifactStatus, usize)>>,
    fail: bool,
}

#[async_trait::async_trait]
impl ArtifactObserver for Recorder {
    async fn on_event(&self, event: &ArtifactEvent) -> anyhow::Result<()> {
        self.events.lock().unwrap().push((
            event.kind,
            event.record.status,
            event.record.versions.len(),
        ));
        if self.fail {
            anyhow::bail!("card pipeline down");
        }
        Ok(())
    }
}

/// Counts uploads; links expire far in the future unless `expired`.
#[derive(Default)]
struct FakePublisher {
    uploads: Mutex<u32>,
    expired: bool,
}

#[async_trait::async_trait]
impl ArtifactPublisher for FakePublisher {
    async fn publish(
        &self,
        record: &ArtifactRecord,
        version: &ArtifactVersion,
        options: &PublishOptions,
    ) -> anyhow::Result<PublishedLink> {
        let mut uploads = self.uploads.lock().unwrap();
        *uploads += 1;
        Ok(PublishedLink {
            url: format!(
                "https://share.example/{}/v{}/{}",
                record.id, version.version, *uploads
            ),
            expires_at_millis: Some(if self.expired { 1 } else { u64::MAX }),
            published_at_millis: 1,
            public: options.public,
        })
    }
}

fn workdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tiny-artifacts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn service(root: &Path) -> (Arc<Artifacts>, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let artifacts = Artifacts::new(Arc::new(MemStore::default()))
        .with_observer(recorder.clone())
        .with_file_roots(FileRoots::new([root.to_path_buf()]));
    (Arc::new(artifacts), recorder)
}

fn json_of(result: &tinytools::ToolResult) -> serde_json::Value {
    serde_json::from_str(&result.text()).unwrap()
}

#[tokio::test]
async fn an_image_is_filed_by_path_with_a_checked_reference() {
    let root = workdir();
    std::fs::write(root.join("chart.png"), b"\x89PNG fake").unwrap();
    let (artifacts, recorder) = service(&root);
    let create = CreateArtifactTool::new(artifacts.clone(), Author::agent("agent"));

    // Relative to the working folder.
    let result = create
        .execute(json!({ "kind": "image", "title": "Chart", "path": "chart.png" }))
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.text());
    let data = json_of(&result);
    assert_eq!(data["status"], "ready");

    let id = data["artifact_id"].as_str().unwrap();
    let record = artifacts.get(id).await.unwrap().unwrap();
    let file = record.latest().unwrap().file.clone().unwrap();
    assert!(file.path.ends_with("chart.png"));
    assert_eq!(file.size_bytes, 9);
    assert_eq!(file.media_type.as_deref(), Some("image/png"));
    // Older readers still find the path in `body`.
    assert_eq!(record.latest().unwrap().body, file.path);
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec![(ArtifactEventKind::Ready, ArtifactStatus::Ready, 1)]
    );
}

#[tokio::test]
async fn a_file_that_is_missing_a_directory_or_outside_is_refused() {
    let root = workdir();
    let outside = workdir();
    std::fs::write(outside.join("secret.txt"), b"x").unwrap();
    std::fs::create_dir_all(root.join("folder")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("secret.txt"), root.join("link.txt")).unwrap();
    let (artifacts, _) = service(&root);
    let create = CreateArtifactTool::new(artifacts.clone(), Author::agent("agent"));

    let mut paths = vec![
        "nope.pdf".to_string(),
        "folder".to_string(),
        outside.join("secret.txt").display().to_string(),
        "../secret.txt".to_string(),
    ];
    if cfg!(unix) {
        paths.push("link.txt".to_string());
    }
    for path in paths {
        let result = create
            .execute(json!({ "kind": "file", "title": "F", "path": path }))
            .await
            .unwrap();
        assert!(
            result.is_error,
            "`{path}` must be refused: {}",
            result.text()
        );
    }
    assert!(
        artifacts.list().await.unwrap().is_empty(),
        "nothing is filed"
    );

    // Without roots a host refuses file artifacts outright.
    let bare = Artifacts::new(Arc::new(MemStore::default()));
    let err = bare
        .create(
            ArtifactKind::File,
            "F",
            Content::File("x".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, ArtifactsError::File(FileRefError::NoRoots)),
        "{err}"
    );
}

#[tokio::test]
async fn content_must_fit_the_kind() {
    let root = workdir();
    let (artifacts, _) = service(&root);
    let create = CreateArtifactTool::new(artifacts.clone(), Author::agent("agent"));
    // A text kind with no body is a malformed call.
    assert!(create
        .execute(json!({ "kind": "markdown", "title": "T", "path": "x.md" }))
        .await
        .is_err());
    let err = artifacts
        .create(
            ArtifactKind::Image,
            "I",
            Content::Text("hi".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, ArtifactsError::WrongContent { .. }), "{err}");
}

#[tokio::test]
async fn an_identical_text_revision_appends_nothing() {
    let root = workdir();
    let (artifacts, _) = service(&root);
    let saved = artifacts
        .create(
            ArtifactKind::Markdown,
            "Notes",
            Content::Text("# a".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap();
    let id = saved.record.id;

    let same = artifacts
        .revise(
            &id,
            Content::Text("# a".into()),
            &Author::operator("me"),
            None,
        )
        .await
        .unwrap();
    assert!(!same.appended);
    assert_eq!(same.version, 1);

    let edited = artifacts
        .revise(
            &id,
            Content::Text("# b".into()),
            &Author::operator("me"),
            Some("tweak".into()),
        )
        .await
        .unwrap();
    assert!(edited.appended);
    assert_eq!(edited.version, 2);
    let latest = edited.record.latest().unwrap();
    assert_eq!(latest.author, ArtifactAuthor::Operator);
    assert_eq!(latest.note.as_deref(), Some("tweak"));
    assert!(edited.record.human_edit_diff().is_some());
}

#[tokio::test]
async fn a_slow_producer_opens_pending_then_completes_or_fails() {
    let root = workdir();
    std::fs::write(root.join("deck.pptx"), b"pk").unwrap();
    let (artifacts, recorder) = service(&root);

    let opened = artifacts
        .begin(ArtifactKind::File, "Deck", Some("deck-1".into()))
        .await
        .unwrap();
    assert_eq!(opened.record.status, ArtifactStatus::Pending);
    assert_eq!(opened.version, 0);
    assert!(opened.record.versions.is_empty());

    let done = artifacts
        .revise(
            "deck-1",
            Content::File("deck.pptx".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap();
    assert_eq!(done.record.status, ArtifactStatus::Ready);
    assert_eq!(done.version, 1);

    artifacts
        .begin(ArtifactKind::Image, "Logo", Some("logo-1".into()))
        .await
        .unwrap();
    let failed = artifacts
        .fail("logo-1", "provider timed out")
        .await
        .unwrap();
    assert_eq!(failed.record.status, ArtifactStatus::Failed);
    assert_eq!(failed.record.error.as_deref(), Some("provider timed out"));

    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec![
            (ArtifactEventKind::Pending, ArtifactStatus::Pending, 0),
            (ArtifactEventKind::Ready, ArtifactStatus::Ready, 1),
            (ArtifactEventKind::Pending, ArtifactStatus::Pending, 0),
            (ArtifactEventKind::Failed, ArtifactStatus::Failed, 0),
        ]
    );
}

#[tokio::test]
async fn an_observer_failure_keeps_the_write_and_reports_a_notice() {
    let observer = Arc::new(Recorder {
        fail: true,
        ..Recorder::default()
    });
    let artifacts = Arc::new(Artifacts::new(Arc::new(MemStore::default())).with_observer(observer));
    let create = CreateArtifactTool::new(artifacts.clone(), Author::agent("agent"));
    let result = create
        .execute(json!({ "kind": "text", "title": "T", "body": "hello" }))
        .await
        .unwrap();
    assert!(!result.is_error);
    let data = json_of(&result);
    assert!(data["notice"]
        .as_str()
        .unwrap()
        .contains("card pipeline down"));
    assert_eq!(
        artifacts.list().await.unwrap().len(),
        1,
        "the record stands"
    );
}

#[tokio::test]
async fn publishing_records_the_link_on_that_version() {
    let root = workdir();
    let store = Arc::new(MemStore::default());
    let without = Artifacts::new(store.clone());
    let saved = without
        .create(
            ArtifactKind::Html,
            "Page",
            Content::Text("<p>1</p>".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap();
    let id = saved.record.id;
    assert!(matches!(
        without.publish(&id, None, &PublishOptions::default()).await,
        Err(ArtifactsError::NoPublisher)
    ));

    let publisher = Arc::new(FakePublisher::default());
    let artifacts = Artifacts::new(store)
        .with_publisher(publisher.clone())
        .with_file_roots(FileRoots::new([root]));
    artifacts
        .revise(
            &id,
            Content::Text("<p>2</p>".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap();
    let private = PublishOptions::default();
    let first = artifacts.publish(&id, Some(1), &private).await.unwrap();
    assert_eq!(first.version, 1);
    assert!(!first.reused);

    let record = artifacts.get(&id).await.unwrap().unwrap();
    assert_eq!(
        record.version(1).unwrap().published.as_ref(),
        Some(&first.link)
    );
    assert!(
        record.version(2).unwrap().published.is_none(),
        "v2 was not shared"
    );

    // Sharing the same version again reuses its live link…
    let again = artifacts.publish(&id, Some(1), &private).await.unwrap();
    assert!(again.reused);
    assert_eq!(again.link, first.link);
    assert_eq!(*publisher.uploads.lock().unwrap(), 1);
    // …unless a fresh link, or a public one, is asked for.
    let fresh = PublishOptions {
        fresh: true,
        ..PublishOptions::default()
    };
    assert!(
        !artifacts
            .publish(&id, Some(1), &fresh)
            .await
            .unwrap()
            .reused
    );
    let public = PublishOptions {
        public: true,
        ..PublishOptions::default()
    };
    let shared = artifacts.publish(&id, Some(1), &public).await.unwrap();
    assert!(!shared.reused);
    assert!(shared.link.public);
    assert_eq!(*publisher.uploads.lock().unwrap(), 3);

    assert!(matches!(
        artifacts.publish(&id, Some(9), &private).await,
        Err(ArtifactsError::NoVersion { .. })
    ));
}

#[tokio::test]
async fn an_expired_link_is_replaced() {
    let artifacts =
        Artifacts::new(Arc::new(MemStore::default())).with_publisher(Arc::new(FakePublisher {
            expired: true,
            ..FakePublisher::default()
        }));
    let id = artifacts
        .create(
            ArtifactKind::Text,
            "T",
            Content::Text("x".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap()
        .record
        .id;
    let options = PublishOptions::default();
    artifacts.publish(&id, None, &options).await.unwrap();
    assert!(!artifacts.publish(&id, None, &options).await.unwrap().reused);
}

#[tokio::test]
async fn an_older_file_version_is_shared_only_while_its_file_is_unchanged() {
    let root = workdir();
    std::fs::write(root.join("deck.pptx"), b"v1").unwrap();
    let artifacts = Artifacts::new(Arc::new(MemStore::default()))
        .with_publisher(Arc::new(FakePublisher::default()))
        .with_file_roots(FileRoots::new([root.clone()]));
    let id = artifacts
        .create(
            ArtifactKind::File,
            "Deck",
            Content::File("deck.pptx".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap()
        .record
        .id;
    // A producer overwrites the same file for its next version.
    std::fs::write(root.join("deck.pptx"), b"v2-longer").unwrap();
    artifacts
        .revise(
            &id,
            Content::File("deck.pptx".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap();

    let options = PublishOptions::default();
    assert!(matches!(
        artifacts.publish(&id, Some(1), &options).await,
        Err(ArtifactsError::FileChanged { version: 1, .. })
    ));
    assert_eq!(
        artifacts
            .publish(&id, None, &options)
            .await
            .unwrap()
            .version,
        2
    );
}

#[tokio::test]
async fn the_share_tool_returns_the_link() {
    let artifacts = Arc::new(
        Artifacts::new(Arc::new(MemStore::default()))
            .with_publisher(Arc::new(FakePublisher::default())),
    );
    assert!(artifacts.can_publish());
    let id = artifacts
        .create(
            ArtifactKind::Markdown,
            "Notes",
            Content::Text("# a".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap()
        .record
        .id;
    let share = ShareArtifactTool::new(artifacts.clone());
    assert!(share.external_effect());
    let result = share
        .execute(json!({ "artifact_id": id, "public": true }))
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.text());
    let data = json_of(&result);
    assert!(data["url"]
        .as_str()
        .unwrap()
        .starts_with("https://share.example/"));
    assert_eq!(data["public"], true);
    assert_eq!(data["version"], 1);

    // A bad version is a malformed call; an unknown id is a refusal.
    assert!(share
        .execute(json!({ "artifact_id": id, "version": 0 }))
        .await
        .is_err());
    assert!(
        share
            .execute(json!({ "artifact_id": "nope" }))
            .await
            .unwrap()
            .is_error
    );
}

#[tokio::test]
async fn a_file_artifact_is_revised_by_path_and_deleted_by_opt_in_tool() {
    let root = workdir();
    std::fs::write(root.join("a.csv"), b"1").unwrap();
    std::fs::write(root.join("b.csv"), b"22").unwrap();
    let (artifacts, recorder) = service(&root);
    let saved = artifacts
        .create(
            ArtifactKind::File,
            "Data",
            Content::File("a.csv".into()),
            &Author::agent("a"),
            None,
        )
        .await
        .unwrap();
    let id = saved.record.id;

    let update = UpdateArtifactTool::new(artifacts.clone(), Author::agent("agent"));
    let result = update
        .execute(json!({ "artifact_id": id, "path": "b.csv" }))
        .await
        .unwrap();
    assert!(!result.is_error, "{}", result.text());
    assert_eq!(json_of(&result)["version"], 2);

    let delete = DeleteArtifactTool::new(artifacts.clone());
    assert_eq!(
        delete.permission_level(),
        tinytools::PermissionLevel::Dangerous
    );
    let result = delete.execute(json!({ "artifact_id": id })).await.unwrap();
    assert!(!result.is_error);
    assert!(
        delete
            .execute(json!({ "artifact_id": id }))
            .await
            .unwrap()
            .is_error
    );
    assert_eq!(
        recorder.events.lock().unwrap().last().map(|e| e.0),
        Some(ArtifactEventKind::Deleted)
    );
}

#[test]
fn a_record_written_before_status_and_files_reads_as_ready() {
    let legacy = json!({
        "id": "a-1",
        "title": "Old",
        "kind": "markdown",
        "versions": [{
            "version": 1, "body": "# old", "author": "agent",
            "authorId": "agent", "createdAtMillis": 1
        }],
        "createdAtMillis": 1,
        "updatedAtMillis": 1
    });
    let record: ArtifactRecord = serde_json::from_value(legacy).unwrap();
    assert_eq!(record.status, ArtifactStatus::Ready);
    assert!(record.error.is_none());
    assert!(record.latest().unwrap().file.is_none());

    // And a new record omits the empty optional fields when written.
    let out = serde_json::to_value(&record).unwrap();
    assert!(out["versions"][0].get("file").is_none());
    assert!(out["versions"][0].get("published").is_none());
}

#[tokio::test]
async fn tool_versions_are_written_by_the_agent_the_tools_were_built_for() {
    let artifacts = Arc::new(Artifacts::new(Arc::new(MemStore::default())));
    let tools = tiny_artifacts::registered_tools(artifacts.clone(), Author::agent("researcher"));
    let create = tools
        .iter()
        .find(|t| t.name() == "create_artifact")
        .unwrap();
    let result = create
        .execute(json!({ "kind": "text", "title": "T", "body": "v1" }))
        .await
        .unwrap();
    let id = json_of(&result)["artifact_id"]
        .as_str()
        .unwrap()
        .to_string();
    let update = tools
        .iter()
        .find(|t| t.name() == "update_artifact")
        .unwrap();
    update
        .execute(json!({ "artifact_id": id, "body": "v2" }))
        .await
        .unwrap();

    let record = artifacts.get(&id).await.unwrap().unwrap();
    assert!(record
        .versions
        .iter()
        .all(|v| v.author == ArtifactAuthor::Agent && v.author_id == "researcher"));
}
