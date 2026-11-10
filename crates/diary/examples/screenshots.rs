//! Renders the main pages to PNG files (for docs and design review).
//! Run with `./dev.sh screenshots`; output goes to `target/screenshots/`.

use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use diary::window::{Context, DiaryWindow};
use diary_core::clock::{Clock, ManualClock};
use diary_core::crypto::KdfCost;
use diary_core::entry::Entry;
use diary_core::secrecy::SecretString;
use diary_core::store::Diary;

const SAMPLE: &str = "The first frost this morning. The garden looked like someone had \
sugared it overnight, and the cat refused to go further than the doorstep.\n\n\
Spent the afternoon finishing the bookshelf. It wobbles a little, but it is mine, \
and the books seem happy enough.\n\n\
Note to self: call Anna about the weekend.";

fn wait(mut done: impl FnMut() -> bool) {
    let ctx = gtk::glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() && Instant::now() < deadline {
        ctx.iteration(false);
        std::thread::sleep(Duration::from_millis(5));
    }
    let end = Instant::now() + Duration::from_millis(300);
    while Instant::now() < end {
        ctx.iteration(false);
    }
}

fn shoot(w: &DiaryWindow, name: &str) {
    let window = w.window();
    let (width, height) = (window.width(), window.height());
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, f64::from(width), f64::from(height));
    let node = snapshot.to_node().expect("something to draw");
    let renderer = window.renderer().expect("renderer");
    let texture = renderer.render_texture(&node, None);
    std::fs::create_dir_all("target/screenshots").unwrap();
    let path = format!("target/screenshots/{name}.png");
    texture.save_to_png(&path).unwrap();
    println!("{path}");
}

fn main() {
    gtk::init().unwrap();
    adw::init().unwrap();
    if std::env::args().any(|a| a == "--dark") {
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    }
    let dir = tempfile::tempdir().unwrap();
    let clock = Rc::new(ManualClock::new("2026-11-10T20:55:03+01:00"));
    let ctx = Context {
        config_file: dir.path().join("settings"),
        device: "laptop".into(),
        kdf: KdfCost(2),
        clock: clock.clone(),
    };
    let folder = dir.path().join("Nextcloud/Diary");

    let w = DiaryWindow::new(ctx.clone());
    w.window().present();
    wait(|| false);
    shoot(&w, "1-setup");

    w.folder_chosen(&folder);
    wait(|| false);
    shoot(&w, "2-create-password");

    let pw = SecretString::from("password1".to_owned());
    let diary = Diary::create(&folder, &pw, KdfCost(2), "laptop").unwrap();
    for day in ["2026-11-02", "2026-11-05", "2026-11-09"] {
        let mut e = Entry::new(clock.now());
        e.body = "…".into();
        diary
            .save(day.parse().unwrap(), &e, None, clock.now())
            .unwrap();
    }
    w.window().destroy();

    let w = DiaryWindow::new(ctx);
    w.window().present();
    w.password_page().password.set_text("password1");
    wait(|| false);
    shoot(&w, "3-unlock");
    w.password_page().submit.emit_clicked();
    wait(|| w.visible_page() == "editor");
    shoot(&w, "4-editor-empty");
    w.editor_page().text.buffer().set_text(SAMPLE);
    w.editor_page().pointer_moved(1.0, 1.0);
    wait(|| false);
    shoot(&w, "5-editor");
}
