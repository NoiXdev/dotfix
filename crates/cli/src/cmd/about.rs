use anyhow::Result;
use dotfix_core::about;

/// Version and where to read more.
///
/// `--version` already prints the number. This exists for the other half:
/// the links. They are not discoverable from a terminal otherwise, and the
/// window shows them — a window that can answer a question the command line
/// cannot is a tool with two personalities.
///
/// Deliberately reads nothing: someone whose setup is too broken for any
/// other command still needs to find the documentation.
pub fn run() -> Result<()> {
    println!("dotfix {}", about::VERSION);
    println!("Keep macOS terminal setups in sync across machines");
    println!();
    let width = about::LINKS.iter().map(|(l, _)| l.len()).max().unwrap_or(0);
    for (label, url) in about::LINKS {
        println!("  {label:<width$}  {url}");
    }
    Ok(())
}
