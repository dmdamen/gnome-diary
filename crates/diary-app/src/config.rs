//! The app's own settings (not the diary): where the diary lives and the
//! text size. Kept in a small `key=value` file in the user's config folder,
//! which inside Flatpak is private to the app.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const DEFAULT_FONT_SIZE: u32 = 13;
pub const MIN_FONT_SIZE: u32 = 9;
pub const MAX_FONT_SIZE: u32 = 28;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    file: PathBuf,
    pub folder: Option<PathBuf>,
    pub font_size: u32,
}

impl Config {
    /// Reads the settings from `file`. A missing or unreadable file gives
    /// the defaults (so the app asks for a folder again).
    pub fn load(file: &Path) -> Self {
        let mut config = Self {
            file: file.to_owned(),
            folder: None,
            font_size: DEFAULT_FONT_SIZE,
        };
        let text = fs::read_to_string(file).unwrap_or_default();
        for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
            match (key.trim(), value.trim()) {
                ("folder", v) if !v.is_empty() => config.folder = Some(v.into()),
                ("font-size", v) => {
                    if let Ok(size) = v.parse() {
                        config.set_font_size(size);
                    }
                }
                _ => {}
            }
        }
        config
    }

    pub fn save(&self) -> io::Result<()> {
        let mut text = format!("font-size={}\n", self.font_size);
        if let Some(folder) = &self.folder {
            text += &format!("folder={}\n", folder.display());
        }
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&self.file, text)
    }

    /// Sets the font size, kept within the allowed range.
    pub fn set_font_size(&mut self, size: u32) {
        self.font_size = size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config::load(&dir.path().join("settings"));
        assert_eq!(c.folder, None);
        assert_eq!(c.font_size, DEFAULT_FONT_SIZE);
    }

    #[test]
    fn saves_and_loads_back() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("diary/settings");
        let mut c = Config::load(&file);
        c.folder = Some("/home/me/Nextcloud/Diary = mine".into());
        c.set_font_size(16);
        c.save().unwrap();
        assert_eq!(Config::load(&file), c);
    }

    #[test]
    fn unknown_keys_and_bad_values_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings");
        fs::write(&file, "colour=blue\nfont-size=huge\nnonsense\nfolder=\n").unwrap();
        let c = Config::load(&file);
        assert_eq!(c.font_size, DEFAULT_FONT_SIZE);
        assert_eq!(c.folder, None);
    }

    #[test]
    fn font_size_is_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = Config::load(&dir.path().join("s"));
        c.set_font_size(2);
        assert_eq!(c.font_size, MIN_FONT_SIZE);
        c.set_font_size(200);
        assert_eq!(c.font_size, MAX_FONT_SIZE);
        let file = dir.path().join("s");
        fs::write(&file, "font-size=500\n").unwrap();
        assert_eq!(Config::load(&file).font_size, MAX_FONT_SIZE);
    }

    #[test]
    fn save_reports_errors() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("file");
        fs::write(&blocker, "").unwrap();
        let c = Config::load(&blocker.join("settings"));
        assert!(c.save().is_err());
    }
}
