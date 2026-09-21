use dotfix_core::drift::{Drift, Report};

/// Why a package is listed, naming the switched-off set it came from.
///
/// "not in any set" is a lie when the package sits in a set that is merely
/// inactive — and acting on that lie by ignoring it writes a permanent
/// per-machine exception for something that is only parked.
fn origin(declared_in: &[String]) -> String {
    match declared_in {
        [] => "not in any set".to_string(),
        [one] => format!("in set `{one}`, which is off on this machine"),
        many => format!(
            "in sets {}, all off on this machine",
            many.iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// The same, as a clause to append where a verb already leads the line.
fn origin_suffix(declared_in: &[String]) -> Option<String> {
    match declared_in {
        [] => None,
        [one] => Some(format!(" — was in set `{one}`, now off")),
        many => Some(format!(
            " — was in sets {}, now off",
            many.iter()
                .map(|s| format!("`{s}`"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

pub fn print_report(report: &Report) {
    if report.is_empty() {
        println!("everything in sync");
        return;
    }

    // Printed first and apart from the drift list: these are not things
    // `apply` will do, and mixing them in would suggest otherwise.
    if !report.missing.is_empty() {
        println!("not installed (dotfix cannot install these for you):");
        for m in &report.missing {
            println!("  ! {}  (needed by set `{}`)", m.name, m.set);
            if let Some(hint) = &m.hint {
                println!("      {hint}");
            }
        }
        if !report.items.is_empty() {
            println!();
        }
    }

    for item in &report.items {
        match item {
            Drift::IncomingPackage(p) => println!("  + {}  (install)", p.name),
            Drift::IncomingFile { target, set } => {
                println!("  + {}  ({set})", target.display())
            }
            Drift::LocallyRemoved(p) => {
                println!(
                    "  ? {}  (removed here — drop from set or reinstall)",
                    p.name
                )
            }
            Drift::Unmanaged {
                package,
                declared_in,
            } => println!("  ? {}  ({})", package.name, origin(declared_in)),
            Drift::RemovedPackage {
                package,
                blocked_by,
                declared_in,
            } if blocked_by.is_empty() => println!(
                "  - {}  (uninstall{})",
                package.name,
                origin_suffix(declared_in).unwrap_or_default()
            ),
            Drift::RemovedPackage {
                package,
                blocked_by,
                ..
            } => println!(
                "  - {}  (skipped, still required by {})",
                package.name,
                blocked_by.join(", ")
            ),
            Drift::RemovedFile { target } => println!("  - {}", target.display()),
            Drift::LocalEdit { target, set, .. } => {
                println!("  ~ {}  ({set} — edited locally)", target.display())
            }
        }
    }
}

use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn confirm(prompt: &str) -> anyhow::Result<bool> {
    print!("{prompt} [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Backup directory name. Seconds since the epoch keeps it sortable and needs
/// no date library.
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

pub fn prompt(label: &str) -> anyhow::Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut value = String::new();
    io::stdin().read_line(&mut value)?;
    Ok(value.trim().to_string())
}
