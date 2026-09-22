//! Thin wrappers around `dotfix-core` and [`crate::view`]. A command may
//! orchestrate, never decide — anything worth a test belongs in `view` or in
//! the core crate.

use std::path::{Path, PathBuf};

use dotfix_core::adopt::{self, Proposal};
use dotfix_core::apply::{execute, plan_overwrite, plan_selected};
use dotfix_core::config::Repo;
use dotfix_core::diffview::{self, FileDiff};
use dotfix_core::drift::Drift;
use dotfix_core::init::remote::{self, RepoRequest};
use dotfix_core::init::ssh::{self, Reachability};
use dotfix_core::init::{self, Plan, Source, github_host};
use dotfix_core::ports::{Commit, Git, RealBrew, RealExec, RealFsys, RealGit};
use dotfix_core::sets::{self, Entry};
use dotfix_core::settings;

use crate::ctx::Ctx;
use crate::view::{self, Overview};

/// Tauri needs a `Serialize` error; core errors already render themselves
/// usefully and never carry secret values.
pub fn to_cmd_err(err: dotfix_core::Error) -> String {
    err.to_string()
}

/// Every command that changed something — and `refresh` — ends here.
///
/// The window is about to render this exact `Overview`; the menubar must not
/// keep showing the picture it replaced. A tray update failing is not a
/// reason to fail the command the user actually asked for (the window still
/// gets the truth), so it is reported and swallowed.
fn settled(app: &tauri::AppHandle, overview: Overview) -> Overview {
    if let Err(err) = crate::tray::update(app, &overview) {
        eprintln!("dotfix: could not update the menubar: {err}");
    }
    overview
}

fn seconds_since_epoch() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
        .to_string()
}

/// Whether `adopt_item` may act on this drift at all.
///
/// The brief's `adopt_item` would pick `Proposal::WriteBackFile` for a
/// `Drift::LocalEdit` and write the hand-edited local file straight into the
/// repository. That is the one action that silently changes what every other
/// machine receives on its next sync, so the design keeps it a CLI operation
/// (`dotfix adopt`) rather than something the menubar window can trigger with
/// a click. The UI is never expected to call it that way, but a guarantee
/// that rests only on the caller's good manners is not a guarantee — so this
/// is enforced here, not just documented.
fn adoptable(drift: &Drift) -> Result<(), String> {
    if matches!(drift, Drift::LocalEdit { .. }) {
        return Err(
            "editing a managed file back into the repository is a CLI operation: run \
             `dotfix adopt`"
                .to_string(),
        );
    }
    Ok(())
}

/// Human wording for a proposal, used only to explain why the action the
/// window asked for is not one of them.
fn proposal_name(proposal: &Proposal) -> &'static str {
    match proposal {
        Proposal::AddPackage { .. } => "adding it to a set",
        Proposal::DropPackage { .. } => "dropping it from its set",
        Proposal::IgnorePackage { .. } => "ignoring it",
        Proposal::WriteBackFile { .. } => "writing it back to the repository",
        Proposal::DeclareRequirement { .. } => "declare",
        Proposal::RefuseFile { .. } => "refusing it",
    }
}

/// Pick the proposal the window asked for, or say why there is none.
///
/// The `ignore` flag is the whole request: `true` means "the Ignore button",
/// anything else means "the primary button". Returning an error when nothing
/// matches is the point — this used to fall through to `Ok(overview)`, so a
/// click that wrote nothing looked exactly like a click that worked, and the
/// row simply stayed on screen with no explanation.
fn choose_proposal(proposals: Vec<Proposal>, ignore: bool, id: &str) -> Result<Proposal, String> {
    let asked = if ignore { "ignore" } else { "adopt" };
    if let Some(proposal) = proposals
        .iter()
        .find(|p| matches!(p, Proposal::IgnorePackage { .. }) == ignore)
    {
        return Ok(proposal.clone());
    }

    let offered: Vec<&str> = proposals.iter().map(proposal_name).collect();
    Err(if offered.is_empty() {
        format!("cannot {asked} `{id}`: dotfix has no action for this item")
    } else {
        format!(
            "cannot {asked} `{id}`: the only thing dotfix offers for it is {}",
            offered.join(" or ")
        )
    })
}

/// Current state without touching the network.
#[tauri::command]
pub fn overview() -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let inspection = ctx.engine().inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(with_undeclared(&ctx, view::overview(&inspection)))
}

/// Pull first, then report. Surfaces a diverged repository as an error rather
/// than merging.
#[tauri::command]
pub fn refresh(app: tauri::AppHandle) -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let engine = ctx.engine();
    engine.sync(&ctx.local).map_err(to_cmd_err)?;
    let inspection = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(settled(
        &app,
        with_undeclared(&ctx, view::overview(&inspection)),
    ))
}

/// Apply the selected drift items and return the state afterwards.
#[tauri::command]
pub fn apply_items(app: tauri::AppHandle, ids: Vec<String>) -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let engine = ctx.engine();

    let inspection = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    let plan = plan_selected(&inspection, &ids);
    if !plan.is_empty() {
        execute(&plan, &engine, &seconds_since_epoch()).map_err(to_cmd_err)?;
    }

    let after = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(settled(&app, with_undeclared(&ctx, view::overview(&after))))
}

/// Overwrite specific hand-edited files with what the repository would
/// write. Deliberately its own command, calling `plan_overwrite` rather
/// than `apply_items`/`plan_selected`: a `local_edit` id is refused by
/// `plan_selected` on purpose (see `adoptable`, and
/// `dotfix_core::apply::plan`'s doc comment) so that a bulk apply can never
/// silently clobber a hand edit. This is the explicit, single-file opposite
/// of that safety property — the only path that discards a local edit, and
/// it only ever acts on the ids it is given.
#[tauri::command]
pub fn overwrite_files(app: tauri::AppHandle, ids: Vec<String>) -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let engine = ctx.engine();

    let inspection = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    let plan = plan_overwrite(&inspection, &ids);
    if !plan.is_empty() {
        execute(&plan, &engine, &seconds_since_epoch()).map_err(to_cmd_err)?;
    }

    let after = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(settled(&app, with_undeclared(&ctx, view::overview(&after))))
}

/// Adopt one drift item. `set` chooses the target set for a package;
/// `ignore = true` takes the ignore branch instead.
///
/// Refuses any id whose drift is a `Drift::LocalEdit` — see [`adoptable`] —
/// and errors rather than reporting success when the requested action is not
/// one of the proposals for that item — see [`choose_proposal`].
#[tauri::command]
pub fn adopt_item(
    app: tauri::AppHandle,
    id: String,
    set: Option<String>,
    ignore: bool,
) -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let engine = ctx.engine();
    let inspection = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;

    let default_set = set.unwrap_or_else(|| {
        repo.machines
            .get(&ctx.local.machine)
            .and_then(|m| m.sets.first().cloned())
            .unwrap_or_else(|| "core".to_string())
    });

    let drift = inspection
        .report
        .items
        .iter()
        .find(|d| d.id() == id)
        .ok_or_else(|| {
            let asked = if ignore { "ignore" } else { "adopt" };
            format!("cannot {asked} `{id}`: it is no longer in the current overview")
        })?;
    adoptable(drift)?;

    let proposal = choose_proposal(
        adopt::proposals_for(drift, &repo, &default_set),
        ignore,
        &id,
    )?;
    adopt::apply_proposal(&proposal, &repo, &ctx.local.machine, &ctx.fs).map_err(to_cmd_err)?;

    let after = engine.inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(settled(&app, with_undeclared(&ctx, view::overview(&after))))
}

