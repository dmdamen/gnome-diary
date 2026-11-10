//! Search across all entries.

use std::cell::RefCell;

use adw::prelude::*;
use chrono::NaiveDate;
use diary_core::search::Hit;

use crate::window::day_title;

pub struct SearchPage {
    pub root: adw::ToolbarView,
    pub entry: gtk::SearchEntry,
    pub results: gtk::ListBox,
    pub empty: adw::StatusPage,
    dates: RefCell<Vec<NaiveDate>>,
}

impl SearchPage {
    pub fn new() -> Self {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Search your diary")
            .hexpand(true)
            .build();
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(
            &adw::Clamp::builder()
                .maximum_size(400)
                .child(&entry)
                .build(),
        ));
        let results = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .valign(gtk::Align::Start)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(12)
            .margin_end(12)
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("edit-find-symbolic")
            .title("No Entries Found")
            .visible(false)
            .vexpand(true)
            .build();
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        column.append(
            &adw::Clamp::builder()
                .maximum_size(720)
                .child(&results)
                .build(),
        );
        column.append(&empty);
        let root = adw::ToolbarView::new();
        root.add_top_bar(&header);
        root.set_content(Some(
            &gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Never)
                .child(&column)
                .build(),
        ));
        SearchPage {
            root,
            entry,
            results,
            empty,
            dates: RefCell::new(Vec::new()),
        }
    }

    /// Shows `hits`; `query_given` tells an empty result from no search.
    pub fn show_hits(&self, hits: &[Hit], query_given: bool) {
        self.results.remove_all();
        for hit in hits {
            let row = adw::ActionRow::builder()
                .title(day_title(hit.date))
                .subtitle(glib_escape(&hit.snippet))
                .activatable(true)
                .build();
            self.results.append(&row);
        }
        self.results.set_visible(!hits.is_empty());
        self.empty.set_visible(query_given && hits.is_empty());
        *self.dates.borrow_mut() = hits.iter().map(|h| h.date).collect();
    }

    pub fn date_at(&self, index: i32) -> Option<NaiveDate> {
        self.dates
            .borrow()
            .get(usize::try_from(index).ok()?)
            .copied()
    }

    pub fn result_count(&self) -> u32 {
        self.results.observe_children().n_items()
    }
}

impl Default for SearchPage {
    fn default() -> Self {
        Self::new()
    }
}

fn glib_escape(text: &str) -> String {
    gtk::glib::markup_escape_text(text).to_string()
}
