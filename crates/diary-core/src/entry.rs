//! The plain-text format of one day's entry:
//!
//! ```text
//! created: 2026-11-10T20:55:03+01:00
//! modified: 2026-11-10T21:04:41+01:00
//!
//! What I wrote today…
//! ```
//!
//! The dates are kept by the app; the writer never types them.

use chrono::{DateTime, FixedOffset};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub created: DateTime<FixedOffset>,
    pub modified: DateTime<FixedOffset>,
    pub body: String,
}

impl Entry {
    /// A new, empty entry made at `now`.
    pub fn new(now: DateTime<FixedOffset>) -> Self {
        Self {
            created: now,
            modified: now,
            body: String::new(),
        }
    }

    /// Reads an entry. Text without a valid header (for example a file
    /// written by another tool) becomes the body, dated `fallback`.
    pub fn parse(text: &str, fallback: DateTime<FixedOffset>) -> Self {
        parse_header(text).unwrap_or_else(|| Self {
            created: fallback,
            modified: fallback,
            body: text.to_owned(),
        })
    }

    pub fn render(&self) -> String {
        format!(
            "created: {}\nmodified: {}\n\n{}",
            self.created.to_rfc3339(),
            self.modified.to_rfc3339(),
            self.body
        )
    }

    /// Replaces the body. `modified` moves to `now` only when the text
    /// really changed. Returns whether it did.
    pub fn set_body(&mut self, body: &str, now: DateTime<FixedOffset>) -> bool {
        if self.body == body {
            return false;
        }
        body.clone_into(&mut self.body);
        self.modified = now;
        true
    }
}

fn parse_header(text: &str) -> Option<Entry> {
    let mut lines = text.split_inclusive('\n');
    let mut field = |name: &str| {
        let line = lines.next()?.trim_end_matches(['\r', '\n']);
        let value = line.strip_prefix(name)?.strip_prefix(": ")?;
        DateTime::parse_from_rfc3339(value).ok()
    };
    let created = field("created")?;
    let modified = field("modified")?;
    let blank = lines.next()?;
    if !blank.trim_end_matches(['\r', '\n']).is_empty() {
        return None;
    }
    Some(Entry {
        created,
        modified,
        body: lines.collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(s).unwrap()
    }

    #[test]
    fn new_entry_is_empty_and_dated_now() {
        let e = Entry::new(t("2026-11-10T20:55:03+01:00"));
        assert_eq!(e.created, t("2026-11-10T20:55:03+01:00"));
        assert_eq!(e.modified, e.created);
        assert_eq!(e.body, "");
    }

    #[test]
    fn renders_header_blank_line_and_body() {
        let mut e = Entry::new(t("2026-11-10T20:55:03+01:00"));
        e.set_body("Dear diary,\nhello.", t("2026-11-10T21:04:41+01:00"));
        assert_eq!(
            e.render(),
            "created: 2026-11-10T20:55:03+01:00\n\
             modified: 2026-11-10T21:04:41+01:00\n\
             \n\
             Dear diary,\nhello."
        );
    }

    #[test]
    fn parse_round_trips_render() {
        let mut e = Entry::new(t("2026-11-10T20:55:03+01:00"));
        e.set_body("line one\n\nline three\n", t("2026-11-10T22:00:00+01:00"));
        assert_eq!(Entry::parse(&e.render(), t("2000-01-01T00:00:00Z")), e);
    }

    #[test]
    fn parse_accepts_crlf_line_endings_in_header() {
        let text =
            "created: 2026-11-10T20:55:03+01:00\r\nmodified: 2026-11-10T21:00:00+01:00\r\n\r\nbody";
        let e = Entry::parse(text, t("2000-01-01T00:00:00Z"));
        assert_eq!(e.modified, t("2026-11-10T21:00:00+01:00"));
        assert_eq!(e.body, "body");
    }

    #[test]
    fn text_without_header_becomes_body_with_fallback_dates() {
        let fallback = t("2026-11-09T08:00:00+01:00");
        let e = Entry::parse("Just some notes\nfrom elsewhere", fallback);
        assert_eq!(e.body, "Just some notes\nfrom elsewhere");
        assert_eq!(e.created, fallback);
        assert_eq!(e.modified, fallback);
    }

    #[test]
    fn broken_dates_make_the_whole_text_the_body() {
        let text = "created: not a date\nmodified: 2026-11-10T21:00:00+01:00\n\nbody";
        let e = Entry::parse(text, t("2026-11-09T08:00:00+01:00"));
        assert_eq!(e.body, text);
    }

    #[test]
    fn header_without_blank_line_is_not_a_header() {
        let text = "created: 2026-11-10T20:55:03+01:00\nmodified: 2026-11-10T21:00:00+01:00\nbody";
        let e = Entry::parse(text, t("2026-11-09T08:00:00+01:00"));
        assert_eq!(e.body, text);
    }

    #[test]
    fn empty_text_is_an_empty_entry() {
        let e = Entry::parse("", t("2026-11-09T08:00:00+01:00"));
        assert_eq!(e.body, "");
    }

    #[test]
    fn set_body_only_touches_modified_when_text_changes() {
        let mut e = Entry::new(t("2026-11-10T20:00:00+01:00"));
        assert!(e.set_body("a", t("2026-11-10T20:01:00+01:00")));
        assert!(!e.set_body("a", t("2026-11-10T20:02:00+01:00")));
        assert_eq!(e.modified, t("2026-11-10T20:01:00+01:00"));
        assert_eq!(e.created, t("2026-11-10T20:00:00+01:00"));
    }
}
