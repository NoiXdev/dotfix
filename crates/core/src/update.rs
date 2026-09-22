//! Is a newer dotfix published?
//!
//! Asks the GitHub releases API — the repository is public, so no token is
//! needed — and compares what it finds with the running version. It only
//! ever reports; nothing here installs anything.
//!
//! The request runs through [`Exec`] and `curl` rather than an HTTP crate.
//! dotfix is macOS-only, so curl is always there, and going through the same
//! port as every other external command means this is testable against a
//! canned response instead of a network.
//!
//! There is no background polling and no cache: the check happens when
//! somebody opens About or runs `dotfix about`, and at no other time. A tool
//! that asks a server about its user on a timer needs a setting to turn that
//! off and a paragraph explaining itself; one that asks only when asked
//! needs neither.

use serde::Deserialize;

use crate::ports::Exec;

const RELEASES_URL: &str = "https://api.github.com/repos/NoiXdev/dotfix/releases?per_page=20";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Check {
    /// Nothing newer is published.
    Current,
    /// A newer release exists.
    Newer { version: String, url: String },
    /// The question could not be answered — offline, rate-limited, GitHub
    /// having a bad day. Deliberately not an error: failing to reach a
    /// server is not a problem with the user's installation, and About must
    /// still show the version it already knows.
    Unknown(String),
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Compare the newest suitable release with `current`.
///
/// A pre-release only counts when the running version is itself a
/// pre-release. Someone on 1.0.0 must not be nudged towards 1.1.0-beta.1 —
/// that is the same rule the Homebrew tap follows, where only final releases
/// are published. Someone already on a beta, however, would otherwise never
/// hear about the next one, since `/releases/latest` skips pre-releases
/// entirely. That is why this reads the list rather than `latest`.
pub fn check(exec: &dyn Exec, current: &str) -> Check {
    let body = match exec.run(
        "curl",
        &[
            "-fsSL",
            "--max-time",
            "10",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: dotfix",
            RELEASES_URL,
        ],
    ) {
        Ok(body) => body,
        Err(e) => return Check::Unknown(e.to_string()),
    };
    newest(&body, current)
}

/// The decision itself, separated from fetching so it can be tested against
/// real GitHub payloads without a process at all.
/// A payload that cannot be read is [`Check::Unknown`] rather than an error:
/// the caller's job is to show the user something sensible either way, and
/// there is nothing they could do differently about a malformed response.
pub fn newest(body: &str, current: &str) -> Check {
    let releases: Vec<Release> = match serde_json::from_str(body) {
        Ok(r) => r,
        Err(e) => return Check::Unknown(format!("cannot read GitHub's answer: {e}")),
    };
    let Some(mine) = Version::parse(current) else {
        return Check::Unknown(format!("cannot read `{current}` as a version"));
    };

    let best = releases
        .iter()
        .filter(|r| !r.draft)
        // A beta hears about betas and finals; a final hears only finals.
        .filter(|r| mine.is_prerelease() || !r.prerelease)
        .filter_map(|r| Version::parse(r.tag_name.trim_start_matches('v')).map(|v| (v, r)))
        .max_by(|(a, _), (b, _)| a.cmp(b));

    match best {
        Some((v, r)) if v > mine => Check::Newer {
            version: v.to_string(),
            url: r.html_url.clone(),
        },
        _ => Check::Current,
    }
}

/// Just enough semver for the versions dotfix itself publishes: `X.Y.Z` or
/// `X.Y.Z-<id>.<n>`, which is the only shape the release workflow can
/// produce. Anything else fails to parse and is skipped rather than guessed
/// at.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    /// `None` for a final release. Ordered after a release with the same
    /// numbers, per semver: 1.0.0-beta.1 comes *before* 1.0.0.
    pre: Option<(String, u64)>,
}

impl Version {
    fn parse(s: &str) -> Option<Self> {
        let (base, pre) = match s.split_once('-') {
            Some((base, rest)) => {
                let (id, n) = rest.split_once('.')?;
                if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
                    return None;
                }
                (base, Some((id.to_string(), n.parse().ok()?)))
            }
            None => (s, None),
        };
        let mut parts = base.split('.');
        let v = Version {
            major: parts.next()?.parse().ok()?,
            minor: parts.next()?.parse().ok()?,
            patch: parts.next()?.parse().ok()?,
            pre,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(v)
    }

