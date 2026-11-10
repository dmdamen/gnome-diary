//! The main window: moves between the setup, password, editor and search
//! pages, and connects the widgets to the [`Session`].

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use chrono::{Datelike, NaiveDate};
use diary_app::config::Config;
use diary_app::password;
use diary_app::session::{Notice, Session};
use diary_core::clock::{Clock, SystemClock};
use diary_core::crypto::{CryptoError, KdfCost};
use diary_core::secrecy::SecretString;
use diary_core::store::{self, Diary, StoreError};
use gtk::{gio, glib};

use crate::APP_ID;
use crate::editor_page::EditorPage;
use crate::password_page::{Mode, PasswordPage};
use crate::preferences::Preferences;
use crate::search_page::SearchPage;
use crate::setup_page::SetupPage;

/// Everything the window needs from the outside world.
#[derive(Clone)]
pub struct Context {
    pub config_file: PathBuf,
    /// This computer's name, used in sync conflict copies.
    pub device: String,
    pub kdf: KdfCost,
    pub clock: Rc<dyn Clock>,
}

impl Context {
    pub fn system() -> Self {
        Context {
            config_file: glib::user_config_dir().join("diary").join("settings"),
            device: diary_app::device::device_name(Path::new("/proc/sys/kernel/hostname")),
            kdf: KdfCost::default(),
            clock: Rc::new(SystemClock),
        }
    }
}

/// "Tuesday 10 November 2026", in the user's language.
pub fn day_title(date: NaiveDate) -> String {
    glib_date(date)
        .format("%A %-d %B %Y")
        .map(|s| s.to_string())
        .unwrap_or_else(|_| date.to_string())
}

pub fn day_subtitle(date: NaiveDate, today: NaiveDate) -> &'static str {
    if date == today {
        "Today"
    } else if date.succ_opt() == Some(today) {
        "Yesterday"
    } else {
        ""
    }
}

fn glib_date(date: NaiveDate) -> glib::DateTime {
    glib::DateTime::from_local(
        date.year(),
        date.month() as i32,
        date.day() as i32,
        12,
        0,
        0.0,
    )
    .expect("valid date")
}

const ACTIONS_NEEDING_SESSION: &[&str] = &[
    "previous",
    "next",
    "today",
    "calendar",
    "search",
    "lock",
    "preferences",
];

#[derive(Clone)]
pub struct DiaryWindow {
    inner: Rc<Inner>,
}

struct Inner {
    ctx: Context,
    window: adw::ApplicationWindow,
    stack: adw::ViewStack,
    toasts: adw::ToastOverlay,
    setup: SetupPage,
    password: PasswordPage,
    editor: EditorPage,
    search: SearchPage,
    prefs: Preferences,
    config: RefCell<Config>,
    session: RefCell<Option<Session>>,
    font_css: gtk::CssProvider,
    last_message: RefCell<Option<String>>,
    autosave: RefCell<Option<glib::SourceId>>,
    /// Set while the code itself changes the text or calendar, so those
    /// changes are not mistaken for the user's.
    syncing: Cell<bool>,
}

struct WeakWindow(Weak<Inner>);

impl WeakWindow {
    fn upgrade(&self) -> Option<DiaryWindow> {
        self.0.upgrade().map(|inner| DiaryWindow { inner })
    }
}

