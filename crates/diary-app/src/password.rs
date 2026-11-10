//! Rules for choosing a new password.

pub const MIN_LENGTH: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordProblem {
    TooShort,
    Mismatch,
}

impl PasswordProblem {
    pub fn message(self) -> &'static str {
        match self {
            PasswordProblem::TooShort => "Use at least 8 characters",
            PasswordProblem::Mismatch => "The passwords do not match",
        }
    }
}

/// Checks a new password and its confirmation. An empty confirmation is
/// not yet a mismatch (the user may still be typing).
pub fn check_new(password: &str, confirm: &str) -> Result<(), PasswordProblem> {
    if password.chars().count() < MIN_LENGTH {
        Err(PasswordProblem::TooShort)
    } else if !confirm.is_empty() && confirm != password {
        Err(PasswordProblem::Mismatch)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_passwords_are_refused() {
        assert_eq!(check_new("short", "short"), Err(PasswordProblem::TooShort));
        assert_eq!(check_new("", ""), Err(PasswordProblem::TooShort));
    }

    #[test]
    fn length_counts_characters_not_bytes() {
        assert_eq!(
            check_new("ééééééé", "ééééééé"),
            Err(PasswordProblem::TooShort)
        );
    }

    #[test]
    fn confirmation_must_match() {
        assert_eq!(
            check_new("long enough", "long enougH"),
            Err(PasswordProblem::Mismatch)
        );
        assert_eq!(check_new("long enough", "long enough"), Ok(()));
    }

    #[test]
    fn problems_have_messages() {
        assert!(PasswordProblem::TooShort.message().contains('8'));
        assert!(PasswordProblem::Mismatch.message().contains("match"));
    }
}
