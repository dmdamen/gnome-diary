//! UI tests, run against a headless Wayland compositor (see `./dev.sh`).
//! GTK must run on one thread, so this uses its own harness
//! (`harness = false`) and runs every test in turn on the main thread.

use std::rc::Rc;
use std::time::{Duration, Instant};

use adw::prelude::*;
use diary::window::{Context, DiaryWindow};
use diary_core::clock::{Clock, ManualClock};
use diary_core::crypto::KdfCost;
use diary_core::entry::Entry;
use diary_core::secrecy::SecretString;
use diary_core::store::Diary;

const NOW: &str = "2026-11-10T20:55:03+01:00";

struct Env {
    dir: tempfile::TempDir,
    clock: Rc<ManualClock>,
}

impl Env {
    fn new() -> Self {
        Env {
            dir: tempfile::tempdir().unwrap(),
            clock: Rc::new(ManualClock::new(NOW)),
        }
    }
    fn config_file(&self) -> std::path::PathBuf {
        self.dir.path().join("config/settings")
    }
    fn diary_dir(&self) -> std::path::PathBuf {
        self.dir.path().join("Sync/Diary")
    }
    fn context(&self) -> Context {
        Context {
            config_file: self.config_file(),
            device: "laptop".into(),
            kdf: KdfCost(2),
            clock: self.clock.clone(),
        }
    }
    fn window(&self) -> DiaryWindow {
        let w = DiaryWindow::new(self.context());
        w.window().present();
        w
    }
    /// A diary that already exists (e.g. synced from another computer),
    /// with the config pointing at it.
    fn existing_diary(&self, days: &[(&str, &str)]) {
        let pw = SecretString::from("password1".to_owned());
        let diary = Diary::create(&self.diary_dir(), &pw, KdfCost(2), "desktop").unwrap();
        for (day, body) in days {
            let mut e = Entry::new(self.clock.now());
            e.body = (*body).into();
            diary
                .save(day.parse().unwrap(), &e, None, self.clock.now())
                .unwrap();
        }
        std::fs::create_dir_all(self.config_file().parent().unwrap()).unwrap();
        std::fs::write(
            self.config_file(),
            format!("folder={}\n", self.diary_dir().display()),
        )
        .unwrap();
    }
    fn day_on_disk(&self, day: &str) -> Option<String> {
        let pw = SecretString::from("password1".to_owned());
        let diary = Diary::open(&self.diary_dir(), &pw, "x").unwrap();
        let bytes = std::fs::read(diary.path_for(day.parse().unwrap())).ok()?;
        Some(String::from_utf8(diary.keys().decrypt(&bytes).unwrap()).unwrap())
    }
}

/// Runs the main loop until `done` holds (or fails after 10 s).
fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let ctx = gtk::glib::MainContext::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        ctx.iteration(false);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn settle() {
    let ctx = gtk::glib::MainContext::default();
    while ctx.iteration(false) {}
}

fn unlock(env: &Env, w: &DiaryWindow) {
    let page = w.password_page();
    page.password.set_text("password1");
    page.submit.emit_clicked();
    wait_until("editor", || w.visible_page() == "editor");
    let _ = env;
}

/// Activates a window action, like a shortcut or menu item would. Disabled
/// actions do nothing.
fn act(w: &DiaryWindow, name: &str) {
    gtk::gio::prelude::ActionGroupExt::activate_action(w.window(), name, None);
}

fn type_text(w: &DiaryWindow, text: &str) {
    w.editor_page().text.buffer().set_text(text);
}

fn text(w: &DiaryWindow) -> String {
    let buffer = w.editor_page().text.buffer();
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .into()
}

// First run ------------------------------------------------------------

fn first_run_asks_for_a_folder() {
    let env = Env::new();
    let w = env.window();
    assert_eq!(w.visible_page(), "setup");
    assert!(w.setup_page().choose.is_sensitive());
}

fn empty_folder_asks_for_a_new_password() {
    let env = Env::new();
    let w = env.window();
    w.folder_chosen(&env.diary_dir());
    assert_eq!(w.visible_page(), "password");
    let page = w.password_page();
    assert!(page.confirm.is_visible());
    assert_eq!(page.submit.label().unwrap(), "Create Diary");
    let saved = std::fs::read_to_string(env.config_file()).unwrap();
    assert!(saved.contains(&format!("folder={}", env.diary_dir().display())));
}

