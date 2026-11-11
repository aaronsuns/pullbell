//! The menu bar item: title `n+m` next to a bell, and the menu.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Wry};

use super::{lock, shared, windows};
use crate::core::health::Status;
use crate::core::inbox::{clear_label, nothing_label, row, title};
use crate::core::model::PullRequest;

const TRAY_ID: &str = "pullbell";
const MAX_MENU_ITEMS: usize = 10;
const ROW_CHARS: usize = 70;

enum Target {
    Item(String),
    Url(String),
}

/// Menu ids are short ("t3"); what they point at lives here.
static TARGETS: Mutex<Option<HashMap<String, Target>>> = Mutex::new(None);

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let icon = Image::from_bytes(include_bytes!("../../icons/tray@2x.png"))?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .title("0")
        .tooltip("Pullbell")
        .menu(&build_menu(app)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, ev| on_menu(app, ev.id().as_ref()))
        .build(app)?;
    Ok(())
}

/// Refresh title and menu from current state. Must not be called while holding Shared locks.
pub fn update(app: &AppHandle) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let Some(tray) = app2.tray_by_id(TRAY_ID) else {
            return;
        };
        let sh = shared(&app2);
        let settings = lock(&sh.settings).clone();
        let (n, m) = lock(&sh.state).counts(&settings);
        let _ = tray.set_title(Some(title(n, m)));
        match build_menu(&app2) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => log::error!("menu build failed: {e}"),
        }
    });
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let sh = shared(app);
    let settings = lock(&sh.settings).clone();
    let items = lock(&sh.state).sorted(&settings);
    let prs = lock(&sh.prs).clone();
    let status = sh.status();
    let mut targets = HashMap::new();
    let mut next_id = 0usize;
    let mut tid = |t: Target, targets: &mut HashMap<String, Target>| {
        next_id += 1;
        let id = format!("t{next_id}");
        targets.insert(id.clone(), t);
        id
    };

    let menu = Menu::new(app)?;
    let sep = || PredefinedMenuItem::separator(app);

    if let Some(s) = &status {
        menu.append(&MenuItem::with_id(
            app,
            "status",
            s.label(),
            true,
            None::<&str>,
        )?)?;
        menu.append(&sep()?)?;
    }

    let needs: Vec<_> = items
        .iter()
        .filter(|i| settings.needs_you(i.kind))
        .collect();
    let fyi = items.len() - needs.len();
    if needs.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "none",
            nothing_label(fyi),
            false,
            None::<&str>,
        )?)?;
    }
    for i in needs.iter().take(MAX_MENU_ITEMS) {
        let id = tid(Target::Item(i.key.clone()), &mut targets);
        menu.append(&MenuItem::with_id(
            app,
            id,
            row(i, ROW_CHARS),
            true,
            None::<&str>,
        )?)?;
    }
    menu.append(&MenuItem::with_id(
        app,
        "clear",
        clear_label(needs.len(), fyi),
        !items.is_empty(),
        None::<&str>,
    )?)?;
    menu.append(&sep()?)?;

    let mut pr_sub = |label: String, prs: &[PullRequest]| -> tauri::Result<Submenu<Wry>> {
        let sub = Submenu::new(app, label, true)?;
        if prs.is_empty() {
            sub.append(&MenuItem::new(app, "None", false, None::<&str>)?)?;
        }
        for p in prs {
            let id = tid(Target::Url(p.url.clone()), &mut targets);
            let text: String = p.row().chars().take(ROW_CHARS + 10).collect();
            sub.append(&MenuItem::with_id(app, id, text, true, None::<&str>)?)?;
        }
        Ok(sub)
    };
    menu.append(&pr_sub(
        format!("My open PRs ({})", prs.mine.len()),
        &prs.mine,
    )?)?;
    menu.append(&pr_sub(
        format!("Waiting for my review ({})", prs.waiting.len()),
        &prs.waiting,
    )?)?;
    menu.append(&sep()?)?;

    let accel_item = |id: &str, text: &str, acc: Option<&String>| -> tauri::Result<MenuItem<Wry>> {
        MenuItem::with_id(app, id, text, true, acc.map(String::as_str))
            .or_else(|_| MenuItem::with_id(app, id, text, true, None::<&str>))
    };
    menu.append(&accel_item(
        "open_queue",
        "Open queue",
        settings.shortcuts.queue.as_ref(),
    )?)?;
    menu.append(&accel_item(
        "open_myprs",
        "Open my PRs",
        settings.shortcuts.my_prs.as_ref(),
    )?)?;
    menu.append(&accel_item(
        "refresh",
        "Refresh now",
        settings.shortcuts.refresh.as_ref(),
    )?)?;
    menu.append(&sep()?)?;
    menu.append(&MenuItem::with_id(
        app,
        "settings",
        "Settings…",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "gh_notifications",
        "GitHub notifications",
        true,
        None::<&str>,
    )?)?;
    menu.append(&sep()?)?;
    menu.append(&MenuItem::with_id(
        app,
        "about",
        "About Pullbell",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Quit Pullbell",
        true,
        None::<&str>,
    )?)?;

    *lock(&TARGETS) = Some(targets);
    Ok(menu)
}

fn on_menu(app: &AppHandle, id: &str) {
    let sh = shared(app);
    match id {
        "status" => match sh.status() {
            Some(Status::NotSignedIn) => windows::open(app, "welcome"),
            Some(Status::KeychainDenied) => {
                lock(&sh.health).keychain_denied = false;
                sh.wake.notify_one();
            }
            Some(Status::Sso { url, .. }) => super::open_url(app, &url),
            Some(Status::UpdateAvailable { .. }) => super::updater::install_now(app),
            Some(Status::Refusing) => sh.wake.notify_one(),
            None => {}
        },
        "clear" => {
            let keys: Vec<String> = lock(&sh.state)
                .items
                .iter()
                .map(|i| i.key.clone())
                .collect();
            super::mark_done(app, &keys, false);
        }
        "open_queue" => windows::open(app, "queue"),
        "open_myprs" => windows::open(app, "myprs"),
        "refresh" => sh.wake.notify_one(),
        "settings" => windows::open(app, "settings"),
        "gh_notifications" => super::open_url(app, "https://github.com/notifications"),
        "about" => windows::open(app, "about"),
        "quit" => app.exit(0),
        other => {
            let target = lock(&TARGETS).as_mut().and_then(|m| m.remove(other));
            match target {
                Some(Target::Item(key)) => super::mark_done(app, &[key], true),
                Some(Target::Url(url)) => super::open_url(app, &url),
                None => {}
            }
        }
    }
}
