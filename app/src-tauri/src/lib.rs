//! The dotfix menubar app. All decisions live in [`view`] and in
//! `dotfix-core`; this crate only wires them to Tauri.

pub mod commands;
pub mod ctx;
pub mod tray;
pub mod view;

use std::path::{Path, PathBuf};

use tauri_plugin_autostart::ManagerExt;

/// Whether this running copy may claim the `dotfix` login item.
///
/// The autostart plugin registers whatever path it happens to be running
/// from. Without this gate a `cargo tauri dev` run — or a double-click on a
/// freshly bundled `.app` still sitting in `target/` — silently repoints the
/// login item at a build artefact. The next `cargo clean` deletes it, and
/// from then on nothing starts at login, with no error anywhere to say why.
/// That is exactly what happened to this machine's login item twice while
/// building the app.
///
/// "Installed" means the directories macOS actually installs into:
/// `/Applications` or `~/Applications`. Anything else leaves the login item
/// untouched — a development build has no business owning it.
fn is_installed_copy(exe: &Path, home: Option<&Path>) -> bool {
    // `Path::starts_with` compares whole components, so `/Applications-evil`
    // is not a match. A string prefix test would accept it.
    exe.starts_with("/Applications")
        || home.is_some_and(|h| exe.starts_with(h.join("Applications")))
}