fn new_password_must_be_long_enough_and_match() {
    let env = Env::new();
    let w = env.window();
    w.folder_chosen(&env.diary_dir());
    let page = w.password_page();
    page.password.set_text("short");
    assert!(!page.submit.is_sensitive());
    assert_eq!(page.problem.label(), "Use at least 8 characters");
    page.password.set_text("long enough");
    page.confirm.set_text("long enougH");
    assert!(!page.submit.is_sensitive());
    assert_eq!(page.problem.label(), "The passwords do not match");
    page.confirm.set_text("long enough");
    assert!(page.submit.is_sensitive());
    assert!(!page.problem.is_visible());
}

fn creating_a_diary_opens_today() {
    let env = Env::new();
    let w = env.window();
    w.folder_chosen(&env.diary_dir());
    let page = w.password_page();
    page.password.set_text("password1");
    page.confirm.set_text("password1");
    page.confirm.emit_by_name::<()>("entry-activated", &[]);
    wait_until("editor", || w.visible_page() == "editor");
    assert!(env.diary_dir().join(".diary/identity.age").is_file());
    let title = w.editor_page().title.clone();
    assert_eq!(title.title(), "Tuesday 10 November 2026");
    assert_eq!(title.subtitle(), "Today");
    assert_eq!(text(&w), "");
    // The password is not kept in the entry fields.
    assert_eq!(w.password_page().password.text(), "");
}

fn diary_folder_from_another_computer_asks_to_unlock() {
    let env = Env::new();
    env.existing_diary(&[]);
    std::fs::remove_file(env.config_file()).unwrap();
    let w = env.window();
    w.folder_chosen(&env.diary_dir());
    let page = w.password_page();
    assert!(!page.confirm.is_visible());
    assert_eq!(page.submit.label().unwrap(), "Unlock");
}

fn folder_that_cannot_be_used_shows_error() {
    let env = Env::new();
    let blocker = env.dir.path().join("file");
    std::fs::write(&blocker, "").unwrap();
    let w = env.window();
    w.folder_chosen(&blocker.join("Diary"));
    let page = w.password_page();
    page.password.set_text("password1");
    page.confirm.set_text("password1");
    page.submit.emit_clicked();
    wait_until("error", || page.problem.is_visible());
    assert_eq!(w.visible_page(), "password");
    assert!(page.submit.is_sensitive());
}

fn other_folder_button_returns_to_setup() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    w.password_page().other_folder.emit_clicked();
    assert_eq!(w.visible_page(), "setup");
}

// Unlocking ------------------------------------------------------------

fn configured_diary_starts_at_unlock() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    assert_eq!(w.visible_page(), "password");
    assert_eq!(w.password_page().submit.label().unwrap(), "Unlock");
}

fn wrong_password_is_refused() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    let page = w.password_page();
    page.password.set_text("not it at all");
    page.submit.emit_clicked();
    assert!(!page.submit.is_sensitive(), "busy while unlocking");
    wait_until("error", || page.problem.is_visible());
    assert_eq!(page.problem.label(), "Wrong password");
    assert_eq!(w.visible_page(), "password");
}

fn unlock_shows_todays_entry() {
    let env = Env::new();
    env.existing_diary(&[("2026-11-10", "Morning: coffee.")]);
    let w = env.window();
    unlock(&env, &w);
    assert_eq!(text(&w), "Morning: coffee.");
}

fn missing_configured_folder_goes_back_to_setup() {
    let env = Env::new();
    std::fs::create_dir_all(env.config_file().parent().unwrap()).unwrap();
    std::fs::write(env.config_file(), "folder=/nowhere/at/all\n").unwrap();
    let w = env.window();
    assert_eq!(w.visible_page(), "setup");
}

// Writing --------------------------------------------------------------

fn typing_is_saved_after_a_pause() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    type_text(&w, "Dear diary");
    assert!(env.day_on_disk("2026-11-10").is_none());
    env.clock.advance(chrono::Duration::seconds(2));
    wait_until("autosave", || env.day_on_disk("2026-11-10").is_some());
    let saved = env.day_on_disk("2026-11-10").unwrap();
    assert!(saved.starts_with("created: 2026-11-10T20:55:03+01:00"));
    assert!(saved.ends_with("Dear diary"));
}

