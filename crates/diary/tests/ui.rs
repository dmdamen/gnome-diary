//! UI tests. GTK must run on one thread, so this uses its own harness
//! (`harness = false`) and runs every test in turn on the main thread.

use gtk::prelude::*;

fn smoke_window_presents() {
    let window = gtk::Window::new();
    window.present();
    assert!(window.is_visible());
    window.destroy();
}

fn main() {
    gtk::init().expect("GTK needs a display; run through ./dev.sh");
    adw::init().unwrap();
    let tests: &[(&str, fn())] = &[("smoke_window_presents", smoke_window_presents)];
    for (name, test) in tests {
        test();
        println!("ui test {name} ... ok");
    }
}
