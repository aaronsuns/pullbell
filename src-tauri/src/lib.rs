//! Pullbell: a menu bar GitHub inbox.

pub mod app;
pub mod auth;
pub mod core;
pub mod github;
pub mod logging;
pub mod sync;

use tauri::{Manager, RunEvent};

use app::{commands, lock, poller, shared, shortcuts, tray, updater, windows, Shared};

pub fn run() {
    logging::init(logging::default_path());
    log::info!("Pullbell {} starting", env!("CARGO_PKG_VERSION"));

    let app = tauri::Builder::default()
        .plugin(shortcuts::plugin())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::mark_done,
            commands::open_url,
            commands::refresh_now,
            commands::get_settings,
            commands::save_settings,
            commands::validate_shortcut,
            commands::display_shortcut,
            commands::pause_shortcuts,
            commands::resume_shortcuts,
            commands::list_teams,
            commands::chrome_profiles,
            commands::sign_in_start,
            commands::sign_in_cancel,
            commands::sign_out,
            commands::test_banner,
            commands::finish_welcome,
            commands::check_updates,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let dir = app.path().app_data_dir()?;
            app.manage(Shared::load(dir, auth::platform_store()));
            let handle = app.handle().clone();

            app::banner::init();
            tray::create(&handle)?;
            shortcuts::register_all(&handle);
            commands::apply_autostart(&handle);
            updater::announce_if_updated(&handle);

            let sh = shared(&handle);
            let has_token = matches!(auth::load_tokens(sh.store.as_ref()), Ok(Some(_)));
            let welcome_done = lock(&sh.settings).welcome_done;
            if !welcome_done || (!has_token && auth::client_id().is_some()) {
                windows::open(&handle, "welcome");
            }
            poller::start(handle.clone());
            updater::start(handle);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building Pullbell");

    app.run(|_app, event| {
        // Closing the last window must not quit a menu bar app.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}
