//! Hardware-free import of an explicitly selected Minolta client artifact set.
//!
//! This module never starts a process and never treats text emitted by the client
//! as proof of an OS exit, child reaping, or SANE cleanup.

use crate::{
    archive::{ArchiveManager, CaptureManifest, FileMetadata, OfflineCaptureProduct},
    log_parser::{LogStream, ParsedLog, SidecarLogParser, TransferAccumulator},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

const RAW_BYTES: usize = 1_438_200;
const WIDTH: u32 = 564;
const HEIGHT: u32 = 850;
const BYTES_PER_LINE: u32 = 1_692;
const MAX_TIFF_BYTES: u64 = 4 * 1024 * 1024;
const MAX_LOG_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PARAMS_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone)]
pub struct OfflineArtifactPaths {
    pub raw: PathBuf,
    pub tiff: PathBuf,
    pub params_txt: PathBuf,
    pub client_log: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StagingStatus {
    InProgress,
    Failed,
    Archived,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StagingManifest {
    pub schema_version: u8,
    pub capture_id: String,
    pub status: StagingStatus,
    pub files: Vec<FileMetadata>,
    pub archived_capture_id: Option<String>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessEvidence {
    NotObservedByOfflineImport,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineImportReport {
    pub manifest: CaptureManifest,
    pub process_evidence: ProcessEvidence,
    pub sane_cleanup_evidence: ProcessEvidence,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy)]
enum Artifact {
    Raw,
    Tiff,
    Params,
    ClientLog,
}

impl Artifact {
    fn name(self) -> &'static str {
        match self {
            Self::Raw => "capture.raw",
            Self::Tiff => "capture.tiff",
            Self::Params => "capture.params.txt",
            Self::ClientLog => "capture.client.log",
        }
    }

    fn media_type(self) -> &'static str {
        match self {
            Self::Raw => "application/x-beblog-raw",
            Self::Tiff => "image/tiff",
            Self::Params | Self::ClientLog => "text/plain",
        }
    }

    fn max_bytes(self) -> u64 {
        match self {
            Self::Raw => RAW_BYTES as u64,
            Self::Tiff => MAX_TIFF_BYTES,
            Self::Params => MAX_PARAMS_BYTES,
            Self::ClientLog => MAX_LOG_BYTES,
        }
    }
}

pub fn import_offline_artifacts(
    archive: &ArchiveManager,
    frame: u8,
    sources: &OfflineArtifactPaths,
) -> io::Result<OfflineImportReport> {
    if !(1..=4).contains(&frame) {
        return Err(invalid_data(
            "D3-B3a-Profil akzeptiert ausschließlich Frames 1–4",
        ));
    }
    let (stage_dir, mut stage) = reserve_stage(archive.root_path())?;
    let copies = [
        (Artifact::Raw, &sources.raw),
        (Artifact::Tiff, &sources.tiff),
        (Artifact::Params, &sources.params_txt),
        (Artifact::ClientLog, &sources.client_log),
    ];
    for (artifact, source) in copies {
        match copy_into_stage(source, &stage_dir.join(artifact.name()), artifact) {
            Ok(metadata) => {
                stage.files.push(metadata);
                if let Err(error) = write_staging_manifest(&stage_dir, &stage) {
                    return Err(with_stage_context(error, &stage.capture_id));
                }
            }
            Err(error) => {
                stage.status = StagingStatus::Failed;
                stage.failure = Some(error.to_string());
                let _ = write_staging_manifest(&stage_dir, &stage);
                return Err(with_stage_context(error, &stage.capture_id));
            }
        }
    }

    let product = match read_and_validate_stage(frame, &stage_dir) {
        Ok(product) => product,
        Err(error) => {
            stage.status = StagingStatus::Failed;
            stage.failure = Some(error.to_string());
            let _ = write_staging_manifest(&stage_dir, &stage);
            return Err(with_stage_context(error, &stage.capture_id));
        }
    };

    let pending = match archive.begin_offline_import(frame) {
        Ok(pending) => pending,
        Err(error) => {
            stage.status = StagingStatus::Failed;
            stage.failure = Some(error.to_string());
            let _ = write_staging_manifest(&stage_dir, &stage);
            return Err(with_stage_context(error, &stage.capture_id));
        }
    };
    let manifest = match archive.finalize_offline_import(&pending, product) {
        Ok(manifest) => manifest,
        Err(error) => {
            stage.status = StagingStatus::Failed;
            stage.failure = Some(error.to_string());
            let _ = write_staging_manifest(&stage_dir, &stage);
            return Err(with_stage_context(error, &stage.capture_id));
        }
    };

    stage.status = StagingStatus::Archived;
    stage.archived_capture_id = Some(manifest.capture_id.clone());
    let warnings = write_staging_manifest(&stage_dir, &stage)
        .err()
        .map(|error| {
            format!(
                "Capture {} ist archiviert; Stagingstatus {} konnte nicht aktualisiert werden: {error}",
                manifest.capture_id, stage.capture_id
            )
        })
        .into_iter()
        .collect();
    Ok(OfflineImportReport {
        manifest,
        process_evidence: ProcessEvidence::NotObservedByOfflineImport,
        sane_cleanup_evidence: ProcessEvidence::NotObservedByOfflineImport,
        warnings,
    })
}

fn reserve_stage(root: &Path) -> io::Result<(PathBuf, StagingManifest)> {
    fs::create_dir_all(root)?;
    let canonical_root = fs::canonicalize(root)?;
    let staging_root = canonical_root.join(".staging");
    match fs::symlink_metadata(&staging_root) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => return Err(invalid_data("Stagingpfad ist kein echtes Verzeichnis")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(&staging_root)?;
        }
        Err(error) => return Err(error),
    }
    for _ in 0..16 {
        let capture_id = format!("stage-{}", uuid::Uuid::new_v4().simple());
        let stage_dir = staging_root.join(&capture_id);
        match fs::create_dir(&stage_dir) {
            Ok(()) => {
                let manifest = StagingManifest {
                    schema_version: 1,
                    capture_id,
                    status: StagingStatus::InProgress,
                    files: Vec::new(),
                    archived_capture_id: None,
                    failure: None,
                };
                if let Err(error) = write_staging_manifest(&stage_dir, &manifest) {
                    return Err(with_stage_context(error, &manifest.capture_id));
                }
                return Ok((stage_dir, manifest));
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "Exklusiver Stagingbereich nach mehreren Versuchen nicht verfügbar",
    ))
}

fn copy_into_stage(
    source: &Path,
    destination: &Path,
    artifact: Artifact,
) -> io::Result<FileMetadata> {
    let source_meta = fs::symlink_metadata(source)?;
    if !source_meta.file_type().is_file() {
        return Err(invalid_data(format!(
            "Quelle {} ist kein reguläres, symlinkfreies Artefakt",
            source.display()
        )));
    }
    if source_meta.len() > artifact.max_bytes() {
        return Err(invalid_data(format!(
            "{} überschreitet die Importgrößengrenze",
            artifact.name()
        )));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let input = options.open(source)?;
    if !input.metadata()?.file_type().is_file() {
        return Err(invalid_data("Artefaktquelle ist kein reguläres Fileobjekt"));
    }
    let mut bytes = Vec::with_capacity(source_meta.len() as usize);
    input
        .take(artifact.max_bytes() + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > artifact.max_bytes() {
        return Err(invalid_data(format!(
            "{} überschreitet die Importgrößengrenze",
            artifact.name()
        )));
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    output.write_all(&bytes)?;
    output.sync_all()?;
    Ok(file_metadata(artifact, &bytes))
}

fn file_metadata(artifact: Artifact, bytes: &[u8]) -> FileMetadata {
    FileMetadata {
        name: artifact.name().into(),
        media_type: artifact.media_type().into(),
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(bytes)),
    }
}

fn read_and_validate_stage(frame: u8, directory: &Path) -> io::Result<OfflineCaptureProduct> {
    let stage_meta = fs::symlink_metadata(directory)?;
    if !stage_meta.file_type().is_dir() {
        return Err(invalid_data("Staging-Capture ist kein echtes Verzeichnis"));
    }
    let stage: StagingManifest =
        serde_json::from_slice(&read_staged_file(directory, "stage.json", 1024 * 1024)?)
            .map_err(io::Error::other)?;
    if stage.status != StagingStatus::InProgress || stage.files.len() != 4 {
        return Err(invalid_data(
            "Stagingmanifest ist unvollständig oder nicht offen",
        ));
    }
    for artifact in [
        Artifact::Raw,
        Artifact::Tiff,
        Artifact::Params,
        Artifact::ClientLog,
    ] {
        let recorded = stage
            .files
            .iter()
            .find(|file| file.name == artifact.name())
            .ok_or_else(|| invalid_data("Stagingmanifest führt ein Artefakt nicht auf"))?;
        let bytes = read_staged_file(directory, artifact.name(), artifact.max_bytes())?;
        if bytes.len() as u64 != recorded.bytes
            || format!("{:x}", Sha256::digest(&bytes)) != recorded.sha256
        {
            return Err(invalid_data(
                "Staging-Artefaktgröße oder Prüfsumme wurde verändert",
            ));
        }
    }
    let raw = read_staged_file(directory, Artifact::Raw.name(), Artifact::Raw.max_bytes())?;
    let tiff = read_staged_file(directory, Artifact::Tiff.name(), Artifact::Tiff.max_bytes())?;
    let params_txt = read_staged_file(
        directory,
        Artifact::Params.name(),
        Artifact::Params.max_bytes(),
    )?;
    let client_log = read_staged_file(
        directory,
        Artifact::ClientLog.name(),
        Artifact::ClientLog.max_bytes(),
    )?;
    validate_artifact_bytes(frame, &raw, &tiff, &params_txt, &client_log)?;
    Ok(OfflineCaptureProduct {
        raw,
        tiff,
        params_txt,
        client_log,
    })
}

fn read_staged_file(directory: &Path, name: &str, max_bytes: u64) -> io::Result<Vec<u8>> {
    let path = directory.join(name);
    let metadata = fs::symlink_metadata(&path)?;
    if !metadata.file_type().is_file() || metadata.len() > max_bytes {
        return Err(invalid_data(
            "Staging-Artefakt ist kein reguläres oder zu großes File",
        ));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let file = options.open(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(invalid_data(
            "Staging-Artefakt wurde durch kein reguläres File geöffnet",
        ));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(max_bytes + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max_bytes {
        return Err(invalid_data(
            "Staging-Artefakt überschreitet die Größenbegrenzung",
        ));
    }
    Ok(bytes)
}

pub(crate) fn validate_artifact_bytes(
    frame: u8,
    raw: &[u8],
    tiff: &[u8],
    params_txt: &[u8],
    client_log: &[u8],
) -> io::Result<()> {
    if raw.len() != RAW_BYTES {
        return Err(invalid_data(format!(
            "RAW-Größe {} statt {} Byte",
            raw.len(),
            RAW_BYTES
        )));
    }
    let params = parse_params(params_txt)?;
    validate_profile_metadata(&params, frame)?;
    validate_tiff_profile(tiff)?;
    validate_log_profile(frame, client_log)?;
    Ok(())
}

fn parse_params(bytes: &[u8]) -> io::Result<std::collections::BTreeMap<String, String>> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid_data("Metadaten sind kein UTF-8"))?;
    if text.contains('\0') {
        return Err(invalid_data("Metadaten enthalten NUL-Bytes"));
    }
    let mut values = std::collections::BTreeMap::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| invalid_data("Metadatenzeile ist nicht key=value"))?;
        if key.is_empty() || value.is_empty() || values.insert(key.into(), value.into()).is_some() {
            return Err(invalid_data("Leerer oder doppelter Metadatenschlüssel"));
        }
    }
    Ok(values)
}

fn validate_profile_metadata(
    params: &std::collections::BTreeMap<String, String>,
    frame: u8,
) -> io::Result<()> {
    for (key, expected) in [
        ("format", "RGB"),
        ("bytes_per_line", "1692"),
        ("pixels_per_line", "564"),
        ("lines", "850"),
        ("depth", "8"),
        ("raw_bytes", "1438200"),
        ("terminal_status", "5"),
    ] {
        if params.get(key).map(String::as_str) != Some(expected) {
            return Err(invalid_data(format!(
                "Metadatenprofil stimmt bei {key} nicht mit RGB8/600-dpi-Profil überein"
            )));
        }
    }
    if params.get("device").is_none_or(String::is_empty) {
        return Err(invalid_data("Metadaten enthalten keinen Gerätebezeichner"));
    }
    for key in ["resolution", "resolution_dpi", "dpi"] {
        if params.get(key).is_some_and(|value| value != "600") {
            return Err(invalid_data(format!(
                "Metadatenauflösung {key} ist nicht 600 dpi"
            )));
        }
    }
    if let Some(metadata_frame) = params.get("frame") {
        if metadata_frame.parse::<u8>().ok() != Some(frame) {
            return Err(invalid_data(
                "Metadaten enthalten eine ungültige Frameposition",
            ));
        }
    }
    let line_bytes = 564_u64 * 3 * 8 / 8;
    if line_bytes != u64::from(BYTES_PER_LINE)
        || u64::from(BYTES_PER_LINE) * u64::from(HEIGHT) != RAW_BYTES as u64
    {
        return Err(invalid_data("Interner RGB8-Profilvertrag ist inkonsistent"));
    }
    Ok(())
}

fn validate_log_profile(frame: u8, bytes: &[u8]) -> io::Result<()> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid_data("Clientlog ist kein UTF-8"))?;
    if text.contains('\0') {
        return Err(invalid_data("Clientlog enthält NUL-Bytes"));
    }
    let parser = SidecarLogParser;
    let mut parsed = Vec::new();
    let mut normalized = Vec::new();
    for line in text.lines() {
        let stripped = strip_timestamp(line);
        let event = parser.parse(LogStream::Stderr, line);
        match event {
            ParsedLog::Malformed { .. } | ParsedLog::ReadFailed { .. } => {
                return Err(invalid_data(format!(
                    "Ungültige oder fehlgeschlagene Clientdiagnose: {line}"
                )))
            }
            _ => {}
        }
        parsed.push(event);
        normalized.push(stripped.to_string());
    }
    let position = |predicate: fn(&ParsedLog) -> bool| {
        parsed
            .iter()
            .position(predicate)
            .ok_or_else(|| invalid_data("Erforderliche Clientdiagnose fehlt"))
    };
    let initialized = position(|event| matches!(event, ParsedLog::Initialized))?;
    let opened = position(|event| matches!(event, ParsedLog::ScannerOpened))?;
    let holder = position(|event| matches!(event, ParsedLog::HolderDetected))?;
    let configured = position(|event| matches!(event, ParsedLog::FrameConfigured))?;
    let started = position(|event| matches!(event, ParsedLog::ScanStarted))?;
    if !(initialized < opened && opened < holder && holder < configured && configured < started) {
        return Err(invalid_data(
            "Clientdiagnosen verletzen die erwartete Startreihenfolge",
        ));
    }
    let expected_parameters = "format=RGB last=1 bpl=1692 pixels=564 lines=850 depth=8";
    let frame_line = format!("sane_control_option(frame, SET={frame}) -> 0");
    let frame_at = normalized
        .iter()
        .position(|line| line.contains(&frame_line));
    let mode_at = normalized
        .iter()
        .position(|line| line.contains("sane_control_option(mode, SET='Color') -> 0"));
    let resolution_at = normalized
        .iter()
        .position(|line| line.contains("sane_control_option(resolution, SET=600) -> 0"));
    let pre_parameters_at = normalized
        .iter()
        .position(|line| line.contains("parameters(pre): ") && line.contains(expected_parameters));
    let post_parameters_at = normalized
        .iter()
        .position(|line| line.contains("parameters(post): ") && line.contains(expected_parameters));
    if frame_at.is_none_or(|index| index != configured)
        || mode_at.is_none_or(|index| index <= configured || index >= started)
        || resolution_at.is_none_or(|index| index <= configured || index >= started)
        || pre_parameters_at.is_none_or(|index| index <= configured || index >= started)
        || post_parameters_at.is_none_or(|index| index <= started)
        || mode_at
            .zip(pre_parameters_at)
            .is_none_or(|(mode, pre)| mode >= pre)
        || resolution_at
            .zip(pre_parameters_at)
            .is_none_or(|(resolution, pre)| resolution >= pre)
    {
        return Err(invalid_data(
            "Clientlog bestätigt Frame/RGB/600-dpi-Profil nicht in der erwarteten Reihenfolge",
        ));
    }
    let mut transfer = TransferAccumulator::new(RAW_BYTES as u64);
    let mut eof_at = None;
    let mut total_at = None;
    let mut tiff_at = None;
    let mut exit_at = None;
    let mut client_total = None;
    for (index, event) in parsed.iter().enumerate().skip(started + 1) {
        match event {
            ParsedLog::ReadChunk { bytes } if eof_at.is_none() => {
                transfer
                    .record_chunk(*bytes)
                    .map_err(|error| invalid_data(format!("RAW-Chunkfolge ungültig: {error:?}")))?;
            }
            ParsedLog::ReadChunk { .. } => return Err(invalid_data("RAW-Chunk nach EOF")),
            ParsedLog::Eof { final_chunk_bytes } if eof_at.is_none() => {
                if *final_chunk_bytes > 0 {
                    transfer.record_chunk(*final_chunk_bytes).map_err(|error| {
                        invalid_data(format!("Finaler EOF-Chunk ungültig: {error:?}"))
                    })?;
                }
                if transfer.total() != RAW_BYTES as u64 {
                    return Err(invalid_data("EOF vor vollständiger RAW-Übertragung"));
                }
                eof_at = Some(index);
            }
            ParsedLog::Eof { .. } => return Err(invalid_data("Doppeltes EOF im Clientlog")),
            ParsedLog::CaptureComplete {
                total_bytes,
                terminal_status,
            } => {
                if eof_at.is_none() || total_at.is_some() || *terminal_status != 5 {
                    return Err(invalid_data(
                        "Capture-Gesamtstand steht vor EOF oder hat falschen Status",
                    ));
                }
                transfer
                    .verify_reported_total(*total_bytes)
                    .map_err(|error| {
                        invalid_data(format!(
                            "Client-Gesamtbytezahl widerspricht Chunks: {error:?}"
                        ))
                    })?;
                client_total = Some(*total_bytes);
                total_at = Some(index);
            }
            ParsedLog::TiffWritten { .. } => {
                if total_at.is_none() || tiff_at.is_some() {
                    return Err(invalid_data(
                        "TIFF-Diagnose steht vor Gesamtstand oder ist doppelt",
                    ));
                }
                tiff_at = Some(index);
            }
            ParsedLog::ClientExit { code } if *code != 0 || exit_at.replace(index).is_some() => {
                return Err(invalid_data(
                    "Client-Exitdiagnose ist fehlerhaft oder doppelt",
                ));
            }
            ParsedLog::ClientExit { .. } => {}
            _ => {}
        }
    }
    let eof_at = eof_at.ok_or_else(|| invalid_data("Clientlog enthält kein EOF"))?;
    let total_at =
        total_at.ok_or_else(|| invalid_data("Clientlog enthält keinen Capture-Gesamtstand"))?;
    let tiff_at =
        tiff_at.ok_or_else(|| invalid_data("Clientlog enthält keine TIFF-Finalisierung"))?;
    let exit_at = exit_at.ok_or_else(|| invalid_data("Clientlog enthält keine Exitdiagnose"))?;
    let metadata_at = normalized
        .iter()
        .position(|line| line.starts_with("metadata ") && line.ends_with(" -> written"));
    let close_at = normalized.iter().position(|line| line == "sane_close");
    let sane_exit_at = normalized.iter().position(|line| line == "sane_exit");
    if !(eof_at < total_at && total_at < tiff_at && tiff_at < exit_at)
        || close_at.is_none_or(|index| index <= tiff_at || index >= exit_at)
        || sane_exit_at.is_none_or(|index| index <= tiff_at || index >= exit_at)
        || metadata_at.is_none_or(|index| index <= total_at || index >= tiff_at)
        || close_at
            .zip(sane_exit_at)
            .is_none_or(|(close, exit)| close >= exit)
        || client_total != Some(RAW_BYTES as u64)
    {
        return Err(invalid_data(
            "Clientlog-Finalisierung ist unvollständig oder widersprüchlich",
        ));
    }
    Ok(())
}

