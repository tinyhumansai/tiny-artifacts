//! The ready-made file-system store.

use std::path::PathBuf;

use tiny_artifacts::{ArtifactKind, ArtifactRecord, ArtifactStore, FsArtifactStore};

fn dir() -> PathBuf {
    std::env::temp_dir().join(format!("tiny-artifacts-fs-{}", uuid::Uuid::new_v4()))
}

fn record(id: &str, updated: u64) -> ArtifactRecord {
    let mut r = ArtifactRecord::new(id, id, ArtifactKind::Markdown, "# hi", "agent", 1);
    r.updated_at_millis = updated;
    r
}

#[tokio::test]
async fn records_round_trip_and_list_newest_first() {
    let root = dir().join("versioned");
    let store = FsArtifactStore::new(&root);
    // The directory does not exist yet: nothing stored, no error.
    assert!(store.list().await.unwrap().is_empty());
    assert!(store.get("a").await.unwrap().is_none());

    store.upsert(&record("old", 1)).await.unwrap();
    store.upsert(&record("new", 2)).await.unwrap();
    let ids: Vec<String> = store
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(ids, ["new", "old"]);
    assert_eq!(store.get("old").await.unwrap(), Some(record("old", 1)));

    // Replacing keeps one file per artifact and leaves no staging files.
    store.upsert(&record("old", 3)).await.unwrap();
    let names: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let mut names = names;
    names.sort();
    assert_eq!(names, ["new.json", "old.json"]);

    assert!(store.delete("old").await.unwrap());
    assert!(!store.delete("old").await.unwrap());
    assert_eq!(store.list().await.unwrap().len(), 1);
}

#[tokio::test]
async fn unsafe_ids_are_refused() {
    let store = FsArtifactStore::new(dir());
    for id in ["", ".", "..", "../x", "a/b", "a\\b", ".hidden"] {
        assert!(store.get(id).await.is_err(), "`{id}` must be refused");
        assert!(
            store.upsert(&record(id, 1)).await.is_err(),
            "`{id}` must be refused"
        );
    }
}

#[tokio::test]
async fn an_unreadable_record_is_skipped_by_list_but_reported_by_get() {
    let root = dir();
    let store = FsArtifactStore::new(&root);
    store.upsert(&record("good", 1)).await.unwrap();
    std::fs::write(root.join("bad.json"), b"{ not json").unwrap();
    std::fs::write(root.join("notes.txt"), b"ignored").unwrap();

    let listed = store.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "good");
    assert!(store.get("bad").await.is_err());
}

#[tokio::test]
async fn records_written_by_earlier_hosts_still_load() {
    // The on-disk shape OpenHuman wrote before this store moved here.
    let root = dir();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("a-1.json"),
        br##"{"id":"a-1","title":"Old","kind":"markdown","versions":[{"version":1,"body":"# old","author":"agent","authorId":"agent","createdAtMillis":1}],"createdAtMillis":1,"updatedAtMillis":1}"##,
    )
    .unwrap();
    let record = FsArtifactStore::new(&root)
        .get("a-1")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(record.title, "Old");
    assert_eq!(record.latest().unwrap().body, "# old");
}
