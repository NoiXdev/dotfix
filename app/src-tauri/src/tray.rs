//! Menubar presence. The glyph, tooltip and menu model are pure functions so
//! they can be tested without a running app; only [`build`] and [`update`]
//! touch Tauri.
//!
//! [`build`] sets the icon on the builder rather than leaving the tray bare
//! for [`update`] to fill in: a status item with no image has zero width and
//! is simply invisible, which is not a state worth being able to reach.

use dotfix_core::drift::Counts;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::view::Overview;

pub const TRAY_ID: &str = "dotfix-tray";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    /// Nothing to do. Plain mark.
    Quiet,
    /// Something drifted. Mark with a status dot.
    Drift,
}

impl Glyph {
    /// The PNG bytes for this glyph, embedded in the binary at compile time.
    ///
    /// These are template images: monochrome with transparency, so macOS
    /// inverts them for a dark menubar and for the pressed state on its own.
    ///
    /// Embedding (rather than `Image::from_path` against
    /// `app.path().resource_dir()`) is deliberate: `resource_dir()` only
    /// resolves to a real `Resources/` folder inside a *built* `.app`
    /// bundle. Under `tauri dev` the binary runs straight out of
    /// `target/debug/`, so that path never exists and the tray failed to
    /// build on every `tauri dev` run — a `bundle.resources` entry would
    /// have fixed the bundled case but left `tauri dev` broken. Compile-time
    /// embedding behaves identically in both, and these are two tiny
    /// monochrome PNGs that must exist whenever the binary does, so there is
    /// no reason to load them from disk at all.
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Glyph::Quiet => include_bytes!("../icons/tray-quiet.png"),
            Glyph::Drift => include_bytes!("../icons/tray-drift.png"),
        }
    }
}

pub fn glyph_for(counts: &Counts) -> Glyph {
    let quiet = counts.incoming == 0
        && counts.unmanaged == 0
        && counts.removed == 0
        && counts.local_edits == 0;
    if quiet { Glyph::Quiet } else { Glyph::Drift }
}

/// The same sentence the shell prints, so the menubar and the terminal can
/// never tell the user two different things.
pub fn tooltip_for(overview: &Overview) -> String {
    overview
        .status_line
        .clone()
        .unwrap_or_else(|| "dotfix — everything in sync".to_string())
}

pub fn menu_model(overview: &Overview) -> Vec<(String, String)> {
    let c = &overview.counts;
    let outstanding = c.incoming + c.removed + c.local_edits + c.unmanaged;
    let open = if outstanding == 0 {
        "Open dotfix".to_string()
    } else if c.incoming > 0 {
        format!(
            "Open dotfix ({} change{})",
            c.incoming,
            if c.incoming == 1 { "" } else { "s" }
        )
    } else {
        format!("Open dotfix ({outstanding} to review)")
    };

    vec![
        ("open".to_string(), open),
        ("refresh".to_string(), "Check now".to_string()),
        ("quit".to_string(), "Quit".to_string()),
    ]
}

/// What the tooltip says when a check could not be carried out.
///
/// A tray icon has exactly three channels: the glyph, the tooltip and the
/// menu. Swallowing a failed "Check now" is what made it look like the menu
/// item did nothing at all, and pretending the machine is quiet would be
/// worse — so the failure goes in the tooltip and the glyph is left alone.
pub fn failure_tooltip(err: &str) -> String {
    format!("dotfix: check failed — {err}")
}

pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn menu_for(app: &AppHandle, overview: &Overview) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;
    for (id, label) in menu_model(overview) {
        menu.append(&MenuItem::with_id(app, &id, &label, true, None::<&str>)?)?;
    }
    Ok(menu)
}

/// Put `overview` on the menubar: glyph, tooltip and menu, all derived from
/// that one value via the same pure functions above, so the three can never
/// disagree with each other or with the window.
///
/// This is the only place that writes to the tray. Everything that learns
/// something new about the machine — startup, "Check now", and every command
/// that changed state — calls it, because a menubar icon that was computed
/// once at login tells the user the machine is quiet for the rest of the day.
pub fn update(app: &AppHandle, overview: &Overview) -> tauri::Result<()> {
    // A missing tray here is a bug, not a quiet no-op: `build` created it, so
    // failing to find it means the menubar is showing something other than
    // what we just computed. Reporting Ok would hide exactly that — it is how
    // the app once shipped with no menubar icon at all while every check
    // passed.
    let tray = app.tray_by_id(TRAY_ID).ok_or_else(|| {
        tauri::Error::Anyhow(anyhow::anyhow!(
            "tray `{TRAY_ID}` not found — it should have been created during setup"
        ))
    })?;
    let icon = tauri::image::Image::from_bytes(glyph_for(&overview.counts).bytes())?;
    // Atomically, or macOS renders the icon twice and the menubar flickers.
    tray.set_icon_with_as_template(Some(icon), true)?;
    tray.set_tooltip(Some(tooltip_for(overview)))?;
    tray.set_menu(Some(menu_for(app, overview)?))?;
    Ok(())
}