#[tauri::command]
pub fn list_sets() -> Result<Vec<Entry>, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;
    sets::list(&repo, &ctx.fs, &ctx.local.machine).map_err(to_cmd_err)
}

// --- settings ---
//
// Everything here has a `dotfix config` equivalent and calls the same core
// functions, so the two front ends cannot drift apart on what a setting
// means.

#[tauri::command]
pub fn read_settings() -> Result<settings::Settings, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    settings::read(&ctx.fs, &ctx.git, &ctx.local).map_err(to_cmd_err)
}

#[tauri::command]
pub fn set_remote(url: String) -> Result<settings::Settings, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    settings::set_remote(&ctx.git, &ctx.local, &url).map_err(to_cmd_err)?;
    settings::read(&ctx.fs, &ctx.git, &ctx.local).map_err(to_cmd_err)
}

/// Switch the secret provider, verifying first.
///
/// The verification renders every template against the candidate provider
/// and writes nothing. A missing secret surfaces here, named, rather than at
/// an apply days later — which is the whole reason this is not a plain write.
#[tauri::command]
pub fn set_provider(provider: String, vault: Option<String>) -> Result<settings::Settings, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let kind = parse_provider(&provider)?;
    let engine = ctx.engine();
    let verify = |p, v: Option<&str>| engine.verify_secrets(&ctx.local, p, v);
    settings::set_provider(&ctx.fs, &ctx.local, kind, vault, &verify).map_err(to_cmd_err)?;
    settings::read(&ctx.fs, &ctx.git, &ctx.local).map_err(to_cmd_err)
}

/// Stop ignoring a package. The inverse of the Ignore button, which had
/// none — the package returns to the unmanaged list at the next check.
/// Commit what changed and push it. The counterpart to "Check now", which
/// only ever pulled.
#[tauri::command]
pub fn publish() -> Result<dotfix_core::engine::Published, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let message = format!("chore: update from {}", ctx.local.machine);
    ctx.engine()
        .publish(&ctx.local, &message)
        .map_err(to_cmd_err)
}

#[tauri::command]
pub fn unignore(name: String) -> Result<settings::Settings, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    settings::unignore(&ctx.fs, &ctx.local, &name).map_err(to_cmd_err)?;
    settings::read(&ctx.fs, &ctx.git, &ctx.local).map_err(to_cmd_err)
}

#[tauri::command]
pub fn rename_machine(new_name: String) -> Result<settings::Settings, String> {
    validate_machine_name(&new_name)?;
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let moved =
        settings::rename_machine(&ctx.fs, &ctx.paths, &ctx.local, &new_name).map_err(to_cmd_err)?;
    settings::read(&ctx.fs, &ctx.git, &moved).map_err(to_cmd_err)
}

fn parse_provider(raw: &str) -> Result<dotfix_core::config::ProviderKind, String> {
    use dotfix_core::config::ProviderKind;
    match raw {
        "keychain" => Ok(ProviderKind::Keychain),
        "1password" => Ok(ProviderKind::OnePassword),
        "age" => Ok(ProviderKind::Age),
        other => Err(format!(
            "unknown secret provider `{other}` — expected keychain, 1password or age"
        )),
    }
}

/// What the About area shows: the version, and where to read more.
#[derive(serde::Serialize)]
pub struct About {
    pub version: String,
    pub links: Vec<Link>,
}

#[derive(serde::Serialize)]
pub struct Link {
    pub label: String,
    pub url: String,
}

/// Reads nothing on disk, so About stays answerable on a machine where
/// everything else fails — which is exactly when someone goes looking for
/// the documentation.
#[tauri::command]
pub fn about() -> About {
    About {
        version: dotfix_core::about::VERSION.to_string(),
        links: dotfix_core::about::LINKS
            .iter()
            .map(|(label, url)| Link {
                label: label.to_string(),
                url: url.to_string(),
            })
            .collect(),
    }
}

/// Open one of dotfix's own links in the browser.
///
/// Refuses anything else. The argument arrives from a webview, and a command
/// that opens whatever it is handed is a way to open anything — the same
/// reason `open_in_repo` below refuses paths outside the repository.
#[tauri::command]
pub fn open_link(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    if !dotfix_core::about::is_known(&url) {
        return Err(format!("`{url}` is not one of dotfix's own links"));
    }
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Open a file inside the repository in whatever the user edits it with.
///
/// Takes a repository-relative path and joins it onto the repository root,
/// then refuses anything that escapes. A command that opens whatever path it
/// is handed opens anything — and this one is called from a webview.
///
/// Editing happens in the user's editor, not in a text box in a menubar
/// panel: a broken `set.toml` stops dotfix entirely, and the thing that
/// should catch that is an editor they already trust. "Check now" re-reads
/// the repository afterwards.
#[tauri::command]
pub fn open_in_repo(app: tauri::AppHandle, relative: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let root = ctx
        .local
        .repo
        .canonicalize()
        .map_err(|e| format!("cannot resolve the repository: {e}"))?;
    let target = root.join(&relative);
    let target = target
        .canonicalize()
        .map_err(|_| format!("`{relative}` is not a file in the repository"))?;

    if !target.starts_with(&root) {
        return Err(format!("`{relative}` is outside the repository"));
    }

    app.opener()
        .open_path(target.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Add or remove a package in a set, and report the set list afresh.
///
/// Writes `set.toml` from the parsed structure, so comments and field order
/// in that file do not survive — the same trade `dotfix adopt` already makes.
/// Record software that is here but no set declares.
///
/// The mirror of adopting an unmanaged package, and the same shape: the
/// window asks which set, the core appends the entry without disturbing the
/// rest of the file.
#[tauri::command]
pub fn declare_requirement(
    app: tauri::AppHandle,
    name: String,
    set: String,
) -> Result<Overview, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;
    let active = repo
        .machines
        .get(&ctx.local.machine)
        .map(|m| m.sets.clone())
        .unwrap_or_default();

    let proposal = adopt::undeclared_requirements(&repo, &active, &ctx.fs, &ctx.paths.home, &set)
        .into_iter()
        .find(|p| match p {
            adopt::Proposal::DeclareRequirement { requirement, .. } => requirement.name == name,
            _ => false,
        })
        .ok_or_else(|| format!("`{name}` is not undeclared software on this machine"))?;

    adopt::apply_proposal(&proposal, &repo, &ctx.local.machine, &ctx.fs).map_err(to_cmd_err)?;

    let after = ctx.engine().inspect(&ctx.local).map_err(to_cmd_err)?;
    Ok(settled(&app, with_undeclared(&ctx, view::overview(&after))))
}

/// Fill in the undeclared list. Kept apart from `view::overview`, which sees
/// only an inspection and has no filesystem to look at.
fn with_undeclared(ctx: &Ctx, mut overview: Overview) -> Overview {
    let Ok(repo) = Repo::load(&ctx.fs, &ctx.local.repo) else {
        return overview;
    };
    let active = repo
        .machines
        .get(&ctx.local.machine)
        .map(|m| m.sets.clone())
        .unwrap_or_default();

    overview.undeclared =
        adopt::undeclared_requirements(&repo, &active, &ctx.fs, &ctx.paths.home, "")
            .into_iter()
            .filter_map(|p| match p {
                adopt::Proposal::DeclareRequirement { requirement, .. } => Some(view::Undeclared {
                    name: requirement.name,
                    hint: requirement.hint,
                }),
                _ => None,
            })
            .collect();
    overview
}

#[tauri::command]
pub fn edit_set_package(
    set: String,
    package: String,
    cask: bool,
    add: bool,
) -> Result<Vec<Entry>, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;
    sets::edit_package(&ctx.fs, &repo, &set, &package, cask, add).map_err(to_cmd_err)?;

    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;
    sets::list(&repo, &ctx.fs, &ctx.local.machine).map_err(to_cmd_err)
}

#[tauri::command]
pub fn toggle_set(app: tauri::AppHandle, name: String, on: bool) -> Result<Vec<Entry>, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;

    let cfg = sets::toggle(&repo, &ctx.local.machine, &name, on).map_err(to_cmd_err)?;
    sets::save(&ctx.fs, &repo, &ctx.local.machine, &cfg).map_err(to_cmd_err)?;

    let repo = Repo::load(&ctx.fs, &ctx.local.repo).map_err(to_cmd_err)?;
    let entries = sets::list(&repo, &ctx.fs, &ctx.local.machine).map_err(to_cmd_err)?;

    // Turning a set on or off changes which packages and files dotfix wants,
    // so the drift picture just changed even though this command returns only
    // the set list. Re-inspect and push that to the menubar; the window pulls
    // its own fresh overview right after this call. A failure here is not
    // worth failing the toggle over — the toggle itself is already saved.
    if let Ok(inspection) = ctx.engine().inspect(&ctx.local) {
        settled(&app, with_undeclared(&ctx, view::overview(&inspection)));
    }

    Ok(entries)
}

