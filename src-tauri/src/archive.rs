use crate::batch_plan::{D6BatchManifest, D6BatchRequest, D6BatchStatus, D6FrameStatus};
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
static BATCH_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static D6_BATCH_TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const BATCH_ID_ATTEMPTS: usize = 16;

fn legacy_capture_kind() -> CaptureKind {
    CaptureKind::UnclassifiedLegacy
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BatchArchiveStatus {
    InProgress,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchManifest {
    pub schema_version: u8,
    pub batch_id: String,
    pub frames: Vec<u8>,
    pub status: BatchArchiveStatus,
    pub completed_frames: Vec<u8>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureKind {
    Simulation,
    Production,
    OfflineImport,
    UnclassifiedLegacy,
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
    #[serde(default)]
    pub batch_id: Option<String>,
    pub status: ArchiveStatus,
    #[serde(default = "legacy_capture_kind")]
    pub capture_kind: CaptureKind,
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
    pub kind: CaptureKind,
}

#[derive(Debug, PartialEq, Eq)]
pub struct OfflineCaptureProduct {
    pub raw: Vec<u8>,
    pub tiff: Vec<u8>,
    pub params_txt: Vec<u8>,
    pub client_log: Vec<u8>,
    pub(crate) profile: crate::offline_import::ValidatedRgb8Profile,
}

#[derive(Clone)]
pub struct ArchiveManager {
    root: PathBuf,
}

enum BeginBatchError {
    Collision,
    Io(io::Error),
}

impl ArchiveManager {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) fn root_path(&self) -> &Path {
        &self.root
    }

    /// Persist a D6 plan in its own schema and namespace. D1 journals remain in
    /// `batches/` and retain their existing format and recovery behavior.
    pub fn begin_d6_batch(&self, request: &D6BatchRequest) -> io::Result<D6BatchManifest> {
        request.validate().map_err(invalid_data)?;
        let directory = self.root.join("batches").join("d6");
        fs::create_dir_all(&directory)?;
        for _ in 0..BATCH_ID_ATTEMPTS {
            let manifest = D6BatchManifest::planned(request);
            let path = d6_batch_path(&directory, manifest.batch_id.as_str());
            match write_new_d6_manifest(&path, &manifest) {
                Ok(()) => return Ok(manifest),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Eindeutige D6-Batch-ID nach mehreren Versuchen nicht verfügbar",
        ))
    }

    /// Reserve the next frame and durably associate its unique capture ID before
    /// artifact validation/finalization is allowed to mark that capture complete.
    pub fn begin_d6_frame(
        &self,
        batch_id: &str,
        frame: u8,
        capture_id: &str,
    ) -> io::Result<String> {
        validate_archive_id(capture_id)?;
        self.update_d6_manifest(batch_id, |manifest| {
            if !matches!(
                manifest.status,
                D6BatchStatus::Planned | D6BatchStatus::Running
            ) {
                return Err(invalid_data("D6-Batch ist nicht mehr aktiv"));
            }
            let Some(index) = manifest
                .frames
                .iter()
                .position(|entry| entry.frame == frame)
            else {
                return Err(invalid_data("Frame ist nicht Teil des D6-Batchplans"));
            };
            if manifest.frames[..index]
                .iter()
                .any(|entry| entry.status != D6FrameStatus::Completed)
                || manifest.frames[index].status != D6FrameStatus::Planned
            {
                return Err(invalid_data(
                    "D6-Frames müssen strikt der Reihe nach starten",
                ));
            }
            if manifest
                .frames
                .iter()
                .any(|entry| entry.capture_id.as_deref() == Some(capture_id))
            {
                return Err(invalid_data(
                    "Capture-ID ist bereits einem D6-Frame zugeordnet",
                ));
            }
            manifest.status = D6BatchStatus::Running;
            manifest.frames[index].status = D6FrameStatus::Running;
            manifest.frames[index].capture_id = Some(capture_id.to_string());
            Ok(manifest.frames[index].frame_id.as_str().to_string())
        })
    }

    /// Persist successful artifact validation and capture association before a
    /// caller finalizes the archive capture. This method does not itself validate
    /// files and is deliberately not connected to the D5 import path in D6-B1.
    pub fn mark_d6_frame_validated(
        &self,
        batch_id: &str,
        frame: u8,
        capture_id: &str,
    ) -> io::Result<()> {
        self.update_d6_manifest(batch_id, |manifest| {
            let entry = d6_frame_mut(manifest, frame)?;
            if entry.status != D6FrameStatus::Running
                || entry.capture_id.as_deref() != Some(capture_id)
            {
                return Err(invalid_data(
                    "D6-Validierung benötigt den laufenden, zugeordneten Capture",
                ));
            }
            entry.status = D6FrameStatus::Validated;
            Ok(())
        })
    }

    /// Journal completion is permitted only after the prior durable Validated
    /// association. The future orchestrator must call this after archive
    /// finalization succeeds; failures leave the capture and journal diagnosable.
    pub fn complete_d6_frame(&self, batch_id: &str, frame: u8) -> io::Result<()> {
        self.update_d6_manifest(batch_id, |manifest| {
            let entry = d6_frame_mut(manifest, frame)?;
            if entry.status != D6FrameStatus::Validated || entry.capture_id.is_none() {
                return Err(invalid_data(
                    "D6-Capture kann ohne persistierte Validierung nicht abgeschlossen werden",
                ));
            }
            entry.status = D6FrameStatus::Completed;
            if manifest
                .frames
                .iter()
                .all(|frame| frame.status == D6FrameStatus::Completed)
            {
                manifest.status = D6BatchStatus::Completed;
            }
            Ok(())
        })
    }

    pub fn fail_d6_frame(
        &self,
        batch_id: &str,
        frame: u8,
        detail: impl Into<String>,
    ) -> io::Result<()> {
        let detail = detail.into();
        self.update_d6_manifest(batch_id, |manifest| {
            let entry = d6_frame_mut(manifest, frame)?;
            if !matches!(
                entry.status,
                D6FrameStatus::Running | D6FrameStatus::Validated
            ) {
                return Err(invalid_data("Nur ein aktiver D6-Frame kann fehlschlagen"));
            }
            entry.status = D6FrameStatus::Failed;
            entry.failure = Some(detail.clone());
            manifest.status = D6BatchStatus::Failed;
            manifest.failure = Some(detail);
            Ok(())
        })
    }

    pub fn cancel_d6_batch(
        &self,
        batch_id: &str,
        active_frame: Option<u8>,
        detail: impl Into<String>,
    ) -> io::Result<()> {
        let detail = detail.into();
        self.update_d6_manifest(batch_id, |manifest| {
            if active_frame.is_none()
                && manifest.frames.iter().any(|entry| {
                    matches!(
                        entry.status,
                        D6FrameStatus::Running | D6FrameStatus::Validated
                    )
                })
            {
                return Err(invalid_data(
                    "Aktiver D6-Frame muss beim Abbruchjournal benannt werden",
                ));
            }
            if let Some(frame) = active_frame {
                let entry = d6_frame_mut(manifest, frame)?;
                if !matches!(
                    entry.status,
                    D6FrameStatus::Running | D6FrameStatus::Validated
                ) {
                    return Err(invalid_data(
                        "Nur ein aktiver D6-Frame kann abgebrochen werden",
                    ));
                }
                entry.status = D6FrameStatus::Cancelled;
                entry.failure = Some(detail.clone());
            }
            if !matches!(
                manifest.status,
                D6BatchStatus::Planned | D6BatchStatus::Running
            ) {
                return Err(invalid_data("D6-Batch ist bereits abgeschlossen"));
            }
            manifest.status = D6BatchStatus::Cancelled;
            manifest.failure = Some(detail);
            Ok(())
        })
    }

    fn update_d6_manifest<T>(
        &self,
        batch_id: &str,
        update: impl FnOnce(&mut D6BatchManifest) -> io::Result<T>,
    ) -> io::Result<T> {
        validate_archive_id(batch_id)?;
        let directory = self.root.join("batches").join("d6");
        let path = d6_batch_path(&directory, batch_id);
        let mut manifest: D6BatchManifest = serde_json::from_slice(&fs::read(&path)?)?;
        manifest.validate().map_err(invalid_data)?;
        if manifest.batch_id.as_str() != batch_id {
            return Err(invalid_data(
                "D6-Journal-ID stimmt nicht mit dem Dateinamen überein",
            ));
        }
        let result = update(&mut manifest)?;
        write_d6_manifest(&path, &manifest)?;
        Ok(result)
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

    pub fn begin(
        &self,
        request: &StartScanRequest,
        kind: CaptureKind,
    ) -> io::Result<PendingCapture> {
        ensure_public_capture_kind(kind)?;
        self.begin_with_id(
            &Self::next_capture_id(),
            request,
            kind,
            None,
            600,
            1_438_200,
        )
    }

    pub(crate) fn begin_offline_import(
        &self,
        frame: u8,
        profile: crate::offline_import::ValidatedRgb8Profile,
    ) -> io::Result<PendingCapture> {
        if !(1..=6).contains(&frame) {
            return Err(invalid_data("Offline-Import unterstützt Frames 1–6"));
        }
        let request = StartScanRequest {
            frame,
            simulation: crate::model::SimulationMode::Success,
        };
        for _ in 0..BATCH_ID_ATTEMPTS {
            let id = format!("offline-{}", uuid::Uuid::new_v4().simple());
            match self.begin_with_id(
                &id,
                &request,
                CaptureKind::OfflineImport,
                None,
                profile.resolution_dpi,
                profile.raw_bytes,
            ) {
                Ok(pending) => return Ok(pending),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "Eindeutige Capture-ID nach mehreren Versuchen nicht verfügbar",
        ))
    }

    pub fn begin_batch_frame(
        &self,
        batch_id: &str,
        request: &StartScanRequest,
        kind: CaptureKind,
    ) -> io::Result<PendingCapture> {
        ensure_public_capture_kind(kind)?;
        self.begin_with_id(
            &Self::next_capture_id(),
            request,
            kind,
            Some(batch_id.to_string()),
            600,
            1_438_200,
        )
    }

    #[cfg(test)]
    pub fn begin_batch(&self, batch_id: &str, frames: &[u8]) -> io::Result<()> {
        match self.begin_batch_once(batch_id, frames) {
            Ok(()) => Ok(()),
            Err(BeginBatchError::Collision) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Batchjournal-ID bereits vorhanden: {batch_id}"),
            )),
            Err(BeginBatchError::Io(error)) => Err(error),
        }
    }

    pub fn begin_batch_unique(&self, frames: &[u8]) -> io::Result<String> {
        self.begin_batch_unique_with(frames, || {
            format!("batch-{}", uuid::Uuid::new_v4().simple())
        })
    }

    fn begin_batch_unique_with<F>(&self, frames: &[u8], mut next_id: F) -> io::Result<String>
    where
        F: FnMut() -> String,
    {
        for _ in 0..BATCH_ID_ATTEMPTS {
            let id = next_id();
            match self.begin_batch_once(&id, frames) {
                Ok(()) => return Ok(id),
                Err(BeginBatchError::Collision) => continue,
                Err(BeginBatchError::Io(error)) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("Batchjournal-ID nach {BATCH_ID_ATTEMPTS} Versuchen nicht verfügbar"),
        ))
    }

    fn begin_batch_once(&self, batch_id: &str, frames: &[u8]) -> Result<(), BeginBatchError> {
        validate_archive_id(batch_id).map_err(BeginBatchError::Io)?;
        let directory = self.root.join("batches");
        fs::create_dir_all(&directory).map_err(BeginBatchError::Io)?;
        let manifest = BatchManifest {
            schema_version: 1,
            batch_id: batch_id.into(),
            frames: frames.to_vec(),
            status: BatchArchiveStatus::InProgress,
            completed_frames: Vec::new(),
            failure: None,
        };
        let path = directory.join(format!("{batch_id}.json"));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|error| {
                if error.kind() == io::ErrorKind::AlreadyExists {
                    BeginBatchError::Collision
                } else {
                    BeginBatchError::Io(error)
                }
            })?;
        serde_json::to_writer_pretty(&mut file, &manifest)
            .map_err(io::Error::other)
            .map_err(BeginBatchError::Io)?;
        file.write_all(b"\n").map_err(BeginBatchError::Io)?;
        file.sync_all().map_err(BeginBatchError::Io)
    }

    pub fn update_batch(
        &self,
        batch_id: &str,
        completed_frame: Option<u8>,
        status: Option<BatchArchiveStatus>,
        failure: Option<String>,
    ) -> io::Result<()> {
        validate_archive_id(batch_id)?;
        let path = self.root.join("batches").join(format!("{batch_id}.json"));
        let mut manifest: BatchManifest = serde_json::from_slice(&fs::read(&path)?)?;
        if manifest.status != BatchArchiveStatus::InProgress {
            return Err(invalid_data("Batchmanifest ist bereits terminal"));
        }
        if let Some(frame) = completed_frame {
            if !manifest.frames.contains(&frame) || manifest.completed_frames.contains(&frame) {
                return Err(invalid_data(
                    "Ungültiger oder doppelter abgeschlossener Batch-Frame",
                ));
            }
            manifest.completed_frames.push(frame);
        }
        if let Some(status) = status {
            manifest.status = status;
            manifest.failure = failure;
        }
        write_batch_manifest(&path, &manifest)
    }

    fn begin_with_id(
        &self,
        id: &str,
        request: &StartScanRequest,
        kind: CaptureKind,
        batch_id: Option<String>,
        resolution_dpi: u16,
        expected_raw_bytes: u64,
    ) -> io::Result<PendingCapture> {
        fs::create_dir_all(&self.root)?;
        let directory = self.root.join(id);
        fs::create_dir(&directory)?; // fails on collision; never reuses a capture
        let manifest = CaptureManifest {
            schema_version: 2,
            capture_id: id.to_string(),
            batch_id,
            status: ArchiveStatus::InProgress,
            capture_kind: kind,
            frame: request.frame,
            pixel_format: "RGB8".into(),
            resolution_dpi,
            expected_raw_bytes,
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
        if product.kind != CaptureKind::Simulation {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "Production-Finalisierung benötigt einen künftig typisierten Prozessnachweis",
            ));
        }
        self.finalize_unchecked(pending, product)
    }

    #[cfg(test)]
    fn finalize_for_archive_validator_test(
        &self,
        pending: &PendingCapture,
        product: CaptureProduct,
    ) -> io::Result<CaptureManifest> {
        self.finalize_unchecked(pending, product)
    }

    fn finalize_unchecked(
        &self,
        pending: &PendingCapture,
        product: CaptureProduct,
    ) -> io::Result<CaptureManifest> {
        let result = self.finalize_inner(pending, product);
        if let Err(error) = &result {
            if let Err(mark_error) = self.mark(
                pending,
                ArchiveStatus::Failed,
                format!("Finalisierung/Validierung: {error}"),
            ) {
                return Err(io::Error::new(
                    error.kind(),
                    format!(
                        "{error}; Capturefehlerstatus konnte ebenfalls nicht persistiert werden: {mark_error}; Recovery erforderlich"
                    ),
                ));
            }
        }
        result
    }

    fn finalize_inner(
        &self,
        pending: &PendingCapture,
        product: CaptureProduct,
    ) -> io::Result<CaptureManifest> {
        let mut manifest = read_manifest(&pending.directory)?;
        if manifest.status != ArchiveStatus::InProgress {
            return Err(invalid_data("Capture ist nicht mehr in Bearbeitung"));
        }
        if manifest.capture_kind != product.kind {
            return Err(invalid_data(
                "Capture-Herkunft stimmt nicht mit Produkt überein",
            ));
        }
        if product.kind == CaptureKind::OfflineImport {
            return Err(invalid_data(
                "Offline-Import muss den profilierten Vier-Artefaktpfad verwenden",
            ));
        }
        if product.expected_raw_bytes != manifest.expected_raw_bytes {
            return Err(invalid_data(format!(
                "Angekündigte RAW-Größe {} entspricht nicht dem Captureprofil {}",
                product.expected_raw_bytes, manifest.expected_raw_bytes
            )));
        }
        if product.expected_raw_bytes == 0
            || (product.kind == CaptureKind::Production
                && product.raw.len() as u64 != product.expected_raw_bytes)
        {
            return Err(invalid_data(format!(
                "RAW-Dateigröße {} entspricht nicht der erwarteten Größe {}",
                product.raw.len(),
                product.expected_raw_bytes
            )));
        }
        validate_raw(&product.raw, product.kind)?;
        validate_tiff(&product.tiff, product.kind)?;

        let raw = write_original(
            &pending.directory,
            "capture.raw",
            "application/x-beblog-raw",
            &product.raw,
        )?;
        manifest.files.push(raw);
        write_manifest(&pending.directory, &manifest)?;
        let tiff = write_original(
            &pending.directory,
            "capture.tiff",
            "image/tiff",
            &product.tiff,
        )?;
        manifest.files.push(tiff);
        validate_manifest_files(&pending.directory, &manifest)?;
        manifest.status = ArchiveStatus::Completed;
        write_manifest(&pending.directory, &manifest)?;
        Ok(manifest)
    }

    pub fn mark(
        &self,
        pending: &PendingCapture,
        status: ArchiveStatus,
        reason: impl Into<String>,
    ) -> io::Result<()> {
        if matches!(status, ArchiveStatus::InProgress | ArchiveStatus::Completed) {
            return Err(invalid_data(
                "Dieser Archivstatus darf nicht über die Fehler-Markierung gesetzt werden",
            ));
        }
        let mut manifest = read_manifest(&pending.directory)?;
        if manifest.status != ArchiveStatus::InProgress {
            return Err(invalid_data(
                "Terminaler Archivstatus darf nicht überschrieben werden",
            ));
        }
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
        let staging = self.root.join(".staging");
        if let Ok(staging_metadata) = fs::symlink_metadata(&staging) {
            if !staging_metadata.file_type().is_dir() {
                return Err(invalid_data("Stagingpfad ist kein echtes Verzeichnis"));
            }
            for entry in fs::read_dir(&staging)? {
                let directory = entry?.path();
                let Ok(directory_metadata) = fs::symlink_metadata(&directory) else {
                    continue;
                };
                let stage_manifest_path = directory.join("stage.json");
                let stage_manifest_metadata = fs::symlink_metadata(&stage_manifest_path);
                if !directory_metadata.file_type().is_dir()
                    || !stage_manifest_metadata
                        .as_ref()
                        .is_ok_and(|metadata| metadata.file_type().is_file())
                {
                    continue;
                }
                let bytes = fs::read(stage_manifest_path)?;
                let mut manifest: crate::offline_import::StagingManifest =
                    serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                if manifest.status == crate::offline_import::StagingStatus::InProgress {
                    manifest.status = crate::offline_import::StagingStatus::Interrupted;
                    manifest.failure =
                        Some("Application ended before offline artifact validation".into());
                    crate::offline_import::write_staging_manifest(&directory, &manifest)?;
                    recovered.push(format!("staging:{}", manifest.capture_id));
                }
            }
        } else if staging.exists() {
            return Err(io::Error::other(
                "Stagingpfad konnte nicht untersucht werden",
            ));
        }
        let batches = self.root.join("batches");
        if batches.exists() {
            for entry in fs::read_dir(&batches)? {
                let path = entry?.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let mut manifest: BatchManifest = serde_json::from_slice(&fs::read(&path)?)?;
                if manifest.status == BatchArchiveStatus::InProgress {
                    manifest.status = BatchArchiveStatus::Interrupted;
                    manifest.failure =
                        Some("Application ended; batch is never resumed automatically".into());
                    write_batch_manifest(&path, &manifest)?;
                    recovered.push(manifest.batch_id);
                }
            }
        }
        let d6_batches = batches.join("d6");
        if let Ok(metadata) = fs::symlink_metadata(&d6_batches) {
            if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
                return Err(invalid_data(
                    "D6-Batchjournalpfad ist kein echtes Verzeichnis",
                ));
            }
            for entry in fs::read_dir(&d6_batches)? {
                let path = entry?.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let mut manifest: D6BatchManifest = serde_json::from_slice(&fs::read(&path)?)?;
                manifest.validate().map_err(invalid_data)?;
                if path.file_stem().and_then(|name| name.to_str())
                    != Some(manifest.batch_id.as_str())
                {
                    return Err(invalid_data(
                        "D6-Journal-ID stimmt nicht mit dem Dateinamen überein",
                    ));
                }
                if matches!(
                    manifest.status,
                    D6BatchStatus::Planned | D6BatchStatus::Running
                ) {
                    manifest.status = D6BatchStatus::Interrupted;
                    manifest.failure =
                        Some("Application ended; D6 batch is never resumed automatically".into());
                    for frame in &mut manifest.frames {
                        if matches!(
                            frame.status,
                            D6FrameStatus::Running | D6FrameStatus::Validated
                        ) {
                            frame.status = D6FrameStatus::Interrupted;
                            frame.failure =
                                Some("Application ended before frame completion".into());
                        }
                    }
                    write_d6_manifest(&path, &manifest)?;
                    recovered.push(manifest.batch_id.as_str().to_string());
                }
            }
        } else if d6_batches.exists() {
            return Err(io::Error::other(
                "D6-Batchjournalpfad konnte nicht untersucht werden",
            ));
        }
        Ok(recovered)
    }

    pub(crate) fn finalize_offline_import(
        &self,
        pending: &PendingCapture,
        product: OfflineCaptureProduct,
    ) -> io::Result<CaptureManifest> {
        let result = self.finalize_offline_inner(pending, product);
        if let Err(error) = &result {
            if let Err(mark_error) = self.mark(
                pending,
                ArchiveStatus::Failed,
                format!("Offline-Import Finalisierung: {error}"),
            ) {
                return Err(io::Error::new(
                    error.kind(),
                    format!("{error}; Fehlerstatus nicht persistiert: {mark_error}"),
                ));
            }
        }
        result
    }

    fn finalize_offline_inner(
        &self,
        pending: &PendingCapture,
        product: OfflineCaptureProduct,
    ) -> io::Result<CaptureManifest> {
        let mut manifest = read_manifest(&pending.directory)?;
        if manifest.status != ArchiveStatus::InProgress
            || manifest.capture_kind != CaptureKind::OfflineImport
        {
            return Err(invalid_data("Capture ist kein offener Offline-Import"));
        }
        let validated_profile = crate::offline_import::validate_artifact_profile(
            manifest.frame,
            &product.raw,
            &product.tiff,
            &product.params_txt,
            &product.client_log,
        )?;
        if product.profile != validated_profile {
            return Err(invalid_data(
                "Übergebene Offline-Profilangabe stimmt nicht mit den Artefakten überein",
            ));
        }
        manifest.resolution_dpi = validated_profile.resolution_dpi;
        manifest.expected_raw_bytes = validated_profile.raw_bytes;
        let artifacts = [
            (
                "capture.raw",
                "application/x-beblog-raw",
                product.raw.as_slice(),
            ),
            ("capture.tiff", "image/tiff", product.tiff.as_slice()),
            (
                "capture.params.txt",
                "text/plain; profile=minolta-params",
                product.params_txt.as_slice(),
            ),
            (
                "capture.client.log",
                "text/plain; profile=minolta-client-log",
                product.client_log.as_slice(),
            ),
        ];
        for (name, media_type, bytes) in artifacts {
            manifest
                .files
                .push(write_original(&pending.directory, name, media_type, bytes)?);
            write_manifest(&pending.directory, &manifest)?;
        }
        validate_manifest_files(&pending.directory, &manifest)?;
        manifest.status = ArchiveStatus::Completed;
        write_manifest(&pending.directory, &manifest)?;
        Ok(manifest)
    }
}

