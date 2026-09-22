//! Where dotfix sends someone who wants to know more.
//!
//! The links live here rather than once in the CLI and once in the app: the
//! window and the terminal must never point at different pages, and a moved
//! page has to be corrected in one place.

/// The released version. One constant rather than each crate's own
/// `CARGO_PKG_VERSION`, because the version a user reports has to be the
/// version of dotfix, not of whichever half of it they happened to run. The
/// release workflow sets every crate in the workspace together, and fails
/// when one of them is missing.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const PROJECT_URL: &str = "https://www.noix.dev/projekte/kontorfix/dotfix";
pub const DOCS_URL: &str = "https://docs.noix.dev/dotfix";

/// What the About area offers, in the order it offers it.
pub const LINKS: [(&str, &str); 2] = [("Project page", PROJECT_URL), ("Documentation", DOCS_URL)];

/// Whether this is one of the links above.
///
/// The app opens URLs on a request from its webview, and a command that
/// opens whatever it is handed opens anything — a `file://` path, or a page
/// that only looks like ours. Matching the exact strings is the whole check:
/// there are two of them, and neither takes a parameter.
pub fn is_known(url: &str) -> bool {
    LINKS.iter().any(|(_, known)| *known == url)
}

/// Whether this is a release page of dotfix's own repository.
///
/// The update check produces a URL that is not known in advance — it names
/// a tag — so it cannot be in the fixed list above, but it still has to be
/// openable. Matching a prefix would be wrong here: `RELEASES_PREFIX`
/// followed by anything includes `.../tag/../../../other`, so the tag is
/// checked to be a single plain path segment.
pub fn is_release_page(url: &str) -> bool {
    let Some(tag) = url.strip_prefix(RELEASES_PREFIX) else {
        return false;
    };
    !tag.is_empty()
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

const RELEASES_PREFIX: &str = "https://github.com/NoiXdev/dotfix/releases/tag/";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_link_is_https() {
        for (label, url) in LINKS {
            assert!(url.starts_with("https://"), "{label} is not https: {url}");
        }
    }

    #[test]
    fn known_links_are_accepted() {
        assert!(is_known(PROJECT_URL));
        assert!(is_known(DOCS_URL));
    }

    #[test]
    fn anything_else_is_refused() {
        // A prefix of ours, a host that merely ends with ours, and a scheme
        // that reaches the filesystem: the three shapes a guard that is not
        // an exact match tends to let through.
        for url in [
            "https://docs.noix.dev",
            "https://docs.noix.dev/dotfix/../../etc",
            "https://docs.noix.dev.example.com/dotfix",
            "file:///etc/passwd",
            "",
        ] {
            assert!(!is_known(url), "{url} should not be openable");
        }
    }

    #[test]
    fn a_release_page_of_ours_is_openable() {
        assert!(is_release_page(
            "https://github.com/NoiXdev/dotfix/releases/tag/v1.0.0-beta.1"
        ));
    }

    #[test]
    fn a_release_url_may_not_climb_out_of_the_tag() {
        // A prefix check alone would accept every one of these.
        for url in [
            "https://github.com/NoiXdev/dotfix/releases/tag/",
            "https://github.com/NoiXdev/dotfix/releases/tag/../../../settings",
            "https://github.com/NoiXdev/dotfix/releases/tag/v1/extra",
            "https://github.com/NoiXdev/other/releases/tag/v1",
            "https://github.com.evil.example/NoiXdev/dotfix/releases/tag/v1",
        ] {
            assert!(!is_release_page(url), "{url} should not be openable");
        }
    }

    #[test]
    fn the_version_is_a_version() {
        assert!(
            VERSION.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "{VERSION} does not look like a version"
        );
    }
}
