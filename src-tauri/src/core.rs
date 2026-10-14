use crate::{
    adapter::{AdapterStep, FakeScannerAdapter, ScannerAdapter},
    archive::{ArchiveManager, ArchiveStatus, BatchArchiveStatus, PendingCapture},
    batch_plan::D6BatchRequest,
    guided_process::{
        D6BatchPhase, D6BatchScanRequest, D6BatchUiEvent, GuidedProcessController,
        GuidedScanRequest,
    },
    model::{
        AppSnapshot, BatchSnapshot, EventLevel, HolderStatus, ScanError, ScanEvent, ScanState,
        SimulationMode, StartBatchRequest, StartScanRequest,
    },
    scanner_discovery::{self, DiscoveryResult},
    scanner_lease::{ScannerAction, ScannerLeaseGuard, ScannerLeaseManager},
    state_machine::ScanStateMachine,
};
use serde::Serialize;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter};

#[cfg(test)]
use crate::process::RecordedFault;

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    pub id: String,
    pub capture_id: String,
    pub frame: u8,
    pub state: ScanState,
    pub created_at_ms: u128,
}

struct ActiveJob {
    id: String,
    cancelled: Arc<AtomicBool>,
}

struct PendingBatch {
    id: String,
    request: StartBatchRequest,
    cancelled: Arc<AtomicBool>,
    confirmed: bool,
    completed_frames: u8,
    current_frame: Option<u8>,
    cooldown_remaining: u8,
}

struct ActiveD6Batch {
    id: String,
    request: D6BatchScanRequest,
    cancelled: Arc<AtomicBool>,
    current_frame: Option<u8>,
    completed_frames: u8,
    cooldown_remaining: u8,
    awaiting_holder_confirmation: bool,
    cleanup_uncertain: bool,
}

struct CoreData {
    active: Option<ActiveJob>,
    state: ScanState,
    progress: u8,
    holder_detected: bool,
    jobs: Vec<JobSummary>,
    batch: Option<PendingBatch>,
    d6_batch: Option<ActiveD6Batch>,
    scanner_lease: Option<ScannerLeaseGuard>,
    recovery_notices: Vec<String>,
}