pub fn run() {
    // FIRST, before Tauri starts any thread. A window opened from Finder
    // inherits launchd's minimal `PATH` — `/usr/bin:/bin:/usr/sbin:/sbin` —
    // which contains no Homebrew prefix, so `brew`, `gh`, `op` and `age`
    // cannot be started at all. The engine reported that as
    // `command `brew leaves` failed: No such file or directory`, which reads
    // like a broken Homebrew rather than a missing `PATH`.
    //
    // `set_var` is sound only while the process is single-threaded, which is
    // why this sits at the top of the entry point rather than in `setup`.
    let path = std::env::var("PATH").unwrap_or_default();
    unsafe { std::env::set_var("PATH", commands::path_with_homebrew(&path)) };

    tauri::Builder::default()
        // FIRST in the chain, deliberately. A second launch must die before it
        // builds a tray icon, registers a login item or opens the config — so
        // this plugin has to run before any of those do.
        //
        // What this actually guards: launching through Finder or Spotlight is
        // already deduplicated by LaunchServices, which sees the running bundle
        // identifier and just activates it. Running the *binary* directly
        // bypasses LaunchServices entirely — a LaunchAgent, a terminal
        // invocation, a double-click on the executable inside the bundle — and
        // does start a second process, with a second menubar icon and a second
        // writer on the same configuration. That is the path this closes.
        //
        // The second process exits; this callback runs in the FIRST one. Show
        // its window, because a user who launches an already-running menubar
        // app is asking to see it, not to be ignored.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_opener::init())
        // The app's own login-item registration. `init` only installs the
        // `AutoLaunchManager` into Tauri's state — it writes no LaunchAgent
        // by itself. The actual registration is the `autolaunch().enable()`
        // call in `setup` below; without it this plugin would be inert and
        // the README's "registers itself as a login item" would be false.
        //
        // This is separate from —
        // and additional to — the LaunchAgent that `dotfix init` installs
        // for the CLI's hourly background check: see the "Background
        // check" section of the README for why both exist and neither
        // replaces the other.
        //
        // DO NOT add `.app_name("dev.noix.dotfix")` here "for consistency
        // with the bundle identifier". With no explicit `.app_name(...)`,
        // tauri-plugin-autostart (via the `auto-launch` crate) derives the
        // LaunchAgent's file name and Label from `package_info().name`,
        // i.e. `productName` = "dotfix", writing
        // `~/Library/LaunchAgents/dotfix.plist` (Label `dotfix`). The CLI's
        // agent is `~/Library/LaunchAgents/dev.noix.dotfix.plist` (Label
        // `dev.noix.dotfix`, see `crates/core/src/agent.rs`) — a different
        // file today. But this app's bundle identifier IS `dev.noix.dotfix`,
        // so setting `.app_name("dev.noix.dotfix")` would make this plugin
        // write to the CLI's LaunchAgent file and silently overwrite it,
        // permanently disabling the hourly background check and the shell
        // status line with no error anywhere. Keep deriving the name from
        // productName.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            commands::about,
            commands::open_link,
            commands::overview,
            commands::refresh,
            commands::apply_items,
            commands::overwrite_files,
            commands::adopt_item,
            commands::list_sets,
            commands::toggle_set,
            commands::open_in_repo,
            commands::edit_set_package,
            commands::declare_requirement,
            commands::read_settings,
            commands::set_remote,
            commands::set_provider,
            commands::rename_machine,
            commands::unignore,
            commands::publish,
            commands::file_diff,
            commands::history,
            commands::init_preflight,
            commands::init_probe_ssh,
            commands::init_clone_target,
            commands::init_deploy_key,
            commands::init_store_token,
            commands::init_create_repo,
            commands::init_run,
        ])
        .setup(|app| {
            // No dock icon: dotfix lives in the menubar.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Register the login item, so the tray icon is there after a
            // reboot without the user launching anything. Deliberately not
            // fatal: enabling writes to ~/Library/LaunchAgents, which can
            // fail on a locked-down or MDM-managed Mac, and a menubar app
            // that refuses to start because it could not arrange to start
            // *next* time is worse than one that simply is not in the login
            // items. Report it and carry on.
            //
            // No `.app_name(...)` is passed to the plugin (see the long
            // comment on `tauri_plugin_autostart::init` above): the name is
            // derived from productName, so this writes
            // `~/Library/LaunchAgents/dotfix.plist` and cannot touch the
            // CLI's `dev.noix.dotfix.plist`.
            //
            // Only an installed copy registers itself — see
            // [`is_installed_copy`]. If we cannot tell where we are running
            // from, leave the login item alone: claiming it wrongly is the
            // failure that is silent, while not claiming it is visible the
            // moment someone looks at their login items.
            let home = std::env::var_os("HOME").map(PathBuf::from);
            match std::env::current_exe() {
                Ok(exe) if is_installed_copy(&exe, home.as_deref()) => {
                    if let Err(err) = app.autolaunch().enable() {
                        eprintln!("dotfix: could not register the app as a login item: {err}");
                    }
                }
                Ok(exe) => eprintln!(
                    "dotfix: running from {} rather than an installed copy — \
                     leaving the login item unchanged",
                    exe.display()
                ),
                Err(err) => eprintln!(
                    "dotfix: could not determine this binary's path ({err}) — \
                     leaving the login item unchanged"
                ),
            }

            tray::build(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Keep the process — and the tray icon — alive. A menubar
                // app that quits when its window closes loses its tray icon
                // and its reason to exist.
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running dotfix");
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "/Users/test";

    fn home() -> PathBuf {
        PathBuf::from(HOME)
    }

    #[test]
    fn an_installed_copy_may_register_itself() {
        assert!(is_installed_copy(
            Path::new("/Applications/dotfix.app/Contents/MacOS/dotfix-app"),
            Some(&home()),
        ));
    }

    #[test]
    fn a_per_user_install_counts_too() {
        assert!(is_installed_copy(
            Path::new("/Users/test/Applications/dotfix.app/Contents/MacOS/dotfix-app"),
            Some(&home()),
        ));
    }

    #[test]
    fn a_bundle_still_sitting_in_the_build_tree_may_not() {
        // The real case: `cargo tauri build` produces this path, and opening
        // it once pointed the login item at it.
        assert!(!is_installed_copy(
            Path::new(
                "/Users/test/_dev/dotfix/target/release/bundle/macos/dotfix.app/Contents/MacOS/dotfix-app"
            ),
            Some(&home()),
        ));
    }

    #[test]
    fn a_plain_debug_binary_may_not_either() {
        assert!(!is_installed_copy(
            Path::new("/Users/test/_dev/dotfix/target/debug/dotfix-app"),
            Some(&home()),
        ));
    }

    #[test]
    fn a_directory_that_merely_starts_with_the_same_letters_is_not_applications() {
        assert!(!is_installed_copy(
            Path::new("/Applications-evil/dotfix.app/Contents/MacOS/dotfix-app"),
            Some(&home()),
        ));
    }

    #[test]
    fn without_a_home_the_system_directory_still_counts() {
        assert!(is_installed_copy(
            Path::new("/Applications/dotfix.app/Contents/MacOS/dotfix-app"),
            None,
        ));
        assert!(!is_installed_copy(
            Path::new("/Users/test/Applications/dotfix.app/Contents/MacOS/dotfix-app"),
            None,
        ));
    }
}