fn header_hides_while_typing() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    let editor = w.editor_page();
    assert!(editor.toolbar.reveals_top_bars());
    assert!(editor.placeholder.is_visible());
    type_text(&w, "x");
    assert!(!editor.toolbar.reveals_top_bars());
    assert!(!editor.placeholder.is_visible());
    editor.pointer_moved(10.0, 10.0);
    assert!(editor.toolbar.reveals_top_bars());
}

fn view_follows_the_cursor_while_typing() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    let view = w.editor_page().text.clone();
    view.grab_focus();
    // Type (not paste) far past the bottom of the window.
    for _ in 0..80 {
        view.emit_insert_at_cursor("another line of text\n");
    }
    // The text view must own the scrolling (inside a plain container it has
    // no adjustment and the view never follows the cursor) …
    wait_until("layout", || {
        view.vadjustment()
            .is_some_and(|a| a.upper() > a.page_size())
    });
    // … so that GTK's "keep the cursor visible" step, run on every
    // keystroke, scrolls it. (Headless windows get no keyboard focus, so
    // that step is triggered directly here.)
    view.scroll_mark_onscreen(&view.buffer().get_insert());
    let scroller = w.editor_page().scroller.clone();
    wait_until("scroll to cursor", || scroller.vadjustment().value() > 0.0);
}

fn a_free_line_stays_below_the_cursor() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    let view = w.editor_page().text.clone();
    wait_until("layout", || view.height() > 0);
    for _ in 0..40 {
        // What a keystroke does: insert, then GTK scrolls the cursor just
        // into view (with no margin). A person types slower than a frame.
        view.emit_insert_at_cursor("another line of text\n");
        view.scroll_mark_onscreen(&view.buffer().get_insert());
        let until = Instant::now() + Duration::from_millis(20);
        while Instant::now() < until {
            gtk::glib::MainContext::default().iteration(false);
        }
    }
    wait_until("text laid out", || {
        view.vadjustment()
            .is_some_and(|a| a.upper() > 2.0 * a.page_size() && a.value() > 0.0)
    });
    let free_below = || {
        let cursor = view.iter_location(&view.buffer().iter_at_mark(&view.buffer().get_insert()));
        let (_, bottom) = view.buffer_to_window_coords(
            gtk::TextWindowType::Widget,
            0,
            cursor.y() + cursor.height(),
        );
        (view.height() - bottom, cursor.height())
    };
    wait_until("a free line below the cursor", || {
        let (free, line) = free_below();
        line > 0 && free >= line
    });
}

fn closing_the_window_saves() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    type_text(&w, "last words");
    w.window().close();
    settle();
    assert!(
        env.day_on_disk("2026-11-10")
            .unwrap()
            .ends_with("last words")
    );
}

fn previous_and_next_move_between_entries() {
    let env = Env::new();
    env.existing_diary(&[("2026-11-01", "first"), ("2026-11-05", "second")]);
    let w = env.window();
    unlock(&env, &w);
    let editor = w.editor_page();
    assert!(!editor.next.is_sensitive());
    act(&w, "previous");
    assert_eq!(text(&w), "second");
    assert_eq!(editor.title.subtitle(), "");
    assert_eq!(editor.title.title(), "Thursday 5 November 2026");
    act(&w, "previous");
    assert_eq!(text(&w), "first");
    assert!(!editor.previous.is_sensitive());
    act(&w, "next");
    act(&w, "next");
    assert_eq!(text(&w), "");
    assert_eq!(editor.title.subtitle(), "Today");
    act(&w, "today");
    assert_eq!(editor.title.subtitle(), "Today");
}

fn yesterday_is_named() {
    let env = Env::new();
    env.existing_diary(&[("2026-11-09", "y")]);
    let w = env.window();
    unlock(&env, &w);
    act(&w, "previous");
    assert_eq!(w.editor_page().title.subtitle(), "Yesterday");
}