impl Default for CoreData {
    fn default() -> Self {
        Self {
            active: None,
            state: ScanState::Idle,
            progress: 0,
            holder_detected: false,
            jobs: Vec::new(),
            batch: None,
            d6_batch: None,
            scanner_lease: None,
            recovery_notices: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub struct AppCore {
    data: Arc<Mutex<CoreData>>,
    adapter: Arc<dyn ScannerAdapter>,
    archive: ArchiveManager,
    clock: Arc<dyn BatchClock>,
    guided: GuidedProcessController,
    scanner_lease: ScannerLeaseManager,
    resource_dir: std::path::PathBuf,
}

pub trait BatchClock: Send + Sync + 'static {
    fn wait_second(&self, cancelled: &AtomicBool);
}
struct SystemBatchClock;
impl BatchClock for SystemBatchClock {
    fn wait_second(&self, cancelled: &AtomicBool) {
        for _ in 0..10 {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

impl Default for AppCore {
    fn default() -> Self {
        Self::new(std::env::temp_dir().join("beblog-digitizer-test-archive"))
    }
}

impl AppCore {
    pub fn new(archive_root: std::path::PathBuf) -> Self {
        Self::new_with_clock(archive_root, Arc::new(SystemBatchClock))
    }

    pub fn new_with_clock(archive_root: std::path::PathBuf, clock: Arc<dyn BatchClock>) -> Self {
        Self::new_with_clock_and_resource(archive_root, clock, std::path::PathBuf::new())
    }

    pub fn new_with_resource(
        archive_root: std::path::PathBuf,
        resource_dir: std::path::PathBuf,
    ) -> Self {
        Self::new_with_clock_and_resource(archive_root, Arc::new(SystemBatchClock), resource_dir)
    }

    fn new_with_clock_and_resource(
        archive_root: std::path::PathBuf,
        clock: Arc<dyn BatchClock>,
        resource_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            data: Arc::new(Mutex::new(CoreData::default())),
            adapter: Arc::new(FakeScannerAdapter::default()),
            archive: ArchiveManager::new(archive_root),
            clock,
            guided: GuidedProcessController::default(),
            scanner_lease: ScannerLeaseManager::default(),
            resource_dir,
        }
    }

    #[cfg(test)]
    fn new_with_recorded_fault(archive_root: std::path::PathBuf, fault: RecordedFault) -> Self {
        Self {
            data: Arc::new(Mutex::new(CoreData::default())),
            adapter: Arc::new(FakeScannerAdapter::with_recorded_fault(fault)),
            archive: ArchiveManager::new(archive_root),
            clock: Arc::new(SystemBatchClock),
            guided: GuidedProcessController::default(),
            scanner_lease: ScannerLeaseManager::default(),
            resource_dir: std::path::PathBuf::new(),
        }
    }

    pub fn start_guided_scan(
        &self,
        app: AppHandle,
        request: GuidedScanRequest,
    ) -> Result<String, String> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
        ensure_idle(&data)?;
        let lease = self.scanner_lease.acquire(ScannerAction::D5SingleFrame)?;
        let on_finished =
            guided_completion_callback(self.data.clone(), app.clone(), cancelled.clone());
        let run_id = self.guided.start_single_frame(
            app,
            self.resource_dir.clone(),
            self.archive.root_path().join(".client-work"),
            self.archive.clone(),
            request,
            cancelled.clone(),
            on_finished,
        )?;
        data.active = Some(ActiveJob {
            id: format!("guided-{run_id}"),
            cancelled,
        });
        data.scanner_lease = Some(lease);
        data.state = ScanState::OpeningScanner;
        data.progress = 0;
        data.holder_detected = false;
        Ok(run_id)
    }

    pub fn start_guided_simulation(
        &self,
        app: AppHandle,
        frame: u8,
        dpi: u16,
        output_directory: std::path::PathBuf,
    ) -> Result<String, String> {
        let request = GuidedScanRequest {
            frame,
            dpi,
            device: "simulator:hardware-free".into(),
            confirmed_device: "simulator:hardware-free".into(),
            output_directory,
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
        ensure_idle(&data)?;
        let lease = self.scanner_lease.acquire(ScannerAction::D1Simulation)?;
        let on_finished =
            guided_completion_callback(self.data.clone(), app.clone(), cancelled.clone());
        let run_id = self.guided.start_simulation(
            app,
            self.archive.root_path().join(".client-work"),
            self.archive.clone(),
            request,
            cancelled.clone(),
            on_finished,
        )?;
        data.active = Some(ActiveJob {
            id: format!("guided-{run_id}"),
            cancelled,
        });
        data.scanner_lease = Some(lease);
        data.state = ScanState::OpeningScanner;
        data.progress = 0;
        data.holder_detected = false;
        Ok(run_id)
    }

    pub fn confirm_guided_holder(&self) -> Result<(), String> {
        self.guided.confirm_holder()
    }

    pub fn cancel_guided_scan(&self) -> Result<(), String> {
        self.guided.cancel()?;
        let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
        if let Some(job) = data
            .active
            .as_ref()
            .filter(|job| job.id.starts_with("guided-"))
        {
            job.cancelled.store(true, Ordering::SeqCst);
            data.state = ScanState::Cancelling;
        }
        Ok(())
    }

    pub fn recover_archive(&self) -> Result<Vec<String>, String> {
        let recovered = self
            .archive
            .recover_incomplete()
            .map_err(|error| format!("Archivwiederherstellung fehlgeschlagen: {error}"))?;
        self.data
            .lock()
            .map_err(|_| "Interner Zustandsfehler")?
            .recovery_notices = recovered.clone();
        Ok(recovered)
    }

    pub fn snapshot(&self) -> AppSnapshot {
        let data = self.data.lock().expect("core mutex poisoned");
        AppSnapshot {
            adapter: self.adapter.name(),
            scanner_status: if data.active.is_some()
                || data.batch.as_ref().is_some_and(|batch| batch.confirmed)
            {
                "busy"
            } else {
                "ready"
            },
            holder_status: if data.holder_detected {
                "detected"
            } else {
                "unknown"
            },
            active_job_id: data
                .active
                .as_ref()
                .map(|job| job.id.clone())
                .or_else(|| data.batch.as_ref().map(|batch| batch.id.clone())),
            state: data.state,
            progress: data.progress,
            batch: data.batch.as_ref().map(batch_snapshot),
            recovery_notices: data.recovery_notices.clone(),
        }
    }

    pub fn jobs(&self) -> Vec<JobSummary> {
        self.data.lock().expect("core mutex poisoned").jobs.clone()
    }

    pub fn start(&self, app: AppHandle, request: StartScanRequest) -> Result<String, String> {
        if !(1..=6).contains(&request.frame) {
            return Err("Frame muss zwischen 1 und 6 liegen".into());
        }
        let id = format!("scan-{:06}", NEXT_JOB.fetch_add(1, Ordering::Relaxed));
        let cancelled = Arc::new(AtomicBool::new(false));
        let pending = {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            ensure_idle(&data)?;
            let lease = self.scanner_lease.acquire(ScannerAction::D1Simulation)?;
            let pending = self
                .archive
                .begin(&request, self.adapter.capture_kind())
                .map_err(|error| format!("Capture-Archiv konnte nicht angelegt werden: {error}"))?;
            data.scanner_lease = Some(lease);
            data.active = Some(ActiveJob {
                id: id.clone(),
                cancelled: cancelled.clone(),
            });
            data.state = ScanState::Initializing;
            data.progress = 0;
            data.holder_detected = false;
            data.jobs.push(JobSummary {
                id: id.clone(),
                capture_id: pending.id.clone(),
                frame: request.frame,
                state: ScanState::Initializing,
                created_at_ms: now_ms(),
            });
            pending
        };

        let core = self.clone();
        let job_id = id.clone();
        std::thread::spawn(move || core.run_job(app, job_id, request, cancelled, pending));
        Ok(id)
    }

    /// Performs discovery only when explicitly requested and holds the core
    /// lock for the complete SANE enumeration, excluding all scan starts.
    pub fn discover_minolta_scanners(&self) -> Result<DiscoveryResult, String> {
        let data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
        ensure_idle(&data)?;
        let _lease = self.scanner_lease.acquire(ScannerAction::Discovery)?;
        let cooldown = self.guided.cooldown_remaining_seconds();
        if cooldown > 0 {
            return Err(format!(
                "Scannererkennung erst nach dem Cooldown in {cooldown} s"
            ));
        }
        scanner_discovery::discover()
    }

    pub fn start_d6_batch<R: tauri::Runtime + 'static>(
        &self,
        app: AppHandle<R>,
        request: D6BatchScanRequest,
    ) -> Result<String, String> {
        self.start_d6_batch_inner(app, request, false)
    }

    #[cfg(test)]
    fn start_d6_batch_simulation<R: tauri::Runtime + 'static>(
        &self,
        app: AppHandle<R>,
        request: D6BatchScanRequest,
    ) -> Result<String, String> {
        self.start_d6_batch_inner(app, request, true)
    }

    fn start_d6_batch_inner<R: tauri::Runtime + 'static>(
        &self,
        app: AppHandle<R>,
        request: D6BatchScanRequest,
        test_simulator: bool,
    ) -> Result<String, String> {
        let plan = D6BatchRequest {
            frames: request.frames.clone(),
        };
        plan.validate()?;
        let initial = guided_request_for_d6(&request, request.frames[0]);
        crate::guided_process::validate_request(&initial)?;
        let cooldown = self.guided.cooldown_remaining_seconds();
        if cooldown > 0 {
            return Err(format!(
                "Nächster Batch erst nach dem Cooldown in {cooldown} s"
            ));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let (id, event) = {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            ensure_idle(&data)?;
            let lease = self.scanner_lease.acquire(ScannerAction::D6Batch)?;
            let manifest = self.archive.begin_d6_batch(&plan).map_err(|error| {
                format!("D6-Batchjournal konnte nicht angelegt werden: {error}")
            })?;
            let id = manifest.batch_id.as_str().to_string();
            data.scanner_lease = Some(lease);
            data.d6_batch = Some(ActiveD6Batch {
                id: id.clone(),
                request: request.clone(),
                cancelled: cancelled.clone(),
                current_frame: None,
                completed_frames: 0,
                cooldown_remaining: 0,
                awaiting_holder_confirmation: false,
                cleanup_uncertain: false,
            });
            data.state = ScanState::Initializing;
            data.progress = 0;
            data.holder_detected = false;
            let event = d6_event(
                data.d6_batch.as_ref().unwrap(),
                D6BatchPhase::Starting,
                "D6-Batch gestartet; der erste neue Clientprozess wird vorbereitet".into(),
            );
            (id, event)
        };
        emit_d6(&app, event);
        let core = self.clone();
        let worker_id = id.clone();
        std::thread::spawn(move || {
            core.run_d6_batch(app, worker_id, request, cancelled, test_simulator)
        });
        Ok(id)
    }

    pub fn confirm_d6_batch_holder<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> Result<(), String> {
        {
            let data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            let batch = data.d6_batch.as_ref().ok_or("Kein aktiver D6-Batch")?;
            if !batch.awaiting_holder_confirmation || batch.current_frame != Some(1) {
                return Err("Die manuelle Halterbestätigung ist derzeit nicht angefordert".into());
            }
        }
        self.guided.confirm_holder()?;
        let event = {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            let batch = data.d6_batch.as_mut().ok_or("D6-Batch wurde beendet")?;
            batch.awaiting_holder_confirmation = false;
            d6_event(
                batch,
                D6BatchPhase::WaitingForHolderDetection,
                "Einlegen bestätigt; tatsächliche Haltererkennung wird abgewartet".into(),
            )
        };
        emit_d6(app, event);
        Ok(())
    }

    pub fn cancel_d6_batch<R: tauri::Runtime>(&self, app: &AppHandle<R>) -> Result<(), String> {
        let event = {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            data.state = ScanState::Cancelling;
            let batch = data.d6_batch.as_mut().ok_or("Kein aktiver D6-Batch")?;
            batch.cancelled.store(true, Ordering::SeqCst);
            batch.awaiting_holder_confirmation = false;
            d6_event(
                batch,
                D6BatchPhase::Cancelling,
                "Batchabbruch angefordert; aktiver Client wird kontrolliert beendet".into(),
            )
        };
        emit_d6(app, event);
        Ok(())
    }

    fn run_d6_batch<R: tauri::Runtime + 'static>(
        &self,
        app: AppHandle<R>,
        batch_id: String,
        request: D6BatchScanRequest,
        cancelled: Arc<AtomicBool>,
        test_simulator: bool,
    ) {
        let mut all_cleanups_confirmed = true;
        for (index, frame) in request.frames.iter().copied().enumerate() {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            let capture_id = format!("d6-capture-{}", uuid::Uuid::new_v4().simple());
            let frame_id = match self.archive.begin_d6_frame(&batch_id, frame, &capture_id) {
                Ok(frame_id) => frame_id,
                Err(error) => {
                    self.finish_d6_batch(
                        &app,
                        &batch_id,
                        D6BatchPhase::Failed,
                        ScanState::BatchFailed,
                        format!("Frame {frame}: Zuordnung im Batchjournal fehlgeschlagen: {error}"),
                        true,
                    );
                    return;
                }
            };
            let pending = match self.archive.begin_d6_capture(&batch_id, frame, &capture_id) {
                Ok(pending) => pending,
                Err(error) => {
                    let journal_error = self
                        .archive
                        .fail_d6_frame(&batch_id, frame, format!("Capture-Reservierung: {error}"))
                        .err();
                    let suffix = journal_error
                        .map(|error| format!("; Journalfehler: {error}"))
                        .unwrap_or_default();
                    self.finish_d6_batch(
                        &app,
                        &batch_id,
                        D6BatchPhase::Failed,
                        ScanState::BatchFailed,
                        format!("Frame {frame}: Capture konnte nicht reserviert werden: {error}{suffix}"),
                        true,
                    );
                    return;
                }
            };
            self.update_d6_progress(
                &app,
                &batch_id,
                Some(frame),
                0,
                D6BatchPhase::Starting,
                format!(
                    "Frame {frame} von {} wird vorbereitet ({frame_id})",
                    request.frames.len()
                ),
            );
            let scan_request = guided_request_for_d6(&request, frame);
            let (finished_tx, finished_rx) = std::sync::mpsc::sync_channel(1);
            let core_for_event = self.clone();
            let app_for_event = app.clone();
            let batch_for_event = batch_id.clone();
            let auto_continue = index > 0;
            let on_client_event = move |client_event| {
                core_for_event.note_d6_process_event(
                    &app_for_event,
                    &batch_for_event,
                    frame,
                    auto_continue,
                    client_event,
                );
            };
            let on_finished = move |result, cleanup_confirmed| {
                let _ = finished_tx.send((result, cleanup_confirmed));
            };
            let launch = if test_simulator {
                #[cfg(test)]
                {
                    self.guided.start_d6_simulated_single_frame(
                        app.clone(),
                        self.archive.root_path().join(".client-work"),
                        self.archive.clone(),
                        scan_request,
                        batch_id.clone(),
                        pending.clone(),
                        cancelled.clone(),
                        auto_continue,
                        on_client_event,
                        on_finished,
                    )
                }
                #[cfg(not(test))]
                {
                    unreachable!("test simulator cannot be selected in production")
                }
            } else {
                self.guided.start_d6_single_frame(
                    app.clone(),
                    self.resource_dir.clone(),
                    self.archive.root_path().join(".client-work"),
                    self.archive.clone(),
                    scan_request,
                    batch_id.clone(),
                    pending.clone(),
                    cancelled.clone(),
                    auto_continue,
                    on_client_event,
                    on_finished,
                )
            };
            if let Err(error) = launch {
                let _ = self
                    .archive
                    .mark(&pending, ArchiveStatus::Failed, error.clone());
                let journal_error = self
                    .archive
                    .fail_d6_frame(&batch_id, frame, error.clone())
                    .err();
                let suffix = journal_error
                    .map(|error| format!("; Journalfehler: {error}"))
                    .unwrap_or_default();
                self.finish_d6_batch(
                    &app,
                    &batch_id,
                    D6BatchPhase::Failed,
                    ScanState::BatchFailed,
                    format!("Frame {frame}: Clientstart fehlgeschlagen: {error}{suffix}"),
                    true,
                );
                return;
            }
            let (result, cleanup_confirmed) = match finished_rx.recv() {
                Ok(value) => value,
                Err(error) => {
                    self.finish_d6_batch(
                        &app,
                        &batch_id,
                        D6BatchPhase::Failed,
                        ScanState::BatchFailed,
                        format!("Frame {frame}: Prozessabschluss konnte nicht bestätigt werden: {error}"),
                        false,
                    );
                    return;
                }
            };
            all_cleanups_confirmed &= cleanup_confirmed;
            if let Err(error) = result {
                let was_cancelled = cancelled.load(Ordering::SeqCst);
                let capture_status = if was_cancelled {
                    ArchiveStatus::Cancelled
                } else {
                    ArchiveStatus::Failed
                };
                let _ = self.archive.mark(&pending, capture_status, error.clone());
                let journal_error = if was_cancelled {
                    self.archive
                        .cancel_d6_batch(&batch_id, Some(frame), error.clone())
                        .err()
                } else {
                    self.archive
                        .fail_d6_frame(&batch_id, frame, error.clone())
                        .err()
                };
                let suffix = journal_error
                    .map(|error| format!("; D6-Journalfehler: {error}; Recovery erforderlich"))
                    .unwrap_or_default();
                let phase = if was_cancelled {
                    D6BatchPhase::Cancelled
                } else {
                    D6BatchPhase::Failed
                };
                let state = if was_cancelled {
                    ScanState::BatchCancelled
                } else {
                    ScanState::BatchFailed
                };
                self.finish_d6_batch(
                    &app,
                    &batch_id,
                    phase,
                    state,
                    format!("Frame {frame}: {error}{suffix}"),
                    cleanup_confirmed,
                );
                return;
            }
            {
                let mut data = self.data.lock().expect("core mutex poisoned");
                if let Some(batch) = data.d6_batch.as_mut().filter(|batch| batch.id == batch_id) {
                    batch.completed_frames = batch.completed_frames.saturating_add(1);
                    batch.awaiting_holder_confirmation = false;
                }
            }
            if cancelled.load(Ordering::SeqCst) && index + 1 < request.frames.len() {
                let journal_error = self
                    .archive
                    .cancel_d6_batch(&batch_id, None, "Batch nach Frameabschluss abgebrochen")
                    .err();
                let suffix = journal_error
                    .map(|error| format!("; D6-Journalfehler: {error}; Recovery erforderlich"))
                    .unwrap_or_default();
                self.finish_d6_batch(
                    &app,
                    &batch_id,
                    D6BatchPhase::Cancelled,
                    ScanState::BatchCancelled,
                    format!(
                        "Batch abgebrochen; bereits archivierte Frames bleiben erhalten{suffix}"
                    ),
                    all_cleanups_confirmed,
                );
                return;
            }
            if index + 1 < request.frames.len() {
                let cooldown_completed = wait_d6_cooldown(
                    self.clock.as_ref(),
                    &cancelled,
                    |remaining| {
                        self.update_d6_progress(
                        &app,
                        &batch_id,
                        Some(frame),
                        remaining,
                        D6BatchPhase::InterFrameCooldown,
                        format!("Frame {frame} archiviert; nächster Prozess startet nach {remaining} Sekunden"),
                    );
                    },
                );
                if !cooldown_completed {
                    let journal_error = self
                        .archive
                        .cancel_d6_batch(&batch_id, None, "Abbruch während Cooldown")
                        .err();
                    let suffix = journal_error
                        .map(|error| format!("; D6-Journalfehler: {error}; Recovery erforderlich"))
                        .unwrap_or_default();
                    self.finish_d6_batch(
                        &app,
                        &batch_id,
                        D6BatchPhase::Cancelled,
                        ScanState::BatchCancelled,
                        format!("Batch während der Bereitschaftspause abgebrochen; abgeschlossene Frames bleiben erhalten{suffix}"),
                        all_cleanups_confirmed,
                    );
                    return;
                }
            }
        }
        let all_frames_completed = self
            .data
            .lock()
            .expect("core mutex poisoned")
            .d6_batch
            .as_ref()
            .is_some_and(|batch| batch.completed_frames as usize == batch.request.frames.len());
        if cancelled.load(Ordering::SeqCst) && !all_frames_completed {
            let journal_error = self
                .archive
                .cancel_d6_batch(&batch_id, None, "Batch vor dem nächsten Frame abgebrochen")
                .err();
            let suffix = journal_error
                .map(|error| format!("; D6-Journalfehler: {error}; Recovery erforderlich"))
                .unwrap_or_default();
            self.finish_d6_batch(
                &app,
                &batch_id,
                D6BatchPhase::Cancelled,
                ScanState::BatchCancelled,
                format!("Batch abgebrochen; abgeschlossene Frames bleiben erhalten{suffix}"),
                all_cleanups_confirmed,
            );
        } else {
            self.finish_d6_batch(
                &app,
                &batch_id,
                D6BatchPhase::Completed,
                ScanState::BatchCompleted,
                "Alle D6-Frames wurden validiert, archiviert und exportiert".into(),
                all_cleanups_confirmed,
            );
        }
    }

    fn note_d6_process_event<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
        frame: u8,
        auto_continue: bool,
        client_event: crate::guided_protocol::ClientEvent,
    ) {
        let (phase, awaiting, message) = match client_event {
            crate::guided_protocol::ClientEvent::Opened => (
                if auto_continue {
                    D6BatchPhase::WaitingForHolderDetection
                } else {
                    D6BatchPhase::AwaitingHolderConfirmation
                },
                !auto_continue,
                if auto_continue {
                    format!("Frame {frame}: OPENED empfangen; Filmhalter bleibt eingesetzt, CONTINUE wird gesendet")
                } else {
                    "OPENED empfangen; Filmhalter jetzt einsetzen und bestätigen".into()
                },
            ),
            crate::guided_protocol::ClientEvent::HolderDetected => (
                D6BatchPhase::Scanning,
                false,
                format!("Frame {frame}: Halter durch den Scanner erkannt"),
            ),
            crate::guided_protocol::ClientEvent::DataChunk { total, .. } => (
                D6BatchPhase::Scanning,
                false,
                format!("Frame {frame}: Datenübertragung bei {total} Byte"),
            ),
            crate::guided_protocol::ClientEvent::Eof { total } => (
                D6BatchPhase::Scanning,
                false,
                format!("Frame {frame}: EOF nach {total} Byte; Artefakte werden geprüft"),
            ),
            _ => return,
        };
        let event = {
            let mut data = self.data.lock().expect("core mutex poisoned");
            let Some(batch) = data.d6_batch.as_mut().filter(|batch| batch.id == id) else {
                return;
            };
            batch.current_frame = Some(frame);
            batch.awaiting_holder_confirmation = awaiting;
            d6_event(batch, phase, message)
        };
        emit_d6(app, event);
    }

