//! Resolve and verify the fixed ARM64 test sidecar resource.
//!
//! This module intentionally provides no process-start API. Only the harmless
//! test sidecar is packaged in D3-B4; production process execution remains gated.

use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
};

/// Full-file digest approved independently for the checked-in ad-hoc signed
/// harmless test sidecar. This is not derived from the candidate at runtime.
#[allow(dead_code)] // Also consumed by the explicit test-sidecar build script.
pub const ARM64_TEST_SIDECAR_SHA256_HEX: &str =
    "09dc4296d9f347fccd2e4fe8aa62a7e39160a04be6a652e2b79adbb0de360729";
const ARM64_TEST_SIDECAR_SHA256: [u8; 32] = [
    0x09, 0xdc, 0x42, 0x96, 0xd9, 0xf3, 0x47, 0xfc, 0xcd, 0x2e, 0x4f, 0xe8, 0xaa, 0x62, 0xa7, 0xe3,
    0x91, 0x60, 0xa0, 0x4b, 0xe6, 0xa6, 0x52, 0xe2, 0xb7, 0x9a, 0xdb, 0xb0, 0xde, 0x36, 0x07, 0x29,
];

pub(crate) fn trusted_arm64_test_sidecar() -> TrustedSha256 {
    TrustedSha256::from_bytes(ARM64_TEST_SIDECAR_SHA256)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrustedSha256([u8; 32]);

impl TrustedSha256 {
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SidecarResource {
    Arm64Test,
}

impl SidecarResource {
    fn relative_path(self) -> PathBuf {
        match self {
            Self::Arm64Test => PathBuf::from("sidecars/macos-aarch64/beblog-test-sidecar"),
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)] // Retained as verification evidence; never grants execution authority.
pub(crate) struct ResolvedSidecar {
    path: PathBuf,
    sha256: [u8; 32],
}

impl ResolvedSidecar {
    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    #[cfg(test)]
    pub(crate) fn sha256(&self) -> [u8; 32] {
        self.sha256
    }
}

pub(crate) struct SidecarResolver;

impl SidecarResolver {
    /// `resource_dir` must come from Tauri's app resource directory, never UI input.
    pub(crate) fn resolve(
        resource_dir: &Path,
        resource: SidecarResource,
        trusted_digest: TrustedSha256,
    ) -> io::Result<ResolvedSidecar> {
        let supplied_root = fs::symlink_metadata(resource_dir)?;
        if supplied_root.file_type().is_symlink() || !supplied_root.file_type().is_dir() {
            return Err(invalid(
                "Sidecar-Ressourcenwurzel darf kein Symlink sein und muss ein Verzeichnis sein",
            ));
        }
        let root = fs::canonicalize(resource_dir)?;
        if !fs::symlink_metadata(&root)?.file_type().is_dir() {
            return Err(invalid(
                "Sidecar-Ressourcenwurzel ist kein reguläres Verzeichnis",
            ));
        }
        let relative = resource.relative_path();
        let mut current = root.clone();
        let components = relative.components().collect::<Vec<_>>();
        for (index, component) in components.iter().enumerate() {
            current.push(component.as_os_str());
            let metadata = fs::symlink_metadata(&current).map_err(|error| {
                if error.kind() == io::ErrorKind::NotFound {
                    io::Error::new(
                        io::ErrorKind::NotFound,
                        format!("Sidecar-Ressource fehlt: {}", current.display()),
                    )
                } else {
                    error
                }
            })?;
            if metadata.file_type().is_symlink() {
                return Err(invalid("Sidecar-Pfad enthält einen Symlink"));
            }
            let is_file = index + 1 == components.len();
            if (is_file && !metadata.file_type().is_file())
                || (!is_file && !metadata.file_type().is_dir())
            {
                return Err(invalid("Sidecar-Pfad enthält einen unerwarteten Dateityp"));
            }
            if fs::canonicalize(&current)? != current {
                return Err(invalid("Sidecar-Pfad wird unerwartet aufgelöst"));
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(&current)?.permissions().mode() & 0o111 == 0 {
                return Err(invalid("Sidecar-Datei ist nicht ausführbar"));
            }
        }

        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let mut file = options.open(&current)?;
        let opened_metadata = file.metadata()?;
        if !opened_metadata.file_type().is_file() {
            return Err(invalid(
                "Geöffnete Sidecar-Ressource ist kein reguläres File",
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let listed_metadata = fs::symlink_metadata(&current)?;
            if opened_metadata.dev() != listed_metadata.dev()
                || opened_metadata.ino() != listed_metadata.ino()
            {
                return Err(invalid("Sidecar-Datei änderte sich während der Prüfung"));
            }
        }
        let digest = hash_reader(&mut file)?;
        if digest != trusted_digest.0 {
            return Err(invalid(
                "SHA-256 des Sidecar-Binaries stimmt nicht mit dem Trust-Anchor überein",
            ));
        }
        validate_arm64_macho(&current)?;
        validate_adhoc_signature(&current)?;
        Ok(ResolvedSidecar {
            path: current,
            sha256: digest,
        })
    }

    /// Test-only bridge: the process runner can resolve only this test harness,
    /// whose path is provided by the OS test runtime and never by a caller/UI.
    /// The digest is recorded for test evidence, not checked against a production
    /// trust anchor; this method is not present in release builds.
    #[cfg(test)]
    pub(crate) fn resolve_test_harness() -> io::Result<ResolvedSidecar> {
        let path = std::env::current_exe()?;
        let mut file = fs::File::open(&path)?;
        let digest = hash_reader(&mut file)?;
        Ok(ResolvedSidecar {
            path,
            sha256: digest,
        })
    }
}

fn validate_arm64_macho(path: &Path) -> io::Result<()> {
    let mut file = fs::File::open(path)?;
    let mut header = [0_u8; 8];
    file.read_exact(&mut header)?;
    // A thin 64-bit little-endian Mach-O header and CPU_TYPE_ARM64 are required.
    // Fat/universal binaries and all non-arm64 executables are deliberately rejected.
    if header[..4] != [0xcf, 0xfa, 0xed, 0xfe] {
        return Err(invalid("Sidecar muss ein dünnes 64-Bit-ARM64-Mach-O sein"));
    }
    let cpu_type = u32::from_le_bytes(header[4..8].try_into().expect("fixed header slice"));
    if cpu_type != 0x0100_000c {
        return Err(invalid("Sidecar-Architektur ist nicht ARM64"));
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn validate_adhoc_signature(path: &Path) -> io::Result<()> {
    use std::ptr;
    use std::{ffi::CString, os::unix::ffi::OsStrExt};

    type CfTypeRef = *const std::ffi::c_void;
    type CfUrlRef = CfTypeRef;
    type SecStaticCodeRef = CfTypeRef;
    type SecRequirementRef = CfTypeRef;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFURLCreateFromFileSystemRepresentation(
            allocator: CfTypeRef,
            buffer: *const u8,
            buf_len: isize,
            is_directory: bool,
        ) -> CfUrlRef;
        fn CFDictionaryGetValue(dictionary: CfTypeRef, key: CfTypeRef) -> CfTypeRef;
        fn CFNumberGetValue(number: CfTypeRef, number_type: isize, value: *mut i32) -> u8;
        fn CFRelease(value: CfTypeRef);
    }

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        fn SecStaticCodeCreateWithPath(
            path: CfUrlRef,
            flags: u32,
            static_code: *mut SecStaticCodeRef,
        ) -> i32;
        fn SecStaticCodeCheckValidity(
            static_code: SecStaticCodeRef,
            flags: u32,
            requirement: SecRequirementRef,
        ) -> i32;
        fn SecCodeCopySigningInformation(
            code: CfTypeRef,
            flags: u32,
            information: *mut CfTypeRef,
        ) -> i32;
        static kSecCodeInfoFlags: CfTypeRef;
    }

    const K_SEC_CS_SIGNING_INFORMATION: u32 = 1 << 1;
    const K_SEC_CODE_SIGNATURE_ADHOC: i32 = 0x0002;
    const K_CF_NUMBER_SINT32_TYPE: isize = 3;
    let path_bytes = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| invalid("Sidecar-Pfad enthält ein NUL-Byte"))?;
    let url = unsafe {
        CFURLCreateFromFileSystemRepresentation(
            ptr::null(),
            path_bytes.as_ptr().cast(),
            path_bytes.as_bytes().len() as isize,
            false,
        )
    };
    if url.is_null() {
        return Err(invalid("Sidecar-Datei-URL konnte nicht erstellt werden"));
    }
    let mut code = ptr::null();
    let create_status = unsafe { SecStaticCodeCreateWithPath(url, 0, &mut code) };
    unsafe { CFRelease(url) };
    if create_status != 0 || code.is_null() {
        return Err(invalid(format!(
            "Signatur nicht lesbar (OSStatus {create_status})"
        )));
    }
    let valid_status = unsafe { SecStaticCodeCheckValidity(code, 0, ptr::null()) };
    if valid_status != 0 {
        unsafe { CFRelease(code) };
        return Err(invalid(format!(
            "Code-Signatur ungültig (OSStatus {valid_status})"
        )));
    }
    let mut information = ptr::null();
    let info_status = unsafe {
        SecCodeCopySigningInformation(code, K_SEC_CS_SIGNING_INFORMATION, &mut information)
    };
    if info_status != 0 || information.is_null() {
        if !information.is_null() {
            unsafe { CFRelease(information) };
        }
        unsafe { CFRelease(code) };
        return Err(invalid(format!(
            "Signaturinformationen fehlen (OSStatus {info_status})"
        )));
    }
    let flags_number = unsafe { CFDictionaryGetValue(information, kSecCodeInfoFlags) };
    let mut flags = 0_i32;
    let has_flags = !flags_number.is_null()
        && unsafe { CFNumberGetValue(flags_number, K_CF_NUMBER_SINT32_TYPE, &mut flags) != 0 };
    unsafe {
        CFRelease(information);
        CFRelease(code);
    }
    if !has_flags || flags & K_SEC_CODE_SIGNATURE_ADHOC == 0 {
        return Err(invalid("Sidecar besitzt keine gültige Ad-hoc-Signatur"));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn validate_adhoc_signature(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Ad-hoc-Code-Signaturprüfung ist nur auf macOS verfügbar",
    ))
}

fn hash_reader(reader: &mut impl Read) -> io::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn test_root() -> PathBuf {
        std::env::temp_dir().join(format!("beblog-sidecar-{}", uuid::Uuid::new_v4().simple()))
    }

    fn resource_path(root: &Path) -> PathBuf {
        root.join(SidecarResource::Arm64Test.relative_path())
    }

    fn fixture(root: &Path, executable: bool) -> PathBuf {
        let path = resource_path(root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            include_bytes!("../sidecars/macos-aarch64/beblog-test-sidecar"),
        )
        .unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(if executable { 0o755 } else { 0o644 });
        fs::set_permissions(&path, permissions).unwrap();
        path
    }

    #[test]
    fn resolves_fixed_resource_only_when_digest_and_executable_status_match() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let path = fixture(&root, true);
        let resolved = SidecarResolver::resolve(
            &root,
            SidecarResource::Arm64Test,
            trusted_arm64_test_sidecar(),
        )
        .unwrap();
        assert_eq!(resolved.path(), fs::canonicalize(path).unwrap());
        assert_eq!(resolved.sha256(), ARM64_TEST_SIDECAR_SHA256);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_missing_binary_wrong_digest_and_non_executable_file() {
        let missing = test_root();
        fs::create_dir_all(&missing).unwrap();
        assert_eq!(
            SidecarResolver::resolve(
                &missing,
                SidecarResource::Arm64Test,
                trusted_arm64_test_sidecar(),
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotFound
        );
        fs::remove_dir_all(&missing).unwrap();

        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        fixture(&root, true);
        assert!(SidecarResolver::resolve(
            &root,
            SidecarResource::Arm64Test,
            TrustedSha256::from_bytes([0; 32]),
        )
        .is_err());
        fs::remove_dir_all(&root).unwrap();

        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        fixture(&root, false);
        assert!(SidecarResolver::resolve(
            &root,
            SidecarResource::Arm64Test,
            trusted_arm64_test_sidecar(),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_symlink_resource_and_symlinked_directory() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let path = fixture(&root, true);
        let external = root.join("external-binary");
        fs::rename(&path, &external).unwrap();
        symlink(&external, &path).unwrap();
        assert!(SidecarResolver::resolve(
            &root,
            SidecarResource::Arm64Test,
            trusted_arm64_test_sidecar(),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();

        let root = test_root();
        let external_root = root.join("external");
        let linked_root = root.join("linked");
        fs::create_dir_all(&linked_root).unwrap();
        fixture(&external_root, true);
        symlink(external_root.join("sidecars"), linked_root.join("sidecars")).unwrap();
        assert!(SidecarResolver::resolve(
            &linked_root,
            SidecarResource::Arm64Test,
            trusted_arm64_test_sidecar(),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_symlinked_resource_root() {
        let root = test_root();
        let actual = root.join("actual");
        let alias = root.join("alias");
        fs::create_dir_all(&root).unwrap();
        fixture(&actual, true);
        symlink(&actual, &alias).unwrap();
        assert!(SidecarResolver::resolve(
            &alias,
            SidecarResource::Arm64Test,
            trusted_arm64_test_sidecar(),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_wrong_architecture_and_unapproved_hash_change() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let path = fixture(&root, true);
        let mut bytes = fs::read(&path).unwrap();
        // Keep a valid thin Mach-O magic but change CPU_TYPE_ARM64 to x86_64.
        bytes[4..8].copy_from_slice(&0x0100_0007_u32.to_le_bytes());
        fs::write(&path, &bytes).unwrap();
        let changed_digest: [u8; 32] = Sha256::digest(&bytes).into();
        let error = SidecarResolver::resolve(
            &root,
            SidecarResource::Arm64Test,
            TrustedSha256::from_bytes(changed_digest),
        )
        .unwrap_err();
        assert!(error.to_string().contains("ARM64"));
        fs::remove_dir_all(root).unwrap();

        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let path = fixture(&root, true);
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(&path, bytes).unwrap();
        assert!(SidecarResolver::resolve(
            &root,
            SidecarResource::Arm64Test,
            trusted_arm64_test_sidecar(),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn rejects_missing_and_invalid_adhoc_signatures() {
        let unsigned = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("sidecars/test-fixtures/beblog-test-sidecar-unsigned");
        assert!(validate_adhoc_signature(&unsigned).is_err());

        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("tampered-signed-sidecar");
        fs::write(
            &path,
            include_bytes!("../sidecars/macos-aarch64/beblog-test-sidecar"),
        )
        .unwrap();
        let mut bytes = fs::read(&path).unwrap();
        bytes[4096] ^= 0x01;
        fs::write(&path, bytes).unwrap();
        assert!(validate_adhoc_signature(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_harness_identity_is_resolved_without_caller_selected_program_path() {
        let resolved = SidecarResolver::resolve_test_harness().unwrap();
        assert_eq!(resolved.path(), std::env::current_exe().unwrap());
        assert_ne!(resolved.sha256(), [0; 32]);
    }
}
