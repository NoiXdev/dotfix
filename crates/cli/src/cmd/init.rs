use anyhow::{Result, bail};
use dotfix_core::init::{self, Plan, Source};
use dotfix_core::paths::Paths;
use dotfix_core::ports::{Fsys, RealBrew, RealExec, RealFsys, RealGit};

use crate::{ctx, ui};

pub fn run(
    repo_url: Option<String>,
    machine: Option<String>,
    set_up_new: bool,
    yes: bool,
) -> Result<()> {
    let home = ctx::home()?;
    let paths = Paths::new(home);
    let fs = RealFsys;

    if fs.exists(&paths.local_config()) {
        bail!(
            "{} already exists — dotfix is already set up on this machine",
            paths.local_config().display()
        );
    }

    let machine = match machine {
        Some(m) => m,
        None => ui::prompt("machine name")?,
    };

    let source = if set_up_new {
        Source::New
    } else {
        let url = match repo_url {
            Some(u) => u,
            None => ui::prompt("repository url")?,
        };
        Source::Clone { url }
    };

    let plan = Plan {
        machine,
        source,
        // The CLI keeps the default; the app offers the choice.
        secret_provider: Default::default(),
        vault: None,
    };

    let pre = init::preflight(&fs, &RealExec, &paths, &plan);
    for check in &pre.checks {
        let mark = if check.ok { "ok  " } else { "FAIL" };
        println!("  [{mark}] {:<18} {}", check.name, check.detail);
    }
    if !pre.passes() {
        bail!("setup cannot continue — see the failing checks above");
    }

    if !yes {
        let what = match &plan.source {
            Source::New => format!(
                "create a new data repository at {}/dotfiles",
                paths.home.display()
            ),
            Source::Clone { .. } => format!("clone into {}/dotfiles", paths.home.display()),
        };
        if !ui::confirm(&format!("{what} and install the background agent?"))? {
            println!("aborted");
            return Ok(());
        }
    }

    let repo = init::create_or_clone(&fs, &RealGit, &RealBrew, &paths, &plan)?;
    init::configure_machine(&fs, &repo, &paths, &plan)?;

    let binary = std::env::current_exe()?;
    let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
    init::install_agent(&fs, &paths, &binary, init::DEFAULT_INTERVAL, &path_env)?;

    println!("dotfix is set up for machine `{}`", plan.machine);
    println!("repository: {}", repo.display());
    if matches!(plan.source, Source::New) {
        println!("everything installed went into set `core` — split it up by editing sets/");
        if fs.exists(&repo.join(init::IMPORTED_FRAGMENT)) {
            println!(
                "your existing ~/.zshrc was imported as {} — it is preserved, not replaced",
                init::IMPORTED_FRAGMENT
            );
        }
        println!("no remote yet — add one and push when you want other machines to follow:");
        println!("  git -C {} remote add origin <url>", repo.display());
    }
    println!("next: review the repository, then run `dotfix apply`");
    Ok(())
}
