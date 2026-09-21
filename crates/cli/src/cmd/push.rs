use anyhow::Result;

use crate::{ctx, ui};

/// Commit what dotfix changed and send it to the remote.
///
/// The counterpart `sync` never had: everything dotfix writes landed in the
/// working tree and stayed there, so a tool for keeping several Macs in sync
/// could pull but never publish.
pub fn run(message: Option<String>, yes: bool) -> Result<()> {
    let ctx = ctx::load()?;
    let engine = ctx.engine();

    let dirty = {
        use dotfix_core::ports::Git;
        ctx.git.dirty_files(&ctx.local.repo)?
    };

    if dirty.is_empty() {
        println!("nothing to commit");
    } else {
        for file in &dirty {
            println!("  {file}");
        }
        if !yes && !ui::confirm("commit these and push?")? {
            println!("aborted");
            return Ok(());
        }
    }

    let message = message.unwrap_or_else(|| format!("chore: update from {}", ctx.local.machine));
    let done = engine.publish(&ctx.local, &message)?;

    if !done.committed.is_empty() {
        println!("committed {} file(s)", done.committed.len());
    }
    if done.pushed {
        println!("pushed — your other machines will see it on their next check");
    } else {
        println!("no remote yet, so nothing was sent");
    }
    Ok(())
}