fn strip_timestamp(line: &str) -> &str {
    line.strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
        .map_or(line, |(_, remainder)| remainder)
}

#[derive(Clone, Copy)]
struct TiffReader<'a> {
    bytes: &'a [u8],
    little: bool,
}

impl<'a> TiffReader<'a> {
    fn u16(self, offset: usize) -> io::Result<u16> {
        let end = offset
            .checked_add(2)
            .ok_or_else(|| invalid_data("TIFF SHORT-Offset überläuft"))?;
        let pair = self
            .bytes
            .get(offset..end)
            .ok_or_else(|| invalid_data("TIFF SHORT außerhalb Datei"))?;
        Ok(if self.little {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        })
    }

    fn u32(self, offset: usize) -> io::Result<u32> {
        let end = offset
            .checked_add(4)
            .ok_or_else(|| invalid_data("TIFF LONG-Offset überläuft"))?;
        let bytes = self
            .bytes
            .get(offset..end)
            .ok_or_else(|| invalid_data("TIFF LONG außerhalb Datei"))?;
        let value = [bytes[0], bytes[1], bytes[2], bytes[3]];
        Ok(if self.little {
            u32::from_le_bytes(value)
        } else {
            u32::from_be_bytes(value)
        })
    }

    fn values(
        self,
        entry: usize,
        expected: usize,
        ranges: &mut Vec<std::ops::Range<usize>>,
    ) -> io::Result<Vec<u32>> {
        let kind = self.u16(entry + 2)?;
        let count = usize::try_from(self.u32(entry + 4)?)
            .map_err(|_| invalid_data("TIFF count passt nicht in usize"))?;
        if count != expected || !matches!(kind, 3 | 4) {
            return Err(invalid_data("Nicht unterstützter TIFF-Tagtyp oder -Count"));
        }
        let item_size = if kind == 3 { 2 } else { 4 };
        let byte_count = count
            .checked_mul(item_size)
            .ok_or_else(|| invalid_data("TIFF-Taggröße überläuft"))?;
        let start = if byte_count <= 4 {
            entry + 8
        } else {
            self.u32(entry + 8)? as usize
        };
        let end = start
            .checked_add(byte_count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid_data("TIFF-Tagoffset außerhalb Datei"))?;
        if byte_count > 4 {
            ranges.push(start..end);
        }
        (0..count)
            .map(|index| {
                if kind == 3 {
                    self.u16(start + index * item_size).map(u32::from)
                } else {
                    self.u32(start + index * item_size)
                }
            })
            .collect()
    }
}

