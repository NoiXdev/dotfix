use anyhow::{Result, bail};
use dotfix_core::config::Repo;
use dotfix_core::sets;

use crate::ctx;

pub fn run(enable: Option<String>, disable: Option<String>) -> Result<()> {
    let ctx = ctx::load()?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo)?;

    if enable.is_none() && disable.is_none() {
        for entry in sets::list(&repo, &ctx.fs, &ctx.local.machine)? {
            let mark = if entry.active { "x" } else { " " };
            println!("  [{mark}] {}", entry.name);
        }
        return Ok(());
    }

    let mut cfg = repo
        .machines
        .get(&ctx.local.machine)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("unknown machine `{}`", ctx.local.machine))?;

    if let Some(name) = enable {
        cfg = sets::toggle(&repo, &ctx.local.machine, &name, true)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    if let Some(name) = disable {
        // Mutate the `cfg` we already hold — which is the *result* of the
        // `--enable` above when one was given — instead of calling
        // `sets::toggle(.., false)`. `toggle` re-derives the set list from
        // `repo`, and `repo` still holds the machine's on-disk list, so it
        // would silently drop the enable that has not been saved yet and
        // `--enable a --disable b` would only ever apply the disable. Only
        // the name still has to be validated against `repo`, which is what
        // the check below does.
        if !repo.sets.contains_key(&name) {
            bail!("unknown set `{name}`");
        }
        cfg.sets.retain(|s| s != &name);
    }

    let path = sets::save(&ctx.fs, &repo, &ctx.local.machine, &cfg)?;
    println!("updated {}", path.display());
    Ok(())
}