    fn update_d6_progress<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
        frame: Option<u8>,
        cooldown: u8,
        phase: D6BatchPhase,
        message: String,
    ) {
        let event = {
            let mut data = self.data.lock().expect("core mutex poisoned");
            let Some(batch) = data.d6_batch.as_mut().filter(|batch| batch.id == id) else {
                return;
            };
            batch.current_frame = frame;
            batch.cooldown_remaining = cooldown;
            d6_event(batch, phase, message)
        };
        emit_d6(app, event);
    }

    fn finish_d6_batch<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
        phase: D6BatchPhase,
        state: ScanState,
        message: String,
        cleanup_confirmed: bool,
    ) {
        let event = {
            let mut data = self.data.lock().expect("core mutex poisoned");
            data.state = state;
            let Some(batch) = data.d6_batch.as_mut().filter(|batch| batch.id == id) else {
                return;
            };
            if !cleanup_confirmed {
                batch.cleanup_uncertain = true;
            }
            let event = d6_event(batch, phase, message);
            if cleanup_confirmed {
                data.d6_batch = None;
                data.scanner_lease = None;
            }
            event
        };
        emit_d6(app, event);
    }

    pub fn guided_cooldown_remaining_seconds(&self) -> u64 {
        self.guided.cooldown_remaining_seconds()
    }

    pub fn cancel(&self, app: &AppHandle) -> Result<(), String> {
        let (id, progress) = {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            let (id, cancelled) = data
                .active
                .as_ref()
                .map(|job| (job.id.clone(), job.cancelled.clone()))
                .ok_or_else(|| "Kein aktiver Scannerjob".to_string())?;
            cancelled.store(true, Ordering::SeqCst);
            data.state = ScanState::Cancelling;
            (id, data.progress)
        };
        emit(
            app,
            ScanEvent::new(
                id.clone(),
                ScanState::Cancelling,
                progress,
                "Kontrollierter Abbruch angefordert",
                EventLevel::Warning,
            ),
        );
        Ok(())
    }

    pub fn start_batch(
        &self,
        app: &AppHandle,
        request: StartBatchRequest,
    ) -> Result<String, String> {
        validate_batch_request(&request)?;
        let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
        ensure_idle(&data)?;
        let lease = self.scanner_lease.acquire(ScannerAction::D1Simulation)?;
        let id = self.create_unique_batch_journal(&request.frames)?;
        data.scanner_lease = Some(lease);
        data.batch = Some(PendingBatch {
            id: id.clone(),
            request,
            cancelled: Arc::new(AtomicBool::new(false)),
            confirmed: false,
            completed_frames: 0,
            current_frame: None,
            cooldown_remaining: 0,
        });
        data.state = ScanState::AwaitingHolderConfirmation;
        data.progress = 0;
        data.holder_detected = false;
        let event = batch_event(
            &data,
            ScanState::AwaitingHolderConfirmation,
            "Filmhalter mit Negativen vorbereiten; anschließend Einlegen bestätigen",
            EventLevel::Info,
        );
        drop(data);
        emit(app, event);
        Ok(id)
    }

    fn create_unique_batch_journal(&self, frames: &[u8]) -> Result<String, String> {
        self.archive
            .begin_batch_unique(frames)
            .map_err(|error| format!("Batchjournal konnte nicht angelegt werden: {error}"))
    }

    pub fn confirm_batch_holder(&self, app: AppHandle) -> Result<(), String> {
        let (id, request, cancelled) = {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            let batch = data.batch.as_mut().ok_or("Kein wartender Batch")?;
            if batch.confirmed {
                return Err("Einlegen wurde bereits bestätigt".into());
            }
            batch.confirmed = true;
            let values = (
                batch.id.clone(),
                batch.request.clone(),
                batch.cancelled.clone(),
            );
            data.state = ScanState::WaitingForHolderDetection;
            values
        };
        emit(
            &app,
            self.batch_event(
                ScanState::WaitingForHolderDetection,
                "Einlegen bestätigt; simuliere Haltererkennung",
                EventLevel::Info,
            ),
        );
        let core = self.clone();
        std::thread::spawn(move || core.run_batch(app, id, request, cancelled));
        Ok(())
    }

    pub fn cancel_batch(&self, app: &AppHandle) -> Result<(), String> {
        let (id, before_confirmation) = {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            let batch = data.batch.as_ref().ok_or("Kein aktiver Batch")?;
            batch.cancelled.store(true, Ordering::SeqCst);
            let before = !batch.confirmed;
            let id = batch.id.clone();
            data.state = ScanState::Cancelling;
            (id, before)
        };
        if before_confirmation {
            self.end_batch(
                app,
                &id,
                ScanState::BatchCancelled,
                "Batch vor Filmhalterbestätigung abgebrochen".into(),
            );
        } else {
            emit(
                app,
                ScanEvent::new(
                    id,
                    ScanState::Cancelling,
                    self.current_progress(),
                    "Batch-Abbruch angefordert",
                    EventLevel::Warning,
                ),
            );
        }
        Ok(())
    }

    fn batch_event(&self, state: ScanState, message: &str, level: EventLevel) -> ScanEvent {
        let data = self.data.lock().expect("core mutex poisoned");
        batch_event(&data, state, message, level)
    }

    fn run_batch(
        &self,
        app: AppHandle,
        id: String,
        request: StartBatchRequest,
        cancelled: Arc<AtomicBool>,
    ) {
        self.run_batch_with_emitter(id, request, cancelled, &mut |event| emit(&app, event));
    }

    fn run_batch_with_emitter(
        &self,
        id: String,
        request: StartBatchRequest,
        cancelled: Arc<AtomicBool>,
        emit_event: &mut dyn FnMut(ScanEvent),
    ) {
        let mut machine = ScanStateMachine::default();
        let _ = machine.transition(ScanState::AwaitingHolderConfirmation);
        let _ = machine.transition(ScanState::WaitingForHolderDetection);
        let mut cancel_archive_note = String::new();
        for (index, frame) in request.frames.iter().copied().enumerate() {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            {
                let mut data = self.data.lock().expect("core mutex poisoned");
                if let Some(batch) = data.batch.as_mut() {
                    batch.current_frame = Some(frame);
                    batch.cooldown_remaining = 0;
                }
                data.state = ScanState::Initializing;
                data.progress = 0;
            }
            let scan_request = StartScanRequest {
                frame,
                simulation: if request.failure_frame == Some(frame) {
                    request.simulation
                } else {
                    SimulationMode::Success
                },
            };
            let pending = match self.archive.begin_batch_frame(
                &id,
                &scan_request,
                self.adapter.capture_kind(),
            ) {
                Ok(p) => p,
                Err(e) => {
                    self.end_batch_with_emitter(
                        &id,
                        ScanState::BatchFailed,
                        format!("Archivstart fehlgeschlagen: {e}"),
                        emit_event,
                    );
                    return;
                }
            };
            let transition_error = Arc::new(Mutex::new(None::<String>));
            let error_slot = transition_error.clone();
            let result = self
                .adapter
                .scan(&scan_request, cancelled.clone(), &mut |step| {
                    if error_slot
                        .lock()
                        .expect("transition error mutex poisoned")
                        .is_some()
                    {
                        return;
                    }
                    if cancelled.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Some(next_state) = step.state {
                        if let Err(error) = machine.transition(next_state) {
                            *error_slot.lock().expect("transition error mutex poisoned") =
                                Some(error.clone());
                            let current = machine.state();
                            emit_event(
                                ScanEvent::new(
                                    id.clone(),
                                    current,
                                    self.current_progress(),
                                    format!(
                                    "Ungültiger Fake-Adapter-Übergang für Frame {frame}: {error}"
                                ),
                                    EventLevel::Error,
                                )
                                .with_batch(
                                    self.batch_event(
                                        current,
                                        "Ungültiger Adapterzustand; Batch wird beendet",
                                        EventLevel::Error,
                                    )
                                    .batch
                                    .unwrap(),
                                ),
                            );
                            return;
                        }
                    }
                    let current_state = machine.state();
                    let mut data = self.data.lock().expect("core mutex poisoned");
                    if let Some(next_state) = step.state {
                        data.state = next_state;
                        data.progress = step.progress;
                    }
                    if let Some(holder_status) = step.holder_status {
                        data.holder_detected = holder_status == HolderStatus::Detected;
                    }
                    let event = ScanEvent::new(
                        id.clone(),
                        current_state,
                        data.progress,
                        format!("Frame {frame}: {}", step.message),
                        step.level,
                    )
                    .with_batch(batch_snapshot(data.batch.as_ref().unwrap()))
                    .with_holder_status(step.holder_status);
                    drop(data);
                    emit_event(event);
                });
            if cancelled.load(Ordering::SeqCst) {
                cancel_archive_note = self
                    .archive
                    .mark(&pending, ArchiveStatus::Cancelled, "Batch cancelled")
                    .err()
                    .map(|error| format!("; Capture-Abbruchstatus nicht gespeichert: {error}; Recovery erforderlich"))
                    .unwrap_or_default();
                break;
            }
            if let Some(error) = transition_error
                .lock()
                .expect("transition error mutex poisoned")
                .take()
            {
                let archive_note = self.archive.mark(
                    &pending,
                    ArchiveStatus::Failed,
                    format!("Invalid adapter transition: {error}"),
                ).err().map(|mark_error| format!("; Capturefehlerstatus nicht gespeichert: {mark_error}; Recovery erforderlich")).unwrap_or_default();
                self.end_batch_with_emitter(
                    &id,
                    ScanState::BatchFailed,
                    format!("Frame {frame}: ungültiger Zustandsübergang: {error}{archive_note}"),
                    emit_event,
                );
                return;
            }
            let product = match result {
                Ok(p) if machine.state() == ScanState::ClosingScanner => p,
                Ok(_) => {
                    let archive_note = self.archive.mark(
                        &pending,
                        ArchiveStatus::Failed,
                        "Incomplete mock lifecycle",
                    ).err().map(|mark_error| format!("; Capturefehlerstatus nicht gespeichert: {mark_error}; Recovery erforderlich")).unwrap_or_default();
                    self.end_batch_with_emitter(
                        &id,
                        ScanState::BatchFailed,
                        format!("Frame {frame}: unvollständiger Lifecycle{archive_note}"),
                        emit_event,
                    );
                    return;
                }
                Err(e) => {
                    let status = if e == ScanError::TimedOut {
                        ArchiveStatus::TimedOut
                    } else {
                        ArchiveStatus::Failed
                    };
                    let archive_note = self.archive.mark(&pending, status, format!("{e:?}"))
                        .err()
                        .map(|mark_error| format!("; Capturefehlerstatus nicht gespeichert: {mark_error}; Recovery erforderlich"))
                        .unwrap_or_default();
                    self.end_batch_with_emitter(
                        &id,
                        ScanState::BatchFailed,
                        format!("Frame {frame} fehlgeschlagen: {e:?}{archive_note}"),
                        emit_event,
                    );
                    return;
                }
            };
            if let Err(e) = self.archive.finalize(&pending, product) {
                self.end_batch_with_emitter(
                    &id,
                    ScanState::BatchFailed,
                    format!("Frame {frame}: Ausgabe ungültig: {e}"),
                    emit_event,
                );
                return;
            }
            if let Err(e) = self.archive.update_batch(&id, Some(frame), None, None) {
                self.end_batch_with_emitter(
                    &id,
                    ScanState::BatchFailed,
                    format!("Batchjournal konnte nicht aktualisiert werden: {e}"),
                    emit_event,
                );
                return;
            }
            if index + 1 < request.frames.len() {
                if machine.transition(ScanState::InterFrameCooldown).is_err() {
                    self.end_batch_with_emitter(
                        &id,
                        ScanState::BatchFailed,
                        "Ungültiger Cooldown-Übergang".into(),
                        emit_event,
                    );
                    return;
                }
                for remaining in (1..=30).rev() {
                    if cancelled.load(Ordering::SeqCst) {
                        break;
                    }
                    {
                        let mut data = self.data.lock().expect("core mutex poisoned");
                        let batch = data.batch.as_mut().unwrap();
                        batch.completed_frames += u8::from(remaining == 30);
                        batch.cooldown_remaining = remaining;
                        data.state = ScanState::InterFrameCooldown;
                    }
                    emit_event(self.batch_event(
                        ScanState::InterFrameCooldown,
                        &format!("Bereitschaftspause: {remaining} Sekunden"),
                        EventLevel::Info,
                    ));
                    self.clock.wait_second(&cancelled);
                }
                if cancelled.load(Ordering::SeqCst) {
                    break;
                }
                let _ = machine.transition(ScanState::Initializing);
            } else {
                if machine.transition(ScanState::BatchCompleted).is_err() {
                    self.end_batch_with_emitter(
                        &id,
                        ScanState::BatchFailed,
                        "Batchabschluss-Zustand ungültig".into(),
                        emit_event,
                    );
                    return;
                }
                if let Some(batch) = self
                    .data
                    .lock()
                    .expect("core mutex poisoned")
                    .batch
                    .as_mut()
                {
                    batch.completed_frames += 1;
                }
            }
        }
        if cancelled.load(Ordering::SeqCst) {
            if machine.state() != ScanState::Cancelling {
                let _ = machine.transition(ScanState::Cancelling);
            }
            let _ = machine.transition(ScanState::BatchCancelled);
            self.end_batch_with_emitter(
                &id,
                ScanState::BatchCancelled,
                format!(
                    "Batch abgebrochen; archivierte Frames bleiben erhalten{cancel_archive_note}"
                ),
                emit_event,
            );
        } else {
            self.end_batch_with_emitter(
                &id,
                ScanState::BatchCompleted,
                "Batch erfolgreich abgeschlossen".into(),
                emit_event,
            );
        }
    }

    fn end_batch(&self, app: &AppHandle, id: &str, state: ScanState, message: String) {
        self.end_batch_with_emitter(id, state, message, &mut |event| emit(app, event));
    }

    fn end_batch_with_emitter(
        &self,
        id: &str,
        state: ScanState,
        message: String,
        emit_event: &mut dyn FnMut(ScanEvent),
    ) {
        let (state, message) = self.persist_batch_terminal(id, state, message);
        let event = {
            let mut data = self.data.lock().expect("core mutex poisoned");
            data.state = state;
            if state == ScanState::BatchCompleted {
                data.progress = 100;
            }
            let event = data.batch.as_ref().map(|b| {
                ScanEvent::new(
                    id.into(),
                    state,
                    data.progress,
                    message,
                    match state {
                        ScanState::BatchCompleted => EventLevel::Info,
                        ScanState::BatchCancelled => EventLevel::Warning,
                        _ => EventLevel::Error,
                    },
                )
                .with_batch(batch_snapshot(b))
            });
            if data.batch.as_ref().is_some_and(|batch| batch.id == id) {
                data.batch = None;
                data.scanner_lease = None;
            }
            event
        };
        if let Some(event) = event {
            emit_event(event);
        }
    }

    fn persist_batch_terminal(
        &self,
        id: &str,
        state: ScanState,
        message: String,
    ) -> (ScanState, String) {
        let status = match state {
            ScanState::BatchCompleted => BatchArchiveStatus::Completed,
            ScanState::BatchCancelled => BatchArchiveStatus::Cancelled,
            _ => BatchArchiveStatus::Failed,
        };
        let failure = if status == BatchArchiveStatus::Completed {
            None
        } else {
            Some(message.clone())
        };
        match self.archive.update_batch(id, None, Some(status), failure) {
            Ok(()) => (state, message),
            Err(error) => {
                let detail = format!("{message}; Batchjournal konnte nicht als {status:?} gespeichert werden: {error}");
                let fallback = self.archive.update_batch(
                    id,
                    None,
                    Some(BatchArchiveStatus::Failed),
                    Some(detail.clone()),
                );
                let suffix = match fallback { Ok(()) => "; Journal als fehlgeschlagen markiert".to_string(), Err(fallback_error) => format!("; auch Fehlerstatus konnte nicht persistiert werden: {fallback_error}; Journal bleibt für Recovery offen") };
                (ScanState::BatchFailed, format!("{detail}{suffix}"))
            }
        }
    }

    fn run_job(
        &self,
        app: AppHandle,
        id: String,
        request: StartScanRequest,
        cancelled: Arc<AtomicBool>,
        pending: PendingCapture,
    ) {
        self.run_job_with_emitter(id, request, cancelled, pending, &mut |event| {
            emit(&app, event)
        });
    }

    fn run_job_with_emitter(
        &self,
        id: String,
        request: StartScanRequest,
        cancelled: Arc<AtomicBool>,
        pending: PendingCapture,
        emit_event: &mut dyn FnMut(ScanEvent),
    ) {
        let mut machine = ScanStateMachine::default();
        let mut transition_error = None;
        let result = self.adapter.scan(&request, cancelled.clone(), &mut |step| {
            if transition_error.is_some() {
                return;
            }
            if cancelled.load(Ordering::SeqCst) {
                emit_event(ScanEvent::new(
                    id.clone(),
                    ScanState::Cancelling,
                    self.current_progress(),
                    "Adapter-Ereignis nach Abbruchanforderung ignoriert",
                    EventLevel::Warning,
                ));
                return;
            }
            match step.state {
                Some(next_state) => match machine.transition(next_state) {
                    Ok(()) => self.record_and_emit_with_emitter(&id, step, emit_event),
                    Err(error) => {
                        transition_error = Some(error.clone());
                        let current = self.snapshot().state;
                        emit_event(ScanEvent::new(
                            id.clone(),
                            current,
                            self.current_progress(),
                            format!("Ungültiger Adapter-Zustandsübergang verworfen: {error}"),
                            EventLevel::Error,
                        ));
                    }
                },
                None => {
                    let state = machine.state();
                    emit_event(
                        ScanEvent::new(
                            id.clone(),
                            state,
                            self.current_progress(),
                            step.message,
                            step.level,
                        )
                        .with_holder_status(step.holder_status),
                    );
                }
            }
        });

        let result = if let Some(error) = transition_error {
            Err(ScanError::ProcessProtocol(format!(
                "Ungültiger Adapter-Zustandsübergang: {error}"
            )))
        } else if cancelled.load(Ordering::SeqCst) {
            Err(ScanError::Cancelled)
        } else {
            result
        };
        let result = result.and_then(|product| {
            if matches!(
                machine.state(),
                ScanState::Finalizing | ScanState::ClosingScanner
            ) {
                Ok(product)
            } else {
                Err(ScanError::IncompleteStream)
            }
        });
        let (terminal, progress, message, level) = match result {
            Ok(product) => match self.archive.finalize(&pending, product) {
                Ok(_) => (
                    ScanState::Completed,
                    100,
                    "Simulierter Scan und Archivierung erfolgreich abgeschlossen".into(),
                    EventLevel::Info,
                ),
                Err(error) => {
                    let _ = self.archive.mark(
                        &pending,
                        ArchiveStatus::Failed,
                        format!("Finalisierung: {error}"),
                    );
                    (
                        ScanState::Failed,
                        self.current_progress(),
                        format!("Archivfinalisierung fehlgeschlagen: {error}"),
                        EventLevel::Error,
                    )
                }
            },
            Err(ScanError::Cancelled) => {
                let _ = machine.transition(ScanState::Cancelling);
                let archive_note = self
                    .archive
                    .mark(
                        &pending,
                        ArchiveStatus::Cancelled,
                        "User requested cancellation",
                    )
                    .err()
                    .map(|error| {
                        format!("; Archivstatus nicht gespeichert: {error}; Recovery erforderlich")
                    })
                    .unwrap_or_default();
                (
                    ScanState::Cancelled,
                    self.current_progress(),
                    format!("Scan kontrolliert abgebrochen{archive_note}"),
                    EventLevel::Warning,
                )
            }
            Err(ScanError::TimedOut) => {
                let archive_note = self
                    .archive
                    .mark(&pending, ArchiveStatus::TimedOut, "Mock process timeout")
                    .err()
                    .map(|error| {
                        format!("; Timeoutstatus nicht gespeichert: {error}; Recovery erforderlich")
                    })
                    .unwrap_or_default();
                (
                    ScanState::TimedOut,
                    self.current_progress(),
                    format!("Zeitüberschreitung des Mock-Prozesses{archive_note}"),
                    EventLevel::Error,
                )
            }
            Err(ScanError::ProcessExit(code)) => {
                let archive_note = self
                    .archive
                    .mark(
                        &pending,
                        ArchiveStatus::Failed,
                        format!("Process exit code {code}"),
                    )
                    .err()
                    .map(|error| {
                        format!("; Fehlerstatus nicht gespeichert: {error}; Recovery erforderlich")
                    })
                    .unwrap_or_default();
                (
                    ScanState::Failed,
                    self.current_progress(),
                    format!("Mock-Prozess endete mit Exitcode {code}{archive_note}"),
                    EventLevel::Error,
                )
            }
            Err(ScanError::IncompleteStream) => {
                let archive_note = self
                    .archive
                    .mark(
                        &pending,
                        ArchiveStatus::Failed,
                        "Missing EOF, byte count, or TIFF confirmation",
                    )
                    .err()
                    .map(|error| {
                        format!("; Fehlerstatus nicht gespeichert: {error}; Recovery erforderlich")
                    })
                    .unwrap_or_default();
                (
                    ScanState::Failed,
                    self.current_progress(),
                    format!("Unvollständiger Datenstrom trotz Exitcode 0{archive_note}"),
                    EventLevel::Error,
                )
            }
            Err(error) => {
                let archive_note = self
                    .archive
                    .mark(&pending, ArchiveStatus::Failed, format!("{error:?}"))
                    .err()
                    .map(|mark_error| {
                        format!(
                            "; Fehlerstatus nicht gespeichert: {mark_error}; Recovery erforderlich"
                        )
                    })
                    .unwrap_or_default();
                (
                    ScanState::Failed,
                    self.current_progress(),
                    format!("Scan fehlgeschlagen: {error:?}{archive_note}"),
                    EventLevel::Error,
                )
            }
        };
        if let Err(error) = machine.transition(terminal) {
            let _ = self.archive.mark(
                &pending,
                ArchiveStatus::Failed,
                format!("Zustandsabschluss fehlgeschlagen: {error}"),
            );
            let terminal = ScanState::Failed;
            let progress = self.current_progress();
            self.finish_job(&id, terminal, progress);
            emit_event(ScanEvent::new(
                id,
                terminal,
                progress,
                format!("Inkonsistenter Jobabschluss wurde als Fehler markiert: {error}"),
                EventLevel::Error,
            ));
            return;
        }
        self.finish_job(&id, terminal, progress);
        emit_event(ScanEvent::new(id, terminal, progress, message, level));
    }

    fn record_and_emit_with_emitter(
        &self,
        id: &str,
        step: AdapterStep,
        emit_event: &mut dyn FnMut(ScanEvent),
    ) {
        let Some(state) = step.state else {
            return;
        };
        {
            let mut data = self.data.lock().expect("core mutex poisoned");
            data.state = state;
            data.progress = step.progress;
            if let Some(holder_status) = step.holder_status {
                data.holder_detected = holder_status == HolderStatus::Detected;
            }
            if let Some(job) = data.jobs.iter_mut().find(|job| job.id == id) {
                job.state = state;
            }
        }
        emit_event(
            ScanEvent::new(
                id.to_string(),
                state,
                step.progress,
                step.message,
                step.level,
            )
            .with_holder_status(step.holder_status),
        );
    }

    fn current_progress(&self) -> u8 {
        self.data.lock().expect("core mutex poisoned").progress
    }

    fn finish_job(&self, id: &str, terminal: ScanState, progress: u8) {
        let mut data = self.data.lock().expect("core mutex poisoned");
        data.state = terminal;
        data.progress = progress;
        if data.active.as_ref().is_some_and(|job| job.id == id) {
            data.active = None;
            data.scanner_lease = None;
        }
        if let Some(job) = data.jobs.iter_mut().find(|job| job.id == id) {
            job.state = terminal;
        }
    }
}

