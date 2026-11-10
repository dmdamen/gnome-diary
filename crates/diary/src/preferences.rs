//! The preferences dialog: text size, diary folder, password.

use adw::prelude::*;
use diary_app::config::{MAX_FONT_SIZE, MIN_FONT_SIZE};

pub struct Preferences {
    pub dialog: adw::PreferencesDialog,
    pub font_size: adw::SpinRow,
    pub folder: adw::ActionRow,
    pub change_folder: gtk::Button,
    pub new_password: adw::PasswordEntryRow,
    pub confirm_password: adw::PasswordEntryRow,
    pub change_password: gtk::Button,
}

impl Preferences {
    pub fn new(font_size: u32) -> Self {
        let size =
            adw::SpinRow::with_range(f64::from(MIN_FONT_SIZE), f64::from(MAX_FONT_SIZE), 1.0);
        size.set_title("Text Size");
        size.set_value(f64::from(font_size));
        let writing = adw::PreferencesGroup::builder().title("Writing").build();
        writing.add(&size);

        let change_folder = gtk::Button::builder()
            .label("Change…")
            .valign(gtk::Align::Center)
            .build();
        let folder = adw::ActionRow::builder()
            .title("Diary Folder")
            .subtitle_selectable(true)
            .build();
        folder.add_suffix(&change_folder);
        let storage = adw::PreferencesGroup::builder()
            .title("Storage")
            .description("Changing the folder locks the diary; your entries stay where they are.")
            .build();
        storage.add(&folder);

        let new_password = adw::PasswordEntryRow::builder()
            .title("New Password")
            .build();
        let confirm_password = adw::PasswordEntryRow::builder()
            .title("Repeat New Password")
            .build();
        let change_password = gtk::Button::builder()
            .label("Change Password")
            .halign(gtk::Align::End)
            .margin_top(12)
            .build();
        let security = adw::PreferencesGroup::builder()
            .title("Password")
            .description(
                "Only the key file is re-encrypted; other devices need the new password too.",
            )
            .build();
        security.add(&new_password);
        security.add(&confirm_password);
        security.add(&change_password);

        let page = adw::PreferencesPage::new();
        page.add(&writing);
        page.add(&storage);
        page.add(&security);
        let dialog = adw::PreferencesDialog::new();
        dialog.add(&page);

        Preferences {
            dialog,
            font_size: size,
            folder,
            change_folder,
            new_password,
            confirm_password,
            change_password,
        }
    }
}
