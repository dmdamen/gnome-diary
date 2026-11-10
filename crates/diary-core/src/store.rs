//! The diary folder on disk.
//!
//! ```text
//! <diary>/
//!   .diary/identity.age        secret key, encrypted with the password
//!   .diary/recipient.txt       public key
//!   .diary/README.txt          how to read the diary without the app
//!   2026/11/2026-11-10.md.age  one encrypted Markdown file per day
//! ```
//!
//! Files are written atomically (temp file, fsync, rename) so a sync client
//! never uploads half a file. Each load remembers the file's [`Version`]; if
//! the file changed on disk before we save (another device synced a new
//! version in), their version is kept as a conflict copy and ours is saved.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use age::secrecy::SecretString;
use chrono::{DateTime, Datelike, FixedOffset, NaiveDate};

use crate::crypto::{CryptoError, KdfCost, Keyring};
use crate::entry::Entry;

const META_DIR: &str = ".diary";
const KEY_FILE: &str = "identity.age";
const RECIPIENT_FILE: &str = "recipient.txt";
const README_FILE: &str = "README.txt";
const SUFFIX: &str = ".md.age";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Crypto(#[from] CryptoError),
    #[error("this folder already contains a diary")]
    AlreadyADiary,
    #[error("this folder does not contain a diary")]
    NotADiary,
}

/// What a file looked like when we last read or wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    modified: SystemTime,
    len: u64,
}

/// One day's entry as loaded from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub entry: Entry,
    /// `None` when there is no file for the day yet.
    pub version: Option<Version>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    pub version: Version,
    /// Set when another device had changed the file: their version was
    /// moved here so nothing is lost.
    pub conflict_copy: Option<PathBuf>,
}

/// An unlocked diary.
pub struct Diary {
    root: PathBuf,
    keys: Keyring,
    device: String,
}

/// Whether `root` already holds a diary (made here or synced from elsewhere).
pub fn is_diary(root: &Path) -> bool {
    root.join(META_DIR).join(KEY_FILE).is_file()
}

const README: &str = "\
This folder is a diary written with Diary, a GNOME app.

