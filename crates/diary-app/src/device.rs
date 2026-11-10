//! A short name for this computer, used in sync conflict copies.

use std::path::Path;

/// Reads the host name from `file` (normally `/proc/sys/kernel/hostname`)
/// and keeps only characters that are safe in a file name.
pub fn device_name(file: &Path) -> String {
    let name: String = std::fs::read_to_string(file)
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    if name.is_empty() {
        "device".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_host_name() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("hostname");
        std::fs::write(&f, "fedora-laptop\n").unwrap();
        assert_eq!(device_name(&f), "fedora-laptop");
    }

    #[test]
    fn strips_unsafe_characters() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("hostname");
        std::fs::write(&f, "my/odd host.lan").unwrap();
        assert_eq!(device_name(&f), "myoddhostlan");
    }

    #[test]
    fn falls_back_when_unknown() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(device_name(&dir.path().join("missing")), "device");
        let f = dir.path().join("hostname");
        std::fs::write(&f, "...\n").unwrap();
        assert_eq!(device_name(&f), "device");
    }
}