fn ensure_idle(data: &CoreData) -> Result<(), String> {
    if data.active.is_some()
        || data.batch.is_some()
        || data.d6_batch.is_some()
        || data.scanner_lease.is_some()
    {
        Err("Ein Scannerjob ist bereits aktiv".into())
    } else {
        Ok(())
    }
}

fn validate_batch_request(request: &StartBatchRequest) -> Result<(), String> {
    if request.frames.is_empty()
        || request.frames.len() > 4
        || request.frames.iter().any(|f| !(1..=4).contains(f))
    {
        return Err("Batch muss Frames 1–4 enthalten".into());
    }
    let mut seen = std::collections::HashSet::new();
    if request.frames.iter().any(|f| !seen.insert(*f)) {
        return Err("Frames dürfen nicht doppelt vorkommen".into());
    }
    if request
        .failure_frame
        .is_some_and(|f| !request.frames.contains(&f))
    {
        return Err("Fehler-Frame ist nicht Teil des Batches".into());
    }
    Ok(())
}

fn batch_snapshot(batch: &PendingBatch) -> BatchSnapshot {
    BatchSnapshot {
        batch_id: batch.id.clone(),
        frames: batch.request.frames.clone(),
        current_frame: batch.current_frame,
        completed_frames: batch.completed_frames,
        total_frames: batch.request.frames.len() as u8,
        cooldown_remaining_seconds: batch.cooldown_remaining,
        awaiting_holder_confirmation: !batch.confirmed,
    }
}

