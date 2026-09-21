mod cmd;
mod ctx;
mod ui;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "dotfix", version, about = "Keep macOS terminal setups in sync")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Apply incoming changes and removals
    Apply {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        no_sync: bool,
    },
    /// Take locally installed packages or edited configs into the repository
    Adopt {
        #[arg(long)]
        set: Option<String>,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        no_sync: bool,
    },
    /// Show what apply would change in one managed file
    Diff {
        /// The file on this machine, e.g. ~/.gitconfig
        target: String,
        #[arg(long)]
        no_sync: bool,
    },
    /// Recent commits in the configuration repository
    History {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Replace a locally edited file with the repository's version
    Overwrite {
        /// Files to overwrite; with none, lists what could be
        targets: Vec<String>,
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        no_sync: bool,
    },
    /// Commit what changed in the repository and push it
    Push {
        /// Commit message; a default naming this machine is used otherwise
        #[arg(long, short)]
        message: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Verify the local setup
    Doctor {
        /// Write the LaunchAgent plist
        #[arg(long)]
        install_agent: bool,
    },
    /// Show or change the remote, the secret provider, or this machine's name
    Config {
        /// Point the repository at a different remote
        #[arg(long)]
        remote: Option<String>,
        /// keychain, 1password or age
        #[arg(long)]
        provider: Option<String>,
        /// 1Password vault, required with --provider 1password
        #[arg(long)]
        vault: Option<String>,
        /// Rename this machine, everywhere the name is written
        #[arg(long)]
        rename: Option<String>,
        /// Stop ignoring a package on this machine
        #[arg(long)]
        unignore: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Set up dotfix on this machine
    Init {
        /// Clone this repository (path B)
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        machine: Option<String>,
        /// Create a new data repository from what is installed here (path A)
        #[arg(long)]
        set_up_new: bool,
        #[arg(long)]
        yes: bool,
    },
    /// List or toggle this machine's sets
    Sets {
        #[arg(long)]
        enable: Option<String>,
        #[arg(long)]
        disable: Option<String>,
    },
    /// Show what differs between this machine and the repository
    Status {
        #[arg(long)]
        json: bool,
        /// Write the shell status line instead of printing a report
        #[arg(long)]
        write_status_line: bool,
        /// Skip the git fetch/pull
        #[arg(long)]
        no_sync: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Apply {
            yes,
            dry_run,
            no_sync,
        } => cmd::apply::run(yes, dry_run, no_sync),
        Command::Adopt { set, yes, no_sync } => cmd::adopt::run(set, yes, no_sync),
        Command::Diff { target, no_sync } => cmd::diff::run(target, no_sync),
        Command::Overwrite {
            targets,
            yes,
            no_sync,
        } => cmd::overwrite::run(targets, yes, no_sync),
        Command::History { limit } => cmd::history::run(limit),
        Command::Push { message, yes } => cmd::push::run(message, yes),
        Command::Doctor { install_agent } => cmd::doctor::run(install_agent),
        Command::Config {
            remote,
            provider,
            vault,
            rename,
            unignore,
            yes,
        } => cmd::config::run(remote, provider, vault, rename, unignore, yes),
        Command::Init {
            repo,
            machine,
            set_up_new,
            yes,
        } => cmd::init::run(repo, machine, set_up_new, yes),
        Command::Sets { enable, disable } => cmd::sets::run(enable, disable),
        Command::Status {
            json,
            write_status_line,
            no_sync,
        } => cmd::status::run(json, write_status_line, no_sync),
    };

    if let Err(err) = result {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}