fn calendar_marks_entries_and_opens_days() {
    let env = Env::new();
    env.existing_diary(&[("2026-11-03", "third"), ("2026-10-20", "october")]);
    let w = env.window();
    unlock(&env, &w);
    let cal = w.editor_page().calendar.clone();
    assert!(cal.day_is_marked(3));
    assert!(!cal.day_is_marked(4));
    // Going to October marks October's days.
    cal.select_day(&gtk::glib::DateTime::from_local(2026, 10, 1, 0, 0, 0.0).unwrap());
    cal.emit_by_name::<()>("prev-month", &[]);
    assert!(cal.day_is_marked(20));
    cal.select_day(&gtk::glib::DateTime::from_local(2026, 11, 3, 0, 0, 0.0).unwrap());
    cal.emit_by_name::<()>("day-selected", &[]);
    assert_eq!(text(&w), "third");
    // Writing marks today.
    act(&w, "today");
    type_text(&w, "now");
    act(&w, "previous");
    assert!(cal.day_is_marked(10));
}

fn search_finds_and_opens_days() {
    let env = Env::new();
    env.existing_diary(&[("2026-10-01", "Swam in the lake"), ("2026-10-02", "Rain")]);
    let w = env.window();
    unlock(&env, &w);
    act(&w, "search");
    assert_eq!(w.visible_page(), "search");
    let search = w.search_page();
    search.entry.set_text("LAKE");
    search.entry.emit_by_name::<()>("search-changed", &[]);
    assert_eq!(search.result_count(), 1);
    assert!(!search.empty.is_visible());
    search.entry.set_text("nothing like this");
    search.entry.emit_by_name::<()>("search-changed", &[]);
    assert_eq!(search.result_count(), 0);
    assert!(search.empty.is_visible());
    search.entry.set_text("lake");
    search.entry.emit_by_name::<()>("search-changed", &[]);
    let row = search.results.row_at_index(0).unwrap();
    search.results.emit_by_name::<()>("row-activated", &[&row]);
    assert_eq!(w.visible_page(), "editor");
    assert_eq!(text(&w), "Swam in the lake");
}

fn search_can_be_left_with_escape_action() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    act(&w, "search");
    w.search_page().entry.emit_by_name::<()>("stop-search", &[]);
    assert_eq!(w.visible_page(), "editor");
}

fn lock_hides_the_text() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    type_text(&w, "private");
    act(&w, "lock");
    assert_eq!(w.visible_page(), "password");
    assert_eq!(text(&w), "");
    assert!(env.day_on_disk("2026-11-10").unwrap().ends_with("private"));
    // Editing actions do nothing while locked.
    act(&w, "previous");
    act(&w, "search");
    assert_eq!(w.visible_page(), "password");
}

fn conflicts_are_announced() {
    let env = Env::new();
    env.existing_diary(&[("2026-11-10", "v1")]);
    let w = env.window();
    unlock(&env, &w);
    std::thread::sleep(Duration::from_millis(20));
    let pw = SecretString::from("password1".to_owned());
    let other = Diary::open(&env.diary_dir(), &pw, "desktop").unwrap();
    other
        .save(
            "2026-11-10".parse().unwrap(),
            &Entry::new(env.clock.now()),
            None,
            env.clock.now(),
        )
        .unwrap();
    type_text(&w, "v1 plus");
    act(&w, "today"); // forces a save
    assert!(w.last_message().unwrap().contains("another device"));
}

fn save_errors_are_announced() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    type_text(&w, "x");
    std::fs::write(env.diary_dir().join("2026"), "").unwrap();
    act(&w, "today");
    assert!(w.last_message().unwrap().starts_with("Could not save"));
    // A day that cannot be opened is reported too.
    std::fs::remove_file(env.diary_dir().join("2026")).unwrap();
    let bad = env.diary_dir().join("2026/11/2026-11-01.md.age");
    std::fs::create_dir_all(bad.parent().unwrap()).unwrap();
    std::fs::write(bad, "garbage").unwrap();
    w.editor_page()
        .calendar
        .select_day(&gtk::glib::DateTime::from_local(2026, 11, 1, 0, 0, 0.0).unwrap());
    w.editor_page()
        .calendar
        .emit_by_name::<()>("day-selected", &[]);
    assert!(w.last_message().unwrap().starts_with("Could not open"));
    // Closing is refused while the text cannot be saved.
    type_text(&w, "unsaved");
    std::fs::remove_dir_all(env.diary_dir().join("2026")).unwrap();
    std::fs::write(env.diary_dir().join("2026"), "").unwrap();
    w.window().close();
    settle();
    assert!(w.window().is_visible());
}

