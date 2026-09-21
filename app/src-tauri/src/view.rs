//! Turns an [`Inspection`] into exactly what the window shows.
//!
//! Every decision about grouping, wording and what may be acted on lives here,
//! so it can be tested without Tauri and without a browser. The React side
//! renders these structs and decides nothing.

use dotfix_core::drift::{Counts, Drift, Missing};
use dotfix_core::engine::Inspection;
use dotfix_core::status_line;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Area {
    /// Things dotfix can apply: installs, uninstalls, file writes.
    Changes,
    /// Things that need a decision before they can be managed.
    Unmanaged,
    /// Managed files edited by hand.
    Configs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Item {
    /// [`Drift::id`], the handle `apply`/`adopt` take.
    pub id: String,
    pub label: String,
    pub area: Area,
    /// One word for what would happen: install, uninstall, write, remove,
    /// adopt, drop, review. The React side keys the row's buttons off this,
    /// so it must name the action that is actually available — not the area
    /// the row happens to sit in.
    pub action: String,
    /// Secondary line: the owning set, or why an action is unavailable.
    pub detail: String,
    /// False when the action cannot be carried out — the row renders disabled
    /// with `detail` explaining why.
    pub actionable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Overview {
    pub counts: Counts,
    pub items: Vec<Item>,
    /// Software a set declares that this machine does not have. Carried
    /// beside `items`, not inside it: every row in there has a button, and
    /// these have nothing dotfix can press.
    pub missing: Vec<Missing>,
    /// Software this machine has that no active set declares — the mirror of
    /// an unmanaged package, and offered the same way.
    pub undeclared: Vec<Undeclared>,
    /// The same line the shell hook prints, so the window and the terminal can
    /// never disagree.
    pub status_line: Option<String>,
}

/// A requirement present here but recorded nowhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Undeclared {
    pub name: String,
    pub hint: Option<String>,
}

pub fn overview(inspection: &Inspection) -> Overview {
    let counts = inspection.report.counts();
    let items = inspection.report.items.iter().map(item_for).collect();

    Overview {
        counts,
        items,
        missing: inspection.report.missing.clone(),
        undeclared: Vec::new(),
        status_line: status_line::render(&counts),
    }
}

pub fn items_in(overview: &Overview, area: Area) -> Vec<&Item> {
    overview.items.iter().filter(|i| i.area == area).collect()
}

fn item_for(drift: &Drift) -> Item {
    let (area, action, detail, actionable) = match drift {
        Drift::IncomingPackage(_) => (Area::Changes, "install", String::new(), true),

        Drift::IncomingFile { set, .. } => (Area::Changes, "write", set.clone(), true),

        Drift::RemovedFile { .. } => (Area::Changes, "remove", String::new(), true),

        Drift::RemovedPackage {
            blocked_by,
            declared_in,
            ..
        } if blocked_by.is_empty() && !declared_in.is_empty() => (
            Area::Changes,
            "apply",
            match declared_in.as_slice() {
                [one] => format!("uninstall — was in set `{one}`, now off"),
                many => format!(
                    "uninstall — was in sets {}, now off",
                    many.iter()
                        .map(|s| format!("`{s}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            true,
        ),

        Drift::RemovedPackage { blocked_by, .. } if blocked_by.is_empty() => {
            (Area::Changes, "uninstall", String::new(), true)
        }
        Drift::RemovedPackage { blocked_by, .. } => (
            Area::Changes,
            "uninstall",
            format!("still required by {}", blocked_by.join(", ")),
            false,
        ),

        Drift::Unmanaged { declared_in, .. } => (
            Area::Unmanaged,
            "adopt",
            match declared_in.as_slice() {
                [] => "in no set".to_string(),
                [one] => format!("in set `{one}`, which is off here"),
                many => format!(
                    "in sets {}, all off here",
                    many.iter()
                        .map(|s| format!("`{s}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            },
            true,
        ),

        // Not "adopt": the only proposal `adopt::proposals_for` returns for
        // this is `DropPackage`. It shares the Unmanaged area with genuinely
        // unmanaged packages because both need a decision, but the decision
        // is a different one — and in particular "Ignore" is not part of it,
        // because `MachineConfig.ignore` is only consulted for packages that
        // are installed but in no set, so ignoring this one would change a
        // file and leave the row exactly where it was.
        Drift::LocallyRemoved(_) => (
            Area::Unmanaged,
            "drop",
            "removed on this machine".to_string(),
            true,
        ),

        Drift::LocalEdit { set, .. } => (Area::Configs, "review", set.clone(), true),
    };

    Item {
        id: drift.id(),
        label: drift.label(),
        area,
        action: action.to_string(),
        detail,
        actionable,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use dotfix_core::drift::{Drift, PackageRef, Report};
    use dotfix_core::engine::Inspection;

    use super::*;

    fn inspection(items: Vec<Drift>) -> Inspection {
        Inspection {
            desired: Default::default(),
            rendered: vec![],
            report: Report {
                items,
                missing: Vec::new(),
            },
        }
    }

    #[test]
    fn sorts_each_drift_class_into_its_area() {
        let o = overview(&inspection(vec![
            Drift::IncomingPackage(PackageRef::formula("alpha")),
            Drift::RemovedPackage {
                package: PackageRef::formula("gone"),
                blocked_by: vec![],
                declared_in: Vec::new(),
            },
            Drift::Unmanaged {
                package: PackageRef::formula("stray"),
                declared_in: Vec::new(),
            },
            Drift::LocallyRemoved(PackageRef::formula("removed-here")),
            Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.gitconfig"),
                set: "core".into(),
                contains_secrets: false,
            },
        ]));

        let labels =
            |a: Area| -> Vec<String> { items_in(&o, a).iter().map(|i| i.label.clone()).collect() };

        assert_eq!(labels(Area::Changes), vec!["alpha", "gone"]);
        assert_eq!(labels(Area::Unmanaged), vec!["stray", "removed-here"]);
        assert_eq!(labels(Area::Configs), vec!["/Users/test/.gitconfig"]);
    }

    #[test]
    fn a_locally_removed_package_is_offered_as_a_drop_not_an_adopt() {
        // `adopt::proposals_for` offers exactly one thing for
        // `LocallyRemoved`: `DropPackage`. Labelling it "adopt" put it in
        // the same row shape as a genuinely unmanaged package, which also
        // offers "Ignore" — and ignoring a locally-removed package does
        // nothing at all, because `MachineConfig.ignore` is only consulted
        // for packages that are installed but in no set. Naming the action
        // for what actually happens is what lets the window offer only the
        // button that works.
        let o = overview(&inspection(vec![Drift::LocallyRemoved(
            PackageRef::formula("gone-here"),
        )]));
        let item = &items_in(&o, Area::Unmanaged)[0];
        assert_eq!(item.action, "drop");
        assert_eq!(item.detail, "removed on this machine");
        assert!(item.actionable);
    }

    #[test]
    fn a_blocked_removal_is_shown_but_not_actionable_and_says_why() {
        let o = overview(&inspection(vec![Drift::RemovedPackage {
            package: PackageRef::formula("openjdk"),
            blocked_by: vec!["maven".into(), "gradle".into()],
            declared_in: Vec::new(),
        }]));

        let item = &items_in(&o, Area::Changes)[0];
        assert!(!item.actionable);
        assert_eq!(item.detail, "still required by maven, gradle");
    }

    #[test]
    fn actions_name_what_would_happen() {
        let o = overview(&inspection(vec![
            Drift::IncomingPackage(PackageRef::formula("alpha")),
            Drift::RemovedPackage {
                package: PackageRef::formula("gone"),
                blocked_by: vec![],
                declared_in: Vec::new(),
            },
            Drift::Unmanaged {
                package: PackageRef::formula("stray"),
                declared_in: Vec::new(),
            },
            Drift::LocalEdit {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
                contains_secrets: false,
            },
        ]));

        let action = |label: &str| -> String {
            o.items
                .iter()
                .find(|i| i.label == label)
                .unwrap()
                .action
                .clone()
        };
        assert_eq!(action("alpha"), "install");
        assert_eq!(action("gone"), "uninstall");
        assert_eq!(action("stray"), "adopt");
        assert_eq!(action("/Users/test/.rc"), "review");
    }

    #[test]
    fn ids_come_straight_from_core_so_apply_can_use_them() {
        let drift = Drift::IncomingPackage(PackageRef::formula("alpha"));
        let expected = drift.id();
        let o = overview(&inspection(vec![drift]));
        assert_eq!(o.items[0].id, expected);
    }

    #[test]
    fn an_empty_report_has_no_items_and_no_status_line() {
        let o = overview(&inspection(vec![]));
        assert!(o.items.is_empty());
        assert_eq!(o.status_line, None);
    }

    #[test]
    fn the_status_line_matches_what_the_shell_would_print() {
        let insp = inspection(vec![Drift::IncomingPackage(PackageRef::formula("alpha"))]);
        let o = overview(&insp);
        assert_eq!(
            o.status_line,
            dotfix_core::status_line::render(&insp.report.counts())
        );
    }

    #[test]
    fn a_generated_file_is_labelled_by_its_path_and_set() {
        let o = overview(&inspection(vec![Drift::IncomingFile {
            target: PathBuf::from("/Users/test/.zshrc"),
            set: dotfix_core::engine::GENERATED_SET.into(),
        }]));
        let item = &items_in(&o, Area::Changes)[0];
        assert_eq!(item.label, "/Users/test/.zshrc");
        assert_eq!(item.detail, "generated");
    }
}
