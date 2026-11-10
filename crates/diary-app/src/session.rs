//! An unlocked diary being written in: which day is open, its text,
//! autosave, and moving between days. The GTK layer only forwards events
//! here and shows the result.

use std::collections::BTreeSet;
use std::path::PathBuf;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate};
use diary_core::clock::Clock;
use diary_core::entry::Entry;
use diary_core::search::{self, Hit};
use diary_core::store::{Diary, StoreError, Version};

/// How long typing must pause before the entry is saved.
pub const AUTOSAVE_DELAY: Duration = Duration::seconds(1);

/// Something the user should be told about after a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Another device changed this day; its version was kept here.
    ConflictKept(PathBuf),
}

pub struct Session {
    diary: Diary,
    clock: Box<dyn Clock>,
    date: NaiveDate,
    entry: Entry,
    version: Option<Version>,
    last_edit: Option<DateTime<FixedOffset>>,
    dates: BTreeSet<NaiveDate>,
}

impl Session {
    /// Opens the diary on today's entry.
    pub fn start(diary: Diary, clock: Box<dyn Clock>) -> Result<Self, StoreError> {
        let now = clock.now();
        let date = now.date_naive();
        let loaded = diary.load(date, now)?;
        let dates = diary.dates()?.into_iter().collect();
        Ok(Self {
            diary,
            clock,
            date,
            entry: loaded.entry,
            version: loaded.version,
            last_edit: None,
            dates,
        })
    }

    pub fn diary(&self) -> &Diary {
        &self.diary
    }

    pub fn now(&self) -> DateTime<FixedOffset> {
        self.clock.now()
    }

    pub fn today(&self) -> NaiveDate {
        self.now().date_naive()
    }

    /// The day being shown.
    pub fn date(&self) -> NaiveDate {
        self.date
    }

    pub fn entry(&self) -> &Entry {
        &self.entry
    }

    /// Days that have an entry (for the calendar).
    pub fn dates(&self) -> &BTreeSet<NaiveDate> {
        &self.dates
    }

    /// Re-reads which days exist, e.g. after a sync brought new ones.
    pub fn refresh_dates(&mut self) -> Result<(), StoreError> {
        self.dates = self.diary.dates()?.into_iter().collect();
        Ok(())
    }

    /// The text in the editor changed.
    pub fn edit(&mut self, text: &str) {
        let now = self.now();
        if self.entry.set_body(text, now) {
            self.last_edit = Some(now);
        }
    }

    /// Whether there are unsaved changes.
    pub fn is_dirty(&self) -> bool {
        self.last_edit.is_some()
    }

    /// Saves if typing has paused for [`AUTOSAVE_DELAY`]. Call regularly.
    pub fn tick(&mut self) -> Result<Option<Notice>, StoreError> {
        match self.last_edit {
            Some(at) if self.now() - at >= AUTOSAVE_DELAY => self.save(),
            _ => Ok(None),
        }
    }

    /// Saves now if there are unsaved changes. An emptied entry removes
    /// the day's file, so looking at a day never creates one.
    pub fn save(&mut self) -> Result<Option<Notice>, StoreError> {
        if !self.is_dirty() {
            return Ok(None);
        }
        let mut notice = None;
        if self.entry.body.trim().is_empty() {
            self.diary.remove(self.date)?;
            self.version = None;
            self.dates.remove(&self.date);
        } else {
            let saved = self
                .diary
                .save(self.date, &self.entry, self.version, self.now())?;
            self.version = Some(saved.version);
            self.dates.insert(self.date);
            notice = saved.conflict_copy.map(Notice::ConflictKept);
        }
        self.last_edit = None;
        Ok(notice)
    }

    /// Saves the current day, then shows `date`. On error the current day
    /// stays open.
    pub fn open(&mut self, date: NaiveDate) -> Result<Option<Notice>, StoreError> {
        let notice = self.save()?;
        let loaded = self.diary.load(date, self.now())?;
        self.date = date;
        self.entry = loaded.entry;
        self.version = loaded.version;
        Ok(notice)
    }

    /// The closest earlier day with an entry.
    pub fn previous(&self) -> Option<NaiveDate> {
        self.dates.range(..self.date).next_back().copied()
    }

    /// The closest later day with an entry, or today when there is none.
    pub fn next(&self) -> Option<NaiveDate> {
        let after = self.date.succ_opt()?;
        let today = self.today();
        match self.dates.range(after..).next() {
            Some(&day) if day <= today => Some(day),
            _ if self.date < today => Some(today),
            _ => None,
        }
    }