fn batch_event(data: &CoreData, state: ScanState, message: &str, level: EventLevel) -> ScanEvent {
    let batch = data.batch.as_ref().expect("batch event without a batch");
    ScanEvent::new(batch.id.clone(), state, data.progress, message, level)
        .with_batch(batch_snapshot(batch))
}

impl ScanEvent {
    fn with_batch(mut self, batch: BatchSnapshot) -> Self {
        self.batch = Some(batch);
        self
    }

    fn with_holder_status(mut self, status: Option<HolderStatus>) -> Self {
        self.holder_status = status;
        self
    }
}

fn guided_completion_callback(
    data_ref: Arc<Mutex<CoreData>>,
    app: AppHandle,
    cancelled: Arc<AtomicBool>,
) -> impl FnOnce(Result<(), String>, bool) + Send + 'static {
    move |result, cleanup_confirmed| {
        let succeeded = result.is_ok();
        let state = if succeeded {
            ScanState::Completed
        } else if cancelled.load(Ordering::SeqCst) {
            ScanState::Cancelled
        } else {
            ScanState::Failed
        };
        if let Ok(mut data) = data_ref.lock() {
            finish_guided_slot(&mut data, state, succeeded, cleanup_confirmed);
        }
        emit(
            &app,
            ScanEvent::new(
                "guided-client".into(),
                state,
                if succeeded { 100 } else { 0 },
                if succeeded {
                    "Scan geprüft und exportiert"
                } else if state == ScanState::Cancelled {
                    "Scan abgebrochen"
                } else {
                    "Scan fehlgeschlagen"
                },
                if succeeded {
                    EventLevel::Info
                } else {
                    EventLevel::Warning
                },
            ),
        );
    }
}