// Preferences ----------------------------------------------------------

fn font_size_is_applied_and_remembered() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    let prefs = w.preferences();
    prefs.font_size.set_value(18.0);
    assert!(w.font_css().contains("font-size: 18pt"));
    let saved = std::fs::read_to_string(env.config_file()).unwrap();
    assert!(saved.contains("font-size=18"));
    let again = env.window();
    assert!(again.font_css().contains("font-size: 18pt"));
}

fn preferences_show_folder_and_change_password() {
    let env = Env::new();
    env.existing_diary(&[("2026-11-01", "kept")]);
    let w = env.window();
    unlock(&env, &w);
    let prefs = w.preferences();
    assert_eq!(
        prefs.folder.subtitle().unwrap(),
        env.diary_dir().display().to_string()
    );
    prefs.new_password.set_text("brand new pw");
    prefs.confirm_password.set_text("different");
    prefs.change_password.emit_clicked();
    assert!(w.last_message().unwrap().contains("match"));
    prefs.confirm_password.set_text("brand new pw");
    prefs.change_password.emit_clicked();
    assert_eq!(w.last_message().unwrap(), "Password changed");
    assert_eq!(prefs.new_password.text(), "");
    let pw = SecretString::from("brand new pw".to_owned());
    assert!(Diary::open(&env.diary_dir(), &pw, "x").is_ok());
}

fn changing_folder_locks_and_returns_to_setup() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    type_text(&w, "saved before leaving");
    w.preferences().change_folder.emit_clicked();
    assert_eq!(w.visible_page(), "setup");
    assert!(
        env.day_on_disk("2026-11-10")
            .unwrap()
            .ends_with("saved before leaving")
    );
}

fn preferences_dialog_and_about_open() {
    let env = Env::new();
    env.existing_diary(&[]);
    let w = env.window();
    unlock(&env, &w);
    act(&w, "preferences");
    act(&w, "about");
    // Opening the popover needs a real input event (headless Weston refuses
    // popup grabs without one), so only check the shortcut is live.
    assert!(w.window().lookup_action("calendar").unwrap().is_enabled());
    settle();
}

fn fullscreen_toggles() {
    let env = Env::new();
    let w = env.window();
    act(&w, "fullscreen");
    wait_until("fullscreen", || w.window().is_fullscreen());
    act(&w, "fullscreen");
    wait_until("unfullscreen", || !w.window().is_fullscreen());
}

// Application ----------------------------------------------------------

fn window_keeps_working_without_a_handle() {
    // The application drops its DiaryWindow right after creating it; the
    // buttons must keep working as long as the GTK window lives.
    let env = Env::new();
    let window = env.window().window().clone();
    settle();
    gtk::gio::prelude::ActionGroupExt::activate_action(&window, "fullscreen", None);
    wait_until("fullscreen", || window.is_fullscreen());
    window.destroy();
}

fn application_opens_a_window() {
    let env = Env::new();
    let app = diary::application::build(Box::new({
        let ctx = env.context();
        move || Context { ..ctx.clone() }
    }));
    app.set_flags(gtk::gio::ApplicationFlags::NON_UNIQUE);
    app.register(None::<&gtk::gio::Cancellable>).unwrap();
    app.activate();
    assert_eq!(app.windows().len(), 1);
    app.activate();
    assert_eq!(app.windows().len(), 1, "a second launch reuses the window");
    assert_eq!(
        app.accels_for_action("win.lock")
            .first()
            .map(|s| s.to_string()),
        Some("<Control>l".into())
    );
    for window in app.windows() {
        window.destroy();
    }
}

fn system_context_uses_user_config_dir() {
    let ctx = Context::system();
    assert!(ctx.config_file.ends_with("diary/settings"));
    assert!(!ctx.device.is_empty());
    assert_eq!(ctx.kdf, KdfCost::default());
    let _ = ctx.clock.now();
}

fn paths_under_home_are_shortened() {
    let home = gtk::glib::home_dir();
    assert_eq!(
        diary::window::display_path(&home.join("Sync/Diary")),
        "~/Sync/Diary"
    );
    assert_eq!(
        diary::window::display_path(std::path::Path::new("/mnt/x")),
        "/mnt/x"
    );
}