    pub fn search(&mut self, query: &str) -> Result<Vec<Hit>, StoreError> {
        self.save()?;
        search::search(&self.diary, query, self.now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diary_core::clock::ManualClock;
    use diary_core::crypto::KdfCost;
    use diary_core::secrecy::SecretString;
    use std::rc::Rc;

    struct Fixture {
        _dir: tempfile::TempDir,
        clock: Rc<ManualClock>,
        session: Session,
    }

    fn d(s: &str) -> NaiveDate {
        s.parse().unwrap()
    }

    fn fixture_with(days: &[(&str, &str)]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let pw = SecretString::from("pw".to_owned());
        let diary = Diary::create(dir.path(), &pw, KdfCost(2), "laptop").unwrap();
        for (day, body) in days {
            let mut e =
                Entry::new(DateTime::parse_from_rfc3339("2026-01-01T12:00:00+01:00").unwrap());
            e.body = (*body).to_owned();
            diary.save(d(day), &e, None, e.created).unwrap();
        }
        let clock = Rc::new(ManualClock::new("2026-11-10T20:55:03+01:00"));
        let session = Session::start(diary, Box::new(clock.clone())).unwrap();
        Fixture {
            _dir: dir,
            clock,
            session,
        }
    }

    fn fixture() -> Fixture {
        fixture_with(&[])
    }

    fn on_disk(f: &Fixture, day: &str) -> Option<String> {
        let diary = f.session.diary();
        let path = diary.path_for(d(day));
        let bytes = std::fs::read(path).ok()?;
        Some(String::from_utf8(diary.keys().decrypt(&bytes).unwrap()).unwrap())
    }

    #[test]
    fn starts_on_today_with_an_empty_entry() {
        let f = fixture();
        assert_eq!(f.session.date(), d("2026-11-10"));
        assert_eq!(f.session.today(), d("2026-11-10"));
        assert_eq!(f.session.entry().body, "");
        assert_eq!(
            f.session.entry().created.to_rfc3339(),
            "2026-11-10T20:55:03+01:00"
        );
        assert!(!f.session.is_dirty());
        assert_eq!(f.session.now(), f.clock.now());
    }

    #[test]
    fn starts_on_todays_existing_entry() {
        let f = fixture_with(&[("2026-11-10", "morning notes")]);
        assert_eq!(f.session.entry().body, "morning notes");
    }

    #[test]
    fn typing_saves_only_after_a_pause() {
        let mut f = fixture();
        f.session.edit("Dear diary");
        assert!(f.session.is_dirty());
        f.clock.advance(Duration::milliseconds(500));
        assert_eq!(f.session.tick().unwrap(), None);
        assert_eq!(on_disk(&f, "2026-11-10"), None);
        f.session.edit("Dear diary, today");
        f.clock.advance(Duration::milliseconds(900));
        f.session.tick().unwrap();
        assert_eq!(on_disk(&f, "2026-11-10"), None);
        f.clock.advance(Duration::milliseconds(100));
        f.session.tick().unwrap();
        assert!(
            on_disk(&f, "2026-11-10")
                .unwrap()
                .ends_with("Dear diary, today")
        );
        assert!(!f.session.is_dirty());
        assert!(f.session.dates().contains(&d("2026-11-10")));
    }

    #[test]
    fn modified_time_is_the_time_of_the_last_edit() {
        let mut f = fixture();
        f.clock.advance(Duration::minutes(5));
        f.session.edit("x");
        f.clock.advance(Duration::minutes(1));
        f.session.save().unwrap();
        let text = on_disk(&f, "2026-11-10").unwrap();
        assert!(text.contains("created: 2026-11-10T20:55:03+01:00"));
        assert!(text.contains("modified: 2026-11-10T21:00:03+01:00"));
    }

    #[test]
    fn tick_without_changes_does_nothing() {
        let mut f = fixture();
        f.clock.advance(Duration::seconds(10));
        assert_eq!(f.session.tick().unwrap(), None);
        assert_eq!(on_disk(&f, "2026-11-10"), None);
    }

    #[test]
    fn same_text_is_not_a_change() {
        let mut f = fixture_with(&[("2026-11-10", "same")]);
        f.session.edit("same");
        assert!(!f.session.is_dirty());
    }

    #[test]
    fn emptying_an_entry_removes_its_file() {
        let mut f = fixture_with(&[("2026-11-10", "oops")]);
        f.session.edit("  \n");
        f.session.save().unwrap();
        assert_eq!(on_disk(&f, "2026-11-10"), None);
        assert!(!f.session.dates().contains(&d("2026-11-10")));
        // Writing again afterwards works.
        f.session.edit("again");
        f.session.save().unwrap();
        assert!(on_disk(&f, "2026-11-10").is_some());
    }

    #[test]
    fn opening_another_day_saves_first() {
        let mut f = fixture_with(&[("2026-11-01", "earlier")]);
        f.session.edit("today's text");
        f.session.open(d("2026-11-01")).unwrap();
        assert!(on_disk(&f, "2026-11-10").unwrap().ends_with("today's text"));
        assert_eq!(f.session.date(), d("2026-11-01"));
        assert_eq!(f.session.entry().body, "earlier");
    }

    #[test]
    fn viewing_an_empty_day_creates_no_file() {
        let mut f = fixture();
        f.session.open(d("2026-10-01")).unwrap();
        f.session.open(d("2026-11-10")).unwrap();
        assert_eq!(on_disk(&f, "2026-10-01"), None);
        assert!(f.session.dates().is_empty());
    }

    #[test]
    fn failed_open_keeps_current_day() {
        let mut f = fixture();
        f.session.edit("keep me");
        let bad = f.session.diary().path_for(d("2026-10-01"));
        std::fs::create_dir_all(bad.parent().unwrap()).unwrap();
        std::fs::write(&bad, "garbage").unwrap();
        assert!(f.session.open(d("2026-10-01")).is_err());
        assert_eq!(f.session.date(), d("2026-11-10"));
        assert_eq!(f.session.entry().body, "keep me");
    }

    #[test]
    fn conflicts_are_reported() {
        let mut f = fixture_with(&[("2026-11-10", "v1")]);
        // Another device rewrites today's file.
        std::thread::sleep(std::time::Duration::from_millis(20));
        let other = Diary::open(
            f.session.diary().root(),
            &SecretString::from("pw".to_owned()),
            "desktop",
        )
        .unwrap();
        let theirs = Entry::new(f.clock.now());
        other
            .save(d("2026-11-10"), &theirs, None, f.clock.now())
            .ok();
        f.session.edit("v1 and more");
        match f.session.save().unwrap() {
            Some(Notice::ConflictKept(path)) => assert!(path.exists()),
            other => panic!("expected conflict, got {other:?}"),
        }
    }

    #[test]
    fn save_errors_keep_changes() {
        let mut f = fixture();
        f.session.edit("unsaved");
        // Make the year folder a file so the save fails.
        std::fs::write(f.session.diary().root().join("2026"), "").unwrap();
        assert!(f.session.save().is_err());
        assert!(f.session.is_dirty());
    }

    #[test]
    fn previous_and_next_walk_days_with_entries() {
        let mut f = fixture_with(&[
            ("2026-10-01", "a"),
            ("2026-10-15", "b"),
            ("2026-11-01", "c"),
        ]);
        assert_eq!(f.session.previous(), Some(d("2026-11-01")));
        assert_eq!(f.session.next(), None);
        f.session.open(d("2026-10-15")).unwrap();
        assert_eq!(f.session.previous(), Some(d("2026-10-01")));
        assert_eq!(f.session.next(), Some(d("2026-11-01")));
        f.session.open(d("2026-11-01")).unwrap();
        // Past the last entry, "next" goes back to today.
        assert_eq!(f.session.next(), Some(d("2026-11-10")));
        f.session.open(d("2026-10-01")).unwrap();
        assert_eq!(f.session.previous(), None);
    }

    #[test]
    fn today_follows_the_clock() {
        let f = fixture();
        f.clock.set("2026-11-11T00:00:01+01:00");
        assert_eq!(f.session.today(), d("2026-11-11"));
        assert_eq!(f.session.next(), Some(d("2026-11-11")));
    }

    #[test]
    fn refresh_picks_up_synced_days() {
        let mut f = fixture();
        let e = Entry::new(f.clock.now());
        f.session
            .diary()
            .save(d("2026-09-09"), &e, None, f.clock.now())
            .unwrap();
        assert!(f.session.dates().is_empty());
        f.session.refresh_dates().unwrap();
        assert!(f.session.dates().contains(&d("2026-09-09")));
    }

    #[test]
    fn search_includes_unsaved_text_after_saving() {
        let mut f = fixture_with(&[("2026-10-01", "the lake was cold")]);
        f.session.edit("swam in the lake again");
        let hits = f.session.search("lake").unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].date, d("2026-11-10"));
    }
}