fn write_batch_manifest(path: &Path, manifest: &BatchManifest) -> io::Result<()> {
    let sequence = BATCH_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temp = path.with_extension(format!("{}.{}.tmp", std::process::id(), sequence));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    serde_json::to_writer_pretty(&mut file, manifest).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temp, path)
}

fn d6_batch_path(directory: &Path, batch_id: &str) -> PathBuf {
    directory.join(format!("{batch_id}.json"))
}

fn write_new_d6_manifest(path: &Path, manifest: &D6BatchManifest) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    serde_json::to_writer_pretty(&mut file, manifest).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()
}

fn write_d6_manifest(path: &Path, manifest: &D6BatchManifest) -> io::Result<()> {
    let sequence = D6_BATCH_TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = path.with_extension(format!("{}.{}.tmp", std::process::id(), sequence));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    serde_json::to_writer_pretty(&mut file, manifest).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, path)
}

fn d6_frame_mut(
    manifest: &mut D6BatchManifest,
    frame: u8,
) -> io::Result<&mut crate::batch_plan::D6FrameJournalEntry> {
    manifest
        .frames
        .iter_mut()
        .find(|entry| entry.frame == frame)
        .ok_or_else(|| invalid_data("Frame ist nicht Teil des D6-Batchplans"))
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

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn ensure_public_capture_kind(kind: CaptureKind) -> io::Result<()> {
    if kind != CaptureKind::Simulation {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Produktions- und Offline-Captures benötigen ihren jeweils geprüften Importpfad",
        ));
    }
    Ok(())
}