#[tauri::command]
pub fn file_diff(target: String) -> Result<FileDiff, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    let inspection = ctx.engine().inspect(&ctx.local).map_err(to_cmd_err)?;
    diffview::unified(&inspection, &PathBuf::from(target), &ctx.fs).map_err(to_cmd_err)
}

#[tauri::command]
pub fn history(limit: usize) -> Result<Vec<Commit>, String> {
    let ctx = Ctx::load().map_err(to_cmd_err)?;
    ctx.git
        .log(&ctx.local.repo, limit.min(200))
        .map_err(to_cmd_err)
}

// --- first-time setup wizard ---
//
// `Ctx::load` cannot be used by anything below: it requires the local
// configuration these commands exist to create, so every command here builds
// the real ports directly and reads `$HOME` itself.

/// The wizard's answers, as they cross from the webview. Kept as strings so
/// the frontend does not have to mirror Rust enums; converted once, here,
/// where an unknown value is an error rather than a silent default.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct WizardPlan {
    pub machine: String,
    pub mode: String,
    pub url: Option<String>,
    pub secret_provider: String,
    pub vault: Option<String>,
}

impl WizardPlan {
    pub fn to_plan(&self) -> std::result::Result<Plan, String> {
        let source = match self.mode.as_str() {
            "new" => Source::New,
            "clone" => Source::Clone {
                url: self
                    .url
                    .clone()
                    .filter(|u| !u.trim().is_empty())
                    .ok_or("cloning needs a repository url")?,
            },
            other => return Err(format!("unknown setup mode `{other}`")),
        };

        let secret_provider = match self.secret_provider.as_str() {
            "keychain" => dotfix_core::config::ProviderKind::Keychain,
            "1password" => dotfix_core::config::ProviderKind::OnePassword,
            "age" => dotfix_core::config::ProviderKind::Age,
            other => return Err(format!("unknown secret provider `{other}`")),
        };

        Ok(Plan {
            machine: validate_machine_name(&self.machine)?,
            source,
            secret_provider,
            vault: self.vault.clone().filter(|v| !v.trim().is_empty()),
        })
    }
}

/// Which step `init_run` stopped at, and why — enough for the wizard to
/// mark every earlier step done and offer a retry on exactly this one,
/// without guessing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FailedStep {
    pub step: String,
    pub message: String,
}

/// What `init_run` reached, so the wizard can mark steps done and retry only
/// the one that failed. `failed` is `None` on success and `Some` on
/// failure — `init_run` itself always returns `Ok(StepOutcome)` once
/// stepping has actually started, precisely so this struct, not a thrown
/// error, is what carries a mid-run failure across the Tauri boundary.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct StepOutcome {
    pub completed: Vec<String>,
    pub repo: Option<String>,
    pub failed: Option<FailedStep>,
    /// Whether an existing `~/.zshrc` was taken into the repository. Worth
    /// saying out loud: the wizard otherwise finishes without a word about
    /// the file it just took responsibility for, and silence there is how a
    /// real configuration came to be replaced by a stub once already.
    pub imported_zshrc: bool,
}

/// One step `init_run` can perform, paired with the name it is reported
/// under. Boxed so the caller can build a list mixing closures that borrow
/// different pieces of local state (the plan, the paths, ...).
type Step<'a> = (
    &'a str,
    Box<dyn FnMut() -> std::result::Result<(), String> + 'a>,
);