fn date_titles() {
    let d = |s: &str| s.parse::<chrono::NaiveDate>().unwrap();
    assert_eq!(
        diary::window::day_title(d("2026-01-02")),
        "Friday 2 January 2026"
    );
    assert_eq!(
        diary::window::day_subtitle(d("2026-01-02"), d("2026-01-02")),
        "Today"
    );
    assert_eq!(
        diary::window::day_subtitle(d("2026-01-01"), d("2026-01-02")),
        "Yesterday"
    );
    assert_eq!(
        diary::window::day_subtitle(d("2026-01-03"), d("2026-01-02")),
        ""
    );
}

fn main() {
    gtk::init().expect("GTK needs a display; run through ./dev.sh");
    adw::init().unwrap();
    let tests: &[(&str, fn())] = &[
        ("first_run_asks_for_a_folder", first_run_asks_for_a_folder),
        (
            "empty_folder_asks_for_a_new_password",
            empty_folder_asks_for_a_new_password,
        ),
        (
            "new_password_must_be_long_enough_and_match",
            new_password_must_be_long_enough_and_match,
        ),
        ("creating_a_diary_opens_today", creating_a_diary_opens_today),
        (
            "diary_folder_from_another_computer_asks_to_unlock",
            diary_folder_from_another_computer_asks_to_unlock,
        ),
        (
            "folder_that_cannot_be_used_shows_error",
            folder_that_cannot_be_used_shows_error,
        ),
        (
            "other_folder_button_returns_to_setup",
            other_folder_button_returns_to_setup,
        ),
        (
            "configured_diary_starts_at_unlock",
            configured_diary_starts_at_unlock,
        ),
        ("wrong_password_is_refused", wrong_password_is_refused),
        ("unlock_shows_todays_entry", unlock_shows_todays_entry),
        (
            "missing_configured_folder_goes_back_to_setup",
            missing_configured_folder_goes_back_to_setup,
        ),
        (
            "typing_is_saved_after_a_pause",
            typing_is_saved_after_a_pause,
        ),
        ("header_hides_while_typing", header_hides_while_typing),
        (
            "view_follows_the_cursor_while_typing",
            view_follows_the_cursor_while_typing,
        ),
        (
            "a_free_line_stays_below_the_cursor",
            a_free_line_stays_below_the_cursor,
        ),
        ("closing_the_window_saves", closing_the_window_saves),
        (
            "previous_and_next_move_between_entries",
            previous_and_next_move_between_entries,
        ),
        ("yesterday_is_named", yesterday_is_named),
        (
            "calendar_marks_entries_and_opens_days",
            calendar_marks_entries_and_opens_days,
        ),
        ("search_finds_and_opens_days", search_finds_and_opens_days),
        (
            "search_can_be_left_with_escape_action",
            search_can_be_left_with_escape_action,
        ),
        ("lock_hides_the_text", lock_hides_the_text),
        ("conflicts_are_announced", conflicts_are_announced),
        ("save_errors_are_announced", save_errors_are_announced),
        (
            "font_size_is_applied_and_remembered",
            font_size_is_applied_and_remembered,
        ),
        (
            "preferences_show_folder_and_change_password",
            preferences_show_folder_and_change_password,
        ),
        (
            "changing_folder_locks_and_returns_to_setup",
            changing_folder_locks_and_returns_to_setup,
        ),
        (
            "preferences_dialog_and_about_open",
            preferences_dialog_and_about_open,
        ),
        ("fullscreen_toggles", fullscreen_toggles),
        (
            "window_keeps_working_without_a_handle",
            window_keeps_working_without_a_handle,
        ),
        ("application_opens_a_window", application_opens_a_window),
        (
            "system_context_uses_user_config_dir",
            system_context_uses_user_config_dir,
        ),
        ("date_titles", date_titles),
        (
            "paths_under_home_are_shortened",
            paths_under_home_are_shortened,
        ),
    ];
    let filter = std::env::args().nth(1).filter(|a| !a.starts_with('-'));
    let mut ran = 0;
    for (name, test) in tests {
        if filter.as_deref().is_some_and(|f| !name.contains(f)) {
            continue;
        }
        test();
        settle();
        println!("ui test {name} ... ok");
        ran += 1;
    }
    println!("ui: {ran} passed");
}