fn validate_archive_id(id: &str) -> io::Result<()> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(invalid_data("Ungültige Archiv-ID"));
    }
    Ok(())
}

fn validate_manifest_files(directory: &Path, manifest: &CaptureManifest) -> io::Result<()> {
    for metadata in &manifest.files {
        let bytes = fs::read(directory.join(&metadata.name))?;
        if bytes.len() as u64 != metadata.bytes
            || format!("{:x}", Sha256::digest(&bytes)) != metadata.sha256
        {
            return Err(invalid_data(format!(
                "Dateikonsistenz fehlgeschlagen: {}",
                metadata.name
            )));
        }
    }
    Ok(())
}

fn validate_tiff(bytes: &[u8], kind: CaptureKind) -> io::Result<()> {
    if kind == CaptureKind::Simulation {
        return if bytes.starts_with(b"BEBLOG-MOCK-TIFF\n") {
            Ok(())
        } else {
            Err(invalid_data("Ungültige TIFF-Simulationsfixture"))
        };
    }
    if bytes.len() < 8 {
        return Err(invalid_data("TIFF-Header unvollständig"));
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(invalid_data("Ungültige TIFF-Byteordnung")),
    };
    let u16_at = |offset: usize| -> u16 {
        let pair = [bytes[offset], bytes[offset + 1]];
        if little {
            u16::from_le_bytes(pair)
        } else {
            u16::from_be_bytes(pair)
        }
    };
    let u32_at = |offset: usize| -> u32 {
        let value = [
            bytes[offset],
            bytes[offset + 1],
            bytes[offset + 2],
            bytes[offset + 3],
        ];
        if little {
            u32::from_le_bytes(value)
        } else {
            u32::from_be_bytes(value)
        }
    };
    if u16_at(2) != 42 {
        return Err(invalid_data("Ungültige TIFF-Magic"));
    }
    let ifd = u32_at(4) as usize;
    if ifd
        .checked_add(2)
        .filter(|end| *end <= bytes.len())
        .is_none()
    {
        return Err(invalid_data("TIFF-IFD-Offset außerhalb der Datei"));
    }
    let entries = u16_at(ifd) as usize;
    let end = ifd
        .checked_add(2)
        .and_then(|start| start.checked_add(entries.checked_mul(12)?))
        .and_then(|end| end.checked_add(4));
    if end.filter(|end| *end <= bytes.len()).is_none() {
        return Err(invalid_data("TIFF-IFD ist abgeschnitten"));
    }
    let mut strips: Option<(Vec<u32>, Vec<u32>)> = None;
    let mut tiles: Option<(Vec<u32>, Vec<u32>)> = None;
    let mut has_width = false;
    let mut has_height = false;
    for index in 0..entries {
        let entry = ifd + 2 + index * 12;
        let tag = u16_at(entry);
        if matches!(tag, 256 | 257) {
            let field_type = u16_at(entry + 2);
            let count = u32_at(entry + 4);
            let value = match field_type {
                3 if count == 1 => u16_at(entry + 8) as u32,
                4 if count == 1 => u32_at(entry + 8),
                _ => return Err(invalid_data("Ungültige TIFF-Bildabmessung")),
            };
            if value == 0 {
                return Err(invalid_data("TIFF-Bildabmessung ist null"));
            }
            if tag == 256 {
                has_width = true;
            } else {
                has_height = true;
            }
            continue;
        }
        if !matches!(tag, 273 | 279 | 324 | 325) {
            continue;
        }
        let field_type = u16_at(entry + 2);
        let count = u32_at(entry + 4) as usize;
        if count == 0 || !matches!(field_type, 3 | 4) {
            return Err(invalid_data("Ungültiger TIFF-Strip-/Tile-Eintrag"));
        }
        let type_size = if field_type == 3 { 2 } else { 4 };
        let byte_size = count
            .checked_mul(type_size)
            .ok_or_else(|| invalid_data("TIFF-Eintragsgröße überläuft"))?;
        let data_start = if byte_size <= 4 {
            entry + 8
        } else {
            u32_at(entry + 8) as usize
        };
        let data_end = data_start
            .checked_add(byte_size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| invalid_data("TIFF-Strip-/Tile-Array außerhalb der Datei"))?;
        let mut values = Vec::with_capacity(count);
        for item in 0..count {
            let position = data_start + item * type_size;
            values.push(if field_type == 3 {
                u16_at(position) as u32
            } else {
                u32_at(position)
            });
        }
        let _ = data_end;
        match tag {
            273 => strips.get_or_insert_with(|| (Vec::new(), Vec::new())).0 = values,
            279 => strips.get_or_insert_with(|| (Vec::new(), Vec::new())).1 = values,
            324 => tiles.get_or_insert_with(|| (Vec::new(), Vec::new())).0 = values,
            325 => tiles.get_or_insert_with(|| (Vec::new(), Vec::new())).1 = values,
            _ => unreachable!(),
        };
    }
    let has_image_data = [strips, tiles]
        .into_iter()
        .flatten()
        .any(|(offsets, sizes)| {
            !offsets.is_empty()
                && offsets.len() == sizes.len()
                && offsets.iter().zip(sizes).all(|(offset, size)| {
                    size > 0
                        && (*offset as usize)
                            .checked_add(size as usize)
                            .is_some_and(|end| end <= bytes.len())
                })
        });
    if !has_width || !has_height || !has_image_data {
        return Err(invalid_data(
            "TIFF-Abmessungen oder konsistente Bilddaten-Strips/Tiles fehlen",
        ));
    }
    Ok(())
}

