//! Encryption, in the standard age format (<https://age-encryption.org>).
//!
//! A diary has one X25519 key pair. The secret half lives in
//! `.diary/identity.age`, itself encrypted with the user's password (age's
//! scrypt mode). Entries are encrypted to the public half. Unlocking costs
//! one slow scrypt step; after that every entry decrypts quickly.
//!
//! Because it is all plain age, the diary can be read without this app:
//! `age -d -i .diary/identity.age 2026/11/2026-11-10.md.age`.

use std::str::FromStr;

use age::secrecy::{ExposeSecret, SecretString};
use age::x25519;

/// How expensive the password key derivation is (age's scrypt log2 N).
/// The default matches the `age` tool; tests use a cheap one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfCost(pub u8);

impl Default for KdfCost {
    fn default() -> Self {
        KdfCost(18)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CryptoError {
    #[error("wrong password")]
    WrongPassword,
    #[error("the key file is damaged")]
    BadKeyFile,
    #[error("this file could not be decrypted with the diary's key")]
    Undecryptable,
}

/// The unlocked key pair of a diary.
pub struct Keyring {
    identity: x25519::Identity,
}

impl Keyring {
    /// Makes a new key pair.
    pub fn generate() -> Self {
        Self {
            identity: x25519::Identity::generate(),
        }
    }

    /// Opens `.diary/identity.age` with the password.
    pub fn unlock(key_file: &[u8], password: &SecretString) -> Result<Self, CryptoError> {
        let plain =
            age::decrypt(&age::scrypt::Identity::new(password.clone()), key_file).map_err(|e| {
                match e {
                    age::DecryptError::DecryptionFailed => CryptoError::WrongPassword,
                    _ => CryptoError::BadKeyFile,
                }
            })?;
        let text = SecretString::from(String::from_utf8_lossy(&plain).into_owned());
        let identity = text
            .expose_secret()
            .lines()
            .find_map(|line| x25519::Identity::from_str(line.trim()).ok())
            .ok_or(CryptoError::BadKeyFile)?;
        Ok(Self { identity })
    }

    /// The contents for `.diary/identity.age`: the secret key in age-keygen
    /// format, encrypted with `password`.
    pub fn key_file(&self, password: &SecretString, cost: KdfCost) -> Vec<u8> {
        let mut recipient = age::scrypt::Recipient::new(password.clone());
        recipient.set_work_factor(cost.0);
        let plain = SecretString::from(format!(
            "# Diary key. Decrypt entries with: age -d -i identity.age FILE\n\
             # public key: {}\n{}\n",
            self.recipient(),
            self.identity.to_string().expose_secret()
        ));
        age::encrypt(&recipient, plain.expose_secret().as_bytes()).expect("encrypting to scrypt")
    }

    /// The public key (`age1…`), stored in `.diary/recipient.txt`.
    pub fn recipient(&self) -> String {
        self.identity.to_public().to_string()
    }

    pub fn encrypt(&self, plaintext: &[u8]) -> Vec<u8> {
        age::encrypt(&self.identity.to_public(), plaintext).expect("encrypting to x25519")
    }

    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>, CryptoError> {
        age::decrypt(&self.identity, ciphertext).map_err(|_| CryptoError::Undecryptable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const CHEAP: KdfCost = KdfCost(2);

    fn pw(s: &str) -> SecretString {
        SecretString::from(s.to_owned())
    }

    #[test]
    fn default_cost_matches_age_tool() {
        assert_eq!(KdfCost::default(), KdfCost(18));
    }

    #[test]
    fn entries_round_trip() {
        let keys = Keyring::generate();
        let sealed = keys.encrypt(b"Dear diary");
        assert_ne!(sealed, b"Dear diary");
        assert!(sealed.starts_with(b"age-encryption.org/v1\n"));
        assert_eq!(keys.decrypt(&sealed).unwrap(), b"Dear diary");
    }

    #[test]
    fn empty_entry_round_trips() {
        let keys = Keyring::generate();
        assert_eq!(keys.decrypt(&keys.encrypt(b"")).unwrap(), b"");
    }

    #[test]
    fn unlock_with_right_password_gives_same_keys() {
        let keys = Keyring::generate();
        let file = keys.key_file(&pw("correct horse"), CHEAP);
        let again = Keyring::unlock(&file, &pw("correct horse")).unwrap();
        assert_eq!(again.recipient(), keys.recipient());
        assert_eq!(again.decrypt(&keys.encrypt(b"x")).unwrap(), b"x");
    }

    #[test]
    fn unlock_with_wrong_password_fails() {
        let file = Keyring::generate().key_file(&pw("right"), CHEAP);
        assert_eq!(
            Keyring::unlock(&file, &pw("wrong")).err(),
            Some(CryptoError::WrongPassword)
        );
    }

    #[test]
    fn unlock_rejects_garbage_key_file() {
        assert_eq!(
            Keyring::unlock(b"not age at all", &pw("x")).err(),
            Some(CryptoError::BadKeyFile)
        );
    }

    #[test]
    fn unlock_rejects_key_file_that_holds_no_key() {
        let file = age::encrypt(
            &age::scrypt::Recipient::new(pw("pw")),
            b"# just a comment\n",
        )
        .unwrap();
        assert_eq!(
            Keyring::unlock(&file, &pw("pw")).err(),
            Some(CryptoError::BadKeyFile)
        );
    }

    #[test]
    fn unlock_rejects_key_file_not_protected_by_a_password() {
        let other = Keyring::generate();
        let file = other.encrypt(b"AGE-SECRET-KEY-1");
        assert_eq!(
            Keyring::unlock(&file, &pw("pw")).err(),
            Some(CryptoError::BadKeyFile)
        );
    }

    #[test]
    fn password_can_be_changed_by_rewriting_key_file() {
        let keys = Keyring::generate();
        let new_file = keys.key_file(&pw("new"), CHEAP);
        assert!(Keyring::unlock(&new_file, &pw("new")).is_ok());
    }

    #[test]
    fn foreign_or_damaged_files_do_not_decrypt() {
        let keys = Keyring::generate();
        let other = Keyring::generate();
        assert_eq!(
            keys.decrypt(&other.encrypt(b"x")).err(),
            Some(CryptoError::Undecryptable)
        );
        assert_eq!(
            keys.decrypt(b"plain text").err(),
            Some(CryptoError::Undecryptable)
        );
        let mut sealed = keys.encrypt(b"some text");
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert_eq!(
            keys.decrypt(&sealed).err(),
            Some(CryptoError::Undecryptable)
        );
    }

    #[test]
    fn recipient_is_an_age_public_key() {
        assert!(Keyring::generate().recipient().starts_with("age1"));
    }

    // Interop with the real `age` command (installed in the dev container).

    fn age_cli(args: &[&str]) -> Vec<u8> {
        let out = Command::new("age").args(args).output().expect("age CLI");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    #[test]
    fn age_cli_decrypts_our_entries() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keyring::generate();
        // What `age -d .diary/identity.age` would give the user.
        let plain_key = age::decrypt(
            &age::scrypt::Identity::new(pw("pw")),
            &keys.key_file(&pw("pw"), CHEAP),
        )
        .unwrap();
        let id = dir.path().join("id");
        let entry = dir.path().join("entry.age");
        std::fs::write(&id, plain_key).unwrap();
        std::fs::write(&entry, keys.encrypt(b"hello from Diary")).unwrap();
        let out = age_cli(&["-d", "-i", id.to_str().unwrap(), entry.to_str().unwrap()]);
        assert_eq!(out, b"hello from Diary");
    }

    #[test]
    fn we_decrypt_files_made_by_age_cli() {
        let dir = tempfile::tempdir().unwrap();
        let keys = Keyring::generate();
        let plain = dir.path().join("plain");
        std::fs::write(&plain, "written elsewhere").unwrap();
        let sealed = age_cli(&["-r", &keys.recipient(), plain.to_str().unwrap()]);
        assert_eq!(keys.decrypt(&sealed).unwrap(), b"written elsewhere");
    }
}
