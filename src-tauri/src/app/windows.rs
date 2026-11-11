//! Pullbell's windows. All are created on demand from the same frontend bundle; the label picks
//! the view (`index.html#queue`).

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

struct Spec {
    title: &'static str,
    size: (f64, f64),
    on_top: bool,
    resizable: bool,
}

fn spec(label: &str) -> Option<Spec> {
    Some(match label {
        "queue" => Spec {
            title: "Pullbell — Queue",
            size: (560.0, 460.0),
            on_top: true,
            resizable: true,
        },
        "myprs" => Spec {
            title: "Pullbell — My PRs",
            size: (600.0, 480.0),
            on_top: true,
            resizable: true,
        },
        "settings" => Spec {
            title: "Pullbell Settings",
            size: (620.0, 600.0),
            on_top: false,
            resizable: true,
        },
        "welcome" => Spec {
            title: "Welcome to Pullbell",
            size: (480.0, 520.0),
            on_top: false,
            resizable: false,
        },
        "about" => Spec {
            title: "About Pullbell",
            size: (360.0, 280.0),
            on_top: false,
            resizable: false,
        },
        _ => return None,
    })
}

pub fn open(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let Some(s) = spec(label) else { return };
    let built = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html#{label}").into()),
    )
    .title(s.title)
    .inner_size(s.size.0, s.size.1)
    .resizable(s.resizable)
    .always_on_top(s.on_top)
    .center()
    .focused(true)
    .build();
    match built {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => log::error!("could not open {label} window: {e}"),
    }
}

/// Is any Pullbell window visible? (The updater only restarts when none is.)
pub fn any_visible(app: &AppHandle) -> bool {
    app.webview_windows()
        .values()
        .any(|w| w.is_visible().unwrap_or(false))
}