fn validate_tiff_profile(bytes: &[u8]) -> io::Result<()> {
    if bytes.len() < 8 {
        return Err(invalid_data("TIFF-Header unvollständig"));
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(invalid_data("TIFF-Byteordnung ungültig")),
    };
    let reader = TiffReader { bytes, little };
    if reader.u16(2)? != 42 {
        return Err(invalid_data(
            "Nur klassisches TIFF-Magic 42 wird unterstützt",
        ));
    }
    let ifd = reader.u32(4)? as usize;
    let entry_count = reader.u16(ifd)? as usize;
    let ifd_end = ifd
        .checked_add(2)
        .and_then(|value| value.checked_add(entry_count.checked_mul(12)?))
        .and_then(|value| value.checked_add(4))
        .filter(|end| *end <= bytes.len())
        .ok_or_else(|| invalid_data("TIFF-IFD oder Next-IFD-Feld ist abgeschnitten"))?;
    if reader.u32(ifd + 2 + entry_count * 12)? != 0 {
        return Err(invalid_data(
            "Mehrseitige TIFF-Dateien werden nicht unterstützt",
        ));
    }
    let allowed = [
        256_u16, 257, 258, 259, 262, 273, 277, 278, 279, 282, 283, 284, 296, 274, 305, 339,
    ];
    let mut tags = std::collections::BTreeMap::<u16, (usize, Vec<u32>)>::new();
    let mut extra_ranges = Vec::new();
    extra_ranges.push(0..ifd_end);
    for index in 0..entry_count {
        let entry = ifd + 2 + index * 12;
        let tag = reader.u16(entry)?;
        if !allowed.contains(&tag) || tags.contains_key(&tag) {
            return Err(invalid_data(format!(
                "Unbekannter oder doppelter TIFF-Tag {tag}"
            )));
        }
        let field_type = reader.u16(entry + 2)?;
        let field_type_supported = match tag {
            256 | 257 | 273 | 278 | 279 => matches!(field_type, 3 | 4),
            258 | 259 | 262 | 274 | 277 | 284 | 296 | 339 => field_type == 3,
            282 | 283 => field_type == 5,
            305 => field_type == 2,
            _ => false,
        };
        if !field_type_supported {
            return Err(invalid_data(format!(
                "Nicht unterstützter TIFF-Typ für Tag {tag}"
            )));
        }
        let expected_count = match tag {
            258 | 282 | 283 | 339 => 3,
            273 | 279 => 1,
            305 => usize::try_from(reader.u32(entry + 4)?).unwrap_or(0),
            _ => 1,
        };
        let values = if tag == 305 {
            let count = expected_count;
            let kind = reader.u16(entry + 2)?;
            if kind != 2 || count == 0 {
                return Err(invalid_data("TIFF Software-Tag ist ungültig"));
            }
            let start = if count <= 4 {
                entry + 8
            } else {
                reader.u32(entry + 8)? as usize
            };
            let end = start
                .checked_add(count)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| invalid_data("TIFF Software-Tag außerhalb Datei"))?;
            extra_ranges.push(start..end);
            vec![]
        } else if matches!(tag, 282 | 283) {
            if reader.u16(entry + 2)? != 5 || reader.u32(entry + 4)? != 1 {
                return Err(invalid_data(
                    "TIFF-Auflösung muss ein einzelner RATIONAL-Wert sein",
                ));
            }
            let offset = reader.u32(entry + 8)? as usize;
            let end = offset
                .checked_add(8)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(|| invalid_data("TIFF-Auflösung außerhalb Datei"))?;
            extra_ranges.push(offset..end);
            let numerator = reader.u32(offset)?;
            let denominator = reader.u32(offset + 4)?;
            if denominator == 0 {
                vec![0, 0, 0]
            } else {
                vec![numerator, denominator, 1]
            }
        } else {
            reader.values(entry, expected_count, &mut extra_ranges)?
        };
        tags.insert(tag, (reader.u16(entry + 2)? as usize, values));
    }
    let one = |tag: u16| -> io::Result<u32> {
        tags.get(&tag)
            .and_then(|(_, values)| values.first().copied())
            .ok_or_else(|| invalid_data(format!("TIFF-Pflichttag {tag} fehlt")))
    };
    if one(256)? != WIDTH
        || one(257)? != HEIGHT
        || one(259)? != 1
        || one(262)? != 2
        || one(277)? != 3
        || one(278)? != HEIGHT
        || one(284)? != 1
    {
        return Err(invalid_data(
            "TIFF-Abmessungen, Kompression oder RGB-Layout widersprechen dem Profil",
        ));
    }
    if tags.get(&258).map(|(_, values)| values.as_slice()) != Some(&[8, 8, 8][..]) {
        return Err(invalid_data("TIFF BitsPerSample muss [8,8,8] sein"));
    }
    let has_x_resolution = tags.contains_key(&282);
    let has_y_resolution = tags.contains_key(&283);
    if has_x_resolution != has_y_resolution
        || (has_x_resolution
            && (tags.get(&282).map(|(_, values)| values.as_slice()) != Some(&[600, 1, 1][..])
                || tags.get(&283).map(|(_, values)| values.as_slice()) != Some(&[600, 1, 1][..])
                || tags.get(&296).map(|(_, values)| values.as_slice()) != Some(&[2][..])))
        || (!has_x_resolution && tags.contains_key(&296))
    {
        return Err(invalid_data(
            "TIFF-Auflösung muss, falls enthalten, konsistent 600 dpi sein",
        ));
    }
    if let Some((_, orientation)) = tags.get(&274) {
        if orientation.as_slice() != [1] {
            return Err(invalid_data("TIFF-Orientierung wird nicht unterstützt"));
        }
    }
    if tags.get(&339).map(|(_, values)| values.as_slice()) != Some(&[1, 1, 1][..]) {
        return Err(invalid_data("TIFF-SampleFormat muss unsigned integer sein"));
    }
    let offsets = tags
        .get(&273)
        .ok_or_else(|| invalid_data("TIFF StripOffsets fehlt"))?
        .1
        .clone();
    let byte_counts = tags
        .get(&279)
        .ok_or_else(|| invalid_data("TIFF StripByteCounts fehlt"))?
        .1
        .clone();
    if offsets.len() != byte_counts.len() || offsets.is_empty() {
        return Err(invalid_data("TIFF Strip-Arrays passen nicht zusammen"));
    }
    let mut image_ranges = Vec::new();
    let mut total = 0_u64;
    for (offset, count) in offsets.into_iter().zip(byte_counts) {
        let start = offset as usize;
        let end = start
            .checked_add(count as usize)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| invalid_data("TIFF-Bilddatenbereich außerhalb Datei"))?;
        if count == 0
            || extra_ranges
                .iter()
                .any(|range| start < range.end && range.start < end)
            || image_ranges
                .iter()
                .any(|range: &std::ops::Range<usize>| start < range.end && range.start < end)
        {
            return Err(invalid_data(
                "TIFF-Bilddaten fehlen, überlappen oder überschreiben Struktur",
            ));
        }
        image_ranges.push(start..end);
        total = total
            .checked_add(u64::from(count))
            .ok_or_else(|| invalid_data("TIFF-Stripbytezahl überläuft"))?;
    }
    if total != RAW_BYTES as u64
        || image_ranges.iter().map(|range| range.len()).sum::<usize>() != RAW_BYTES
    {
        return Err(invalid_data(
            "TIFF-Strips enthalten nicht genau das RGB8-RAW-Profil",
        ));
    }
    Ok(())
}

