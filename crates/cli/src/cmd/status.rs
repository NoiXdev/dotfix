use anyhow::Result;
use dotfix_core::status_line;

use crate::{ctx, ui};

pub fn run(json: bool, write_status_line: bool, no_sync: bool) -> Result<()> {
    let ctx = ctx::load()?;
    let engine = ctx.engine();

    if !no_sync {
        // A background run must stay silent when the network is down.
        if let Err(err) = engine.sync(&ctx.local) {
            if write_status_line {
                return Ok(());
            }
            eprintln!("warning: could not sync repository: {err}");
        }
    }

    let inspection = engine.inspect(&ctx.local)?;

    if write_status_line {
        let line = status_line::render(&inspection.report.counts()).unwrap_or_default();
        dotfix_core::ports::Fsys::write(&ctx.fs, &ctx.paths.status_line(), &line, 0o644)?;
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&inspection.report)?);
    } else {
        ui::print_report(&inspection.report);
    }

    Ok(())
}
