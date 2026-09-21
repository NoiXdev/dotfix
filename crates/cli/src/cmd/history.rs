use anyhow::Result;
use dotfix_core::ports::Git;

use crate::ctx;

/// Recent commits in the configuration repository.
///
/// Plain `git log` in `~/dotfiles` says the same thing. It is here because
/// the window has it, and a window that can answer a question the command
/// line cannot is a tool with two personalities.
pub fn run(limit: usize) -> Result<()> {
    let ctx = ctx::load()?;
    let commits = ctx.git.log(&ctx.local.repo, limit.min(200))?;

    if commits.is_empty() {
        println!("no commits yet");
        return Ok(());
    }
    for commit in commits {
        println!("  {}  {}  {}", commit.hash, commit.date, commit.subject);
    }
    Ok(())
}