pub(crate) fn write_staging_manifest(
    directory: &Path,
    manifest: &StagingManifest,
) -> io::Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
    let temporary = directory.join(format!("stage-{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    fs::rename(temporary, directory.join("stage.json"))
}

fn with_stage_context(error: io::Error, stage_id: &str) -> io::Error {
    io::Error::new(error.kind(), format!("{} (Staging-ID: {stage_id})", error))
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::{ArchiveStatus, CaptureKind};

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("beblog-offline-{}", uuid::Uuid::new_v4().simple()))
    }

    fn params() -> Vec<u8> {
        b"device=fixture:offline\nformat=RGB\nbytes_per_line=1692\npixels_per_line=564\nlines=850\ndepth=8\nraw_bytes=1438200\nterminal_status=5\n".to_vec()
    }

    fn log() -> Vec<u8> {
        let mut text = String::from(
            "sane_init -> 0 (Success)\nsane_open (only call) -> 0 (Success)\nframe descriptor range=1..6 quant=1\nsane_control_option(frame, SET=1) -> 0 (Success)\nsane_control_option(mode, SET='Color') -> 0 (Success)\nsane_control_option(resolution, SET=600) -> 0 (Success)\nparameters(pre): format=RGB last=1 bpl=1692 pixels=564 lines=850 depth=8\nsane_start (only call) -> 0 (Success)\nparameters(post): format=RGB last=1 bpl=1692 pixels=564 lines=850 depth=8\n",
        );
        text.push_str("synthetic unknown diagnostic: retained as non-fatal\n");
        for _ in 0..87 {
            text.push_str("sane_read -> 0 (Success), bytes=16384\n");
        }
        text.push_str(
            "sane_read -> 0 (Success), bytes=12792\nsane_read -> 5 (End of file reached), bytes=0\ncapture ended: total=1438200 terminal=5 (End of file reached)\nmetadata /ignored/path.params.txt -> written\nTIFF /ignored/path.tiff -> written\nsane_close\nsane_exit\nclient exit -> 0 (Success)\n",
        );
        text.into_bytes()
    }

    fn put16(bytes: &mut [u8], offset: usize, value: u16) {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put32(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn tiff(compression: u16, corrupt_ifd: bool, truncate: bool) -> Vec<u8> {
        // Minimal little-endian, one-strip, uncompressed RGB TIFF for the fixed profile.
        let tags: [(u16, u16, u32, u32); 11] = [
            (256, 4, 1, WIDTH),
            (257, 4, 1, HEIGHT),
            (258, 3, 3, 0),
            (259, 3, 1, compression.into()),
            (262, 3, 1, 2),
            (273, 4, 1, 0),
            (277, 3, 1, 3),
            (278, 4, 1, HEIGHT),
            (279, 4, 1, RAW_BYTES as u32),
            (284, 3, 1, 1),
            (339, 3, 3, 0),
        ];
        let ifd = 8_usize;
        let ifd_end = ifd + 2 + tags.len() * 12 + 4;
        let bits_at = ifd_end;
        let data_at = bits_at + 6;
        let sample_format_at = data_at + RAW_BYTES;
        let mut bytes = vec![0_u8; sample_format_at + 6];
        bytes[..2].copy_from_slice(b"II");
        put16(&mut bytes, 2, 42);
        put32(&mut bytes, 4, ifd as u32);
        put16(&mut bytes, ifd, tags.len() as u16);
        for (index, (tag, kind, count, value)) in tags.iter().enumerate() {
            let pos = ifd + 2 + index * 12;
            put16(&mut bytes, pos, *tag);
            put16(&mut bytes, pos + 2, *kind);
            put32(&mut bytes, pos + 4, *count);
            let value = match tag {
                258 => bits_at as u32,
                273 => data_at as u32,
                339 => sample_format_at as u32,
                _ => *value,
            };
            if *kind == 3 && *count == 1 {
                put16(&mut bytes, pos + 8, value as u16);
            } else {
                put32(&mut bytes, pos + 8, value);
            }
        }
        put32(&mut bytes, ifd + 2 + tags.len() * 12, 0);
        put16(&mut bytes, bits_at, 8);
        put16(&mut bytes, bits_at + 2, 8);
        put16(&mut bytes, bits_at + 4, 8);
        put16(&mut bytes, sample_format_at, 1);
        put16(&mut bytes, sample_format_at + 2, 1);
        put16(&mut bytes, sample_format_at + 4, 1);
        if corrupt_ifd {
            put32(&mut bytes, 4, u32::MAX);
        }
        if truncate {
            bytes.truncate(bytes.len() - 8);
        }
        bytes
    }

    fn write_sources(
        base: &Path,
        raw: &[u8],
        tiff_bytes: &[u8],
        params_bytes: &[u8],
        log_bytes: &[u8],
    ) -> OfflineArtifactPaths {
        fs::create_dir_all(base).unwrap();
        let paths = OfflineArtifactPaths {
            raw: base.join("source.raw"),
            tiff: base.join("source.tiff"),
            params_txt: base.join("source.params.txt"),
            client_log: base.join("source.client.log"),
        };
        for (path, bytes) in [
            (&paths.raw, raw),
            (&paths.tiff, tiff_bytes),
            (&paths.params_txt, params_bytes),
            (&paths.client_log, log_bytes),
        ] {
            fs::write(path, bytes).unwrap();
        }
        paths
    }

    #[test]
    fn imports_valid_synthetic_set_as_offline_not_production_and_hashes_all_originals() {
        let root = root();
        let source_root = root.join("input");
        let raw = vec![42; RAW_BYTES];
        let sources = write_sources(
            &source_root,
            &raw,
            &tiff(1, false, false),
            &params(),
            &log(),
        );
        let report =
            import_offline_artifacts(&ArchiveManager::new(root.clone()), 1, &sources).unwrap();
        assert_eq!(report.manifest.capture_kind, CaptureKind::OfflineImport);
        assert_eq!(report.manifest.status, ArchiveStatus::Completed);
        assert_eq!(report.manifest.files.len(), 4);
        assert_eq!(
            report.process_evidence,
            ProcessEvidence::NotObservedByOfflineImport
        );
        assert_eq!(
            report.sane_cleanup_evidence,
            ProcessEvidence::NotObservedByOfflineImport
        );
        let capture = root.join(&report.manifest.capture_id);
        assert_eq!(fs::read(capture.join("capture.raw")).unwrap(), raw);
        assert_eq!(
            fs::read(capture.join("capture.params.txt")).unwrap(),
            params()
        );
        for entry in &report.manifest.files {
            let archived = fs::read(capture.join(&entry.name)).unwrap();
            assert_eq!(archived.len() as u64, entry.bytes);
            assert_eq!(format!("{:x}", Sha256::digest(archived)), entry.sha256);
        }
        assert_eq!(fs::read(&sources.raw).unwrap().len(), RAW_BYTES);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_missing_files_wrong_raw_size_and_inconsistent_parameters() {
        let root = root();
        let archive = ArchiveManager::new(root.clone());
        let source = root.join("input");
        let good_raw = vec![1; RAW_BYTES];
        let good_tiff = tiff(1, false, false);
        let paths = write_sources(&source, &good_raw, &good_tiff, &params(), &log());
        fs::remove_file(&paths.client_log).unwrap();
        assert!(import_offline_artifacts(&archive, 1, &paths).is_err());
        let paths = write_sources(
            &source,
            &good_raw[..RAW_BYTES - 1],
            &good_tiff,
            &params(),
            &log(),
        );
        assert!(import_offline_artifacts(&archive, 1, &paths).is_err());
        let mut bad_params = params();
        bad_params = String::from_utf8(bad_params)
            .unwrap()
            .replace("depth=8", "depth=16")
            .into_bytes();
        let paths = write_sources(&source, &good_raw, &good_tiff, &bad_params, &log());
        assert!(import_offline_artifacts(&archive, 1, &paths).is_err());
        assert!(fs::read_dir(root.join(".staging")).unwrap().count() >= 3);
        for entry in fs::read_dir(root.join(".staging")).unwrap() {
            let directory = entry.unwrap().path();
            let manifest: StagingManifest =
                serde_json::from_slice(&fs::read(directory.join("stage.json")).unwrap()).unwrap();
            assert_eq!(manifest.status, StagingStatus::Failed);
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_bad_tiff_ifd_offsets_compression_and_truncated_pixels() {
        let raw = vec![1; RAW_BYTES];
        let mut bad_strip_offset = tiff(1, false, false);
        let ifd = 8;
        let entry_count =
            u16::from_le_bytes([bad_strip_offset[ifd], bad_strip_offset[ifd + 1]]) as usize;
        for index in 0..entry_count {
            let entry = ifd + 2 + index * 12;
            if u16::from_le_bytes([bad_strip_offset[entry], bad_strip_offset[entry + 1]]) == 273 {
                put32(&mut bad_strip_offset, entry + 8, u32::MAX);
            }
        }
        for tiff_bytes in [
            tiff(1, true, false),
            tiff(5, false, false),
            tiff(1, false, true),
            bad_strip_offset,
        ] {
            assert!(validate_artifact_bytes(1, &raw, &tiff_bytes, &params(), &log()).is_err());
        }
    }

    #[test]
    fn archive_collision_preserves_existing_capture_file_and_marks_import_failed() {
        let root = root();
        let archive = ArchiveManager::new(root.clone());
        let pending = archive.begin_offline_import(1).unwrap();
        let raw_path = pending.directory.join("capture.raw");
        fs::write(&raw_path, b"pre-existing original").unwrap();
        let error = archive
            .finalize_offline_import(
                &pending,
                OfflineCaptureProduct {
                    raw: vec![9; RAW_BYTES],
                    tiff: tiff(1, false, false),
                    params_txt: params(),
                    client_log: log(),
                },
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(raw_path).unwrap(), b"pre-existing original");
        let manifest: CaptureManifest =
            serde_json::from_slice(&fs::read(pending.directory.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest.status, ArchiveStatus::Failed);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn generic_archive_api_cannot_create_a_production_capture_without_proof_path() {
        let archive = ArchiveManager::new(root());
        let error = archive
            .begin(
                &crate::model::StartScanRequest {
                    frame: 1,
                    simulation: crate::model::SimulationMode::Success,
                },
                CaptureKind::Production,
            )
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlink_source_and_does_not_follow_log_path() {
        use std::os::unix::fs::symlink;
        let root = root();
        let input = root.join("input");
        fs::create_dir_all(&input).unwrap();
        let paths = write_sources(
            &input,
            &vec![1; RAW_BYTES],
            &tiff(1, false, false),
            &params(),
            &log(),
        );
        let target = input.join("real.raw");
        fs::write(&target, vec![1; RAW_BYTES]).unwrap();
        fs::remove_file(&paths.raw).unwrap();
        symlink(&target, &paths.raw).unwrap();
        assert!(import_offline_artifacts(&ArchiveManager::new(root.clone()), 1, &paths).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn recovery_marks_interrupted_staging_without_resuming_import() {
        let root = root();
        let archive = ArchiveManager::new(root.clone());
        let (_directory, manifest) = reserve_stage(&root).unwrap();
        let notices = archive.recover_incomplete().unwrap();
        assert_eq!(notices, vec![format!("staging:{}", manifest.capture_id)]);
        let stage_dir = root.join(".staging").join(manifest.capture_id);
        let recovered: StagingManifest =
            serde_json::from_slice(&fs::read(stage_dir.join("stage.json")).unwrap()).unwrap();
        assert_eq!(recovered.status, StagingStatus::Interrupted);
        assert!(archive.recover_incomplete().unwrap().is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}
