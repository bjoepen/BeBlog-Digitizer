//! Resolve a fixed bundled sidecar resource without PATH lookup or shell use.
//!
//! Resolution verifies identity only; this module intentionally provides no
//! process-start API. Production process execution remains gated for D3-B3c.

use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TrustedSha256([u8; 32]);

impl TrustedSha256 {
    pub(crate) const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SidecarResource {
    MinoltaBatchFrame,
}

impl SidecarResource {
    fn relative_path(self) -> PathBuf {
        match self {
            Self::MinoltaBatchFrame => PathBuf::from(format!(
                "sidecars/macos-{}/minolta-batch-frame",
                std::env::consts::ARCH
            )),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ResolvedSidecar {
    path: PathBuf,
    sha256: [u8; 32],
}

impl ResolvedSidecar {
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
        root.join(SidecarResource::MinoltaBatchFrame.relative_path())
    }

    fn fixture(root: &Path, executable: bool) -> ([u8; 32], PathBuf) {
        let path = resource_path(root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"harmless offline sidecar identity fixture").unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(if executable { 0o755 } else { 0o644 });
        fs::set_permissions(&path, permissions).unwrap();
        let digest: [u8; 32] = Sha256::digest(fs::read(&path).unwrap()).into();
        (digest, path)
    }

    #[test]
    fn resolves_fixed_resource_only_when_digest_and_executable_status_match() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let (digest, path) = fixture(&root, true);
        let resolved = SidecarResolver::resolve(
            &root,
            SidecarResource::MinoltaBatchFrame,
            TrustedSha256::from_bytes(digest),
        )
        .unwrap();
        assert_eq!(resolved.path(), fs::canonicalize(path).unwrap());
        assert_eq!(resolved.sha256(), digest);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_missing_binary_wrong_digest_and_non_executable_file() {
        let missing = test_root();
        fs::create_dir_all(&missing).unwrap();
        assert_eq!(
            SidecarResolver::resolve(
                &missing,
                SidecarResource::MinoltaBatchFrame,
                TrustedSha256::from_bytes([0; 32]),
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotFound
        );
        fs::remove_dir_all(&missing).unwrap();

        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let (_digest, _) = fixture(&root, true);
        assert!(SidecarResolver::resolve(
            &root,
            SidecarResource::MinoltaBatchFrame,
            TrustedSha256::from_bytes([0; 32]),
        )
        .is_err());
        fs::remove_dir_all(&root).unwrap();

        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let (digest, _) = fixture(&root, false);
        assert!(SidecarResolver::resolve(
            &root,
            SidecarResource::MinoltaBatchFrame,
            TrustedSha256::from_bytes(digest),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_symlink_resource_and_symlinked_directory() {
        let root = test_root();
        fs::create_dir_all(&root).unwrap();
        let (digest, path) = fixture(&root, true);
        let external = root.join("external-binary");
        fs::rename(&path, &external).unwrap();
        symlink(&external, &path).unwrap();
        assert!(SidecarResolver::resolve(
            &root,
            SidecarResource::MinoltaBatchFrame,
            TrustedSha256::from_bytes(digest),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();

        let root = test_root();
        let external_root = root.join("external");
        let linked_root = root.join("linked");
        fs::create_dir_all(&linked_root).unwrap();
        let (digest, _) = fixture(&external_root, true);
        symlink(external_root.join("sidecars"), linked_root.join("sidecars")).unwrap();
        assert!(SidecarResolver::resolve(
            &linked_root,
            SidecarResource::MinoltaBatchFrame,
            TrustedSha256::from_bytes(digest),
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
        let (digest, _) = fixture(&actual, true);
        symlink(&actual, &alias).unwrap();
        assert!(SidecarResolver::resolve(
            &alias,
            SidecarResource::MinoltaBatchFrame,
            TrustedSha256::from_bytes(digest),
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_harness_identity_is_resolved_without_caller_selected_program_path() {
        let resolved = SidecarResolver::resolve_test_harness().unwrap();
        assert_eq!(resolved.path(), std::env::current_exe().unwrap());
        assert_ne!(resolved.sha256(), [0; 32]);
    }
}
