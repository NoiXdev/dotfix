use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::paths::Paths;
use crate::ports::Fsys;

pub const LABEL: &str = "dev.noix.dotfix";

/// A LaunchAgent runs at **login**, not at system boot. Boot-time execution
/// would need a LaunchDaemon running as root, which reaches neither the user
/// keychain nor Homebrew correctly.
pub fn plist(binary: &Path, interval: u32, path_env: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{binary}</string>
        <string>status</string>
        <string>--write-status-line</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>StartInterval</key>
    <integer>{interval}</integer>
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>{path_env}</string>
    </dict>
    <key>ProcessType</key>
    <string>Background</string>
</dict>
</plist>
"#,
        binary = binary.display()
    )
}

pub fn install(
    fs: &dyn Fsys,
    paths: &Paths,
    binary: &Path,
    interval: u32,
    path_env: &str,
) -> Result<PathBuf> {
    let target = paths.launch_agent();
    fs.write(&target, &plist(binary, interval, path_env), 0o644)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::paths::Paths;
    use crate::ports::fake::FakeFsys;

    #[test]
    fn plist_runs_at_load_and_on_an_interval() {
        let out = plist(
            Path::new("/opt/homebrew/bin/dotfix"),
            3600,
            "/opt/homebrew/bin:/usr/bin:/bin",
        );
        assert!(out.contains("<key>RunAtLoad</key>"));
        assert!(out.contains("<key>StartInterval</key>"));
        assert!(out.contains("<integer>3600</integer>"));
    }

    #[test]
    fn plist_calls_status_with_write_status_line() {
        let out = plist(Path::new("/opt/homebrew/bin/dotfix"), 3600, "/usr/bin");
        assert!(out.contains("<string>status</string>"));
        assert!(out.contains("<string>--write-status-line</string>"));
    }

    #[test]
    fn plist_sets_path_because_launchd_does_not_inherit_it() {
        let out = plist(
            Path::new("/opt/homebrew/bin/dotfix"),
            3600,
            "/opt/homebrew/bin:/usr/bin",
        );
        assert!(out.contains("<key>EnvironmentVariables</key>"));
        assert!(out.contains("/opt/homebrew/bin:/usr/bin"));
    }

    #[test]
    fn plist_is_valid_property_list_xml() {
        let out = plist(Path::new("/opt/homebrew/bin/dotfix"), 3600, "/usr/bin");
        let parsed: plist::Value = plist::from_bytes(out.as_bytes()).expect("valid plist");
        let dict = parsed.as_dictionary().expect("a dict at the top level");
        assert_eq!(dict.get("Label").and_then(|v| v.as_string()), Some(LABEL));
        assert_eq!(
            dict.get("StartInterval")
                .and_then(|v| v.as_signed_integer()),
            Some(3600)
        );
    }

    #[test]
    fn install_writes_to_the_launch_agents_directory() {
        let fs = FakeFsys::new();
        let paths = Paths::new(PathBuf::from("/Users/test"));
        let written = install(
            &fs,
            &paths,
            Path::new("/opt/homebrew/bin/dotfix"),
            3600,
            "/usr/bin",
        )
        .unwrap();
        assert_eq!(
            written,
            PathBuf::from("/Users/test/Library/LaunchAgents/dev.noix.dotfix.plist")
        );
        assert!(fs.read(&written).unwrap().contains(LABEL));
    }
}