/// The skip-already-done / stop-at-first-failure decision `init_run` applies
/// across its steps, kept separate from what the steps actually do so it can
/// be unit tested without a real `$HOME` or any of the real ports — a step
/// closure that fails here only ever returns a plain string, never panics or
/// touches the filesystem.
///
/// A step already named in `already` is skipped without being called at
/// all — this is what makes a retry resume rather than restart: a machine
/// that got through `create_or_clone` and `configure_machine` before
/// `install_agent` failed passes those two names back in, and this loop
/// never touches `create_or_clone` again. The first step that is not in
/// `already` and fails stops the whole sequence immediately; nothing after
/// it runs, and everything before it — including steps that were already
/// done coming in — stays recorded as completed.
fn sequence_steps(
    already: &[String],
    mut steps: Vec<Step<'_>>,
) -> (Vec<String>, Option<FailedStep>) {
    let mut completed: Vec<String> = already.to_vec();
    for (name, run) in steps.iter_mut() {
        if completed.iter().any(|c| c == name) {
            continue;
        }
        match run() {
            Ok(()) => completed.push((*name).to_string()),
            Err(message) => {
                return (
                    completed,
                    Some(FailedStep {
                        step: (*name).to_string(),
                        message,
                    }),
                );
            }
        }
    }
    (completed, None)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeployKeyView {
    pub public: String,
    pub host_alias: String,
}

/// What a LaunchAgent must run: the `dotfix` CLI, never the app.
const CLI_NAME: &str = "dotfix";

/// Homebrew's two prefixes, Apple silicon first. A GUI started from Finder
/// inherits a minimal `PATH` that has never seen either, so they are tried
/// explicitly rather than hoped for.
pub(crate) const HOMEBREW_BINS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin"];

/// `current` with Homebrew's prefixes appended, skipping any already there.
///
/// Finding the CLI was not the only thing that needed this. Everything the
/// engine spawns — `brew`, `gh`, `op`, `age` — lives in one of those prefixes
/// too, and a window opened from Finder could not start any of them: the
/// failure surfaced as `command \`brew leaves\` failed: No such file or
/// directory`, which reads like a broken Homebrew rather than a `PATH` the
/// app was never given. `git` and the `ssh` tools are in `/usr/bin` and were
/// always fine, which is why setup worked and the first refresh did not.
///
/// Appended rather than prepended: a directory the user really does have on
/// their `PATH` keeps its precedence, and this only adds what is missing.
///
/// A Homebrew installed somewhere else entirely is still invisible. Asking
/// the login shell for its `PATH` would cover that, at the cost of spawning a
/// shell on every start and hanging when that shell does.
pub(crate) fn path_with_homebrew(current: &str) -> std::ffi::OsString {
    let mut dirs: Vec<PathBuf> = std::env::split_paths(current).collect();
    for extra in HOMEBREW_BINS {
        let extra = PathBuf::from(extra);
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    std::env::join_paths(dirs).unwrap_or_else(|_| current.into())
}

/// What the wizard says when there is no CLI to point the agent at.
const CLI_NOT_FOUND: &str = "the `dotfix` command-line tool is not installed, \
     and the hourly background check is the CLI running itself — so no agent \
     was installed rather than one that could never work. Everything else on \
     this Mac is set up. Install the CLI (`brew install dotfix` once the tap \
     is published, or put `dotfix` on your PATH) and run \
     `dotfix doctor --install-agent`.";

/// Where the `dotfix` CLI lives, if it is installed at all.
///
/// Deliberately *not* `std::env::current_exe()`: inside the app that is the
/// Tauri binary, which ignores argv entirely. An agent pointing at it would
/// relaunch the menubar app every hour and never write a status line —
/// silently disabling the background check the same run just scaffolded,
/// while `doctor` still reported "launch agent ok" because it only checks
/// that the file exists.
///
/// `exists` is a parameter so the decision can be tested without a real
/// filesystem; the caller passes `Path::is_file`.
fn find_cli(path_env: &str, exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    std::env::split_paths(path_env)
        .chain(HOMEBREW_BINS.iter().map(PathBuf::from))
        .map(|dir| dir.join(CLI_NAME))
        .find(|candidate| exists(candidate))
}

fn home() -> Result<PathBuf, String> {
    std::env::var("HOME")
        .map(PathBuf::from)
        .map_err(|_| "HOME is not set".to_string())
}

/// Where a clone will really come from, and which host key it needs first.
///
/// Both are decided by the authentication the user ended up with, not by
/// what they typed, so the wizard asks for this once and shows the URL
/// before setup runs — dotfix must never quietly clone from somewhere other
/// than what was entered.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CloneTarget {
    pub url: String,
    /// The host to pin in `known_hosts` first, or `None` when no ssh host
    /// key is involved (https) or the host is not one dotfix pins.
    pub host_to_pin: Option<String>,
}

fn parse_auth(auth: &str) -> Result<init::Auth, String> {
    match auth {
        "ssh" => Ok(init::Auth::Ssh),
        "deploy_key" => Ok(init::Auth::DeployKey),
        "token" => Ok(init::Auth::Token),
        other => Err(format!("unknown authentication method `{other}`")),
    }
}

/// Which host to pin before cloning `url`, if any.
///
/// Never the ssh alias, always the real host: `ssh-keyscan` resolves DNS and
/// knows nothing about `~/.ssh/config`, so asking it for `github.com-dotfix`
/// can only fail. An https URL needs no host key at all.
fn host_to_pin(url: &str) -> Option<String> {
    let lower = url.trim().to_ascii_lowercase();
    if lower.starts_with("https://") || lower.starts_with("http://") {
        return None;
    }
    github_host(url).map(|host| {
        if host == ssh::HOST_ALIAS {
            "github.com".to_string()
        } else {
            host.to_string()
        }
    })
}

/// The effective clone URL for `typed` under `auth`, plus the host key it
/// needs. The pure decision behind both [`init_clone_target`] and
/// [`init_run`], so the URL the wizard shows is by construction the URL the
/// clone uses.
fn clone_target(typed: &str, auth: &str) -> Result<CloneTarget, String> {
    let url = init::effective_clone_url(typed, parse_auth(auth)?).map_err(to_cmd_err)?;
    Ok(CloneTarget {
        host_to_pin: host_to_pin(&url),
        url,
    })
}

/// Reject a machine name the rest of dotfix could not carry.
///
/// The name becomes a file name twice over — `machines/<name>.toml` in the
/// repository and `~/.ssh/dotfix_<name>_ed25519` for a deploy key — and a
/// table key in TOML. A slash, a quote, whitespace or a leading dot breaks
/// one of those long after the wizard has finished and with no terminal
/// open to work out why, so it is refused here, while there is still a
/// field to point at. A dot inside the name is fine: `mbp.local` is an
/// ordinary macOS host name.
///
/// An *empty* name is deliberately not an error: pre-flight already reports
/// it as a failing check beside the field, which is the calmer place for
/// "you have not filled this in yet" than a thrown banner.
fn validate_machine_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.starts_with('.') {
        return Err("machine name must not start with a dot".to_string());
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(
            "machine name may only contain letters, digits, dots, dashes and underscores"
                .to_string(),
        );
    }
    Ok(trimmed.to_string())
}

/// Reject a username before it reaches [`remote::store_token`].
///
/// That function builds a `git-credential` payload as unescaped,
/// newline-separated `key=value` fields. A username containing a newline (or
/// any other whitespace or control character) could inject an extra field
/// into that payload, so the value is trimmed and, if anything suspicious
/// remains — including nothing at all, for an all-whitespace input — refused
/// here, before it ever reaches the credential helper.
fn validate_username(user: &str) -> Result<String, String> {
    let trimmed = user.trim();
    if trimmed.is_empty() {
        return Err("username must not be empty".to_string());
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("username must not contain whitespace or control characters".to_string());
    }
    Ok(trimmed.to_string())
}

/// Pre-flight checks for the plan the wizard has assembled so far. Writes
/// nothing.
#[tauri::command]
pub fn init_preflight(plan: WizardPlan) -> Result<init::Preflight, String> {
    let plan = plan.to_plan()?;
    let paths = dotfix_core::paths::Paths::new(home()?);
    Ok(init::preflight(&RealFsys, &RealExec, &paths, &plan))
}

/// Whether this machine's ssh setup already reaches `host`.
///
/// The host matters: a generated deploy key is scoped to
/// [`ssh::HOST_ALIAS`] with `IdentitiesOnly yes`, so probing `github.com`
/// after adding it would never offer that key and "Test connection" could
/// never succeed. The wizard probes `github.com` first and the alias once a
/// key exists.
#[tauri::command]
pub fn init_probe_ssh(host: String) -> Result<String, String> {
    Ok(match ssh::probe(&RealExec, &host) {
        Reachability::Ready => "ready".to_string(),
        Reachability::NeedsKey => "needs_key".to_string(),
        Reachability::Unreachable { detail } => format!("unreachable: {detail}"),
    })
}

/// Generate (or reuse) a deploy key scoped to `machine`, and make sure the
/// ssh alias that scopes it to this repository exists.
#[tauri::command]
pub fn init_deploy_key(machine: String) -> Result<DeployKeyView, String> {
    let home = home()?;
    let key = ssh::ensure_deploy_key(&RealFsys, &RealExec, &home, &machine).map_err(to_cmd_err)?;
    ssh::ensure_ssh_config(&RealFsys, &home, &key).map_err(to_cmd_err)?;
    Ok(DeployKeyView {
        public: key.public,
        host_alias: key.host_alias,
    })
}

