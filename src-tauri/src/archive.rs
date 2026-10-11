use crate::model::StartScanRequest;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static CAPTURE_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ArchiveStatus {
    InProgress,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    pub name: String,
    pub media_type: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureManifest {
    pub schema_version: u8,
    pub capture_id: String,
    pub status: ArchiveStatus,
    pub frame: u8,
    pub pixel_format: String,
    pub resolution_dpi: u16,
    pub expected_raw_bytes: u64,
    pub files: Vec<FileMetadata>,
    pub failure: Option<String>,
}

#[derive(Debug)]
pub struct PendingCapture {
    pub id: String,
    pub directory: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct CaptureProduct {
    pub raw: Vec<u8>,
    pub tiff: Vec<u8>,
    pub expected_raw_bytes: u64,
}

#[derive(Clone)]
pub struct ArchiveManager {
    root: PathBuf,
}

impl ArchiveManager {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[cfg(test)]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn next_capture_id() -> String {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let sequence = CAPTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        format!("capture-{millis}-{sequence:06}")
    }

    pub fn begin(&self, request: &StartScanRequest) -> io::Result<PendingCapture> {
        self.begin_with_id(&Self::next_capture_id(), request)
    }

    fn begin_with_id(&self, id: &str, request: &StartScanRequest) -> io::Result<PendingCapture> {
        fs::create_dir_all(&self.root)?;
        let directory = self.root.join(id);
        fs::create_dir(&directory)?; // fails on collision; never reuses a capture
        let manifest = CaptureManifest {
            schema_version: 1,
            capture_id: id.to_string(),
            status: ArchiveStatus::InProgress,
            frame: request.frame,
            pixel_format: "RGB8".into(),
            resolution_dpi: 600,
            expected_raw_bytes: 1_438_200,
            files: Vec::new(),
            failure: None,
        };
        write_manifest(&directory, &manifest)?;
        Ok(PendingCapture {
            id: id.to_string(),
            directory,
        })
    }

    pub fn finalize(
        &self,
        pending: &PendingCapture,
        product: CaptureProduct,
    ) -> io::Result<CaptureManifest> {
        let raw = write_original(
            &pending.directory,
            "capture.raw",
            "application/x-beblog-raw",
            &product.raw,
        )?;
        let tiff = write_original(
            &pending.directory,
            "capture.tiff",
            "image/tiff",
            &product.tiff,
        )?;
        let mut manifest = read_manifest(&pending.directory)?;
        manifest.status = ArchiveStatus::Completed;
        manifest.expected_raw_bytes = product.expected_raw_bytes;
        manifest.files = vec![raw, tiff];
        write_manifest(&pending.directory, &manifest)?;
        Ok(manifest)
    }

    pub fn mark(
        &self,
        pending: &PendingCapture,
        status: ArchiveStatus,
        reason: impl Into<String>,
    ) -> io::Result<()> {
        let mut manifest = read_manifest(&pending.directory)?;
        manifest.status = status;
        manifest.failure = Some(reason.into());
        write_manifest(&pending.directory, &manifest)
    }

    pub fn recover_incomplete(&self) -> io::Result<Vec<String>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut recovered = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let directory = entry?.path();
            if !directory.is_dir() || !directory.join("manifest.json").is_file() {
                continue;
            }
            let mut manifest = read_manifest(&directory)?;
            if manifest.status == ArchiveStatus::InProgress {
                manifest.status = ArchiveStatus::Interrupted;
                manifest.failure = Some("Application ended before capture finalization".into());
                write_manifest(&directory, &manifest)?;
                recovered.push(manifest.capture_id);
            }
        }
        Ok(recovered)
    }
}

fn write_original(
    directory: &Path,
    name: &str,
    media_type: &str,
    bytes: &[u8],
) -> io::Result<FileMetadata> {
    let path = directory.join(name);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    Ok(FileMetadata {
        name: name.into(),
        media_type: media_type.into(),
        bytes: bytes.len() as u64,
        sha256,
    })
}

fn read_manifest(directory: &Path) -> io::Result<CaptureManifest> {
    let bytes = fs::read(directory.join("manifest.json"))?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn write_manifest(directory: &Path, manifest: &CaptureManifest) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
    let temporary = directory.join("manifest.json.tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(temporary, directory.join("manifest.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SimulationMode;

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "beblog-{label}-{}",
            ArchiveManager::next_capture_id()
        ))
    }
    fn request() -> StartScanRequest {
        StartScanRequest {
            frame: 1,
            simulation: SimulationMode::Success,
        }
    }

    #[test]
    fn refuses_capture_directory_collision() {
        let root = temp_root("collision");
        let archive = ArchiveManager::new(root.clone());
        archive.begin_with_id("fixed", &request()).unwrap();
        assert_eq!(
            archive
                .begin_with_id("fixed", &request())
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finalizes_originals_with_checksums_and_no_overwrite() {
        let root = temp_root("finalize");
        let archive = ArchiveManager::new(root.clone());
        let pending = archive.begin(&request()).unwrap();
        let product = CaptureProduct {
            raw: b"raw-fixture".to_vec(),
            tiff: b"tiff-fixture".to_vec(),
            expected_raw_bytes: 1_438_200,
        };
        let manifest = archive.finalize(&pending, product).unwrap();
        assert_eq!(manifest.status, ArchiveStatus::Completed);
        assert_eq!(manifest.files.len(), 2);
        assert!(manifest.files.iter().all(|file| file.sha256.len() == 64));
        assert!(archive
            .finalize(
                &pending,
                CaptureProduct {
                    raw: vec![],
                    tiff: vec![],
                    expected_raw_bytes: 0
                }
            )
            .is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovers_interrupted_manifest() {
        let root = temp_root("recovery");
        let archive = ArchiveManager::new(root.clone());
        let pending = archive.begin(&request()).unwrap();
        assert_eq!(
            archive.recover_incomplete().unwrap(),
            vec![pending.id.clone()]
        );
        assert_eq!(
            read_manifest(&pending.directory).unwrap().status,
            ArchiveStatus::Interrupted
        );
        assert!(archive.recover_incomplete().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