    fn is_prerelease(&self) -> bool {
        self.pre.is_some()
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if let Some((id, n)) = &self.pre {
            write!(f, "-{id}.{n}")?;
        }
        Ok(())
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| match (&self.pre, &other.pre) {
                // A version with a pre-release part is *older* than the same
                // numbers without one. Getting this backwards would tell
                // every 1.0.0 user to move to 1.0.0-beta.1.
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                // alpha < beta < rc happens to be ASCII order, which is also
                // what semver prescribes for alphanumeric identifiers.
                (Some((a_id, a_n)), Some((b_id, b_n))) => a_id.cmp(b_id).then(a_n.cmp(b_n)),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::fake::FakeExec;

    fn release(tag: &str, prerelease: bool) -> String {
        format!(
            r#"{{"tag_name":"{tag}","html_url":"https://github.com/NoiXdev/dotfix/releases/tag/{tag}","draft":false,"prerelease":{prerelease}}}"#
        )
    }

    fn body(entries: &[(&str, bool)]) -> String {
        let items: Vec<String> = entries.iter().map(|(t, p)| release(t, *p)).collect();
        format!("[{}]", items.join(","))
    }

    // --- ordering -------------------------------------------------------

    #[test]
    fn a_prerelease_is_older_than_the_same_numbers_without_one() {
        // Backwards, this tells every 1.0.0 user to "upgrade" to a beta.
        let beta = Version::parse("1.0.0-beta.1").unwrap();
        let final_ = Version::parse("1.0.0").unwrap();
        assert!(beta < final_);
    }

    #[test]
    fn prereleases_of_one_version_are_ordered_by_stage_then_number() {
        let order = [
            "1.0.0-alpha.1",
            "1.0.0-alpha.2",
            "1.0.0-beta.1",
            "1.0.0-beta.2",
            "1.0.0-rc.1",
            "1.0.0",
        ];
        for pair in order.windows(2) {
            let a = Version::parse(pair[0]).unwrap();
            let b = Version::parse(pair[1]).unwrap();
            assert!(a < b, "{} should come before {}", pair[0], pair[1]);
        }
    }

    #[test]
    fn numbers_compare_numerically_not_as_text() {
        // "10" sorts before "9" as text; a released 1.0.10 must still win.
        assert!(Version::parse("1.0.9").unwrap() < Version::parse("1.0.10").unwrap());
        assert!(Version::parse("1.0.0-beta.9").unwrap() < Version::parse("1.0.0-beta.10").unwrap());
        assert!(Version::parse("1.9.0").unwrap() < Version::parse("1.10.0").unwrap());
    }

    #[test]
    fn unreadable_versions_are_refused_rather_than_guessed_at() {
        for s in [
            "",
            "1.0",
            "1.0.0.0",
            "1.0.0-beta",
            "v1.0.0",
            "1.0.0-beta.x",
            "x.y.z",
        ] {
            assert!(Version::parse(s).is_none(), "{s} should not parse");
        }
    }

    // --- the decision ---------------------------------------------------

    #[test]
    fn a_newer_final_release_is_offered() {
        let out = newest(&body(&[("v1.0.1", false), ("v1.0.0", false)]), "1.0.0");
        assert_eq!(
            out,
            Check::Newer {
                version: "1.0.1".into(),
                url: "https://github.com/NoiXdev/dotfix/releases/tag/v1.0.1".into(),
            }
        );
    }

    #[test]
    fn the_newest_wins_regardless_of_the_order_github_returns() {
        let out = newest(
            &body(&[("v1.0.1", false), ("v1.2.0", false), ("v1.0.9", false)]),
            "1.0.0",
        );
        assert!(matches!(out, Check::Newer { ref version, .. } if version == "1.2.0"));
    }

    #[test]
    fn running_the_newest_release_reports_current() {
        assert_eq!(newest(&body(&[("v1.0.0", false)]), "1.0.0"), Check::Current);
    }

    #[test]
    fn running_ahead_of_every_release_reports_current() {
        // A local build from main is newer than anything published; it must
        // not be told to downgrade.
        assert_eq!(newest(&body(&[("v1.0.0", false)]), "1.1.0"), Check::Current);
    }

    #[test]
    fn a_final_version_is_never_pointed_at_a_prerelease() {
        // The rule the Homebrew tap follows: only final releases reach
        // someone who is on a final release.
        let out = newest(
            &body(&[("v1.1.0-beta.1", true), ("v1.0.0", false)]),
            "1.0.0",
        );
        assert_eq!(out, Check::Current);
    }

    #[test]
    fn a_prerelease_hears_about_the_next_prerelease() {
        // Otherwise a beta tester never learns there is a newer beta —
        // /releases/latest skips them entirely, which is why this reads the
        // list instead.
        let out = newest(
            &body(&[("v1.0.0-beta.2", true), ("v1.0.0-beta.1", true)]),
            "1.0.0-beta.1",
        );
        assert!(matches!(out, Check::Newer { ref version, .. } if version == "1.0.0-beta.2"));
    }

    #[test]
    fn a_prerelease_also_hears_about_a_final_release() {
        let out = newest(
            &body(&[("v1.0.0", false), ("v1.0.0-beta.1", true)]),
            "1.0.0-beta.1",
        );
        assert!(matches!(out, Check::Newer { ref version, .. } if version == "1.0.0"));
    }

    #[test]
    fn drafts_are_ignored() {
        let json = r#"[{"tag_name":"v2.0.0","html_url":"u","draft":true,"prerelease":false}]"#;
        assert_eq!(newest(json, "1.0.0"), Check::Current);
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_skipped_not_fatal() {
        let out = newest(&body(&[("nightly", false), ("v1.0.1", false)]), "1.0.0");
        assert!(matches!(out, Check::Newer { ref version, .. } if version == "1.0.1"));
    }

    #[test]
    fn no_releases_at_all_is_current_not_an_error() {
        assert_eq!(newest("[]", "1.0.0"), Check::Current);
    }

    #[test]
    fn a_malformed_answer_is_unknown_rather_than_an_error() {
        assert!(matches!(newest("not json", "1.0.0"), Check::Unknown(_)));
    }

    // --- the fetch ------------------------------------------------------

    #[test]
    fn being_offline_is_unknown_and_never_fails() {
        let key = format!(
            "curl -fsSL --max-time 10 -H Accept: application/vnd.github+json -H User-Agent: dotfix {RELEASES_URL}"
        );
        let exec = FakeExec::new([]).with_error(&key, "Could not resolve host: api.github.com");
        match check(&exec, "1.0.0") {
            Check::Unknown(msg) => assert!(msg.contains("api.github.com"), "{msg}"),
            other => panic!("offline should be Unknown, got {other:?}"),
        }
    }

    #[test]
    fn the_fetch_and_the_decision_agree() {
        let key = format!(
            "curl -fsSL --max-time 10 -H Accept: application/vnd.github+json -H User-Agent: dotfix {RELEASES_URL}"
        );
        let exec = FakeExec::new([(key.as_str(), body(&[("v1.0.1", false)]).as_str())]);
        assert!(matches!(check(&exec, "1.0.0"), Check::Newer { .. }));
    }
}
