//! GTK user interface for Diary. The widgets stay thin: decisions are made
//! in `diary-app`, storage and encryption live in `diary-core`.

pub mod application;
pub mod editor_page;
pub mod password_page;
pub mod preferences;
pub mod search_page;
pub mod setup_page;
pub mod window;

pub const APP_ID: &str = "io.github.diary.Diary";
