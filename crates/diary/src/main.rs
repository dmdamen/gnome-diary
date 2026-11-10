fn main() -> gtk::glib::ExitCode {
    use gtk::prelude::*;
    diary::application::build(Box::new(diary::window::Context::system)).run()
}
