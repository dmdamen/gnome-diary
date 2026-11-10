//! First run: choose where the diary lives.

use adw::prelude::*;

pub struct SetupPage {
    pub root: adw::ToolbarView,
    pub choose: gtk::Button,
}

impl SetupPage {
    pub fn new() -> Self {
        let choose = gtk::Button::builder()
            .label("Choose Folder…")
            .halign(gtk::Align::Center)
            .css_classes(["suggested-action", "pill"])
            .build();
        let status = adw::StatusPage::builder()
            .icon_name("accessories-text-editor-symbolic")
            .title("Welcome to Diary")
            .description(
                "Choose a folder for your diary. Pick one that your online storage \
                 syncs, and your diary is backed up, encrypted.",
            )
            .child(&choose)
            .build();
        SetupPage {
            root: page_with_header(&status),
            choose,
        }
    }
}

impl Default for SetupPage {
    fn default() -> Self {
        Self::new()
    }
}

/// A page with only the window controls above it.
pub(crate) fn page_with_header(content: &impl IsA<gtk::Widget>) -> adw::ToolbarView {
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::builder().show_title(false).build());
    view.set_content(Some(content));
    view
}
