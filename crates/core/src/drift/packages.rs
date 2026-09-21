use std::collections::{BTreeMap, BTreeSet};

use crate::config::Desired;
use crate::drift::{Drift, PackageRef};
use crate::error::Result;
use crate::ports::Brew;
use crate::state::Applied;

/// Three-way diff over packages.
///
/// | desired | actual | applied | outcome          |
/// |---------|--------|---------|------------------|
/// | yes     | no     | no      | IncomingPackage  |
/// | yes     | no     | yes     | LocallyRemoved   |
/// | no      | yes    | no      | Unmanaged        |
/// | no      | yes    | yes     | RemovedPackage   |
/// | yes     | yes    | —       | in sync          |
pub fn diff(
    desired: &Desired,
    actual_brew: &BTreeSet<String>,
    actual_cask: &BTreeSet<String>,
    applied: &Applied,
    brew: &dyn Brew,
    ignore: &[String],
) -> Result<Vec<Drift>> {
    let mut out = Vec::new();
    out.extend(diff_kind(
        &desired.brew,
        actual_brew,
        &applied.brew,
        &desired.inactive_brew,
        brew,
        ignore,
        false,
    )?);
    out.extend(diff_kind(
        &desired.cask,
        actual_cask,
        &applied.cask,
        &desired.inactive_cask,
        brew,
        ignore,
        true,
    )?);
    Ok(out)
}

