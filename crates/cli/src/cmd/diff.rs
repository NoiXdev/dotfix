use anyhow::Result;
use dotfix_core::diffview::{self, LineKind};

use crate::ctx;

/// Show what `apply` would change in one managed file.
///
/// The window has had this since the second phase; the command line never
/// did, so the only way to see a change before accepting it was to click.
/// Every line comes back already redacted — `diffview` passes them through
/// the values resolved while rendering that file, so a secret cannot reach
/// this output any more than it can reach the webview.
pub fn run(target: String, no_sync: bool) -> Result<()> {
    let ctx = ctx::load()?;
    let engine = ctx.engine();
    if !no_sync {
        engine.sync(&ctx.local)?;
    }

    let inspection = engine.inspect(&ctx.local)?;
    let diff = diffview::unified(&inspection, &std::path::PathBuf::from(&target), &ctx.fs)?;

    println!("--- {} (on disk)", diff.target.display());
    println!("+++ {} (set `{}`)", diff.target.display(), diff.set);
    for line in &diff.lines {
        let mark = match line.kind {
            LineKind::Context => ' ',
            LineKind::Added => '+',
            LineKind::Removed => '-',
        };
        println!("{mark}{}", line.text);
    }
    if diff.truncated {
        println!("… truncated at {} lines", diffview::MAX_LINES);
    }
    Ok(())
}
