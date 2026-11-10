//! Storage, encryption and entry format for Diary. Nothing in this crate
//! depends on GTK, so all of it is tested with plain unit tests.

pub mod clock;
pub mod crypto;
pub mod entry;
pub mod search;
pub mod store;

/// Re-exported so callers use the same secret types as the crypto code.
pub use age::secrecy;
