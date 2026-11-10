//! The GApplication: one window, keyboard shortcuts.

use adw::prelude::*;

use crate::APP_ID;
use crate::window::{Context, DiaryWindow};

const SHORTCUTS: &[(&str, &[&str])] = &[
    ("win.lock", &["<Control>l"]),
    ("win.previous", &["<Alt>Left"]),
    ("win.next", &["<Alt>Right"]),
    ("win.today", &["<Alt>Home"]),
    ("win.calendar", &["<Control>o"]),
    ("win.search", &["<Control>f"]),
    ("win.preferences", &["<Control>comma"]),
    ("win.fullscreen", &["F11"]),
    ("window.close", &["<Control>w", "<Control>q"]),
];

/// Builds the application. `context` supplies the window's environment
/// (tests pass their own).
pub fn build(context: Box<dyn Fn() -> Context>) -> adw::Application {
    let app = adw::Application::builder().application_id(APP_ID).build();
    for (action, keys) in SHORTCUTS {
        app.set_accels_for_action(action, keys);
    }
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        let window = DiaryWindow::new(context());
        window.window().set_application(Some(app));
        window.window().present();
    });
    app
}
