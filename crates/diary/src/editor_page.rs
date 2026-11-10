//! The writing view: a clean page of text, with a header that steps aside
//! while you type.

use std::cell::Cell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::gio;

const TEXT_WIDTH: i32 = 720;
const TEXT_TIGHTENING: i32 = 560;

pub struct EditorPage {
    pub root: adw::ToolbarView,
    pub toolbar: adw::ToolbarView,
    pub title: adw::WindowTitle,
    pub previous: gtk::Button,
    pub next: gtk::Button,
    pub calendar_button: gtk::MenuButton,
    pub calendar: gtk::Calendar,
    pub scroller: gtk::ScrolledWindow,
    pub text: gtk::TextView,
    pub placeholder: gtk::Label,
    last_pointer: Rc<Cell<(f64, f64)>>,
}

impl EditorPage {
    pub fn new() -> Self {
        let title = adw::WindowTitle::new("", "");
        let previous = icon_button("go-previous-symbolic", "Previous Entry", "win.previous");
        let next = icon_button("go-next-symbolic", "Next Entry", "win.next");
        let calendar = gtk::Calendar::new();
        let calendar_button = gtk::MenuButton::builder()
            .icon_name("x-office-calendar-symbolic")
            .tooltip_text("Choose a Day")
            .popover(&gtk::Popover::builder().child(&calendar).build())
            .build();

        let menu = gio::Menu::new();
        let main = gio::Menu::new();
        main.append(Some("Search"), Some("win.search"));
        main.append(Some("Go to Today"), Some("win.today"));
        main.append(Some("Lock"), Some("win.lock"));
        menu.append_section(None, &main);
        let app = gio::Menu::new();
        app.append(Some("Preferences"), Some("win.preferences"));
        app.append(Some("About Diary"), Some("win.about"));
        menu.append_section(None, &app);
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text("Main Menu")
            .menu_model(&menu)
            .primary(true)
            .build();

        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&title));
        header.pack_start(&previous);
        header.pack_start(&calendar_button);
        header.pack_start(&next);
        header.pack_end(&menu_button);

        let text = gtk::TextView::builder()
            .wrap_mode(gtk::WrapMode::WordChar)
            .top_margin(72)
            .bottom_margin(120)
            .left_margin(24)
            .right_margin(24)
            .pixels_below_lines(6)
            .css_classes(["diary-text"])
            .vexpand(true)
            .build();
        text.update_property(&[gtk::accessible::Property::Label("Diary entry")]);
        let placeholder = gtk::Label::builder()
            .label("Write about your day…")
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .margin_top(72)
            .margin_start(24)
            .can_target(false)
            .css_classes(["diary-placeholder"])
            .build();
        // ClampScrollable hands the scroll position to the text view itself,
        // so the view follows the cursor while typing.
        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(
                &adw::ClampScrollable::builder()
                    .maximum_size(TEXT_WIDTH)
                    .tightening_threshold(TEXT_TIGHTENING)
                    .child(&text)
                    .build(),
            )
            .build();
        // The placeholder floats above, clamped the same way so it lines up.
        let overlay = gtk::Overlay::builder().child(&scroller).build();
        overlay.add_overlay(
            &adw::Clamp::builder()
                .maximum_size(TEXT_WIDTH)
                .tightening_threshold(TEXT_TIGHTENING)
                .valign(gtk::Align::Start)
                .can_target(false)
                .child(&placeholder)
                .build(),
        );

        let toolbar = adw::ToolbarView::builder()
            .top_bar_style(adw::ToolbarStyle::Flat)
            .extend_content_to_top_edge(true)
            .build();
        toolbar.add_top_bar(&header);
        toolbar.set_content(Some(&overlay));

        let motion = gtk::EventControllerMotion::new();
        toolbar.add_controller(motion.clone());

        let page = EditorPage {
            root: toolbar.clone(),
            toolbar,
            title,
            previous,
            next,
            calendar_button,
            calendar,
            scroller,
            text,
            placeholder,
            last_pointer: Rc::new(Cell::new((f64::NAN, f64::NAN))),
        };
        let view = page.text.downgrade();
        // GTK scrolls the cursor just into view after each keystroke; run
        // afterwards (on idle) so the free line wins.
        // Typing moves the cursor without a mark-set signal, so watch text
        // changes as well as explicit cursor moves (arrows, clicks).
        let later = move || {
            let view = view.clone();
            gtk::glib::idle_add_local_once(move || {
                if let Some(view) = view.upgrade() {
                    keep_free_line_around_cursor(&view);
                }
            });
        };
        let buffer = page.text.buffer();
        buffer.connect_changed({
            let later = later.clone();
            move |_| later()
        });
        buffer.connect_mark_set(move |buffer, _, mark| {
            if *mark == buffer.get_insert() {
                later();
            }
        });
        let toolbar = page.toolbar.clone();
        let last = page.last_pointer.clone();
        motion.connect_motion(move |_, x, y| {
            if moved(&last, x, y) {
                toolbar.set_reveal_top_bars(true);
            }
        });
        page
    }

    /// The text changed: get the header out of the way.
    pub fn typing(&self, empty: bool) {
        self.toolbar.set_reveal_top_bars(false);
        self.placeholder.set_visible(empty);
    }

    /// The mouse moved: bring the header back.
    pub fn pointer_moved(&self, x: f64, y: f64) {
        if moved(&self.last_pointer, x, y) {
            self.toolbar.set_reveal_top_bars(true);
        }
    }
}

impl Default for EditorPage {
    fn default() -> Self {
        Self::new()
    }
}

/// GTK also reports "motion" when widgets move under a still pointer, so
/// only count real movement.
fn moved(last: &Cell<(f64, f64)>, x: f64, y: f64) -> bool {
    let (lx, ly) = last.replace((x, y));
    lx.is_nan() || (x - lx).abs() + (y - ly).abs() > 2.0
}

fn icon_button(icon: &str, tooltip: &str, action: &str) -> gtk::Button {
    gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .action_name(action)
        .build()
}

/// Scrolls so the cursor stays at least one line away from the top and
/// bottom edges, instead of writing on the very last visible line.
fn keep_free_line_around_cursor(view: &gtk::TextView) {
    let page = view.vadjustment().map_or(0.0, |a| a.page_size());
    if page <= 0.0 {
        return;
    }
    let buffer = view.buffer();
    let line = f64::from(
        view.iter_location(&buffer.iter_at_mark(&buffer.get_insert()))
            .height(),
    );
    // GTK expresses the margin as a fraction of the visible height.
    let margin = (line / page).clamp(0.0, 0.45);
    view.scroll_to_mark(&buffer.get_insert(), margin, false, 0.0, 0.0);
}
