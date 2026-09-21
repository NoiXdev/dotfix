use anyhow::{Result, bail};
use dotfix_core::config::ProviderKind;
use dotfix_core::settings;

use crate::{ctx, ui};

/// Show or change what `init` decided once.
///
/// Every change the window can make has to be reachable from here too —
/// otherwise the tool has two answers to the same question, and the one
/// people script against is the poorer of the two.
pub fn run(
    remote: Option<String>,
    provider: Option<String>,
    vault: Option<String>,
    rename: Option<String>,
    unignore: Option<String>,
    yes: bool,
) -> Result<()> {
    let ctx = ctx::load()?;

    if let Some(package) = unignore {
        let left = settings::unignore(&ctx.fs, &ctx.local, &package)?;
        println!("`{package}` is no longer ignored — it will show up as unmanaged again");
        if left.is_empty() {
            println!("nothing is ignored on this machine now");
        }
        return Ok(());
    }

    if remote.is_none() && provider.is_none() && rename.is_none() {
        let s = settings::read(&ctx.fs, &ctx.git, &ctx.local)?;
        println!("  machine          {}", s.machine);
        println!("  repository       {}", s.repo.display());
        println!(
            "  remote           {}",
            s.remote.as_deref().unwrap_or("none — add one to sync")
        );
        println!("  secret provider  {}", provider_name(s.secret_provider));
        if let Some(v) = &s.vault {
            println!("  vault            {v}");
        }
        if s.ignored.is_empty() {
            println!("  ignored          none");
        } else {
            println!("  ignored          {}", s.ignored.join(", "));
            println!("                   remove one with --unignore <name>");
        }
        return Ok(());
    }

    if let Some(url) = remote {
        settings::set_remote(&ctx.git, &ctx.local, &url)?;
        println!("remote is now {url}");
    }

    if let Some(kind) = provider {
        let kind = parse_provider(&kind)?;
        // The check that makes this safe: a provider recorded but unable to
        // serve fails at the next apply, not here, and possibly on a
        // schedule with nobody watching.
        let engine = ctx.engine();
        let verify = |p: ProviderKind, v: Option<&str>| engine.verify_secrets(&ctx.local, p, v);
        let cfg = settings::set_provider(&ctx.fs, &ctx.local, kind, vault, &verify)?;
        println!(
            "secret provider is now {}{}",
            provider_name(cfg.secret_provider),
            cfg.vault
                .map(|v| format!(" (vault {v})"))
                .unwrap_or_default()
        );
        println!("commit the repository so your other machines see it");
    }

    if let Some(new_name) = rename {
        if !yes
            && !ui::confirm(&format!(
                "rename this machine from `{}` to `{new_name}`?",
                ctx.local.machine
            ))?
        {
            println!("aborted");
            return Ok(());
        }
        let moved = settings::rename_machine(&ctx.fs, &ctx.paths, &ctx.local, &new_name)?;
        println!("this machine is now `{}`", moved.machine);
        println!("commit the repository so your other machines see it");
        println!(
            "a deploy key registered on GitHub still carries the old name as its title — \
             dotfix cannot change that without registering a new key"
        );
    }

    Ok(())
}

fn provider_name(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Keychain => "keychain",
        ProviderKind::OnePassword => "1password",
        ProviderKind::Age => "age",
    }
}

fn parse_provider(raw: &str) -> Result<ProviderKind> {
    Ok(match raw {
        "keychain" => ProviderKind::Keychain,
        "1password" => ProviderKind::OnePassword,
        "age" => ProviderKind::Age,
        other => bail!("unknown secret provider `{other}` — expected keychain, 1password or age"),
    })
}