/// Store a GitHub token in the macOS Keychain via git's credential helper.
#[tauri::command]
pub fn init_store_token(user: String, token: String) -> Result<(), String> {
    let user = validate_username(&user)?;
    remote::store_token(&RealExec, "github.com", &user, &token).map_err(to_cmd_err)
}

/// The URL a clone will really use, given how it will authenticate, and the
/// host key it needs first. The wizard shows this before running setup.
#[tauri::command]
pub fn init_clone_target(url: String, auth: String) -> Result<CloneTarget, String> {
    clone_target(&url, &auth)
}

/// Create a new private repository through the user's own authenticated
/// `gh`.
#[tauri::command]
pub fn init_create_repo(owner: String, name: String) -> Result<String, String> {
    remote::create_repo(&RealExec, &RepoRequest { owner, name }).map_err(to_cmd_err)
}

/// Run first-time setup: create or clone the data repository, configure this
/// machine, and install the background agent.
///
/// `completed` is whatever the previous call already reported back —
/// `[]` for a first attempt — and any step named in it is skipped rather
/// than redone, so a retry after a mid-run failure resumes instead of
/// starting over. That matters beyond convenience: repeating
/// `create_or_clone` against a repository that already exists asks git to
/// commit nothing, which fails, and used to be treated the same as any
/// other scaffold failure — deleting the very directory `configure_machine`
/// had already written into. See `dotfix_core::init::create_or_clone`'s own
/// resume guard for the other half of that fix; this function's job is
/// simply to never call it again once the wizard says it already ran.
///
/// Unlike the version of this function that only ever threw its error away
/// with `?`, a failure partway through is not this function's own `Err` —
/// it is carried back in `StepOutcome::failed`, alongside every step name
/// that got there first. Only `to_plan` and reading `$HOME` — neither of
/// which is one of the four numbered steps, and both of which mean nothing
/// below can even begin — still fail this call outright.
#[tauri::command]
pub fn init_run(
    app: tauri::AppHandle,
    plan: WizardPlan,
    completed: Vec<String>,
    auth: String,
) -> Result<StepOutcome, String> {
    let mut plan = plan.to_plan()?;
    let home = home()?;
    let paths = dotfix_core::paths::Paths::new(home.clone());
    let root = paths.home.join("dotfiles");

    // What the user typed is not always what can be cloned: a deploy key is
    // only ever offered for its own ssh alias, and a token only over https.
    // Resolve the effective URL from the authentication actually in use and
    // clone *that* — the wizard has already shown it. A URL too malformed to
    // resolve fails the whole call before any step runs, which is the one
    // case the wizard reports as a banner rather than a step.
    let pin_host = match &plan.source {
        Source::Clone { url } => {
            let target = clone_target(url, &auth)?;
            plan.source = Source::Clone { url: target.url };
            target.host_to_pin
        }
        Source::New => None,
    };

    let mut steps: Vec<Step<'_>> = Vec::new();

    // A fresh Mac has never talked to GitHub over ssh, so `create_or_clone`'s
    // `git.clone_to` would otherwise stop at the host-key prompt with no
    // terminal to answer it. Pin the host key first, from GitHub's published
    // fingerprints — this step MUST stay before `create_or_clone` below for
    // any clone whose host is GitHub, or a real network clone would run with
    // no host-key protection at all.
    if let Some(host) = pin_host {
        let home = home.clone();
        steps.push((
            "ensure_host_known",
            Box::new(move || {
                ssh::ensure_host_known(&RealFsys, &RealExec, &home, &host, ssh::GITHUB_FINGERPRINTS)
                    .map(|_| ())
                    .map_err(to_cmd_err)
            }),
        ));
    }

    steps.push((
        "create_or_clone",
        Box::new(|| {
            init::create_or_clone(&RealFsys, &RealGit, &RealBrew, &paths, &plan)
                .map(|_| ())
                .map_err(to_cmd_err)
        }),
    ));

    steps.push((
        "configure_machine",
        Box::new(|| init::configure_machine(&RealFsys, &root, &paths, &plan).map_err(to_cmd_err)),
    ));

    steps.push((
        "install_agent",
        Box::new(|| {
            let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
            // No CLI means no agent at all. Writing a plist that points at
            // this binary would install a background check that can never
            // run; an honest gap the user is told about is better than a
            // green tick over a broken one.
            let binary =
                find_cli(&path_env, &|p| p.is_file()).ok_or_else(|| CLI_NOT_FOUND.to_string())?;
            init::install_agent(
                &RealFsys,
                &paths,
                &binary,
                init::DEFAULT_INTERVAL,
                &path_env,
            )
            .map(|_| ())
            .map_err(to_cmd_err)
        }),
    ));

    let (done, failed) = sequence_steps(&completed, steps);

    // The machine finally has a configuration, so the menubar must stop
    // showing the unconfigured state. Tolerating failure here: the wizard
    // still needs to report the outcome for the steps it actually ran, and
    // the window is about to navigate away from the wizard regardless. Only
    // worth doing once every step has actually succeeded.
    if failed.is_none()
        && let Ok(o) = overview()
    {
        let _ = crate::tray::update(&app, &o);
    }

    Ok(StepOutcome {
        completed: done,
        repo: Some(root.display().to_string()),
        failed,
        imported_zshrc: root.join(init::IMPORTED_FRAGMENT).is_file(),
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use dotfix_core::adopt::Proposal;
    use dotfix_core::drift::{Drift, PackageRef};
    use dotfix_core::init::url_host;

    #[test]
    fn a_finder_launched_path_gains_the_homebrew_prefixes() {
        // The PATH a GUI inherits from launchd. Without the prefixes, nothing
        // the engine spawns from Homebrew can start at all.
        let out = path_with_homebrew("/usr/bin:/bin:/usr/sbin:/sbin");
        let dirs: Vec<_> = std::env::split_paths(&out).collect();
        assert!(
            dirs.contains(&PathBuf::from("/opt/homebrew/bin")),
            "{dirs:?}"
        );
        assert!(dirs.contains(&PathBuf::from("/usr/local/bin")), "{dirs:?}");
    }

    #[test]
    fn a_prefix_already_present_is_not_added_twice() {
        let out = path_with_homebrew("/opt/homebrew/bin:/usr/bin");
        let dirs: Vec<_> = std::env::split_paths(&out).collect();
        assert_eq!(
            dirs.iter()
                .filter(|d| *d == &PathBuf::from("/opt/homebrew/bin"))
                .count(),
            1,
            "{dirs:?}"
        );
    }

    #[test]
    fn the_users_own_directories_keep_their_precedence() {
        // Appended, never prepended: a `brew` the user deliberately shadows
        // must stay shadowed.
        let out = path_with_homebrew("/Users/test/bin:/usr/bin");
        let dirs: Vec<_> = std::env::split_paths(&out).collect();
        assert_eq!(dirs[0], PathBuf::from("/Users/test/bin"));
        assert_eq!(dirs[1], PathBuf::from("/usr/bin"));
    }

    #[test]
    fn the_missing_cli_message_reads_as_a_sentence() {
        // Same defect as the core url message: a multi-line literal without
        // `\` continuations rendered its own indentation into the text a
        // user sees when setup could not install the background agent.
        assert!(
            !CLI_NOT_FOUND.contains("  "),
            "runaway whitespace in: {CLI_NOT_FOUND}"
        );
    }

    use super::*;

    #[test]
    fn errors_reach_the_frontend_as_readable_strings() {
        let err = dotfix_core::Error::UnknownMachine("box-nine".into());
        assert_eq!(
            to_cmd_err(err),
            "machine `box-nine` not found in repository"
        );
    }

    #[test]
    fn an_error_string_never_carries_a_secret_value() {
        // Error::Secret is built from a name and a reason; the value is never
        // one of its fields. This test locks that in.
        let err = dotfix_core::Error::Secret {
            name: "api_key".into(),
            reason: "item not found".into(),
        };
        let text = to_cmd_err(err);
        assert!(text.contains("api_key"));
        assert!(text.contains("item not found"));
    }

    // `adopt_item` must never write a hand-edited local file back into the
    // repository: that is the one action that silently changes what every
    // other machine receives next sync, so it stays a CLI-only operation
    // (`dotfix adopt`). `adopt_item` needs a real `Ctx` — and therefore a
    // real HOME and repository — so instead of driving the command end to
    // end, this exercises the pure decision it delegates to.

    // `choose_proposal` is the guard against the command's old behaviour:
    // find no matching proposal, write nothing, and still return `Ok` with a
    // fresh overview — so the window showed the row still there, no error,
    // and no explanation.

    #[test]
    fn the_requested_proposal_is_chosen_when_it_is_on_offer() {
        let proposals = vec![
            Proposal::AddPackage {
                package: PackageRef::formula("bravo"),
                set: "core".into(),
            },
            Proposal::IgnorePackage {
                package: PackageRef::formula("bravo"),
            },
        ];
        assert!(matches!(
            choose_proposal(proposals.clone(), false, "unmanaged:formula:bravo"),
            Ok(Proposal::AddPackage { .. })
        ));
        assert!(matches!(
            choose_proposal(proposals, true, "unmanaged:formula:bravo"),
            Ok(Proposal::IgnorePackage { .. })
        ));
    }

    #[test]
    fn ignoring_a_locally_removed_package_errors_instead_of_silently_doing_nothing() {
        // The only proposal for `Drift::LocallyRemoved` is `DropPackage`.
        let proposals = vec![Proposal::DropPackage {
            package: PackageRef::formula("alpha"),
            set: "core".into(),
        }];
        let err = choose_proposal(proposals, true, "locally_removed:formula:alpha").unwrap_err();
        assert!(err.contains("ignore"), "{err}");
        assert!(err.contains("locally_removed:formula:alpha"), "{err}");
        assert!(err.contains("dropping it from its set"), "{err}");
    }

    #[test]
    fn an_item_with_no_proposals_at_all_errors_rather_than_reporting_success() {
        let err = choose_proposal(vec![], false, "removed_file:/Users/test/.old").unwrap_err();
        assert!(err.contains("adopt"), "{err}");
        assert!(err.contains("removed_file:/Users/test/.old"), "{err}");
        assert!(err.contains("no action"), "{err}");
    }

    #[test]
    fn a_local_edit_is_refused_pointing_at_the_cli() {
        let drift = Drift::LocalEdit {
            target: PathBuf::from("/Users/test/.gitconfig"),
            set: "core".into(),
            contains_secrets: false,
        };
        assert_eq!(
            adoptable(&drift),
            Err(
                "editing a managed file back into the repository is a CLI operation: run \
                 `dotfix adopt`"
                    .to_string()
            )
        );
    }

    #[test]
    fn every_other_drift_variant_is_adoptable() {
        let variants = [
            Drift::IncomingPackage(PackageRef::formula("alpha")),
            Drift::IncomingFile {
                target: PathBuf::from("/Users/test/.rc"),
                set: "core".into(),
            },
            Drift::LocallyRemoved(PackageRef::formula("beta")),
            Drift::Unmanaged {
                package: PackageRef::formula("gamma"),
                declared_in: Vec::new(),
            },
            Drift::RemovedPackage {
                package: PackageRef::formula("delta"),
                blocked_by: vec![],
                declared_in: Vec::new(),
            },
            Drift::RemovedFile {
                target: PathBuf::from("/Users/test/.old"),
            },
        ];
        for drift in &variants {
            assert_eq!(adoptable(drift), Ok(()), "{drift:?} should be adoptable");
        }
    }

    // --- wizard commands (task 8) ---

    #[test]
    fn a_wizard_plan_converts_to_a_core_plan() {
        let wp = WizardPlan {
            machine: "box-one".into(),
            mode: "clone".into(),
            url: Some("git@github.com:example/dotfiles.git".into()),
            secret_provider: "1password".into(),
            vault: Some("Example".into()),
        };
        let plan = wp.to_plan().unwrap();

        assert_eq!(plan.machine, "box-one");
        assert_eq!(
            plan.source,
            dotfix_core::init::Source::Clone {
                url: "git@github.com:example/dotfiles.git".into()
            }
        );
        assert_eq!(
            plan.secret_provider,
            dotfix_core::config::ProviderKind::OnePassword
        );
        assert_eq!(plan.vault.as_deref(), Some("Example"));
    }

    // --- locating the CLI the LaunchAgent runs (final review, C2) ---

    #[test]
    fn no_cli_anywhere_is_reported_rather_than_falling_back_to_this_binary() {
        // The one branch that matters: pointing the agent at the app would
        // relaunch the menubar every hour and never write a status line,
        // while `doctor` cheerfully reported "launch agent ok".
        assert_eq!(find_cli("", &|_| false), None);
        assert_eq!(find_cli("/nowhere:/also-nowhere", &|_| false), None);
    }

    #[test]
    fn the_cli_is_found_on_path_first() {
        let found = find_cli("/opt/tools/bin:/usr/bin", &|p| {
            p == Path::new("/usr/bin/dotfix")
        });
        assert_eq!(found, Some(PathBuf::from("/usr/bin/dotfix")));
    }

    #[test]
    fn the_usual_homebrew_locations_are_tried_when_path_has_nothing() {
        // A GUI launched from Finder inherits a minimal PATH that has never
        // seen Homebrew, which is exactly the fresh-Mac case.
        for prefix in ["/opt/homebrew/bin", "/usr/local/bin"] {
            let expected = PathBuf::from(prefix).join("dotfix");
            assert_eq!(
                find_cli("/usr/bin:/bin", &|p| p == expected),
                Some(expected.clone())
            );
        }
    }

    #[test]
    fn the_missing_cli_message_says_what_to_install_and_what_still_works() {
        assert!(CLI_NOT_FOUND.contains("dotfix"));
        assert!(
            CLI_NOT_FOUND.contains("brew install"),
            "the user has no terminal open to work this out: {CLI_NOT_FOUND}"
        );
        assert!(CLI_NOT_FOUND.contains("PATH"));
    }

    // --- the effective clone url (final review, C3/I2) ---
    //
    // `clone_target` is the whole decision: which URL a clone actually uses
    // and which host key it needs. Both `init_clone_target` (what the wizard
    // shows the user) and `init_run` (what it clones) go through it, so the
    // two cannot drift apart.

    #[test]
    fn working_ssh_clones_the_url_as_typed_and_pins_github() {
        let target = clone_target("git@github.com:example-user/dotfiles.git", "ssh").unwrap();
        assert_eq!(target.url, "git@github.com:example-user/dotfiles.git");
        assert_eq!(target.host_to_pin.as_deref(), Some("github.com"));
    }

    #[test]
    fn a_deploy_key_clones_through_its_alias_but_pins_the_real_host() {
        // `ssh-keyscan` resolves DNS, not `~/.ssh/config`, so asking it for
        // the alias could only ever fail.
        let target =
            clone_target("git@github.com:example-user/dotfiles.git", "deploy_key").unwrap();
        assert_eq!(
            target.url,
            "git@github.com-dotfix:example-user/dotfiles.git"
        );
        assert_eq!(
            target.host_to_pin.as_deref(),
            Some("github.com"),
            "never the alias"
        );
    }

    #[test]
    fn a_token_clones_over_https_and_needs_no_host_key() {
        let target = clone_target("git@github.com:example-user/dotfiles.git", "token").unwrap();
        assert_eq!(target.url, "https://github.com/example-user/dotfiles.git");
        assert_eq!(target.host_to_pin, None, "https carries no ssh host key");
    }

    #[test]
    fn a_non_github_host_is_cloned_as_typed_and_pinned_by_nobody() {
        let target = clone_target("git@gitlab.com:example-user/dotfiles.git", "ssh").unwrap();
        assert_eq!(target.url, "git@gitlab.com:example-user/dotfiles.git");
        assert_eq!(
            target.host_to_pin, None,
            "dotfix only ships fingerprints for github.com"
        );
    }

    #[test]
    fn a_typed_https_url_needs_no_host_key_either() {
        let target = clone_target("https://github.com/example-user/dotfiles.git", "ssh").unwrap();
        assert_eq!(target.host_to_pin, None);
    }

    #[test]
    fn an_unusable_url_is_refused_rather_than_cloned_from_somewhere_else() {
        let err = clone_target("dotfiles", "deploy_key").unwrap_err();
        assert!(
            err.contains("cannot tell the owner and repository"),
            "{err}"
        );
    }

    #[test]
    fn an_unknown_authentication_method_is_rejected_rather_than_defaulted() {
        let err = clone_target("git@github.com:example-user/dotfiles.git", "magic").unwrap_err();
        assert!(err.contains("magic"), "{err}");
    }

    // --- machine-name validation (final review, I4) ---

    #[test]
    fn a_machine_name_that_would_break_a_path_is_rejected_with_a_reason() {
        for bad in ["../evil", "box one", "box\"one", "box/one", ".hidden"] {
            let err = validate_machine_name(bad).unwrap_err().to_ascii_lowercase();
            assert!(
                err.contains("machine name"),
                "`{bad}` must be refused by name: {err}"
            );
        }
    }

    #[test]
    fn a_dotted_machine_name_is_accepted() {
        // `mbp.local` is an ordinary macOS host name and becomes
        // `~/.ssh/dotfix_mbp.local_ed25519`, which is a perfectly good file
        // name — see the matching core test.
        assert_eq!(validate_machine_name(" mbp.local ").unwrap(), "mbp.local");
    }

    #[test]
    fn an_empty_machine_name_is_left_for_preflight_to_report() {
        // Reported as a failing check line next to the field, not as a
        // thrown banner — the wizard asks for requirements before the name
        // is necessarily filled in.
        assert_eq!(validate_machine_name("  ").unwrap(), "");
    }

    #[test]
    fn a_plan_with_an_unusable_machine_name_never_becomes_a_plan() {
        let err = WizardPlan {
            machine: "box/one".into(),
            mode: "new".into(),
            url: None,
            secret_provider: "keychain".into(),
            vault: None,
        }
        .to_plan()
        .unwrap_err();
        assert!(err.to_ascii_lowercase().contains("machine name"), "{err}");
    }

    #[test]
    fn a_clone_without_a_url_is_rejected_before_anything_runs() {
        let wp = WizardPlan {
            machine: "box-one".into(),
            mode: "clone".into(),
            url: None,
            secret_provider: "keychain".into(),
            vault: None,
        };
        assert!(wp.to_plan().is_err());
    }

    #[test]
    fn an_unknown_mode_is_rejected_rather_than_defaulted() {
        let wp = WizardPlan {
            machine: "box-one".into(),
            mode: "teleport".into(),
            url: None,
            secret_provider: "keychain".into(),
            vault: None,
        };
        let err = wp.to_plan().unwrap_err();
        assert!(err.contains("teleport"), "name what was wrong: {err}");
    }

    #[test]
    fn an_unknown_secret_provider_is_rejected_rather_than_defaulted() {
        let wp = WizardPlan {
            machine: "box-one".into(),
            mode: "new".into(),
            url: None,
            secret_provider: "magic".into(),
            vault: None,
        };
        // Silently falling back to Keychain would write a machine file that
        // disagrees with what the user picked.
        assert!(wp.to_plan().is_err());
    }

    // --- init_run's resumable step sequencing (task 9, fix round 1) ---
    //
    // `sequence_steps` is the pure decision `init_run` makes across its four
    // real steps — skip what is already done, stop at the first failure,
    // never guess which step a bare error string came from — extracted so
    // it can be tested with plain closures instead of a real `$HOME` and
    // real ports. `init_run` itself only wires this to `RealFsys`/`RealGit`/
    // etc. and cannot be unit tested directly (it takes a `tauri::AppHandle`
    // no test can construct).

    #[test]
    fn every_step_runs_in_order_when_none_are_done_yet() {
        let calls = std::cell::RefCell::new(Vec::new());
        let first = || {
            calls.borrow_mut().push("a");
            Ok(())
        };
        let second = || {
            calls.borrow_mut().push("b");
            Ok(())
        };
        let (completed, failed) =
            sequence_steps(&[], vec![("a", Box::new(first)), ("b", Box::new(second))]);

        assert_eq!(completed, vec!["a".to_string(), "b".to_string()]);
        assert_eq!(failed, None);
        assert_eq!(*calls.borrow(), vec!["a", "b"]);
    }

    #[test]
    fn a_failed_step_stops_the_sequence_and_names_itself_precisely() {
        // The scenario the fix exists for: `install_agent` is the one that
        // actually failed, and the wizard must be told exactly that — not
        // "something failed", and not blamed on `create_or_clone`, which
        // already succeeded.
        let calls = std::cell::RefCell::new(Vec::new());
        let create_or_clone = || {
            calls.borrow_mut().push("create_or_clone");
            Ok(())
        };
        let configure_machine = || {
            calls.borrow_mut().push("configure_machine");
            Ok(())
        };
        let install_agent = || {
            calls.borrow_mut().push("install_agent");
            Err("permission denied writing the LaunchAgent".to_string())
        };
        let (completed, failed) = sequence_steps(
            &[],
            vec![
                ("create_or_clone", Box::new(create_or_clone)),
                ("configure_machine", Box::new(configure_machine)),
                ("install_agent", Box::new(install_agent)),
            ],
        );

        assert_eq!(
            completed,
            vec![
                "create_or_clone".to_string(),
                "configure_machine".to_string()
            ]
        );
        assert_eq!(
            failed,
            Some(FailedStep {
                step: "install_agent".to_string(),
                message: "permission denied writing the LaunchAgent".to_string(),
            })
        );
        // Nothing after the failed step ever ran.
        assert_eq!(
            *calls.borrow(),
            vec!["create_or_clone", "configure_machine", "install_agent"]
        );
    }

    #[test]
    fn steps_already_completed_are_skipped_rather_than_redone() {
        // This is the resume behaviour a retry depends on: passing back
        // what already succeeded must mean those steps are never called
        // again at all — not called and their result discarded, actually
        // never invoked — which is what makes a retry safe to run against a
        // `create_or_clone` step whose real implementation is not
        // idempotent-by-re-running (re-running it after success asks git to
        // commit nothing and fails).
        let calls = std::cell::RefCell::new(Vec::new());
        let create_or_clone = || {
            calls.borrow_mut().push("create_or_clone");
            Ok(())
        };
        let configure_machine = || {
            calls.borrow_mut().push("configure_machine");
            Ok(())
        };
        let install_agent = || {
            calls.borrow_mut().push("install_agent");
            Ok(())
        };
        let (completed, failed) = sequence_steps(
            &[
                "create_or_clone".to_string(),
                "configure_machine".to_string(),
            ],
            vec![
                ("create_or_clone", Box::new(create_or_clone)),
                ("configure_machine", Box::new(configure_machine)),
                ("install_agent", Box::new(install_agent)),
            ],
        );

        assert_eq!(
            *calls.borrow(),
            vec!["install_agent"],
            "only the unfinished step may run"
        );
        assert_eq!(
            completed,
            vec![
                "create_or_clone".to_string(),
                "configure_machine".to_string(),
                "install_agent".to_string(),
            ]
        );
        assert_eq!(failed, None);
    }

    #[test]
    fn a_retry_that_fails_again_still_credits_the_steps_already_done() {
        // A second failure (e.g. `install_agent` fails twice in a row) must
        // not un-mark the steps a first attempt already finished.
        let install_agent = || Err("still failing".to_string());
        let (completed, failed) = sequence_steps(
            &[
                "create_or_clone".to_string(),
                "configure_machine".to_string(),
            ],
            vec![("install_agent", Box::new(install_agent))],
        );

        assert_eq!(
            completed,
            vec![
                "create_or_clone".to_string(),
                "configure_machine".to_string()
            ]
        );
        assert_eq!(
            failed,
            Some(FailedStep {
                step: "install_agent".to_string(),
                message: "still failing".to_string(),
            })
        );
    }

    // --- (a) host-key pinning wired into the clone path ---
    //
    // `github_host`/`url_host` are the pure decision `init_run` acts on:
    // whether to call `ensure_host_known` before `create_or_clone` at all.
    // `ensure_host_known` itself already has its own fs/exec-backed tests in
    // `dotfix_core::init::ssh`; what is untested anywhere else is *when* it
    // fires, so that is what these lock down.

    #[test]
    fn a_plain_github_ssh_url_targets_github() {
        assert_eq!(
            github_host("git@github.com:example/dotfiles.git"),
            Some("github.com")
        );
    }

    #[test]
    fn a_host_alias_for_github_also_targets_github() {
        // The exact alias `ssh::ensure_ssh_config` writes for a deploy key —
        // matched against `ssh::HOST_ALIAS` itself, not a copy of the
        // literal, so the two cannot drift apart.
        assert_eq!(
            github_host("git@github.com-dotfix:example/dotfiles.git"),
            Some(ssh::HOST_ALIAS)
        );
    }

    #[test]
    fn an_https_github_url_targets_github() {
        assert_eq!(
            github_host("https://github.com/example/dotfiles.git"),
            Some("github.com")
        );
    }

    #[test]
    fn a_different_host_does_not_target_github() {
        assert_eq!(github_host("git@gitlab.com:example/dotfiles.git"), None);
    }

    #[test]
    fn a_lookalike_subdomain_is_not_matched_as_github() {
        // Regression: matching must be on the host component, not a naive
        // substring of the whole URL — `contains("github.com")` would wrongly
        // accept this.
        assert_eq!(github_host("git@github.com.evil.example:x/y.git"), None);
    }

    #[test]
    fn a_host_that_merely_contains_github_com_in_the_path_is_not_matched() {
        assert_eq!(
            github_host("https://example.com/mirror/github.com/evil.git"),
            None
        );
    }

    #[test]
    fn a_host_that_only_shares_the_alias_prefix_is_not_matched() {
        // Regression for a prefix match: the codebase generates exactly one
        // alias, `ssh::HOST_ALIAS`. A `starts_with("github.com-")` check
        // would wrongly classify this unrelated host as GitHub and run its
        // fingerprints against it.
        assert_eq!(github_host("git@github.com-evil.example:x/y.git"), None);
    }

    #[test]
    fn a_bare_scp_style_github_url_with_no_user_is_still_pinned() {
        // Regression: git's scp shorthand allows an omitted user
        // (`host:path`), not just `user@host:path`. Skipping the host-key
        // check for this shape would be a silent bypass — a clone that goes
        // through with no pinning at all, on any network.
        assert_eq!(
            github_host("github.com:owner/dotfiles.git"),
            Some("github.com")
        );
    }

    #[test]
    fn a_scp_style_github_url_with_a_user_is_still_pinned() {
        assert_eq!(
            github_host("git@github.com:owner/dotfiles.git"),
            Some("github.com")
        );
    }

    #[test]
    fn scp_syntax_has_no_port_so_an_all_digit_remainder_is_still_a_path() {
        // Git's scp shorthand (`[user@]host:path`) has no port field at all —
        // everything after the first colon is the path, always. So
        // `example.com:22` is host `example.com` with path `22`, not host
        // `example.com` on port 22, and must not be misread as one.
        assert_eq!(url_host("example.com:22"), Some("example.com"));
        // The same holds for GitHub itself: `github.com:1234` is a valid, if
        // odd, repository path and must still be pinned — a digit heuristic
        // that turned this into `None` would silently skip the host-key
        // check for a legitimate URL.
        assert_eq!(
            github_host("git@github.com:1234"),
            Some("github.com"),
            "a numeric scp path must not be mistaken for a port and dropped"
        );
    }

    #[test]
    fn an_ssh_url_with_an_explicit_port_still_yields_the_bare_host() {
        // Unlike scp syntax, `ssh://` URLs do have a port, and it must be
        // stripped rather than folded into the host.
        assert_eq!(
            github_host("ssh://github.com:22/owner/repo.git"),
            Some("github.com")
        );
    }

    // --- (b) username validation before it reaches the credential helper ---

    #[test]
    fn a_username_containing_a_newline_is_rejected() {
        // `remote::store_token` builds an unescaped newline-separated
        // `key=value` payload; a newline in the username could inject an
        // extra field into it.
        let err = validate_username("alice\nhost=evil.example").unwrap_err();
        assert!(
            err.contains("whitespace") || err.contains("control"),
            "{err}"
        );
    }

    #[test]
    fn a_username_is_trimmed_before_being_accepted() {
        assert_eq!(validate_username("  alice  ").unwrap(), "alice");
    }

    #[test]
    fn a_username_with_internal_whitespace_is_rejected() {
        assert!(validate_username("al ice").is_err());
    }

    #[test]
    fn an_all_whitespace_username_is_rejected_as_empty_rather_than_stored() {
        // Trimming alone would let this through as an empty username, which
        // would land in the payload as a bare `username=` field.
        let err = validate_username("   ").unwrap_err();
        assert!(err.contains("empty"), "{err}");
    }

    #[test]
    fn an_ordinary_username_is_accepted_unchanged() {
        assert_eq!(validate_username("example-user").unwrap(), "example-user");
    }
}
