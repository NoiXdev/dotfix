use anyhow::{Result, bail};
use dotfix_core::adopt::{self, Proposal, apply_proposal, proposals_for};
use dotfix_core::config::Repo;

use crate::{ctx, ui};

pub fn run(set: Option<String>, yes: bool, no_sync: bool) -> Result<()> {
    let ctx = ctx::load()?;
    let engine = ctx.engine();

    if !no_sync {
        engine.sync(&ctx.local)?;
    }

    let inspection = engine.inspect(&ctx.local)?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo)?;

    let default_set = match set {
        Some(s) if repo.sets.contains_key(&s) => s,
        Some(s) => bail!("unknown set `{s}`"),
        None => repo
            .machines
            .get(&ctx.local.machine)
            .and_then(|m| m.sets.first().cloned())
            .unwrap_or_else(|| "core".to_string()),
    };

    let mut accepted = 0usize;

    // Software that is here but undeclared, offered alongside the drift.
    // Detection only ran at `init`, so anything installed since had no way in.
    let machine = repo
        .machines
        .get(&ctx.local.machine)
        .map(|m| m.sets.clone())
        .unwrap_or_default();
    for proposal in
        adopt::undeclared_requirements(&repo, &machine, &ctx.fs, &ctx.paths.home, &default_set)
    {
        if !yes && !ui::confirm(&format!("{}?", describe(&proposal)))? {
            continue;
        }
        apply_proposal(&proposal, &repo, &ctx.local.machine, &ctx.fs)?;
        accepted += 1;
    }

    for drift in &inspection.report.items {
        for proposal in proposals_for(drift, &repo, &default_set) {
            if let Proposal::RefuseFile { target, reason } = &proposal {
                println!("  refused {}: {reason}", target.display());
                continue;
            }

            if !yes && !ui::confirm(&format!("{}?", describe(&proposal)))? {
                continue;
            }

            apply_proposal(&proposal, &repo, &ctx.local.machine, &ctx.fs)?;
            accepted += 1;
            // Stop after the first accepted proposal for this drift item: the
            // choices are alternatives, not a checklist.
            break;
        }
    }

    if accepted == 0 {
        println!("nothing adopted");
        return Ok(());
    }

    println!("adopted {accepted} item(s) — commit the repository when ready");
    Ok(())
}

fn describe(proposal: &Proposal) -> String {
    match proposal {
        Proposal::AddPackage { package, set } => format!("add {} to set `{set}`", package.name),
        Proposal::DropPackage { package, set } => {
            format!("drop {} from set `{set}`", package.name)
        }
        Proposal::DeclareRequirement { requirement, set } => format!(
            "record `{}` as software set `{set}` needs",
            requirement.name
        ),
        Proposal::IgnorePackage { package } => {
            format!("ignore {} on this machine", package.name)
        }
        Proposal::WriteBackFile { target, source } => {
            format!("write {} back into {}", target.display(), source.display())
        }
        Proposal::RefuseFile { target, .. } => format!("refuse {}", target.display()),
    }
}
