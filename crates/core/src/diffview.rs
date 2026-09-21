use std::path::{Path, PathBuf};

use serde::Serialize;
use similar::{ChangeTag, TextDiff};

use crate::engine::Inspection;
use crate::error::{Error, Result};
use crate::ports::Fsys;
use crate::secrets::redact;

/// Upper bound on diff size. A config file that differs in thousands of lines
/// is not something anyone reads in a popover.
pub const MAX_LINES: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Already redacted. Never construct one of these from raw content.
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileDiff {
    pub target: PathBuf,
    pub set: String,
    pub lines: Vec<DiffLine>,
    pub truncated: bool,
}

/// Line diff between what is on disk and what the repository would write.
///
/// Every line is passed through [`redact`] with the values that were resolved
/// while rendering *this* file, so a secret cannot reach the caller — and
/// therefore cannot reach the webview.
pub fn unified(inspection: &Inspection, target: &Path, fs: &dyn Fsys) -> Result<FileDiff> {
    let rendered = inspection
        .rendered
        .iter()
        .find(|r| r.file.target == target)
        .ok_or_else(|| Error::Config(format!("{} is not a managed file", target.display())))?;

    let local = if fs.exists(target) {
        fs.read(target)?
    } else {
        String::new()
    };

    let values = &rendered.secret_values;
    let old = redact(&local, values);
    let new = redact(&rendered.content, values);

    let diff = TextDiff::from_lines(&old, &new);
    let mut lines = Vec::new();
    let mut truncated = false;

    for change in diff.iter_all_changes() {
        if lines.len() == MAX_LINES {
            truncated = true;
            break;
        }
        let kind = match change.tag() {
            ChangeTag::Equal => LineKind::Context,
            ChangeTag::Insert => LineKind::Added,
            ChangeTag::Delete => LineKind::Removed,
        };
        lines.push(DiffLine {
            kind,
            text: change.value().trim_end_matches('\n').to_string(),
        });
    }

    Ok(FileDiff {
        target: target.to_path_buf(),
        set: rendered.file.set.clone(),
        lines,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::config::{FileMode, ResolvedFile};
    use crate::drift::Report;
    use crate::drift::files::RenderedFile;
    use crate::engine::Inspection;
    use crate::ports::fake::FakeFsys;

    fn inspection(content: &str, secret_values: Vec<String>) -> Inspection {
        Inspection {
            desired: Default::default(),
            rendered: vec![RenderedFile {
                file: ResolvedFile {
                    set: "core".into(),
                    source: PathBuf::from("/repo/sets/core/files/rc.tmpl"),
                    target: PathBuf::from("/Users/test/.rc"),
                    mode: FileMode::Template,
                },
                content: content.to_string(),
                contains_secrets: !secret_values.is_empty(),
                secret_values,
            }],
            report: Report::default(),
        }
    }

    #[test]
    fn shows_removed_and_added_lines_against_the_local_file() {
        let fs = FakeFsys::from([("/Users/test/.rc", "keep\nold\n")]);
        let d = unified(
            &inspection("keep\nnew\n", vec![]),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();

        assert_eq!(
            d.lines,
            vec![
                DiffLine {
                    kind: LineKind::Context,
                    text: "keep".into()
                },
                DiffLine {
                    kind: LineKind::Removed,
                    text: "old".into()
                },
                DiffLine {
                    kind: LineKind::Added,
                    text: "new".into()
                },
            ]
        );
        assert_eq!(d.set, "core");
        assert!(!d.truncated);
    }

    #[test]
    fn an_identical_file_produces_only_context() {
        let fs = FakeFsys::from([("/Users/test/.rc", "same\n")]);
        let d = unified(
            &inspection("same\n", vec![]),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();
        assert!(d.lines.iter().all(|l| l.kind == LineKind::Context));
    }

    #[test]
    fn a_secret_value_never_appears_in_the_diff() {
        let fs = FakeFsys::from([("/Users/test/.rc", "access_key = s3cr3t\nregion = eu\n")]);
        let d = unified(
            &inspection(
                "access_key = s3cr3t\nregion = us\n",
                vec!["s3cr3t".to_string()],
            ),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();

        let all: String = d.lines.iter().map(|l| l.text.as_str()).collect();
        assert!(!all.contains("s3cr3t"), "secret leaked into diff: {all}");
        assert!(all.contains(crate::secrets::REDACTED));
        assert!(all.contains("region"));
    }

    #[test]
    fn a_missing_local_file_diffs_against_nothing() {
        let fs = FakeFsys::new();
        let d = unified(
            &inspection("new\n", vec![]),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();
        assert_eq!(
            d.lines,
            vec![DiffLine {
                kind: LineKind::Added,
                text: "new".into()
            }]
        );
    }

    #[test]
    fn an_unmanaged_target_is_an_error() {
        let fs = FakeFsys::new();
        let err = unified(
            &inspection("x", vec![]),
            Path::new("/Users/test/.other"),
            &fs,
        )
        .unwrap_err();
        assert!(err.to_string().contains(".other"));
    }

    #[test]
    fn a_very_long_diff_is_truncated_and_says_so() {
        let long: String = (0..MAX_LINES + 50).map(|i| format!("line {i}\n")).collect();
        let fs = FakeFsys::from([("/Users/test/.rc", "")]);
        let d = unified(
            &inspection(&long, vec![]),
            Path::new("/Users/test/.rc"),
            &fs,
        )
        .unwrap();
        assert_eq!(d.lines.len(), MAX_LINES);
        assert!(d.truncated);
    }
}
