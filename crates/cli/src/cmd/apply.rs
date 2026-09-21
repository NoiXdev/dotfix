use anyhow::Result;
use dotfix_core::apply::{execute, plan};
use dotfix_core::ports::Fsys;

use crate::{ctx, ui};

pub fn run(yes: bool, dry_run: bool, no_sync: bool) -> Result<()> {
    let ctx = ctx::load()?;
    let engine = ctx.engine();

    if !no_sync {
        engine.sync(&ctx.local)?;
    }

    let inspection = engine.inspect(&ctx.local)?;
    let plan = plan(&inspection);

    if plan.is_empty() {
        println!("nothing to apply");
        return Ok(());
    }

    println!("plan:");
    for line in plan.describe() {
        println!("  {line}");
    }

    if dry_run {
        return Ok(());
    }
    if !yes && !ui::confirm("apply these changes?")? {
        println!("aborted");
        return Ok(());
    }

    execute(&plan, &engine, &ui::timestamp())?;
    println!("applied {} change(s)", plan.actions.len());

    // Refresh the status line so the next shell reflects reality immediately.
    let after = engine.inspect(&ctx.local)?;
    let line = dotfix_core::status_line::render(&after.report.counts()).unwrap_or_default();
    ctx.fs.write(&ctx.paths.status_line(), &line, 0o644)?;

    Ok(())
}