fn validate_raw(bytes: &[u8], kind: CaptureKind) -> io::Result<()> {
    match kind {
        CaptureKind::Simulation if !bytes.starts_with(b"BEBLOG-MOCK-RAW\n") => {
            Err(invalid_data("Ungültige RAW-Simulationsfixture"))
        }
        CaptureKind::Production if bytes.starts_with(b"BEBLOG-MOCK-RAW\n") => Err(invalid_data(
            "Mock-RAW darf nicht als Produktionsscan archiviert werden",
        )),
        CaptureKind::Simulation | CaptureKind::Production => Ok(()),
        CaptureKind::OfflineImport => Err(invalid_data(
            "Offline-Import benötigt die profilierte Vier-Artefaktvalidierung",
        )),
        CaptureKind::UnclassifiedLegacy => Err(invalid_data(
            "Unklassifizierte Altarchive dürfen nicht finalisiert werden",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch_plan::{D6BatchRequest, D6FrameStatus};
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

    fn simulated_product() -> CaptureProduct {
        let raw = b"BEBLOG-MOCK-RAW\nfixture only\n".to_vec();
        CaptureProduct {
            expected_raw_bytes: 1_438_200,
            raw,
            tiff: b"BEBLOG-MOCK-TIFF\nfixture only\n".to_vec(),
            kind: CaptureKind::Simulation,
        }
    }

    fn production_product(raw: Vec<u8>, tiff: Vec<u8>, expected_raw_bytes: u64) -> CaptureProduct {
        CaptureProduct {
            raw,
            tiff,
            expected_raw_bytes,
            kind: CaptureKind::Production,
        }
    }

    fn begin_production_for_validator_test(
        archive: &ArchiveManager,
        request: &StartScanRequest,
    ) -> PendingCapture {
        archive
            .begin_with_id(
                &ArchiveManager::next_capture_id(),
                request,
                CaptureKind::Production,
                None,
                600,
                1_438_200,
            )
            .unwrap()
    }

    fn valid_tiff() -> Vec<u8> {
        vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 4, 0, 0, 1, 4, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 4, 0, 1,
            0, 0, 0, 1, 0, 0, 0, 0x11, 1, 4, 0, 1, 0, 0, 0, 62, 0, 0, 0, 0x17, 1, 4, 0, 1, 0, 0, 0,
            3, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3,
        ]
    }

    #[test]
    fn refuses_capture_directory_collision() {
        let root = temp_root("collision");
        let archive = ArchiveManager::new(root.clone());
        archive
            .begin_with_id(
                "fixed",
                &request(),
                CaptureKind::Simulation,
                None,
                600,
                1_438_200,
            )
            .unwrap();
        assert_eq!(
            archive
                .begin_with_id(
                    "fixed",
                    &request(),
                    CaptureKind::Simulation,
                    None,
                    600,
                    1_438_200
                )
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn batch_id_collision_retries_without_modifying_existing_journal() {
        let root = temp_root("batch-id-collision");
        let archive = ArchiveManager::new(root.clone());
        archive.begin_batch("batch-existing", &[1, 2]).unwrap();
        archive
            .update_batch(
                "batch-existing",
                Some(1),
                Some(BatchArchiveStatus::Failed),
                Some("preserve this failure".into()),
            )
            .unwrap();
        let journal = root.join("batches/batch-existing.json");
        let original = fs::read(&journal).unwrap();
        let mut candidates = ["batch-existing", "batch-new"].into_iter();

        let allocated = archive
            .begin_batch_unique_with(&[1, 2], || candidates.next().unwrap().to_string())
            .unwrap();

        assert_eq!(allocated, "batch-new");
        assert_eq!(fs::read(journal).unwrap(), original);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn non_collision_filesystem_already_exists_error_is_not_retried() {
        let root = temp_root("batch-id-filesystem-error");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("batches"), b"not a directory").unwrap();
        let archive = ArchiveManager::new(root.clone());
        let attempts = std::cell::Cell::new(0);

        let error = archive
            .begin_batch_unique_with(&[1, 2], || {
                attempts.set(attempts.get() + 1);
                "batch-candidate".into()
            })
            .unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            attempts.get(),
            1,
            "only create_new ID collisions are retried"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finalizes_originals_with_checksums_and_no_overwrite() {
        let root = temp_root("finalize");
        let archive = ArchiveManager::new(root.clone());
        let pending = archive.begin(&request(), CaptureKind::Simulation).unwrap();
        let manifest = archive.finalize(&pending, simulated_product()).unwrap();
        assert_eq!(manifest.status, ArchiveStatus::Completed);
        assert_eq!(manifest.capture_kind, CaptureKind::Simulation);
        assert_eq!(manifest.files.len(), 2);
        assert!(manifest.files.iter().all(|file| file.sha256.len() == 64));
        assert_eq!(
            manifest.files[0].bytes,
            simulated_product().raw.len() as u64
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_wrong_production_raw_size_and_marks_failed() {
        let root = temp_root("raw-invalid");
        let archive = ArchiveManager::new(root.clone());
        let pending = begin_production_for_validator_test(&archive, &request());
        let error = archive
            .finalize_for_archive_validator_test(
                &pending,
                production_product(vec![1, 2], valid_tiff(), 1_438_200),
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let manifest = read_manifest(&pending.directory).unwrap();
        assert_eq!(manifest.status, ArchiveStatus::Failed);
        assert!(manifest.files.is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_invalid_production_tiff_structure() {
        let root = temp_root("tiff-invalid");
        let archive = ArchiveManager::new(root.clone());
        let pending = begin_production_for_validator_test(&archive, &request());
        let error = archive
            .finalize_for_archive_validator_test(
                &pending,
                production_product(vec![1, 2], b"II*\0\xff\xff\xff\x7f".to_vec(), 2),
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            read_manifest(&pending.directory).unwrap().status,
            ArchiveStatus::Failed
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn accepts_structurally_valid_production_tiff_and_exact_raw_size() {
        let root = temp_root("production-valid");
        let archive = ArchiveManager::new(root.clone());
        let pending = begin_production_for_validator_test(&archive, &request());
        let manifest = archive
            .finalize_for_archive_validator_test(
                &pending,
                production_product(vec![7; 1_438_200], valid_tiff(), 1_438_200),
            )
            .unwrap();
        assert_eq!(manifest.status, ArchiveStatus::Completed);
        assert_eq!(manifest.files[0].bytes, 1_438_200);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn production_finalization_is_blocked_until_a_process_proof_api_exists() {
        let root = temp_root("production-proof-required");
        let archive = ArchiveManager::new(root.clone());
        let pending = archive
            .begin_with_id(
                "validator-only-production",
                &request(),
                CaptureKind::Production,
                None,
                600,
                1_438_200,
            )
            .unwrap();
        let error = archive
            .finalize(
                &pending,
                production_product(vec![7; 1_438_200], valid_tiff(), 1_438_200),
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            read_manifest(&pending.directory).unwrap().status,
            ArchiveStatus::InProgress
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn verifies_written_file_size_and_checksum_from_disk() {
        let root = temp_root("checksum");
        let archive = ArchiveManager::new(root.clone());
        let pending = archive.begin(&request(), CaptureKind::Simulation).unwrap();
        let manifest = archive.finalize(&pending, simulated_product()).unwrap();
        validate_manifest_files(&pending.directory, &manifest).unwrap();
        fs::write(pending.directory.join("capture.raw"), b"tampered").unwrap();
        assert_eq!(
            validate_manifest_files(&pending.directory, &manifest)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn partial_write_error_leaves_failed_archive_and_preserves_original() {
        let root = temp_root("partial");
        let archive = ArchiveManager::new(root.clone());
        let pending = archive.begin(&request(), CaptureKind::Simulation).unwrap();
        fs::write(
            pending.directory.join("capture.tiff"),
            b"pre-existing-original",
        )
        .unwrap();
        let error = archive.finalize(&pending, simulated_product()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read(pending.directory.join("capture.tiff")).unwrap(),
            b"pre-existing-original"
        );
        let manifest = read_manifest(&pending.directory).unwrap();
        assert_eq!(manifest.status, ArchiveStatus::Failed);
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.files[0].name, "capture.raw");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn finalization_surfaces_a_secondary_archive_status_write_failure() {
        let root = temp_root("finalize-status-error");
        let archive = ArchiveManager::new(root.clone());
        let pending = archive.begin(&request(), CaptureKind::Simulation).unwrap();
        fs::write(pending.directory.join("manifest.json"), b"corrupt manifest").unwrap();

        let error = archive.finalize(&pending, simulated_product()).unwrap_err();

        assert!(error
            .to_string()
            .contains("Capturefehlerstatus konnte ebenfalls nicht persistiert werden"));
        assert!(error.to_string().contains("Recovery erforderlich"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovers_interrupted_manifest() {
        let root = temp_root("recovery");
        let archive = ArchiveManager::new(root.clone());
        let pending = archive.begin(&request(), CaptureKind::Simulation).unwrap();
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

    #[test]
    fn recovers_in_progress_batch_without_resuming_it() {
        let root = temp_root("batch-recovery");
        let archive = ArchiveManager::new(root.clone());
        archive.begin_batch("batch-fixed", &[1, 2, 3, 4]).unwrap();
        archive
            .update_batch("batch-fixed", Some(1), None, None)
            .unwrap();
        assert_eq!(archive.recover_incomplete().unwrap(), vec!["batch-fixed"]);
        let manifest: BatchManifest =
            serde_json::from_slice(&fs::read(root.join("batches/batch-fixed.json")).unwrap())
                .unwrap();
        assert_eq!(manifest.status, BatchArchiveStatus::Interrupted);
        assert_eq!(manifest.completed_frames, vec![1]);
        assert!(archive.recover_incomplete().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn legacy_manifest_is_not_misclassified_as_simulation_or_production() {
        let old = r#"{"schemaVersion":1,"captureId":"legacy","status":"inProgress","frame":1,"pixelFormat":"RGB8","resolutionDpi":600,"expectedRawBytes":1438200,"files":[],"failure":null}"#;
        let manifest: CaptureManifest = serde_json::from_str(old).unwrap();
        assert_eq!(manifest.capture_kind, CaptureKind::UnclassifiedLegacy);
    }

    #[test]
    fn d6_journal_requires_persisted_validation_before_frame_completion() {
        let root = temp_root("d6-journal-contract");
        let archive = ArchiveManager::new(root.clone());
        let manifest = archive.begin_d6_batch(&D6BatchRequest::default()).unwrap();
        let batch_id = manifest.batch_id.as_str();
        assert_eq!(manifest.frames.len(), 4);
        assert!(manifest
            .frames
            .iter()
            .all(|entry| entry.status == D6FrameStatus::Planned));

        let first_frame_id = archive
            .begin_d6_frame(batch_id, 1, "capture-unique-1")
            .unwrap();
        assert!(first_frame_id.ends_with("-frame-1"));
        assert!(archive.complete_d6_frame(batch_id, 1).is_err());
        archive
            .mark_d6_frame_validated(batch_id, 1, "capture-unique-1")
            .unwrap();
        archive.complete_d6_frame(batch_id, 1).unwrap();
        assert!(archive
            .begin_d6_frame(batch_id, 3, "capture-unique-3")
            .is_err());
        archive
            .begin_d6_frame(batch_id, 2, "capture-unique-2")
            .unwrap();

        let persisted: D6BatchManifest = serde_json::from_slice(
            &fs::read(d6_batch_path(&root.join("batches/d6"), batch_id)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            persisted.frames[0].capture_id.as_deref(),
            Some("capture-unique-1")
        );
        assert_eq!(persisted.frames[0].status, D6FrameStatus::Completed);
        assert_eq!(persisted.frames[1].status, D6FrameStatus::Running);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn d6_journal_update_failure_does_not_claim_frame_completion() {
        let root = temp_root("d6-journal-write-failure");
        let archive = ArchiveManager::new(root.clone());
        let manifest = archive
            .begin_d6_batch(&D6BatchRequest { frames: vec![1, 2] })
            .unwrap();
        let path = d6_batch_path(&root.join("batches/d6"), manifest.batch_id.as_str());
        archive
            .begin_d6_frame(manifest.batch_id.as_str(), 1, "capture-journal-error")
            .unwrap();
        archive
            .mark_d6_frame_validated(manifest.batch_id.as_str(), 1, "capture-journal-error")
            .unwrap();
        fs::write(&path, b"damaged journal").unwrap();
        assert!(archive
            .complete_d6_frame(manifest.batch_id.as_str(), 1)
            .is_err());
        assert_eq!(fs::read(&path).unwrap(), b"damaged journal");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn d6_terminal_frame_journal_states_are_explicit_and_ordered() {
        let root = temp_root("d6-terminal-states");
        let archive = ArchiveManager::new(root.clone());
        let failed = archive
            .begin_d6_batch(&D6BatchRequest { frames: vec![1, 2] })
            .unwrap();
        archive
            .begin_d6_frame(failed.batch_id.as_str(), 1, "capture-fail")
            .unwrap();
        archive
            .fail_d6_frame(failed.batch_id.as_str(), 1, "fixture failure")
            .unwrap();
        let failed_now: D6BatchManifest = serde_json::from_slice(
            &fs::read(d6_batch_path(
                &root.join("batches/d6"),
                failed.batch_id.as_str(),
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(failed_now.status, D6BatchStatus::Failed);
        assert_eq!(failed_now.frames[0].status, D6FrameStatus::Failed);
        assert_eq!(failed_now.frames[1].status, D6FrameStatus::Planned);

        let cancelled = archive
            .begin_d6_batch(&D6BatchRequest { frames: vec![1, 2] })
            .unwrap();
        archive
            .begin_d6_frame(cancelled.batch_id.as_str(), 1, "capture-cancel")
            .unwrap();
        archive
            .cancel_d6_batch(cancelled.batch_id.as_str(), Some(1), "fixture cancellation")
            .unwrap();
        let cancelled_now: D6BatchManifest = serde_json::from_slice(
            &fs::read(d6_batch_path(
                &root.join("batches/d6"),
                cancelled.batch_id.as_str(),
            ))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(cancelled_now.status, D6BatchStatus::Cancelled);
        assert_eq!(cancelled_now.frames[0].status, D6FrameStatus::Cancelled);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn d6_batch_becomes_completed_only_after_every_ordered_frame_is_journaled() {
        let root = temp_root("d6-batch-complete");
        let archive = ArchiveManager::new(root.clone());
        let manifest = archive
            .begin_d6_batch(&D6BatchRequest { frames: vec![1, 2] })
            .unwrap();
        for frame in [1, 2] {
            let capture_id = format!("capture-d6-{frame}");
            archive
                .begin_d6_frame(manifest.batch_id.as_str(), frame, &capture_id)
                .unwrap();
            archive
                .mark_d6_frame_validated(manifest.batch_id.as_str(), frame, &capture_id)
                .unwrap();
            archive
                .complete_d6_frame(manifest.batch_id.as_str(), frame)
                .unwrap();
            let persisted: D6BatchManifest = serde_json::from_slice(
                &fs::read(d6_batch_path(
                    &root.join("batches/d6"),
                    manifest.batch_id.as_str(),
                ))
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                persisted.status,
                if frame == 2 {
                    D6BatchStatus::Completed
                } else {
                    D6BatchStatus::Running
                }
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn d6_recovery_marks_in_progress_plan_interrupted_without_resuming() {
        let root = temp_root("d6-recovery");
        let archive = ArchiveManager::new(root.clone());
        let manifest = archive
            .begin_d6_batch(&D6BatchRequest { frames: vec![1, 2] })
            .unwrap();
        archive
            .begin_d6_frame(manifest.batch_id.as_str(), 1, "capture-interrupted")
            .unwrap();
        assert_eq!(
            archive.recover_incomplete().unwrap(),
            vec![manifest.batch_id.as_str()]
        );
        let path = d6_batch_path(&root.join("batches/d6"), manifest.batch_id.as_str());
        let recovered: D6BatchManifest = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(recovered.status, D6BatchStatus::Interrupted);
        assert_eq!(recovered.frames[0].status, D6FrameStatus::Interrupted);
        assert_eq!(recovered.frames[1].status, D6FrameStatus::Planned);
        assert!(archive.recover_incomplete().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
