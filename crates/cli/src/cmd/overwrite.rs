use anyhow::{Result, bail};
use dotfix_core::apply::{execute, plan_overwrite};
use dotfix_core::drift::Drift;

use crate::{ctx, ui};

/// Take the repository's version of a file you edited locally.
///
/// The opposite of `adopt`, and the more destructive of the two: `adopt`
/// changes what every machine receives, while this throws away work on this
/// one. `apply` deliberately refuses to touch a hand-edited file, so this is
/// the only way to say "I want the repository's version after all".
///
/// It existed in the window from the start and had no command-line
/// equivalent — which is how a real `.zshrc` came to be replaced with nobody
/// able to reach for the same action, or its backup, from a terminal.
///
/// `execute` writes a timestamped backup before overwriting, exactly as
/// `apply` does. The path is printed, because a backup nobody can find is
/// not a backup.
pub fn run(targets: Vec<String>, yes: bool, no_sync: bool) -> Result<()> {
    let ctx = ctx::load()?;
    let engine = ctx.engine();
    if !no_sync {
        engine.sync(&ctx.local)?;
    }

    let inspection = engine.inspect(&ctx.local)?;
    let edited: Vec<&Drift> = inspection
        .report
        .items
        .iter()
        .filter(|d| matches!(d, Drift::LocalEdit { .. }))
        .collect();

    if edited.is_empty() {
        println!("no locally edited files — nothing to overwrite");
        return Ok(());
    }

    // No arguments means "show me what I could overwrite", never "all of
    // them": a bare command that destroys every local edit is the wrong
    // default for an action with no undo beyond the backup.
    if targets.is_empty() {
        println!("locally edited files:");
        for drift in &edited {
            println!("  {}", drift.label());
        }
        println!("pass one or more paths to overwrite them");
        return Ok(());
    }

    let ids: Vec<String> = targets
        .iter()
        .map(|t| {
            let wanted = shellexpand(t);
            edited
                .iter()
                .find(|d| d.label() == wanted)
                .map(|d| d.id())
                .ok_or_else(|| anyhow::anyhow!("`{t}` is not a locally edited managed file"))
        })
        .collect::<Result<_>>()?;

    let plan = plan_overwrite(&inspection, &ids);
    if plan.is_empty() {
        bail!("nothing to overwrite");
    }

    for target in &targets {
        println!("  overwrite {target} with the repository's version");
    }
    if !yes && !ui::confirm("this discards the local changes — continue?")? {
        println!("aborted");
        return Ok(());
    }

    let stamp = ui::timestamp();
    execute(&plan, &engine, &stamp)?;
    println!(
        "done — the previous contents are in {}",
        ctx.paths.backups(&stamp).display()
    );
    Ok(())
}

/// `~/x` as the report writes it: absolute, since that is what `label()`
/// returns and what the user sees in `dotfix status`.
fn shellexpand(raw: &str) -> String {
    match raw.strip_prefix("~/") {
        Some(rest) => std::env::var("HOME")
            .map(|h| format!("{h}/{rest}"))
            .unwrap_or_else(|_| raw.to_string()),
        None => raw.to_string(),
    }
}
