//! Full-text search. Entries are decrypted one by one; nothing is indexed
//! to disk, so no plain text ever leaves memory.

use chrono::{DateTime, FixedOffset, NaiveDate};

use crate::store::{Diary, StoreError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub date: NaiveDate,
    /// The line of text around the first match.
    pub snippet: String,
}

/// Days whose text contains `query` (ignoring case), newest first.
/// Days that cannot be decrypted are skipped.
pub fn search(
    diary: &Diary,
    query: &str,
    now: DateTime<FixedOffset>,
) -> Result<Vec<Hit>, StoreError> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for date in diary.dates()?.into_iter().rev() {
        let Ok(loaded) = diary.load(date, now) else {
            continue;
        };
        if let Some(snippet) = snippet(&loaded.entry.body, &query, 80) {
            hits.push(Hit { date, snippet });
        }
    }
    Ok(hits)
}

/// The line holding the first match, shortened to about `max` characters
/// centred on the match.
fn snippet(body: &str, query_lower: &str, max: usize) -> Option<String> {
    body.lines().find_map(|line| {
        let line = line.trim();
        let lower = line.to_lowercase();
        let byte_pos = lower.find(query_lower)?;
        let chars: Vec<char> = line.chars().collect();
        if chars.len() <= max {
            return Some(line.to_owned());
        }
        let pos = lower[..byte_pos].chars().count();
        let room = max.saturating_sub(query_lower.chars().count()) / 2;
        let start = pos.saturating_sub(room).min(chars.len() - max);
        let end = start + max;
        let mut out = String::new();
        if start > 0 {
            out.push('…');
        }
        out.extend(&chars[start..end]);
        if end < chars.len() {
            out.push('…');
        }
        Some(out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfCost;
    use crate::entry::Entry;
    use age::secrecy::SecretString;

    fn t(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    fn diary_with(days: &[(&str, &str)]) -> (tempfile::TempDir, Diary) {
        let dir = tempfile::tempdir().unwrap();
        let pw = SecretString::from("pw".to_owned());
        let diary = Diary::create(dir.path(), &pw, KdfCost(2), "x").unwrap();
        for (day, body) in days {
            let mut e = Entry::new(t("2026-11-10T20:00:00Z"));
            e.set_body(body, t("2026-11-10T20:00:00Z"));
            diary
                .save(day.parse().unwrap(), &e, None, t("2026-11-10T20:00:00Z"))
                .unwrap();
        }
        (dir, diary)
    }

    #[test]
    fn finds_days_newest_first_ignoring_case() {
        let (_d, diary) = diary_with(&[
            ("2026-01-01", "Went skating with Anna."),
            ("2026-02-01", "Nothing happened."),
            ("2026-03-01", "anna called\nWe talked for hours."),
        ]);
        let hits = search(&diary, "ANNA", t("2026-11-10T20:00:00Z")).unwrap();
        assert_eq!(
            hits,
            [
                Hit {
                    date: "2026-03-01".parse().unwrap(),
                    snippet: "anna called".into()
                },
                Hit {
                    date: "2026-01-01".parse().unwrap(),
                    snippet: "Went skating with Anna.".into()
                },
            ]
        );
    }

    #[test]
    fn blank_query_finds_nothing() {
        let (_d, diary) = diary_with(&[("2026-01-01", "text")]);
        assert!(
            search(&diary, "  ", t("2026-11-10T20:00:00Z"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn header_dates_are_not_searched() {
        let (_d, diary) = diary_with(&[("2026-01-01", "text")]);
        assert!(
            search(&diary, "created", t("2026-11-10T20:00:00Z"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn undecryptable_days_are_skipped() {
        let (_d, diary) = diary_with(&[("2026-01-01", "findme")]);
        let bad = diary.path_for("2026-01-02".parse().unwrap());
        std::fs::write(bad, "garbage").unwrap();
        assert_eq!(
            search(&diary, "findme", t("2026-11-10T20:00:00Z"))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn missing_diary_folder_is_an_error() {
        let (dir, diary) = diary_with(&[]);
        drop(dir);
        assert!(search(&diary, "x", t("2026-11-10T20:00:00Z")).is_err());
    }

    #[test]
    fn long_lines_are_shortened_around_the_match() {
        let line = format!("{}needle{}", "a".repeat(100), "b".repeat(100));
        let s = snippet(&line, "needle", 30).unwrap();
        assert!(s.starts_with('…') && s.ends_with('…'));
        assert!(s.contains("needle"));
        assert!(s.chars().count() <= 32);
    }

    #[test]
    fn snippet_handles_multibyte_text_and_line_edges() {
        let s = snippet("ééééé café ééééé", "café", 8).unwrap();
        assert!(s.contains("café"));
        assert_eq!(snippet("short", "short", 80).unwrap(), "short");
        assert_eq!(snippet("abc", "zzz", 80), None);
    }
}