/// Replace the tooltip with a failure, leaving the glyph as it was: a check
/// that could not run has not told us the machine is in sync.
fn report_failure(app: &AppHandle, err: &str) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(failure_tooltip(err)));
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    // On the error path (e.g. `dotfix init` has not run yet) fall back to an
    // empty overview, bound once, so the menu and the tooltip are derived
    // from the exact same value via the exact same pure functions — there is
    // no second place that decides what a quiet tray looks like.
    let overview = crate::commands::overview().unwrap_or(Overview {
        counts: Counts::default(),
        items: vec![],
        missing: vec![],
        undeclared: vec![],
        status_line: None,
    });

    TrayIconBuilder::with_id(TRAY_ID)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            // `refresh` pushes its own result to the tray via
            // `commands::settled`, so a successful check visibly changes the
            // glyph, the tooltip and the menu. Only the failure needs
            // handling here — it used to be discarded with `let _ =`, which
            // is why "Check now" looked like it did nothing.
            "refresh" => {
                if let Err(err) = crate::commands::refresh(app.clone()) {
                    report_failure(app, &err);
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .icon(tauri::image::Image::from_bytes(
            glyph_for(&overview.counts).bytes(),
        )?)
        .icon_as_template(true)
        .tooltip(tooltip_for(&overview))
        .menu(&menu_for(app, &overview)?)
        .build(app)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use dotfix_core::drift::Counts;

    use super::*;
    use crate::view::Overview;

    fn overview(counts: Counts, status_line: Option<&str>) -> Overview {
        Overview {
            missing: Vec::new(),
            undeclared: Vec::new(),
            counts,
            items: vec![],
            status_line: status_line.map(str::to_string),
        }
    }

    #[test]
    fn a_clean_machine_gets_the_quiet_glyph() {
        assert_eq!(glyph_for(&Counts::default()), Glyph::Quiet);
    }

    #[test]
    fn the_two_glyphs_embed_different_non_empty_assets() {
        let quiet = Glyph::Quiet.bytes();
        let drift = Glyph::Drift.bytes();
        assert!(!quiet.is_empty(), "quiet glyph must not be empty");
        assert!(!drift.is_empty(), "drift glyph must not be empty");
        assert_ne!(quiet, drift, "quiet and drift must embed different PNGs");
    }

    #[test]
    fn any_drift_gets_the_drift_glyph() {
        for counts in [
            Counts {
                incoming: 1,
                ..Default::default()
            },
            Counts {
                unmanaged: 1,
                ..Default::default()
            },
            Counts {
                removed: 1,
                ..Default::default()
            },
            Counts {
                local_edits: 1,
                ..Default::default()
            },
        ] {
            assert_eq!(glyph_for(&counts), Glyph::Drift, "for {counts:?}");
        }
    }

    #[test]
    fn the_tooltip_reuses_the_status_line_so_it_cannot_disagree() {
        let o = overview(
            Counts {
                incoming: 2,
                ..Default::default()
            },
            Some("↯ dotfix: 2 changes   →  dotfix apply"),
        );
        assert_eq!(tooltip_for(&o), "↯ dotfix: 2 changes   →  dotfix apply");
    }

    #[test]
    fn a_quiet_tooltip_still_names_the_app() {
        let o = overview(Counts::default(), None);
        assert_eq!(tooltip_for(&o), "dotfix — everything in sync");
    }

    #[test]
    fn the_menu_offers_open_refresh_and_quit_in_that_order() {
        let o = overview(Counts::default(), None);
        let ids: Vec<String> = menu_model(&o).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec!["open", "refresh", "quit"]);
    }

    #[test]
    fn a_failed_check_says_so_in_the_tooltip_rather_than_claiming_sync() {
        let text = failure_tooltip("repository has diverged from origin");
        assert!(text.contains("check failed"), "{text}");
        assert!(text.contains("diverged"), "{text}");
    }

    #[test]
    fn the_open_entry_summarises_outstanding_work() {
        let o = overview(
            Counts {
                incoming: 3,
                ..Default::default()
            },
            Some("x"),
        );
        let (_, label) = menu_model(&o).into_iter().next().unwrap();
        assert_eq!(label, "Open dotfix (3 changes)");
    }
}
