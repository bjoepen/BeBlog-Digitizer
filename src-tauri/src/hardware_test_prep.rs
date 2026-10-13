//! Inert preparation helpers for a separately authorized future hardware test.
//!
//! This module only validates identity/configuration, reserves a fresh directory,
//! and builds an argument vector. It deliberately has no process-start API and is
//! not connected to Tauri commands or application startup.

use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs, io,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const FRAME: u8 = 1;
const DPI: u16 = 2820;
const HOLDER_TIMEOUT_SECONDS: u32 = 120;
const POLL_INTERVAL_SECONDS: u32 = 2;

#[derive(Debug)]
pub(crate) struct PreparedHardwareTest {
    pub client: PathBuf,
    pub output_directory: PathBuf,
    pub arguments: Vec<OsString>,
    pub client_sha256: String,
}

/// Validate a device name and explicit literal confirmation, then prepare—but
/// never start—the fixed D3-B5b single-frame profile.
pub(crate) fn prepare_hardware_test(
    client: &Path,
    expected_sha256: &str,
    device: &str,
    confirmed_device: &str,
    output_root: &Path,
) -> io::Result<PreparedHardwareTest> {
    if device.is_empty()
        || device.trim() != device
        || device
            .chars()
            .any(|character| matches!(character, '\n' | '\r' | '\0'))
        || confirmed_device != device
    {
        return Err(invalid("scanner device must be explicitly confirmed"));
    }
    if !valid_sha256(expected_sha256) {
        return Err(invalid(
            "independent expected SHA-256 must be 64 hex digits",
        ));
    }

    let metadata = fs::symlink_metadata(client)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("client must be a regular, non-symlink file"));
    }
    if metadata.mode() & 0o111 == 0 {
        return Err(invalid("client is not executable"));
    }
    let bytes = fs::read(client)?;
    if !is_thin_arm64_macho(&bytes) {
        return Err(invalid("client is not a thin ARM64 Mach-O executable"));
    }
    let actual_sha256 = format!("{:x}", Sha256::digest(&bytes));
    if !actual_sha256.eq_ignore_ascii_case(expected_sha256) {
        return Err(invalid(
            "client SHA-256 does not match the independent trust value",
        ));
    }

    let output_directory = reserve_output_directory(output_root)?;
    let prefix = output_directory.join("capture");
    let arguments = vec![
        OsString::from("-d"),
        OsString::from(device),
        OsString::from("-o"),
        prefix.into_os_string(),
        OsString::from("-f"),
        OsString::from(FRAME.to_string()),
        OsString::from("-r"),
        OsString::from(DPI.to_string()),
        OsString::from("-t"),
        OsString::from(HOLDER_TIMEOUT_SECONDS.to_string()),
        OsString::from("-i"),
        OsString::from(POLL_INTERVAL_SECONDS.to_string()),
    ];
    Ok(PreparedHardwareTest {
        client: client.to_path_buf(),
        output_directory,
        arguments,
        client_sha256: actual_sha256,
    })
}

fn reserve_output_directory(root: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(root)?;
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid(
            "test output root must be a real directory, not a symlink",
        ));
    }
    let root = root.canonicalize()?;
    let directory = root.join(format!("minolta-2820-frame1-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&directory)?;
    Ok(directory)
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_thin_arm64_macho(bytes: &[u8]) -> bool {
    // MH_MAGIC_64 in little-endian followed by CPU_TYPE_ARM64.
    bytes.len() >= 8
        && bytes[..4] == [0xcf, 0xfa, 0xed, 0xfe]
        && u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) == 0x0100_000c
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::{self, OpenOptions},
        os::unix::{fs::symlink, fs::PermissionsExt},
    };

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("beblog-hwprep-{}", uuid::Uuid::new_v4()))
    }

    fn make_test_client(path: &Path) -> String {
        // Synthetic ARM64 Mach-O header. It is never executed.
        let bytes = [0xcf, 0xfa, 0xed, 0xfe, 0x0c, 0, 0, 1, 1, 2, 3, 4];
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
        format!("{:x}", Sha256::digest(bytes))
    }

    #[test]
    fn prepares_fixed_profile_and_exact_argument_vector_without_starting_process() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let client = root.join("test-client");
        let hash = make_test_client(&client);
        let prepared = prepare_hardware_test(
            &client,
            &hash,
            "confirmed:device-identifier",
            "confirmed:device-identifier",
            &root.join("runs"),
        )
        .unwrap();
        let args: Vec<_> = prepared
            .arguments
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[0..3], ["-d", "confirmed:device-identifier", "-o"]);
        assert_eq!(
            args[3],
            prepared.output_directory.join("capture").to_string_lossy()
        );
        assert_eq!(
            &args[4..],
            ["-f", "1", "-r", "2820", "-t", "120", "-i", "2"]
        );
        assert!(prepared.output_directory.is_dir());
        assert_eq!(prepared.client, client);
        assert_eq!(prepared.client_sha256, hash);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_missing_invalid_or_unconfirmed_client_before_reserving_output() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let missing = root.join("missing-client");
        assert!(prepare_hardware_test(
            &missing,
            &"0".repeat(64),
            "device",
            "device",
            &root.join("runs")
        )
        .is_err());
        assert!(!root.join("runs").exists());

        let valid_client = root.join("valid-client");
        let valid_hash = make_test_client(&valid_client);
        assert!(prepare_hardware_test(
            &valid_client,
            &valid_hash,
            "device",
            "different",
            &root.join("runs")
        )
        .is_err());

        let client = root.join("invalid-client");
        fs::write(&client, b"not an executable").unwrap();
        fs::set_permissions(&client, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(prepare_hardware_test(
            &client,
            &"0".repeat(64),
            "device",
            "device",
            &root.join("runs")
        )
        .is_err());

        // Synthetic thin x86_64 Mach-O header: architecture rejection only;
        // this test file is never executed or linked as a client.
        let wrong_arch = root.join("x86-test-client");
        let x86_header = [0xcf, 0xfa, 0xed, 0xfe, 0x07, 0, 0, 1, 1, 2, 3, 4];
        fs::write(&wrong_arch, x86_header).unwrap();
        fs::set_permissions(&wrong_arch, fs::Permissions::from_mode(0o700)).unwrap();
        let wrong_hash = format!("{:x}", Sha256::digest(x86_header));
        assert!(prepare_hardware_test(
            &wrong_arch,
            &wrong_hash,
            "device",
            "device",
            &root.join("runs")
        )
        .is_err());

        assert!(!root.join("runs").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn refuses_symlink_and_hash_mismatch_and_keeps_reservations_unique() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let client = root.join("client");
        let hash = make_test_client(&client);
        let link = root.join("client-link");
        symlink(&client, &link).unwrap();
        assert!(
            prepare_hardware_test(&link, &hash, "device", "device", &root.join("runs")).is_err()
        );
        assert!(prepare_hardware_test(
            &client,
            &"f".repeat(64),
            "device",
            "device",
            &root.join("runs")
        )
        .is_err());

        let first =
            prepare_hardware_test(&client, &hash, "device", "device", &root.join("runs")).unwrap();
        let marker = first.output_directory.join("preserve.me");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&marker)
            .unwrap();
        let second =
            prepare_hardware_test(&client, &hash, "device", "device", &root.join("runs")).unwrap();
        assert_ne!(first.output_directory, second.output_directory);
        assert!(marker.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
