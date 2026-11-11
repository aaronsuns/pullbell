//! macOS banners. Clicking one opens the item and marks the grouped items done.

use std::sync::atomic::{AtomicUsize, Ordering};

use tauri::AppHandle;

use crate::core::inbox::Banner;

/// Each clickable banner parks a thread until it is clicked or dismissed; cap them.
static WAITING: AtomicUsize = AtomicUsize::new(0);
const MAX_WAITING: usize = 16;

pub fn init() {
    #[cfg(target_os = "macos")]
    {
        // Only works inside the bundled app; in `tauri dev` banners come from the terminal.
        let id = if std::env::current_exe()
            .map(|p| p.to_string_lossy().contains(".app/Contents/MacOS"))
            .unwrap_or(false)
        {
            "se.zexa.pullbell"
        } else {
            "com.apple.Terminal"
        };
        if let Err(e) = mac_notification_sys::set_application(id) {
            log::warn!("banner setup: {e}");
        }
    }
}

pub fn show(app: &AppHandle, b: Banner) {
    let clickable = WAITING.fetch_add(1, Ordering::SeqCst) < MAX_WAITING;
    if !clickable {
        WAITING.fetch_sub(1, Ordering::SeqCst);
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let clicked = send(&b, clickable);
        if clickable {
            WAITING.fetch_sub(1, Ordering::SeqCst);
        }
        if clicked {
            let keys = b.keys.clone();
            let url = b.url.clone();
            let _ = app.clone().run_on_main_thread(move || {
                if keys.is_empty() {
                    if !url.is_empty() {
                        super::open_url(&app, &url);
                    }
                } else {
                    super::mark_done(&app, &keys, true);
                }
            });
        }
    });
}

#[cfg(target_os = "macos")]
fn send(b: &Banner, wait: bool) -> bool {
    use mac_notification_sys::{Notification, NotificationResponse};
    match Notification::new()
        .title(&b.title)
        .message(&b.body)
        .wait_for_click(wait)
        .asynchronous(!wait)
        .send()
    {
        Ok(NotificationResponse::Click) | Ok(NotificationResponse::ActionButton(_)) => true,
        Ok(_) => false,
        Err(e) => {
            log::warn!("banner failed: {e}");
            false
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn send(_b: &Banner, _wait: bool) -> bool {
    false
}