impl DiaryWindow {
    pub fn new(ctx: Context) -> Self {
        let config = Config::load(&ctx.config_file);
        let window = adw::ApplicationWindow::builder()
            .title("Diary")
            .icon_name(APP_ID)
            .default_width(760)
            .default_height(720)
            .build();
        let stack = adw::ViewStack::new();
        let setup = SetupPage::new();
        let password = PasswordPage::new();
        let editor = EditorPage::new();
        let search = SearchPage::new();
        stack.add_named(&setup.root, Some("setup"));
        stack.add_named(&password.root, Some("password"));
        stack.add_named(&editor.root, Some("editor"));
        stack.add_named(&search.root, Some("search"));
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&stack));
        window.set_content(Some(&toasts));

        let display = gtk::prelude::WidgetExt::display(&window);
        let style = gtk::CssProvider::new();
        style.load_from_string(include_str!("style.css"));
        gtk::style_context_add_provider_for_display(
            &display,
            &style,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let font_css = gtk::CssProvider::new();
        gtk::style_context_add_provider_for_display(
            &display,
            &font_css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );

        let prefs = Preferences::new(config.font_size);
        let this = DiaryWindow {
            inner: Rc::new(Inner {
                ctx,
                window,
                stack,
                toasts,
                setup,
                password,
                editor,
                search,
                prefs,
                config: RefCell::new(config),
                session: RefCell::new(None),
                font_css,
                last_message: RefCell::new(None),
                autosave: RefCell::new(None),
                syncing: Cell::new(false),
            }),
        };
        this.apply_font_size();
        this.add_actions();
        this.connect_signals();
        this.set_unlocked(false);

        let folder = this.inner.config.borrow().folder.clone();
        match folder {
            Some(folder) if folder.is_dir() => this.folder_chosen(&folder),
            _ => this.show_page("setup"),
        }
        this
    }

    pub fn window(&self) -> &adw::ApplicationWindow {
        &self.inner.window
    }
    pub fn setup_page(&self) -> &SetupPage {
        &self.inner.setup
    }
    pub fn password_page(&self) -> &PasswordPage {
        &self.inner.password
    }
    pub fn editor_page(&self) -> &EditorPage {
        &self.inner.editor
    }
    pub fn search_page(&self) -> &SearchPage {
        &self.inner.search
    }
    pub fn preferences(&self) -> &Preferences {
        &self.inner.prefs
    }

    pub fn visible_page(&self) -> String {
        self.inner
            .stack
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_default()
    }

    /// The last message shown to the user (as a toast).
    pub fn last_message(&self) -> Option<String> {
        self.inner.last_message.borrow().clone()
    }

    pub fn font_css(&self) -> String {
        self.inner.font_css.to_str().to_string()
    }

    fn downgrade(&self) -> WeakWindow {
        WeakWindow(Rc::downgrade(&self.inner))
    }

    /// A signal handler (for a signal whose only argument is the emitting
    /// `T`) that holds only a weak reference to the window.
    fn on<T: ?Sized>(&self, f: impl Fn(&DiaryWindow) + 'static) -> impl Fn(&T) + 'static {
        let weak = self.downgrade();
        move |_| {
            if let Some(window) = weak.upgrade() {
                f(&window);
            }
        }
    }

    fn show_page(&self, name: &str) {
        self.inner.stack.set_visible_child_name(name);
    }

    fn message(&self, text: String) {
        self.inner.toasts.add_toast(adw::Toast::new(&text));
        *self.inner.last_message.borrow_mut() = Some(text);
    }

    // Setup and unlocking --------------------------------------------------

    /// The user picked the folder for the diary.
    pub fn folder_chosen(&self, folder: &Path) {
        {
            let mut config = self.inner.config.borrow_mut();
            config.folder = Some(folder.to_owned());
            if let Err(e) = config.save() {
                drop(config);
                self.message(format!("Could not save settings: {e}"));
            }
        }
        let mode = if store::is_diary(folder) {
            Mode::Unlock
        } else {
            Mode::Create
        };
        self.inner.password.show(mode, folder);
        self.show_page("password");
        self.inner.password.password.grab_focus();
    }

    fn ask_for_folder(&self) {
        let dialog = gtk::FileDialog::builder()
            .title("Choose a Folder for Your Diary")
            .modal(true)
            .build();
        let weak = self.downgrade();
        dialog.select_folder(
            Some(&self.inner.window),
            gio::Cancellable::NONE,
            move |result| {
                if let (Some(window), Ok(file)) = (weak.upgrade(), result)
                    && let Some(path) = file.path()
                {
                    window.folder_chosen(&path);
                }
            },
        );
    }

    fn submit_password(&self) {
        let page = &self.inner.password;
        if !page.submit.is_sensitive() {
            return;
        }
        let Some(folder) = self.inner.config.borrow().folder.clone() else {
            return;
        };
        let password = SecretString::from(page.password.text().to_string());
        let create = page.mode() == Mode::Create;
        let kdf = self.inner.ctx.kdf;
        let device = self.inner.ctx.device.clone();
        page.set_busy(true);
        let weak = self.downgrade();
        glib::spawn_future_local(async move {
            let result = gio::spawn_blocking(move || {
                if create {
                    Diary::create(&folder, &password, kdf, &device)
                } else {
                    Diary::open(&folder, &password, &device)
                }
            })
            .await
            .expect("unlock thread");
            if let Some(window) = weak.upgrade() {
                window.unlocked(result);
            }
        });
    }

    fn unlocked(&self, result: Result<Diary, StoreError>) {
        let page = &self.inner.password;
        page.set_busy(false);
        let clock: Box<dyn Clock> = Box::new(self.inner.ctx.clock.clone());
        match result.and_then(|diary| Session::start(diary, clock)) {
            Ok(session) => {
                page.clear();
                self.inner
                    .prefs
                    .folder
                    .set_subtitle(&display_path(session.diary().root()));
                *self.inner.session.borrow_mut() = Some(session);
                self.set_unlocked(true);
                self.refresh_editor();
                self.show_page("editor");
                self.inner.editor.text.grab_focus();
            }
            Err(e) => page.show_problem(&problem_text(&e)),
        }
    }

    fn set_unlocked(&self, unlocked: bool) {
        for name in ACTIONS_NEEDING_SESSION {
            self.set_action_enabled(name, unlocked);
        }
    }

    fn set_action_enabled(&self, name: &str, enabled: bool) {
        if let Some(action) = self.inner.window.lookup_action(name)
            && let Ok(action) = action.downcast::<gio::SimpleAction>()
        {
            action.set_enabled(enabled);
        }
    }

    /// Saves and forgets the unlocked diary. Refuses (and says why) when
    /// the text cannot be saved.
    fn lock(&self) -> bool {
        if !self.save_now() {
            return false;
        }
        if let Some(id) = self.inner.autosave.take() {
            id.remove();
        }
        if self.inner.session.take().is_none() {
            return true;
        }
        self.inner.syncing.set(true);
        self.inner.editor.text.buffer().set_text("");
        self.inner.syncing.set(false);
        self.inner.editor.calendar.clear_marks();
        self.set_unlocked(false);
        let folder = self.inner.config.borrow().folder.clone();
        match folder {
            Some(folder) => self.folder_chosen(&folder),
            None => self.show_page("setup"),
        }
        true
    }

    // Writing ------------------------------------------------------------

    /// Shows the session's current day in the editor.
    fn refresh_editor(&self) {
        {
            let guard = self.inner.session.borrow();
            let Some(session) = guard.as_ref() else {
                return;
            };
            let editor = &self.inner.editor;
            let body = &session.entry().body;
            self.inner.syncing.set(true);
            editor.text.buffer().set_text(body);
            editor.calendar.select_day(&glib_date(session.date()));
            self.inner.syncing.set(false);
            editor.placeholder.set_visible(body.is_empty());
            editor.title.set_title(&day_title(session.date()));
            editor
                .title
                .set_subtitle(day_subtitle(session.date(), session.today()));
            editor.toolbar.set_reveal_top_bars(true);
        }
        self.refresh_navigation();
    }

    /// Updates previous/next and the calendar marks after days changed.
    fn refresh_navigation(&self) {
        let session = self.inner.session.borrow();
        let Some(session) = session.as_ref() else {
            return;
        };
        self.set_action_enabled("previous", session.previous().is_some());
        self.set_action_enabled("next", session.next().is_some());
        let calendar = &self.inner.editor.calendar;
        let shown = calendar.date();
        calendar.clear_marks();
        for date in session.dates() {
            if date.year() == shown.year() && date.month() as i32 == shown.month() {
                calendar.mark_day(date.day());
            }
        }
    }

    fn text_changed(&self) {
        if self.inner.syncing.get() {
            return;
        }
        let buffer = self.inner.editor.text.buffer();
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        if let Some(session) = self.inner.session.borrow_mut().as_mut() {
            session.edit(&text);
        }
        self.inner.editor.typing(text.is_empty());
        self.start_autosave();
    }

    fn start_autosave(&self) {
        if self.inner.autosave.borrow().is_some() {
            return;
        }
        let weak = self.downgrade();
        let id = glib::timeout_add_local(Duration::from_millis(250), move || {
            let Some(window) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let result = window
                .inner
                .session
                .borrow_mut()
                .as_mut()
                .map(Session::tick);
            if let Some(result) = result {
                window.report(result, "save");
            }
            let dirty = window
                .inner
                .session
                .borrow()
                .as_ref()
                .is_some_and(Session::is_dirty);
            if dirty {
                glib::ControlFlow::Continue
            } else {
                window.inner.autosave.take();
                glib::ControlFlow::Break
            }
        });
        *self.inner.autosave.borrow_mut() = Some(id);
    }

    /// Saves unsaved text now. Returns false (after telling the user) if
    /// that failed.
    fn save_now(&self) -> bool {
        let result = self.inner.session.borrow_mut().as_mut().map(Session::save);
        match result {
            Some(result) => self.report(result, "save"),
            None => true,
        }
    }

    /// Tells the user about save/open results. Returns whether it worked.
    fn report(&self, result: Result<Option<Notice>, StoreError>, verb: &str) -> bool {
        let ok = match result {
            Ok(None) => true,
            Ok(Some(Notice::ConflictKept(copy))) => {
                let name = copy.file_name().unwrap_or_default().to_string_lossy();
                self.message(format!(
                    "This day was also changed on another device. That version was kept as {name}"
                ));
                true
            }
            Err(e) if verb == "save" => {
                self.message(format!("Could not save: {e}"));
                false
            }
            Err(e) => {
                self.message(format!("Could not open this day: {e}"));
                false
            }
        };
        self.refresh_navigation();
        ok
    }

    fn open_day(&self, date: NaiveDate) {
        if !self.save_now() {
            return;
        }
        let result = self
            .inner
            .session
            .borrow_mut()
            .as_mut()
            .map(|s| s.open(date));
        if let Some(result) = result {
            self.report(result, "open");
        }
        self.refresh_editor();
        self.show_page("editor");
    }

    fn step(&self, forward: bool) {
        let target = self
            .inner
            .session
            .borrow()
            .as_ref()
            .and_then(|s| if forward { s.next() } else { s.previous() });
        if let Some(date) = target {
            self.open_day(date);
        }
    }

    fn calendar_day_selected(&self) {
        if self.inner.syncing.get() {
            return;
        }
        let picked = self.inner.editor.calendar.date();
        let date = NaiveDate::from_ymd_opt(
            picked.year(),
            picked.month() as u32,
            picked.day_of_month() as u32,
        )
        .expect("calendar date");
        self.inner.editor.calendar_button.popdown();
        self.open_day(date);
    }

    // Search -------------------------------------------------------------

    fn show_search(&self) {
        let search = &self.inner.search;
        search.entry.set_text("");
        search.show_hits(&[], false);
        self.show_page("search");
        search.entry.grab_focus();
    }

    fn run_search(&self) {
        let query = self.inner.search.entry.text();
        let result = self
            .inner
            .session
            .borrow_mut()
            .as_mut()
            .map(|s| s.search(&query));
        match result {
            Some(Ok(hits)) => self.inner.search.show_hits(&hits, !query.trim().is_empty()),
            Some(Err(e)) => self.message(format!("Search failed: {e}")),
            None => {}
        }
    }

    // Preferences --------------------------------------------------------

    fn apply_font_size(&self) {
        let size = self.inner.config.borrow().font_size;
        self.inner.font_css.load_from_string(&format!(
            ".diary-text, .diary-placeholder {{ font-size: {size}pt; }}"
        ));
    }

    fn font_size_changed(&self) {
        let size = self.inner.prefs.font_size.value() as u32;
        let result = {
            let mut config = self.inner.config.borrow_mut();
            config.set_font_size(size);
            config.save()
        };
        if let Err(e) = result {
            self.message(format!("Could not save settings: {e}"));
        }
        self.apply_font_size();
    }

    fn change_folder(&self) {
        if self.inner.prefs.dialog.parent().is_some() {
            self.inner.prefs.dialog.close();
        }
        if !self.lock() {
            return;
        }
        let result = {
            let mut config = self.inner.config.borrow_mut();
            config.folder = None;
            config.save()
        };
        if let Err(e) = result {
            self.message(format!("Could not save settings: {e}"));
        }
        self.show_page("setup");
    }

    fn change_password(&self) {
        let prefs = &self.inner.prefs;
        let new = prefs.new_password.text();
        let confirm = prefs.confirm_password.text();
        let problem = match password::check_new(&new, &confirm) {
            Err(problem) => Some(problem),
            Ok(()) if confirm.is_empty() => Some(password::PasswordProblem::Mismatch),
            Ok(()) => None,
        };
        if let Some(problem) = problem {
            self.message(problem.message().to_owned());
            return;
        }
        let result = self.inner.session.borrow().as_ref().map(|s| {
            s.diary()
                .change_password(&SecretString::from(new.to_string()), self.inner.ctx.kdf)
        });
        match result {
            Some(Ok(())) => {
                prefs.new_password.set_text("");
                prefs.confirm_password.set_text("");
                self.message("Password changed".into());
            }
            Some(Err(e)) => self.message(format!("Could not change the password: {e}")),
            None => {}
        }
    }

    fn show_about(&self) {
        adw::AboutDialog::builder()
            .application_name("Diary")
            .application_icon(APP_ID)
            .version(env!("CARGO_PKG_VERSION"))
            .comments("A distraction-free diary, encrypted and kept in plain files you can sync.")
            .license_type(gtk::License::Gpl30)
            .build()
            .present(Some(&self.inner.window));
    }

    // Wiring -------------------------------------------------------------

    fn add_actions(&self) {
        let action = |name: &str, f: fn(&DiaryWindow)| {
            let a = gio::SimpleAction::new(name, None);
            let weak = self.downgrade();
            a.connect_activate(move |_, _| {
                if let Some(w) = weak.upgrade() {
                    f(&w);
                }
            });
            self.inner.window.add_action(&a);
        };
        action("previous", |w| w.step(false));
        action("next", |w| w.step(true));
        action("today", |w| {
            let today = w.inner.session.borrow().as_ref().map(Session::today);
            if let Some(today) = today {
                w.open_day(today);
            }
        });
        action("calendar", |w| w.inner.editor.calendar_button.popup());
        action("search", DiaryWindow::show_search);
        action("lock", |w| {
            w.lock();
        });
        action("preferences", |w| {
            w.inner.prefs.dialog.present(Some(&w.inner.window));
        });
        action("about", DiaryWindow::show_about);
        action("fullscreen", |w| {
            let window = &w.inner.window;
            window.set_fullscreened(!window.is_fullscreen());
        });
    }

    fn connect_signals(&self) {
        let inner = &self.inner;
        inner
            .setup
            .choose
            .connect_clicked(self.on::<gtk::Button>(|w| w.ask_for_folder()));

        let page = &inner.password;
        for row in [&page.password, &page.confirm] {
            row.connect_changed(self.on::<adw::PasswordEntryRow>(|w| w.inner.password.validate()));
            row.connect_entry_activated(self.on::<adw::PasswordEntryRow>(|w| w.submit_password()));
        }
        page.submit
            .connect_clicked(self.on::<gtk::Button>(|w| w.submit_password()));
        page.other_folder
            .connect_clicked(self.on::<gtk::Button>(|w| w.show_page("setup")));

        let editor = &inner.editor;
        editor
            .text
            .buffer()
            .connect_changed(self.on::<gtk::TextBuffer>(|w| w.text_changed()));
        editor
            .calendar
            .connect_day_selected(self.on::<gtk::Calendar>(|w| w.calendar_day_selected()));
        let month_changed = || self.on::<gtk::Calendar>(DiaryWindow::refresh_navigation);
        editor.calendar.connect_prev_month(month_changed());
        editor.calendar.connect_next_month(month_changed());
        editor.calendar.connect_prev_year(month_changed());
        editor.calendar.connect_next_year(month_changed());

        let search = &inner.search;
        search
            .entry
            .connect_search_changed(self.on::<gtk::SearchEntry>(|w| w.run_search()));
        search
            .entry
            .connect_stop_search(self.on::<gtk::SearchEntry>(|w| w.show_page("editor")));
        let weak = self.downgrade();
        search.results.connect_row_activated(move |_, row| {
            if let Some(w) = weak.upgrade()
                && let Some(date) = w.inner.search.date_at(row.index())
            {
                w.open_day(date);
            }
        });

        let prefs = &inner.prefs;
        prefs
            .font_size
            .connect_value_notify(self.on::<adw::SpinRow>(|w| w.font_size_changed()));
        prefs
            .change_folder
            .connect_clicked(self.on::<gtk::Button>(|w| w.change_folder()));
        prefs
            .change_password
            .connect_clicked(self.on::<gtk::Button>(|w| w.change_password()));

        let weak = self.downgrade();
        inner
            .window
            .connect_close_request(move |_| match weak.upgrade() {
                Some(w) if !w.save_now() => glib::Propagation::Stop,
                _ => glib::Propagation::Proceed,
            });
    }
}

fn problem_text(error: &StoreError) -> String {
    match error {
        StoreError::Crypto(CryptoError::WrongPassword) => "Wrong password".into(),
        StoreError::Crypto(CryptoError::BadKeyFile) => {
            "The diary's key file (.diary/identity.age) is damaged".into()
        }
        StoreError::Io(e) => format!("Could not use this folder: {e}"),
        other => other.to_string(),
    }
}

/// A folder as the user thinks of it: `~/Nextcloud/Diary`.
pub fn display_path(path: &Path) -> String {
    match path.strip_prefix(glib::home_dir()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}
