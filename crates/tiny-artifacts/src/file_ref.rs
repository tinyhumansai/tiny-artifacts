//! File references: how an `image` or `file` artifact points at its bytes.
//!
//! A versioned artifact never carries binary content. An image or a generated
//! document is a file on the host's disk, and a version records a [`FileRef`]
//! to it. The reference is checked when it is made, through [`FileRoots`]: the
//! path must name an existing regular file inside one of the folders the host
//! allows. That check is what keeps an agent from filing an artifact that
//! points at nothing, or at a file outside its reach (a symlink included).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A checked reference to a file on the host's disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRef {
    /// Absolute, canonical path of the file.
    pub path: String,
    /// Size in bytes when the reference was made.
    pub size_bytes: u64,
    /// Media type guessed from the extension (e.g. `image/png`), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

impl FileRef {
    /// The file name (last path component).
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        Path::new(&self.path).file_name().and_then(|n| n.to_str())
    }

    /// The lower-case extension, when the file has one.
    #[must_use]
    pub fn extension(&self) -> Option<String> {
        Path::new(&self.path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
    }
}

/// Why a path could not become a [`FileRef`].
#[derive(Debug, thiserror::Error)]
pub enum FileRefError {
    /// The host allows no folders, so no file can be referenced.
    #[error("this host does not allow file artifacts")]
    NoRoots,
    /// The path is empty.
    #[error("the file path is empty")]
    Empty,
    /// Nothing exists at the path.
    #[error("no file at `{0}`")]
    Missing(String),
    /// The path is a directory or another non-file.
    #[error("`{0}` is not a regular file")]
    NotAFile(String),
    /// The path resolves outside every allowed folder.
    #[error("`{0}` is outside the folders artifacts may reference")]
    OutsideRoots(String),
    /// The file system refused a lookup.
    #[error("cannot read `{path}`: {source}")]
    Io {
        /// The path being resolved.
        path: String,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}

/// The folders a host lets artifacts reference.
///
/// A relative path is resolved against the first root (the agent's working
/// folder, typically). Every path is canonicalized before the containment
/// check, so `..` segments and symlinks cannot reach outside.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileRoots {
    roots: Vec<PathBuf>,
}

impl FileRoots {
    /// Roots in priority order; the first one anchors relative paths.
    #[must_use]
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            roots: roots.into_iter().collect(),
        }
    }

    /// The allowed folders.
    #[must_use]
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Resolve `raw` to a checked [`FileRef`].
    ///
    /// # Errors
    ///
    /// Returns a [`FileRefError`] when there are no roots, the path is empty,
    /// nothing is there, it is not a regular file, or it resolves outside
    /// every root.
    pub async fn resolve(&self, raw: &str) -> Result<FileRef, FileRefError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Err(FileRefError::Empty);
        }
        let first = self.roots.first().ok_or(FileRefError::NoRoots)?;
        let candidate = Path::new(raw);
        let joined = if candidate.is_absolute() {
            candidate.to_path_buf()
        } else {
            first.join(candidate)
        };
        let canonical = match tokio::fs::canonicalize(&joined).await {
            Ok(path) => path,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(FileRefError::Missing(raw.to_string()));
            }
            Err(source) => {
                return Err(FileRefError::Io {
                    path: raw.to_string(),
                    source,
                })
            }
        };
        let mut inside = false;
        for root in &self.roots {
            if let Ok(root) = tokio::fs::canonicalize(root).await {
                if canonical.starts_with(root) {
                    inside = true;
                    break;
                }
            }
        }
        if !inside {
            return Err(FileRefError::OutsideRoots(raw.to_string()));
        }
        let meta = tokio::fs::metadata(&canonical)
            .await
            .map_err(|source| FileRefError::Io {
                path: raw.to_string(),
                source,
            })?;
        if !meta.is_file() {
            return Err(FileRefError::NotAFile(raw.to_string()));
        }
        let path = canonical.to_string_lossy().into_owned();
        let media_type = media_type_for(&canonical).map(str::to_string);
        Ok(FileRef {
            path,
            size_bytes: meta.len(),
            media_type,
        })
    }
}

/// Media type for a file extension, for the kinds artifacts commonly hold.
#[must_use]
pub fn media_type_for(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "txt" => "text/plain",
        _ => return None,
    })
}