Every day is a Markdown file encrypted with age (https://age-encryption.org).
The key is in .diary/identity.age, protected by your diary password.
To read a day without the app, run this in the diary folder:

    age -d -i .diary/identity.age 2026/11/2026-11-10.md.age

Do not delete the .diary folder: without it the entries cannot be read.
";

impl Diary {
    /// Starts a new diary in `root` (which may already hold other files).
    /// `device` names this computer in conflict copies.
    pub fn create(
        root: &Path,
        password: &SecretString,
        cost: KdfCost,
        device: &str,
    ) -> Result<Self, StoreError> {
        if is_diary(root) {
            return Err(StoreError::AlreadyADiary);
        }
        let keys = Keyring::generate();
        let meta = root.join(META_DIR);
        write_atomic(&meta.join(README_FILE), README.as_bytes())?;
        write_atomic(
            &meta.join(RECIPIENT_FILE),
            format!("{}\n", keys.recipient()).as_bytes(),
        )?;
        write_atomic(&meta.join(KEY_FILE), &keys.key_file(password, cost))?;
        Ok(Self {
            root: root.to_owned(),
            keys,
            device: device.to_owned(),
        })
    }

    /// Unlocks an existing diary.
    pub fn open(root: &Path, password: &SecretString, device: &str) -> Result<Self, StoreError> {
        if !is_diary(root) {
            return Err(StoreError::NotADiary);
        }
        let key_file = fs::read(root.join(META_DIR).join(KEY_FILE))?;
        Ok(Self {
            root: root.to_owned(),
            keys: Keyring::unlock(&key_file, password)?,
            device: device.to_owned(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn keys(&self) -> &Keyring {
        &self.keys
    }

    /// The file that holds the entry for `date`.
    pub fn path_for(&self, date: NaiveDate) -> PathBuf {
        self.root
            .join(format!("{:04}", date.year()))
            .join(format!("{:02}", date.month()))
            .join(format!("{date}{SUFFIX}"))
    }

    /// Every day that has an entry, oldest first. Only file names are read.
    pub fn dates(&self) -> Result<Vec<NaiveDate>, StoreError> {
        let mut dates = Vec::new();
        for year in subdirs(&self.root)? {
            for month in subdirs(&year)? {
                for file in fs::read_dir(&month)? {
                    let name = file?.file_name();
                    let date = name
                        .to_str()
                        .and_then(|n| n.strip_suffix(SUFFIX))
                        .and_then(|n| n.parse::<NaiveDate>().ok());
                    dates.extend(date);
                }
            }
        }
        dates.sort();
        Ok(dates)
    }

    /// Loads the entry for `date`; a day without a file gives a new entry
    /// created `now`.
    pub fn load(&self, date: NaiveDate, now: DateTime<FixedOffset>) -> Result<Loaded, StoreError> {
        let path = self.path_for(date);
        let Some(version) = version_of(&path)? else {
            return Ok(Loaded {
                entry: Entry::new(now),
                version: None,
            });
        };
        let plain = self.keys.decrypt(&fs::read(&path)?)?;
        Ok(Loaded {
            entry: Entry::parse(&String::from_utf8_lossy(&plain), now),
            version: Some(version),
        })
    }

    /// Saves `entry` for `date`. `expected` is the version returned by the
    /// last load or save.
    pub fn save(
        &self,
        date: NaiveDate,
        entry: &Entry,
        expected: Option<Version>,
        now: DateTime<FixedOffset>,
    ) -> Result<Saved, StoreError> {
        let path = self.path_for(date);
        let current = version_of(&path)?;
        let conflict_copy = match current {
            Some(_) if current != expected => {
                let copy = path.with_file_name(format!(
                    "{date}.conflict-{}-{}{SUFFIX}",
                    self.device,
                    now.format("%Y%m%d-%H%M%S")
                ));
                fs::rename(&path, &copy)?;
                Some(copy)
            }
            _ => None,
        };
        write_atomic(&path, &self.keys.encrypt(entry.render().as_bytes()))?;
        Ok(Saved {
            version: version_of(&path)?.expect("just written"),
            conflict_copy,
        })
    }

    /// Removes the file for `date` (used when an entry is emptied).
    pub fn remove(&self, date: NaiveDate) -> Result<(), StoreError> {
        match fs::remove_file(self.path_for(date)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    /// Re-encrypts the key file with a new password. Entries are untouched.
    pub fn change_password(&self, new: &SecretString, cost: KdfCost) -> Result<(), StoreError> {
        let path = self.root.join(META_DIR).join(KEY_FILE);
        write_atomic(&path, &self.keys.key_file(new, cost))?;
        Ok(())
    }
}

fn version_of(path: &Path) -> io::Result<Option<Version>> {
    match fs::metadata(path) {
        Ok(meta) => Ok(Some(Version {
            modified: meta.modified()?,
            len: meta.len(),
        })),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Subfolders of `dir` whose names are all digits (years, months).
fn subdirs(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for item in fs::read_dir(dir)? {
        let item = item?;
        let numeric = item
            .file_name()
            .to_str()
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        if numeric && item.file_type()?.is_dir() {
            found.push(item.path());
        }
    }
    Ok(found)
}

/// Writes via a temporary file in the same folder, so readers (and sync
/// clients) only ever see the old or the new contents.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().expect("files live in a folder");
    fs::create_dir_all(dir)?;
    let name = path.file_name().expect("file name").to_string_lossy();
    let tmp = dir.join(format!(".{name}.tmp-{}", std::process::id()));
    let mut file = fs::File::create(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&tmp, path)?;
    fs::File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    const CHEAP: KdfCost = KdfCost(2);

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }
    fn t(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }
    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }
    fn new_diary() -> (TempDir, Diary) {
        let dir = tempfile::tempdir().unwrap();
        let diary = Diary::create(dir.path(), &pw("pw"), CHEAP, "laptop").unwrap();
        (dir, diary)
    }
    fn entry(body: &str) -> Entry {
        let mut e = Entry::new(t("2026-11-10T20:00:00+01:00"));
        e.set_body(body, t("2026-11-10T20:05:00+01:00"));
        e
    }
    /// Lets the file system clock move on, so mtimes differ.
    fn tick() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    #[test]
    fn create_writes_key_files_and_readme() {
        let (dir, diary) = new_diary();
        let meta = dir.path().join(".diary");
        assert!(meta.join("identity.age").is_file());
        let recipient = fs::read_to_string(meta.join("recipient.txt")).unwrap();
        assert_eq!(recipient.trim(), diary.keys().recipient());
        let readme = fs::read_to_string(meta.join("README.txt")).unwrap();
        assert!(readme.contains("age -d -i .diary/identity.age"));
        assert_eq!(diary.root(), dir.path());
        assert!(is_diary(dir.path()));
    }

    #[test]
    fn empty_or_missing_folder_is_not_a_diary() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_diary(dir.path()));
        assert!(!is_diary(&dir.path().join("missing")));
    }

    #[test]
    fn create_makes_missing_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Nextcloud/Diary");
        Diary::create(&root, &pw("pw"), CHEAP, "laptop").unwrap();
        assert!(is_diary(&root));
    }

    #[test]
    fn create_refuses_existing_diary() {
        let (dir, _) = new_diary();
        assert!(matches!(
            Diary::create(dir.path(), &pw("x"), CHEAP, "laptop"),
            Err(StoreError::AlreadyADiary)
        ));
    }

    #[test]
    fn open_needs_a_diary_and_the_right_password() {
        let (dir, _) = new_diary();
        assert!(Diary::open(dir.path(), &pw("pw"), "desktop").is_ok());
        assert!(matches!(
            Diary::open(dir.path(), &pw("nope"), "desktop"),
            Err(StoreError::Crypto(CryptoError::WrongPassword))
        ));
        let empty = tempfile::tempdir().unwrap();
        assert!(matches!(
            Diary::open(empty.path(), &pw("pw"), "desktop"),
            Err(StoreError::NotADiary)
        ));
    }

    #[test]
    fn path_is_year_month_date() {
        let (dir, diary) = new_diary();
        assert_eq!(
            diary.path_for(d("2026-03-07")),
            dir.path().join("2026/03/2026-03-07.md.age")
        );
    }

    #[test]
    fn missing_day_loads_as_new_entry() {
        let (_dir, diary) = new_diary();
        let now = t("2026-11-10T07:30:00+01:00");
        let loaded = diary.load(d("2026-11-10"), now).unwrap();
        assert_eq!(loaded.entry, Entry::new(now));
        assert_eq!(loaded.version, None);
    }

    #[test]
    fn saved_entry_loads_back_and_is_encrypted_on_disk() {
        let (_dir, diary) = new_diary();
        let e = entry("Secret thoughts");
        let saved = diary
            .save(d("2026-11-10"), &e, None, t("2026-11-10T20:05:00+01:00"))
            .unwrap();
        assert_eq!(saved.conflict_copy, None);
        let raw = fs::read(diary.path_for(d("2026-11-10"))).unwrap();
        assert!(raw.starts_with(b"age-encryption.org/v1"));
        assert!(!String::from_utf8_lossy(&raw).contains("Secret"));
        let loaded = diary
            .load(d("2026-11-10"), t("2026-11-11T00:00:00Z"))
            .unwrap();
        assert_eq!(loaded.entry, e);
        assert_eq!(loaded.version, Some(saved.version));
    }

    #[test]
    fn save_leaves_no_temp_files() {
        let (_dir, diary) = new_diary();
        diary
            .save(
                d("2026-11-10"),
                &entry("x"),
                None,
                t("2026-11-10T20:05:00Z"),
            )
            .unwrap();
        let month = diary.path_for(d("2026-11-10")).parent().unwrap().to_owned();
        let names: Vec<_> = fs::read_dir(month)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["2026-11-10.md.age"]);
    }

    #[test]
    fn repeated_saves_with_current_version_do_not_conflict() {
        let (_dir, diary) = new_diary();
        let day = d("2026-11-10");
        let first = diary
            .save(day, &entry("a"), None, t("2026-11-10T20:05:00Z"))
            .unwrap();
        tick();
        let second = diary
            .save(
                day,
                &entry("ab"),
                Some(first.version),
                t("2026-11-10T20:06:00Z"),
            )
            .unwrap();
        assert_eq!(second.conflict_copy, None);
        assert_eq!(
            diary
                .load(day, t("2026-11-11T00:00:00Z"))
                .unwrap()
                .entry
                .body,
            "ab"
        );
    }

    #[test]
    fn changed_on_disk_keeps_their_version_as_conflict_copy() {
        let (_dir, diary) = new_diary();
        let day = d("2026-11-10");
        let mine = diary
            .save(day, &entry("mine"), None, t("2026-11-10T20:05:00Z"))
            .unwrap();
        tick();
        // Another device syncs in a different version.
        let other = Diary::open(diary.root(), &pw("pw"), "desktop").unwrap();
        other
            .save(day, &entry("theirs!"), None, t("2026-11-10T20:06:00Z"))
            .ok();
        tick();
        let saved = diary
            .save(
                day,
                &entry("mine, edited"),
                Some(mine.version),
                t("2026-11-10T21:07:08+01:00"),
            )
            .unwrap();
        let copy = saved.conflict_copy.expect("conflict copy");
        assert_eq!(
            copy.file_name().unwrap(),
            "2026-11-10.conflict-laptop-20261110-210708.md.age"
        );
        let theirs = diary.keys().decrypt(&fs::read(&copy).unwrap()).unwrap();
        assert!(String::from_utf8(theirs).unwrap().ends_with("theirs!"));
        assert_eq!(
            diary
                .load(day, t("2026-11-11T00:00:00Z"))
                .unwrap()
                .entry
                .body,
            "mine, edited"
        );
    }

    #[test]
    fn file_appearing_for_a_new_day_is_kept_as_conflict_copy() {
        let (_dir, diary) = new_diary();
        let day = d("2026-11-10");
        let other = Diary::open(diary.root(), &pw("pw"), "desktop").unwrap();
        other
            .save(day, &entry("from desktop"), None, t("2026-11-10T08:00:00Z"))
            .unwrap();
        let saved = diary
            .save(day, &entry("from laptop"), None, t("2026-11-10T09:00:00Z"))
            .unwrap();
        assert!(saved.conflict_copy.is_some());
    }

    #[test]
    fn dates_lists_days_from_file_names_only() {
        let (dir, diary) = new_diary();
        for day in ["2026-11-10", "2025-12-31", "2026-01-05"] {
            diary
                .save(d(day), &entry("x"), None, t("2026-11-10T20:05:00Z"))
                .unwrap();
        }
        // Noise that must be ignored.
        fs::write(
            dir.path().join("2026/11/2026-11-09.conflict-x-1-2.md.age"),
            "x",
        )
        .unwrap();
        fs::write(dir.path().join("2026/11/notes.txt"), "x").unwrap();
        fs::write(dir.path().join("2026/11/2026-13-45.md.age"), "x").unwrap();
        fs::create_dir_all(dir.path().join("Photos")).unwrap();
        fs::write(dir.path().join("2026/stray-file"), "x").unwrap();
        assert_eq!(
            diary.dates().unwrap(),
            [d("2025-12-31"), d("2026-01-05"), d("2026-11-10")]
        );
    }

    #[test]
    fn new_diary_has_no_dates() {
        let (_dir, diary) = new_diary();
        assert!(diary.dates().unwrap().is_empty());
    }

    #[test]
    fn undecryptable_day_is_an_error() {
        let (_dir, diary) = new_diary();
        let path = diary.path_for(d("2026-11-10"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "garbage").unwrap();
        assert!(matches!(
            diary.load(d("2026-11-10"), t("2026-11-10T00:00:00Z")),
            Err(StoreError::Crypto(CryptoError::Undecryptable))
        ));
    }

    #[test]
    fn remove_deletes_day_and_ignores_missing() {
        let (_dir, diary) = new_diary();
        let day = d("2026-11-10");
        diary
            .save(day, &entry("x"), None, t("2026-11-10T20:05:00Z"))
            .unwrap();
        diary.remove(day).unwrap();
        assert!(diary.dates().unwrap().is_empty());
        diary.remove(day).unwrap();
    }

    #[test]
    fn change_password_keeps_entries_readable() {
        let (dir, diary) = new_diary();
        let day = d("2026-11-10");
        diary
            .save(day, &entry("kept"), None, t("2026-11-10T20:05:00Z"))
            .unwrap();
        diary.change_password(&pw("new"), CHEAP).unwrap();
        assert!(Diary::open(dir.path(), &pw("pw"), "x").is_err());
        let reopened = Diary::open(dir.path(), &pw("new"), "x").unwrap();
        assert_eq!(
            reopened
                .load(day, t("2026-11-11T00:00:00Z"))
                .unwrap()
                .entry
                .body,
            "kept"
        );
    }

    #[test]
    fn io_errors_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a-file");
        fs::write(&file, "x").unwrap();
        // A diary cannot be made inside a regular file.
        assert!(matches!(
            Diary::create(&file.join("diary"), &pw("pw"), CHEAP, "laptop"),
            Err(StoreError::Io(_))
        ));
        let err = StoreError::from(io::Error::other("disk on fire"));
        assert_eq!(err.to_string(), "disk on fire");
        assert_eq!(
            StoreError::NotADiary.to_string(),
            "this folder does not contain a diary"
        );
    }
}
