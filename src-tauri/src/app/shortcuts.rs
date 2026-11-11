//! Global shortcuts. Paused while the Settings recorder is active.

use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use super::{lock, shared, windows};

pub fn plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(on_shortcut)
        .build()
}

fn on_shortcut(app: &AppHandle, pressed: &Shortcut, ev: ShortcutEvent) {
    if ev.state != ShortcutState::Pressed {
        return;
    }
    let sc = lock(&shared(app).settings).shortcuts.clone();
    let is = |acc: &Option<String>| {
        acc.as_deref()
            .and_then(|a| a.parse::<Shortcut>().ok())
            .is_some_and(|s| s == *pressed)
    };
    if is(&sc.queue) {
        windows::open(app, "queue");
    } else if is(&sc.my_prs) {
        windows::open(app, "myprs");
    } else if is(&sc.refresh) {
        shared(app).wake.notify_one();
    }
}

/// (Re-)register the shortcuts from Settings.
pub fn register_all(app: &AppHandle) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let sc = lock(&shared(app).settings).shortcuts.clone();
    for acc in [sc.queue, sc.my_prs, sc.refresh].into_iter().flatten() {
        if let Err(e) = gs.register(acc.as_str()) {
            log::warn!("could not register shortcut {acc}: {e}");
        }
    }
}

pub fn pause(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}
