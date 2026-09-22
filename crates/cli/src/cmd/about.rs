use anyhow::Result;
use dotfix_core::about;
use dotfix_core::ports::RealExec;
use dotfix_core::update::{self, Check};

/// Version and where to read more.
///
/// `--version` already prints the number. This exists for the other half:
/// the links. They are not discoverable from a terminal otherwise, and the
/// window shows them — a window that can answer a question the command line
/// cannot is a tool with two personalities.
///
/// Deliberately reads nothing on disk: someone whose setup is too broken for
/// any other command still needs to find the documentation. It does ask
/// GitHub whether a newer release exists — only here, and only because
/// someone ran this command.
pub fn run() -> Result<()> {
    println!("dotfix {}", about::VERSION);
    println!("Keep macOS terminal setups in sync across machines");
    println!();
    let width = about::LINKS.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
    for (label, url) in about::LINKS {
        println!("  {label:<width$}  {url}");
    }

    println!();
    match update::check(&RealExec, about::VERSION) {
        Check::Current => println!("  You are on the newest release."),
        Check::Newer { version, url } => {
            println!("  {version} is available — {url}");
        }
        // Not reaching GitHub says nothing about this installation, so it is
        // a remark rather than a failure and the exit code stays 0.
        Check::Unknown(why) => println!("  Could not check for a newer release: {why}"),
    }
    Ok(())
}