fn diff_kind(
    desired: &BTreeSet<String>,
    actual: &BTreeSet<String>,
    applied: &BTreeSet<String>,
    inactive: &BTreeMap<String, Vec<String>>,
    brew: &dyn Brew,
    ignore: &[String],
    cask: bool,
) -> Result<Vec<Drift>> {
    let mut out = Vec::new();

    for name in desired.difference(actual) {
        let package = PackageRef {
            name: name.clone(),
            cask,
        };
        if applied.contains(name) {
            out.push(Drift::LocallyRemoved(package));
        } else {
            out.push(Drift::IncomingPackage(package));
        }
    }

    for name in actual.difference(desired) {
        if ignore.iter().any(|i| i == name) {
            continue;
        }
        let package = PackageRef {
            name: name.clone(),
            cask,
        };
        if applied.contains(name) {
            // Casks have no dependents; only formulae need the leaf check.
            let blocked_by = if cask {
                Vec::new()
            } else {
                brew.uses_installed(name)?
            };
            out.push(Drift::RemovedPackage {
                package,
                blocked_by,
                declared_in: inactive.get(name).cloned().unwrap_or_default(),
            });
        } else {
            out.push(Drift::Unmanaged {
                package,
                declared_in: inactive.get(name).cloned().unwrap_or_default(),
            });
        }
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::config::Desired;
    use crate::ports::fake::FakeBrew;
    use crate::state::Applied;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn desired(brew: &[&str]) -> Desired {
        Desired {
            machine: "box-one".into(),
            brew: set(brew),
            ..Default::default()
        }
    }

    fn applied(brew: &[&str]) -> Applied {
        Applied {
            brew: set(brew),
            ..Default::default()
        }
    }

    #[test]
    fn desired_but_never_applied_is_incoming() {
        let brew = FakeBrew::new([], []);
        let out = diff(
            &desired(&["alpha"]),
            &set(&[]),
            &set(&[]),
            &applied(&[]),
            &brew,
            &[],
        )
        .unwrap();
        assert_eq!(
            out,
            vec![Drift::IncomingPackage(PackageRef::formula("alpha"))]
        );
    }

    #[test]
    fn desired_and_applied_but_gone_locally_is_locally_removed() {
        let brew = FakeBrew::new([], []);
        let out = diff(
            &desired(&["alpha"]),
            &set(&[]),
            &set(&[]),
            &applied(&["alpha"]),
            &brew,
            &[],
        )
        .unwrap();
        assert_eq!(
            out,
            vec![Drift::LocallyRemoved(PackageRef::formula("alpha"))],
            "a deliberate local uninstall must not be silently reinstalled"
        );
    }

    #[test]
    fn installed_but_in_no_set_is_unmanaged() {
        let brew = FakeBrew::new(["bravo"], []);
        let out = diff(
            &desired(&[]),
            &set(&["bravo"]),
            &set(&[]),
            &applied(&[]),
            &brew,
            &[],
        )
        .unwrap();
        assert_eq!(
            out,
            vec![Drift::Unmanaged {
                package: PackageRef::formula("bravo"),
                declared_in: Vec::new(),
            }]
        );
    }

    #[test]
    fn ignored_packages_are_never_reported_as_unmanaged() {
        let brew = FakeBrew::new(["bravo"], []);
        let out = diff(
            &desired(&[]),
            &set(&["bravo"]),
            &set(&[]),
            &applied(&[]),
            &brew,
            &["bravo".to_string()],
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn dropped_from_a_set_is_removed_when_it_is_a_leaf() {
        let brew = FakeBrew::new(["alpha"], []);
        let out = diff(
            &desired(&[]),
            &set(&["alpha"]),
            &set(&[]),
            &applied(&["alpha"]),
            &brew,
            &[],
        )
        .unwrap();
        assert_eq!(
            out,
            vec![Drift::RemovedPackage {
                package: PackageRef::formula("alpha"),
                blocked_by: vec![],
                declared_in: Vec::new(),
            }]
        );
    }

    #[test]
    fn removal_is_blocked_when_something_still_depends_on_it() {
        let brew = FakeBrew::new(["alpha"], []).with_uses("alpha", ["bravo"]);
        let out = diff(
            &desired(&[]),
            &set(&["alpha"]),
            &set(&[]),
            &applied(&["alpha"]),
            &brew,
            &[],
        )
        .unwrap();
        assert_eq!(
            out,
            vec![Drift::RemovedPackage {
                package: PackageRef::formula("alpha"),
                blocked_by: vec!["bravo".to_string()],
                declared_in: Vec::new(),
            }]
        );
    }

    #[test]
    fn desired_and_installed_is_not_drift() {
        let brew = FakeBrew::new(["alpha"], []);
        let out = diff(
            &desired(&["alpha"]),
            &set(&["alpha"]),
            &set(&[]),
            &applied(&["alpha"]),
            &brew,
            &[],
        )
        .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn casks_are_diffed_separately_and_keep_their_flag() {
        let brew = FakeBrew::new([], []);
        let d = Desired {
            cask: set(&["charlie"]),
            ..Default::default()
        };
        let out = diff(&d, &set(&[]), &set(&[]), &Applied::default(), &brew, &[]).unwrap();
        assert_eq!(
            out,
            vec![Drift::IncomingPackage(PackageRef::cask("charlie"))]
        );
    }

    /// `desired`, plus a package that a switched-off set declares.
    fn desired_with_inactive(brew: &[&str], parked: &[(&str, &str)]) -> Desired {
        let mut d = desired(brew);
        for (pkg, set_name) in parked {
            d.inactive_brew
                .entry(pkg.to_string())
                .or_default()
                .push(set_name.to_string());
        }
        d
    }

    #[test]
    fn a_package_left_by_a_switched_off_set_says_which_set() {
        // Switching a set off is the everyday way to end up here, and
        // "not in any set" is simply untrue then: the obvious reaction is to
        // ignore the package, which writes a permanent per-machine exception
        // for something that is only parked.
        let out = diff(
            &desired_with_inactive(&[], &[("htop", "core")]),
            &set(&["htop"]),
            &BTreeSet::new(),
            &applied(&[]),
            &FakeBrew::new([], []),
            &[],
        )
        .unwrap();

        assert_eq!(
            out,
            vec![Drift::Unmanaged {
                package: PackageRef::formula("htop"),
                declared_in: vec!["core".to_string()],
            }]
        );
    }

    #[test]
    fn a_package_no_set_declares_stays_plainly_unmanaged() {
        let out = diff(
            &desired_with_inactive(&[], &[("htop", "core")]),
            &set(&["some-tool"]),
            &BTreeSet::new(),
            &applied(&[]),
            &FakeBrew::new([], []),
            &[],
        )
        .unwrap();

        assert_eq!(
            out,
            vec![Drift::Unmanaged {
                package: PackageRef::formula("some-tool"),
                declared_in: Vec::new(),
            }],
            "no set declares it, so there is nothing to name"
        );
    }

    #[test]
    fn an_uninstall_also_names_the_set_it_came_from() {
        // Here it matters more than for Unmanaged: this action removes
        // software, and "you switched off `core`" is the whole explanation.
        let out = diff(
            &desired_with_inactive(&[], &[("htop", "core")]),
            &set(&["htop"]),
            &BTreeSet::new(),
            &applied(&["htop"]),
            &FakeBrew::new([], []),
            &[],
        )
        .unwrap();

        assert_eq!(
            out,
            vec![Drift::RemovedPackage {
                package: PackageRef::formula("htop"),
                blocked_by: Vec::new(),
                declared_in: vec!["core".to_string()],
            }]
        );
    }
}