fn guided_request_for_d6(request: &D6BatchScanRequest, frame: u8) -> GuidedScanRequest {
    GuidedScanRequest {
        frame,
        dpi: request.dpi,
        device: request.device.clone(),
        confirmed_device: request.confirmed_device.clone(),
        output_directory: request.output_directory.clone(),
    }
}

fn d6_event(batch: &ActiveD6Batch, phase: D6BatchPhase, message: String) -> D6BatchUiEvent {
    D6BatchUiEvent {
        batch_id: batch.id.clone(),
        phase,
        current_frame: batch.current_frame,
        completed_frames: batch.completed_frames,
        total_frames: batch.request.frames.len() as u8,
        cooldown_remaining_seconds: batch.cooldown_remaining,
        awaiting_holder_confirmation: batch.awaiting_holder_confirmation,
        scanner_lease_retained: batch.cleanup_uncertain,
        message,
    }
}

fn wait_d6_cooldown(
    clock: &dyn BatchClock,
    cancelled: &AtomicBool,
    mut on_second: impl FnMut(u8),
) -> bool {
    for remaining in (1..=30).rev() {
        if cancelled.load(Ordering::SeqCst) {
            return false;
        }
        on_second(remaining);
        clock.wait_second(cancelled);
    }
    !cancelled.load(Ordering::SeqCst)
}

fn finish_guided_slot(
    data: &mut CoreData,
    state: ScanState,
    succeeded: bool,
    cleanup_confirmed: bool,
) {
    if data
        .active
        .as_ref()
        .is_some_and(|job| job.id.starts_with("guided-"))
    {
        data.state = state;
        if succeeded {
            data.progress = 100;
        }
        if cleanup_confirmed {
            data.active = None;
            data.scanner_lease = None;
        }
    }
}

