use anyhow::Result;
use dotfix_core::config::Repo;
use dotfix_core::{agent, doctor};

use crate::ctx;

const DEFAULT_INTERVAL: u32 = 3600;

pub fn run(install_agent: bool) -> Result<()> {
    let ctx = ctx::load()?;

    if install_agent {
        let binary = std::env::current_exe()?;
        let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        let written = agent::install(&ctx.fs, &ctx.paths, &binary, DEFAULT_INTERVAL, &path_env)?;
        println!("installed {}", written.display());
        println!(
            "run: launchctl bootstrap gui/$(id -u) {}",
            written.display()
        );
    }

    let repo = Repo::load(&ctx.fs, &ctx.local.repo)?;
    let machine = repo.machines.get(&ctx.local.machine);

    let checks = doctor::run_checks(
        &ctx.fs,
        &ctx.exec,
        &ctx.paths,
        machine.map(|m| m.secret_provider).unwrap_or_default(),
        machine.and_then(|m| m.vault.as_deref()),
    );

    let mut failed = 0;
    for check in &checks {
        let mark = if check.ok { "ok  " } else { "FAIL" };
        println!("  [{mark}] {:<18} {}", check.name, check.detail);
        if !check.ok {
            failed += 1;
        }
    }

    if failed > 0 {
        println!("\n{failed} check(s) failed");
        std::process::exit(1);
    }
    Ok(())
}
