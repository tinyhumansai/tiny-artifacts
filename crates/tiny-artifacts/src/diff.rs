//! Line-level diffing between artifact revisions, plus the churn summary that
//! makes it a quality signal rather than just a rendering.

use serde::{Deserialize, Serialize};

use crate::types::ArtifactVersion;

/// One line of a diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffOp {
    /// Unchanged, present in both sides.
    Equal,
    /// Only in the newer side.
    Insert,
    /// Only in the older side.
    Delete,
}

/// A single diff line.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    /// What happened to this line.
    pub op: DiffOp,
    /// The line's text (no trailing newline).
    pub text: String,
}

/// A line-level diff between two revisions, plus the churn summary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactDiff {
    /// The older revision's number.
    pub from_version: u32,
    /// The newer revision's number.
    pub to_version: u32,
    /// The line-by-line diff, in newer-side order with deletions interleaved.
    pub lines: Vec<DiffLine>,
    /// Lines only in the newer side.
    pub added: usize,
    /// Lines only in the older side.
    pub removed: usize,
    /// Fraction of the older side's lines that changed, 0.0–1.0.
    pub churn: f64,
}

impl ArtifactDiff {
    /// Diffs two revisions by line.
    pub fn between(from: &ArtifactVersion, to: &ArtifactVersion) -> Self {
        let lines = diff_lines(&from.body, &to.body);
        let added = lines.iter().filter(|l| l.op == DiffOp::Insert).count();
        let removed = lines.iter().filter(|l| l.op == DiffOp::Delete).count();
        let original = from.body.lines().count();
        let churn = if original == 0 {
            if added == 0 { 0.0 } else { 1.0 }
        } else {
            (removed as f64 / original as f64).min(1.0)
        };
        Self {
            from_version: from.version,
            to_version: to.version,
            lines,
            added,
            removed,
            churn,
        }
    }
}

/// Line-level diff via a longest-common-subsequence table.
///
/// Hand-rolled on purpose: the crate takes no diff dependency, and a line LCS
/// is a dozen lines of DP. Quadratic in line count, which is fine for artifact
/// bodies — and bounded below by [`MAX_DIFF_LINES`], past which the diff
/// degrades to a whole-body replace rather than allocating an enormous table.
fn diff_lines(from: &str, to: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = from.lines().collect();
    let b: Vec<&str> = to.lines().collect();

    if a.len() > MAX_DIFF_LINES || b.len() > MAX_DIFF_LINES {
        // Degrade rather than build an n×m table over huge inputs. Still a
        // truthful diff — every old line removed, every new line added.
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend(a.iter().map(|l| DiffLine {
            op: DiffOp::Delete,
            text: (*l).to_string(),
        }));
        out.extend(b.iter().map(|l| DiffLine {
            op: DiffOp::Insert,
            text: (*l).to_string(),
        }));
        return out;
    }

    // lcs[i][j] = length of the LCS of a[i..] and b[j..].
    let mut lcs = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for i in (0..a.len()).rev() {
        for j in (0..b.len()).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut out = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if a[i] == b[j] {
            out.push(DiffLine {
                op: DiffOp::Equal,
                text: a[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(DiffLine {
                op: DiffOp::Delete,
                text: a[i].to_string(),
            });
            i += 1;
        } else {
            out.push(DiffLine {
                op: DiffOp::Insert,
                text: b[j].to_string(),
            });
            j += 1;
        }
    }
    for line in a.iter().skip(i) {
        out.push(DiffLine {
            op: DiffOp::Delete,
            text: (*line).to_string(),
        });
    }
    for line in b.iter().skip(j) {
        out.push(DiffLine {
            op: DiffOp::Insert,
            text: (*line).to_string(),
        });
    }
    out
}

/// Line ceiling past which the line diff degrades to a whole-body replace.
pub const MAX_DIFF_LINES: usize = 2_000;