impl ScanEvent {
    fn new(
        job_id: String,
        state: ScanState,
        progress: u8,
        message: impl Into<String>,
        level: EventLevel,
    ) -> Self {
        Self {
            job_id,
            state,
            progress,
            message: message.into(),
            level,
            timestamp_ms: now_ms(),
            batch: None,
            holder_status: None,
        }
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn emit(app: &AppHandle, event: ScanEvent) {
    let _ = app.emit("scan-event", event);
}

fn emit_d6<R: tauri::Runtime>(app: &AppHandle<R>, event: D6BatchUiEvent) {
    let _ = app.emit("d6-batch-event", event);
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CountingClock(AtomicU64);
    impl BatchClock for CountingClock {
        fn wait_second(&self, _: &AtomicBool) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct CancelOnTick(AtomicU64);
    impl BatchClock for CancelOnTick {
        fn wait_second(&self, cancelled: &AtomicBool) {
            if self.0.fetch_add(1, Ordering::SeqCst) == 2 {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
    }

    struct CancelDuringCooldown(AtomicU64);
    impl BatchClock for CancelDuringCooldown {
        fn wait_second(&self, cancelled: &AtomicBool) {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                cancelled.store(true, Ordering::SeqCst);
            }
        }
    }

    #[test]
    fn d6_cooldown_is_thirty_core_owned_ticks_and_stops_on_cancel() {
        let clock = CountingClock(AtomicU64::new(0));
        let cancelled = AtomicBool::new(false);
        let mut countdown = Vec::new();
        assert!(wait_d6_cooldown(&clock, &cancelled, |second| countdown.push(second)));
        assert_eq!(clock.0.load(Ordering::SeqCst), 30);
        assert_eq!(countdown, (1..=30).rev().collect::<Vec<_>>());

        let clock = CancelOnTick(AtomicU64::new(0));
        let cancelled = AtomicBool::new(false);
        let mut countdown = Vec::new();
        assert!(!wait_d6_cooldown(&clock, &cancelled, |second| countdown.push(second)));
        assert_eq!(countdown, vec![30, 29, 28]);
        assert!(cancelled.load(Ordering::SeqCst));
    }

    #[test]
    fn d6_simulator_batches_of_two_four_and_six_use_d5_runner_and_complete_journals() {
        for count in [2_u8, 4, 6] {
            let root = std::env::temp_dir().join(format!(
                "beblog-d6-core-sim-{count}-{}",
                uuid::Uuid::new_v4()
            ));
            let output = root.join("output");
            std::fs::create_dir_all(&output).unwrap();
            let clock = Arc::new(CountingClock(AtomicU64::new(0)));
            let core = AppCore::new_with_clock(root.join("archive"), clock.clone());
            let app = tauri::test::mock_app();
            let handle = app.handle().clone();
            let frames = (1..=count).collect::<Vec<_>>();
            let batch_id = core
                .start_d6_batch_simulation(
                    handle.clone(),
                    D6BatchScanRequest {
                        frames: frames.clone(),
                        dpi: 600,
                        device: "simulator:hardware-free".into(),
                        confirmed_device: "simulator:hardware-free".into(),
                        output_directory: output.clone(),
                    },
                )
                .unwrap();

            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut confirmed = false;
            loop {
                let should_confirm = core
                    .data
                    .lock()
                    .unwrap()
                    .d6_batch
                    .as_ref()
                    .is_some_and(|batch| batch.awaiting_holder_confirmation);
                if should_confirm && !confirmed {
                    core.confirm_d6_batch_holder(&handle).unwrap();
                    confirmed = true;
                }
                if core.data.lock().unwrap().d6_batch.as_ref().is_none() {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "D6 simulator batch timed out"
                );
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(confirmed, "first frame must require explicit confirmation");
            assert_eq!(core.snapshot().state, ScanState::BatchCompleted);
            assert_eq!(clock.0.load(Ordering::SeqCst), u64::from((count - 1) * 30));
            let journal_path = root
                .join("archive/batches/d6")
                .join(format!("{batch_id}.json"));
            let journal: crate::batch_plan::D6BatchManifest =
                serde_json::from_slice(&std::fs::read(journal_path).unwrap()).unwrap();
            assert_eq!(journal.status, crate::batch_plan::D6BatchStatus::Completed);
            assert_eq!(journal.frames.len(), usize::from(count));
            assert!(journal.frames.iter().all(|entry| {
                entry.status == crate::batch_plan::D6FrameStatus::Completed
                    && entry
                        .validated_artifacts
                        .as_ref()
                        .is_some_and(|files| files.len() == 4)
            }));
            let capture_ids = journal
                .frames
                .iter()
                .map(|entry| entry.capture_id.as_ref().unwrap())
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(capture_ids.len(), usize::from(count));
            let exported = std::fs::read_dir(&output).unwrap().count();
            assert_eq!(exported, usize::from(count));
            assert!(core.data.lock().unwrap().scanner_lease.is_none());
            drop(app);
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn d6_cancel_during_cooldown_keeps_completed_capture_and_does_not_start_next_frame() {
        let root = std::env::temp_dir().join(format!(
            "beblog-d6-cancel-cooldown-{}",
            uuid::Uuid::new_v4()
        ));
        let output = root.join("output");
        std::fs::create_dir_all(&output).unwrap();
        let clock = Arc::new(CancelDuringCooldown(AtomicU64::new(0)));
        let core = AppCore::new_with_clock(root.join("archive"), clock.clone());
        let app = tauri::test::mock_app();
        let handle = app.handle().clone();
        let batch_id = core
            .start_d6_batch_simulation(
                handle.clone(),
                D6BatchScanRequest {
                    frames: vec![1, 2],
                    dpi: 600,
                    device: "simulator:hardware-free".into(),
                    confirmed_device: "simulator:hardware-free".into(),
                    output_directory: output.clone(),
                },
            )
            .unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut confirmed = false;
        loop {
            let should_confirm = core
                .data
                .lock()
                .unwrap()
                .d6_batch
                .as_ref()
                .is_some_and(|batch| batch.awaiting_holder_confirmation);
            if should_confirm && !confirmed {
                core.confirm_d6_batch_holder(&handle).unwrap();
                confirmed = true;
            }
            if core.data.lock().unwrap().d6_batch.is_none() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "D6 cancellation timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(confirmed);
        assert_eq!(core.snapshot().state, ScanState::BatchCancelled);
        assert_eq!(clock.0.load(Ordering::SeqCst), 1);
        let journal_path = root
            .join("archive/batches/d6")
            .join(format!("{batch_id}.json"));
        let journal: crate::batch_plan::D6BatchManifest =
            serde_json::from_slice(&std::fs::read(journal_path).unwrap()).unwrap();
        assert_eq!(journal.status, crate::batch_plan::D6BatchStatus::Cancelled);
        assert_eq!(
            journal.frames[0].status,
            crate::batch_plan::D6FrameStatus::Completed
        );
        assert_eq!(
            journal.frames[1].status,
            crate::batch_plan::D6FrameStatus::Planned
        );
        assert_eq!(std::fs::read_dir(&output).unwrap().count(), 1);
        assert!(core.data.lock().unwrap().scanner_lease.is_none());
        drop(app);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn initial_core_is_fake_and_idle() {
        let core =
            AppCore::new(std::env::temp_dir().join(format!("beblog-core-test-{}", now_ms())));
        let snapshot = core.snapshot();
        assert_eq!(snapshot.adapter, "fake");
        assert_eq!(snapshot.scanner_status, "ready");
        assert_eq!(snapshot.state, ScanState::Idle);
        assert!(core.archive.root().is_absolute());
    }

    #[test]
    fn active_job_excludes_parallel_scanner_job() {
        let data = CoreData {
            active: Some(ActiveJob {
                id: "first".into(),
                cancelled: Arc::new(AtomicBool::new(false)),
            }),
            ..CoreData::default()
        };
        assert_eq!(
            ensure_idle(&data),
            Err("Ein Scannerjob ist bereits aktiv".into())
        );
    }

    #[test]
    fn guided_reservation_is_held_through_opened_wait_and_uncertain_cleanup() {
        let manager = ScannerLeaseManager::default();
        let mut data = CoreData {
            active: Some(ActiveJob {
                id: "guided-opened-wait".into(),
                cancelled: Arc::new(AtomicBool::new(false)),
            }),
            scanner_lease: Some(manager.acquire(ScannerAction::D5SingleFrame).unwrap()),
            state: ScanState::AwaitingHolderConfirmation,
            ..CoreData::default()
        };
        assert!(ensure_idle(&data).is_err());
        assert!(manager.acquire(ScannerAction::Discovery).is_err());

        finish_guided_slot(&mut data, ScanState::Failed, false, false);
        assert!(ensure_idle(&data).is_err());
        assert!(manager.acquire(ScannerAction::D6Batch).is_err());

        finish_guided_slot(&mut data, ScanState::Failed, false, true);
        assert!(ensure_idle(&data).is_ok());
        let lease = manager.acquire(ScannerAction::Discovery).unwrap();
        drop(lease);
    }

    #[test]
    fn guided_reservation_releases_only_after_successful_cleanup() {
        let manager = ScannerLeaseManager::default();
        let mut data = CoreData {
            active: Some(ActiveJob {
                id: "guided-success".into(),
                cancelled: Arc::new(AtomicBool::new(false)),
            }),
            scanner_lease: Some(manager.acquire(ScannerAction::D5SingleFrame).unwrap()),
            ..CoreData::default()
        };
        finish_guided_slot(&mut data, ScanState::Completed, true, true);
        assert_eq!(data.state, ScanState::Completed);
        assert_eq!(data.progress, 100);
        assert!(ensure_idle(&data).is_ok());
    }

    #[test]
    fn awaiting_confirmation_reserves_exclusive_batch_slot_without_scanner_busy() {
        let data = CoreData {
            batch: Some(PendingBatch {
                id: "batch-test".into(),
                request: StartBatchRequest {
                    frames: vec![1, 2, 3, 4],
                    simulation: SimulationMode::Success,
                    failure_frame: None,
                },
                cancelled: Arc::new(AtomicBool::new(false)),
                confirmed: false,
                completed_frames: 0,
                current_frame: None,
                cooldown_remaining: 0,
            }),
            ..CoreData::default()
        };
        assert!(ensure_idle(&data).is_err());
        assert!(!data.batch.as_ref().unwrap().confirmed);
    }

    #[test]
    fn batch_plan_is_ordered_unique_and_fault_targets_must_belong() {
        let request = StartBatchRequest {
            frames: vec![1, 2, 3, 4],
            simulation: SimulationMode::Success,
            failure_frame: None,
        };
        assert!(validate_batch_request(&request).is_ok());
        let bad = StartBatchRequest {
            frames: vec![1, 2, 2],
            ..request.clone()
        };
        assert!(validate_batch_request(&bad).is_err());
        let bad_target = StartBatchRequest {
            frames: vec![1, 2],
            failure_frame: Some(4),
            ..request
        };
        assert!(validate_batch_request(&bad_target).is_err());
    }

    #[test]
    fn batch_ids_are_unique_across_core_instances_and_preserve_existing_archives() {
        let root = std::env::temp_dir().join(format!("beblog-batch-id-restart-{}", now_ms()));
        let archive = ArchiveManager::new(root.clone());

        archive.begin_batch("batch-000001", &[1, 2]).unwrap();
        archive
            .update_batch(
                "batch-000001",
                Some(1),
                Some(BatchArchiveStatus::Completed),
                None,
            )
            .unwrap();
        archive.begin_batch("batch-000002", &[1, 2]).unwrap();
        archive
            .update_batch(
                "batch-000002",
                None,
                Some(BatchArchiveStatus::Failed),
                Some("historical failure".into()),
            )
            .unwrap();
        archive.begin_batch("batch-000003", &[1, 2]).unwrap();
        assert_eq!(
            archive.recover_incomplete().unwrap(),
            vec!["batch-000003".to_string()]
        );

        let capture = archive
            .begin(
                &StartScanRequest {
                    frame: 1,
                    simulation: SimulationMode::Success,
                },
                crate::archive::CaptureKind::Simulation,
            )
            .unwrap();
        archive
            .finalize(
                &capture,
                crate::archive::CaptureProduct {
                    raw: b"BEBLOG-MOCK-RAW\nexisting capture\n".to_vec(),
                    tiff: b"BEBLOG-MOCK-TIFF\nexisting capture\n".to_vec(),
                    expected_raw_bytes: 1_438_200,
                    kind: crate::archive::CaptureKind::Simulation,
                },
            )
            .unwrap();

        let preserved_paths = [
            root.join("batches/batch-000001.json"),
            root.join("batches/batch-000002.json"),
            root.join("batches/batch-000003.json"),
            capture.directory.join("manifest.json"),
            capture.directory.join("capture.raw"),
            capture.directory.join("capture.tiff"),
        ];
        let preserved = preserved_paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect::<Vec<_>>();

        // Separate cores model independent application lifetimes sharing the same archive.
        let first_lifetime = AppCore::new(root.clone());
        let second_lifetime = AppCore::new(root.clone());
        let first_id = first_lifetime
            .create_unique_batch_journal(&[1, 2, 3, 4])
            .unwrap();
        let second_id = second_lifetime
            .create_unique_batch_journal(&[1, 2, 3, 4])
            .unwrap();

        assert_ne!(first_id, second_id);
        for (path, expected) in preserved_paths.iter().zip(preserved) {
            assert_eq!(std::fs::read(path).unwrap(), expected, "{}", path.display());
        }
        assert_eq!(
            serde_json::from_slice::<crate::archive::BatchManifest>(
                &std::fs::read(root.join(format!("batches/{first_id}.json"))).unwrap()
            )
            .unwrap()
            .status,
            BatchArchiveStatus::InProgress
        );
        assert_eq!(
            serde_json::from_slice::<crate::archive::BatchManifest>(
                &std::fs::read(root.join(format!("batches/{second_id}.json"))).unwrap()
            )
            .unwrap()
            .status,
            BatchArchiveStatus::InProgress
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn single_scan_transition_error_cannot_be_overwritten_by_later_progress() {
        let root = std::env::temp_dir().join(format!("beblog-single-transition-{}", now_ms()));
        let core = AppCore::new_with_recorded_fault(root.clone(), RecordedFault::StateRegression);
        let id = "single-transition-regression".to_string();
        let request = StartScanRequest {
            frame: 1,
            simulation: SimulationMode::Success,
        };
        let pending = core
            .archive
            .begin(&request, core.adapter.capture_kind())
            .unwrap();
        let capture_directory = pending.directory.clone();
        {
            let mut data = core.data.lock().unwrap();
            data.active = Some(ActiveJob {
                id: id.clone(),
                cancelled: Arc::new(AtomicBool::new(false)),
            });
            data.state = ScanState::Initializing;
        }

        let mut events = Vec::new();
        core.run_job_with_emitter(
            id,
            request,
            Arc::new(AtomicBool::new(false)),
            pending,
            &mut |event| events.push(event),
        );

        assert_eq!(core.snapshot().state, ScanState::Failed);
        assert_eq!(events.last().unwrap().state, ScanState::Failed);
        assert!(events.iter().any(|event| event
            .message
            .contains("Ungültiger Adapter-Zustandsübergang")));
        assert!(!events
            .iter()
            .any(|event| event.state == ScanState::ClosingScanner));
        let manifest: crate::archive::CaptureManifest = serde_json::from_slice(
            &std::fs::read(capture_directory.join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.status, ArchiveStatus::Failed);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cooldown_clock_is_injectable_and_counts_seconds() {
        let clock = CountingClock(AtomicU64::new(0));
        for _ in 0..30 {
            clock.wait_second(&AtomicBool::new(false));
        }
        assert_eq!(clock.0.load(Ordering::SeqCst), 30);
    }

    #[test]
    fn full_four_frame_fake_batch_completes_with_three_cooldowns_and_archive() {
        let root = std::env::temp_dir().join(format!("beblog-four-frame-batch-{}", now_ms()));
        let clock = Arc::new(CountingClock(AtomicU64::new(0)));
        let core = AppCore::new_with_clock(root.clone(), clock.clone());
        let batch_id = "batch-four-frame-regression".to_string();
        let request = StartBatchRequest {
            frames: vec![1, 2, 3, 4],
            simulation: SimulationMode::Success,
            failure_frame: None,
        };
        core.archive
            .begin_batch(&batch_id, &request.frames)
            .unwrap();
        {
            let mut data = core.data.lock().unwrap();
            data.batch = Some(PendingBatch {
                id: batch_id.clone(),
                request: request.clone(),
                cancelled: Arc::new(AtomicBool::new(false)),
                confirmed: true,
                completed_frames: 0,
                current_frame: Some(1),
                cooldown_remaining: 0,
            });
            data.state = ScanState::WaitingForHolderDetection;
        }

        let mut events = Vec::new();
        let mut process_start_seconds = Vec::new();
        core.run_batch_with_emitter(
            batch_id.clone(),
            request,
            Arc::new(AtomicBool::new(false)),
            &mut |event| {
                if event.message.contains("Mock-Prozess gestartet") {
                    process_start_seconds.push(clock.0.load(Ordering::SeqCst));
                }
                events.push(event);
            },
        );

        assert_eq!(core.snapshot().state, ScanState::BatchCompleted);
        assert_eq!(
            clock.0.load(Ordering::SeqCst),
            90,
            "exactly three 30s cooldowns"
        );
        assert_eq!(process_start_seconds, vec![0, 30, 60, 90]);
        assert_eq!(
            events
                .iter()
                .filter(|e| e.state == ScanState::InterFrameCooldown)
                .count(),
            90
        );
        assert_eq!(events.last().unwrap().state, ScanState::BatchCompleted);

        let batch: crate::archive::BatchManifest = serde_json::from_slice(
            &std::fs::read(root.join("batches/batch-four-frame-regression.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(batch.status, BatchArchiveStatus::Completed);
        assert_eq!(batch.completed_frames, vec![1, 2, 3, 4]);

        let mut captures = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| {
                let manifest = entry.path().join("manifest.json");
                std::fs::read(manifest).ok().and_then(|bytes| {
                    serde_json::from_slice::<crate::archive::CaptureManifest>(&bytes).ok()
                })
            })
            .collect::<Vec<_>>();
        captures.sort_by_key(|capture| capture.frame);
        assert_eq!(captures.len(), 4);
        assert_eq!(
            captures
                .iter()
                .map(|capture| capture.frame)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert!(captures
            .iter()
            .all(|capture| capture.status == ArchiveStatus::Completed));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_first_process_does_not_retry_or_complete_the_batch() {
        let root = std::env::temp_dir().join(format!("beblog-batch-start-error-{}", now_ms()));
        let core = AppCore::new_with_recorded_fault(root.clone(), RecordedFault::StartFailure);
        let id = "batch-no-retry-on-start-error".to_string();
        let request = StartBatchRequest {
            frames: vec![1, 2, 3, 4],
            simulation: SimulationMode::Success,
            failure_frame: None,
        };
        core.archive.begin_batch(&id, &request.frames).unwrap();
        {
            let mut data = core.data.lock().unwrap();
            data.batch = Some(PendingBatch {
                id: id.clone(),
                request: request.clone(),
                cancelled: Arc::new(AtomicBool::new(false)),
                confirmed: true,
                completed_frames: 0,
                current_frame: Some(1),
                cooldown_remaining: 0,
            });
            data.state = ScanState::WaitingForHolderDetection;
        }

        let mut events = Vec::new();
        core.run_batch_with_emitter(
            id.clone(),
            request,
            Arc::new(AtomicBool::new(false)),
            &mut |event| events.push(event),
        );

        assert_eq!(core.snapshot().state, ScanState::BatchFailed);
        assert_eq!(events.last().unwrap().state, ScanState::BatchFailed);
        assert!(!events
            .iter()
            .any(|event| event.state == ScanState::BatchCompleted));
        let last_frame_event = events
            .iter()
            .rposition(|event| event.message.contains("Prozessstart fehlgeschlagen"))
            .unwrap();
        assert_eq!(last_frame_event + 1, events.len() - 1);
        let journal: crate::archive::BatchManifest = serde_json::from_slice(
            &std::fs::read(root.join(format!("batches/{id}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(journal.status, BatchArchiveStatus::Failed);
        assert!(journal.completed_frames.is_empty());
        let captures = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter(|entry| entry.path().join("manifest.json").is_file())
            .count();
        assert_eq!(
            captures, 1,
            "failed frame is not retried and later frames do not start"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn completed_batch_is_not_reported_when_terminal_journal_write_fails() {
        let root = std::env::temp_dir().join(format!("beblog-journal-fault-{}", now_ms()));
        let core = AppCore::new(root.clone());
        core.archive
            .begin_batch("batch-journal", &[1, 2, 3, 4])
            .unwrap();
        std::fs::write(root.join("batches/batch-journal.json"), b"corrupt journal").unwrap();
        let (state, message) = core.persist_batch_terminal(
            "batch-journal",
            ScanState::BatchCompleted,
            "simulated completion".into(),
        );
        assert_eq!(state, ScanState::BatchFailed);
        assert!(message.contains("Batchjournal konnte nicht als Completed gespeichert werden"));
        assert!(message.contains("Journal bleibt für Recovery offen"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn terminal_journal_success_is_required_for_batch_completion() {
        let root = std::env::temp_dir().join(format!("beblog-journal-ok-{}", now_ms()));
        let core = AppCore::new(root.clone());
        core.archive
            .begin_batch("batch-journal", &[1, 2, 3, 4])
            .unwrap();
        let (state, _) = core.persist_batch_terminal(
            "batch-journal",
            ScanState::BatchCompleted,
            "simulated completion".into(),
        );
        assert_eq!(state, ScanState::BatchCompleted);
        let manifest: crate::archive::BatchManifest = serde_json::from_slice(
            &std::fs::read(root.join("batches/batch-journal.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.status, BatchArchiveStatus::Completed);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_and_cancelled_batches_surface_journal_write_errors() {
        for terminal in [ScanState::BatchFailed, ScanState::BatchCancelled] {
            let root = std::env::temp_dir().join(format!(
                "beblog-journal-state-{}-{}",
                now_ms(),
                terminal as u8
            ));
            let core = AppCore::new(root.clone());
            core.archive.begin_batch("batch-journal", &[1, 2]).unwrap();
            std::fs::write(root.join("batches/batch-journal.json"), b"corrupt journal").unwrap();
            let (state, message) = core.persist_batch_terminal(
                "batch-journal",
                terminal,
                "requested terminal outcome".into(),
            );
            assert_eq!(state, ScanState::BatchFailed);
            assert!(message.contains("Batchjournal konnte nicht als"));
            assert!(message.contains("Journal bleibt für Recovery offen"));
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}
