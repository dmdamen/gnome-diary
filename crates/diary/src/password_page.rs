//! Choosing a password for a new diary, or unlocking an existing one.

use std::cell::Cell;
use std::path::Path;

use adw::prelude::*;
use diary_app::password::{self, PasswordProblem};

use crate::setup_page::page_with_header;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Create,
    Unlock,
}

pub struct PasswordPage {
    pub root: adw::ToolbarView,
    pub status: adw::StatusPage,
    pub password: adw::PasswordEntryRow,
    pub confirm: adw::PasswordEntryRow,
    pub problem: gtk::Label,
    pub submit: gtk::Button,
    pub other_folder: gtk::Button,
    mode: Cell<Mode>,
    busy: Cell<bool>,
}

impl PasswordPage {
    pub fn new() -> Self {
        let password = adw::PasswordEntryRow::builder().title("Password").build();
        let confirm = adw::PasswordEntryRow::builder()
            .title("Repeat Password")
            .build();
        let rows = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        rows.append(&password);
        rows.append(&confirm);
        let problem = gtk::Label::builder()
            .css_classes(["error"])
            .wrap(true)
            .visible(false)
            .build();
        let submit = gtk::Button::builder()
            .halign(gtk::Align::Center)
            .css_classes(["suggested-action", "pill"])
            .build();
        let other_folder = gtk::Button::builder()
            .label("Use Another Folder")
            .halign(gtk::Align::Center)
            .css_classes(["flat"])
            .build();
        let column = gtk::Box::new(gtk::Orientation::Vertical, 18);
        for w in [
            rows.upcast_ref::<gtk::Widget>(),
            problem.upcast_ref(),
            submit.upcast_ref(),
            other_folder.upcast_ref(),
        ] {
            column.append(w);
        }
        let clamp = adw::Clamp::builder()
            .maximum_size(400)
            .child(&column)
            .build();
        let status = adw::StatusPage::builder()
            .icon_name("system-lock-screen-symbolic")
            .child(&clamp)
            .build();
        PasswordPage {
            root: page_with_header(&status),
            status,
            password,
            confirm,
            problem,
            submit,
            other_folder,
            mode: Cell::new(Mode::Unlock),
            busy: Cell::new(false),
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode.get()
    }

    /// Resets the page for `folder`.
    pub fn show(&self, mode: Mode, folder: &Path) {
        self.mode.set(mode);
        self.clear();
        let place = folder.display();
        match mode {
            Mode::Create => {
                self.status.set_title("Choose a Password");
                self.status.set_description(Some(&format!(
                    "Your diary in {place} is encrypted with this password. \
                     It cannot be recovered if you forget it."
                )));
            }
            Mode::Unlock => {
                self.status.set_title("Diary Locked");
                self.status
                    .set_description(Some(&format!("Enter the password for {place}")));
            }
        }
        self.confirm.set_visible(mode == Mode::Create);
        self.set_busy(false);
    }

    /// Empties the password fields.
    pub fn clear(&self) {
        self.password.set_text("");
        self.confirm.set_text("");
        self.problem.set_visible(false);
    }

    pub fn set_busy(&self, busy: bool) {
        self.busy.set(busy);
        let label = match (self.mode.get(), busy) {
            (Mode::Create, false) => "Create Diary",
            (Mode::Create, true) => "Creating…",
            (Mode::Unlock, false) => "Unlock",
            (Mode::Unlock, true) => "Unlocking…",
        };
        self.submit.set_label(label);
        self.password.set_sensitive(!busy);
        self.confirm.set_sensitive(!busy);
        self.validate();
    }

    /// Updates the button and hint after typing.
    pub fn validate(&self) {
        let password = self.password.text();
        let confirm = self.confirm.text();
        let (ready, problem) = match self.mode.get() {
            Mode::Unlock => (!password.is_empty(), None),
            Mode::Create => match password::check_new(&password, &confirm) {
                Err(PasswordProblem::TooShort) if password.is_empty() => (false, None),
                Err(problem) => (false, Some(problem.message())),
                Ok(()) => (!confirm.is_empty(), None),
            },
        };
        self.submit.set_sensitive(ready && !self.busy.get());
        match problem {
            Some(text) => self.show_problem(text),
            None => self.problem.set_visible(false),
        }
    }

    pub fn show_problem(&self, text: &str) {
        self.problem.set_label(text);
        self.problem.set_visible(true);
    }
}

impl Default for PasswordPage {
    fn default() -> Self {
        Self::new()
    }
}
