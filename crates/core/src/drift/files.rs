use std::collections::BTreeSet;

use crate::config::ResolvedFile;
use crate::drift::Drift;
use crate::error::Result;
use crate::ports::Fsys;
use crate::render::zshrc::checksum;
use crate::state::Applied;

/// A managed file with its content already rendered for this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedFile {
    pub file: ResolvedFile,
    pub content: String,
    pub contains_secrets: bool,
    /// Values resolved while rendering this file; used only for redaction.
    pub secret_values: Vec<String>,
}

pub fn diff(rendered: &[RenderedFile], applied: &Applied, fs: &dyn Fsys) -> Result<Vec<Drift>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();

    for item in rendered {
        let target = &item.file.target;
        seen.insert(target.clone());

        let want = checksum(&item.content);
        let recorded = applied.files.get(target);

        if !fs.exists(target) {
            out.push(Drift::IncomingFile {
                target: target.clone(),
                set: item.file.set.clone(),
            });
            continue;
        }

        let on_disk = checksum(&fs.read(target)?);

        match recorded {
            // dotfix wrote it and it is untouched: only the repository can differ
            Some(r) if *r == on_disk => {
                if *r != want {
                    out.push(Drift::IncomingFile {
                        target: target.clone(),
                        set: item.file.set.clone(),
                    });
                }
            }
            // Either never written by dotfix, or changed underneath it.
            _ if on_disk != want => out.push(Drift::LocalEdit {
                target: target.clone(),
                set: item.file.set.clone(),
                contains_secrets: item.contains_secrets,
            }),
            _ => {}
        }
    }

    for target in applied.files.keys() {
        if !seen.contains(target) && fs.exists(target) {
            out.push(Drift::RemovedFile {
                target: target.clone(),
            });
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;
    use crate::config::{FileMode, ResolvedFile};
    use crate::ports::fake::FakeFsys;
    use crate::render::zshrc::checksum;
    use crate::state::Applied;

    fn rendered(content: &str) -> RenderedFile {
        RenderedFile {
            file: ResolvedFile {
                set: "core".into(),
                source: PathBuf::from("/repo/sets/core/files/rc.tmpl"),
                target: PathBuf::from("/Users/test/.rc"),
                mode: FileMode::Template,
            },
            content: content.to_string(),
            contains_secrets: false,
            secret_values: vec![],
        }
    }

    fn rendered_with_secret(content: &str) -> RenderedFile {
        RenderedFile {
            contains_secrets: true,
            ..rendered(content)
        }
    }

    fn applied_with(content: &str) -> Applied {
        Applied {
            files: BTreeMap::from([(PathBuf::from("/Users/test/.rc"), checksum(content))]),
            ..Default::default()
        }
    }

    #[test]
    fn a_missing_target_is_incoming() {
        let fs = FakeFsys::new();
        let out = diff(&[rendered("new")], &Applied::default(), &fs).unwrap();
        assert_eq!(
            out,
            vec![Drift::IncomingFile {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
            }]
        );
    }

    #[test]
    fn an_unchanged_target_is_not_drift() {
        let fs = FakeFsys::from([("/Users/test/.rc", "same")]);
        let out = diff(&[rendered("same")], &applied_with("same"), &fs).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn a_repository_change_with_no_local_edit_is_incoming() {
        let fs = FakeFsys::from([("/Users/test/.rc", "old")]);
        let out = diff(&[rendered("new")], &applied_with("old"), &fs).unwrap();
        assert!(matches!(out[0], Drift::IncomingFile { .. }));
    }

    #[test]
    fn a_local_edit_wins_over_a_repository_change() {
        let fs = FakeFsys::from([("/Users/test/.rc", "edited by hand")]);
        let out = diff(&[rendered("new")], &applied_with("old"), &fs).unwrap();
        assert_eq!(
            out,
            vec![Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
                contains_secrets: false,
            }],
            "never overwrite a hand edit without asking"
        );
    }

    #[test]
    fn a_local_edit_carries_whether_its_source_resolves_a_secret() {
        // `adopt` decides from this flag whether writing the file back would
        // commit a resolved secret. It has no other way to know.
        let fs = FakeFsys::from([("/Users/test/.rc", "edited by hand")]);
        let out = diff(&[rendered_with_secret("new")], &applied_with("old"), &fs).unwrap();
        assert_eq!(
            out,
            vec![Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
                contains_secrets: true,
            }]
        );
    }

    #[test]
    fn a_file_no_longer_desired_is_removed() {
        let fs = FakeFsys::from([("/Users/test/.rc", "stale")]);
        let out = diff(&[], &applied_with("stale"), &fs).unwrap();
        assert_eq!(
            out,
            vec![Drift::RemovedFile {
                target: PathBuf::from("/Users/test/.rc"),
            }]
        );
    }
}
