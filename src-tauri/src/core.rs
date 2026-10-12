use crate::{
    adapter::{AdapterStep, FakeScannerAdapter, ScannerAdapter},
    archive::{ArchiveManager, ArchiveStatus, BatchArchiveStatus, PendingCapture},
    model::{
        AppSnapshot, BatchSnapshot, EventLevel, HolderStatus, ScanError, ScanEvent, ScanState,
        SimulationMode, StartBatchRequest, StartScanRequest,
    },
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

struct CoreData {
    active: Option<ActiveJob>,
    state: ScanState,
    progress: u8,
    holder_detected: bool,
    jobs: Vec<JobSummary>,
    batch: Option<PendingBatch>,
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
        Self {
            data: Arc::new(Mutex::new(CoreData::default())),
            adapter: Arc::new(FakeScannerAdapter::default()),
            archive: ArchiveManager::new(archive_root),
            clock,
        }
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
        if !(1..=4).contains(&request.frame) {
            return Err("Frame muss zwischen 1 und 4 liegen".into());
        }
        let id = format!("scan-{:06}", NEXT_JOB.fetch_add(1, Ordering::Relaxed));
        let cancelled = Arc::new(AtomicBool::new(false));
        let pending = {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            ensure_idle(&data)?;
            let pending = self
                .archive
                .begin(&request, self.adapter.capture_kind())
                .map_err(|error| format!("Capture-Archiv konnte nicht angelegt werden: {error}"))?;
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
        let id = format!("batch-{:06}", NEXT_JOB.fetch_add(1, Ordering::Relaxed));
        let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
        ensure_idle(&data)?;
        self.archive
            .begin_batch(&id, &request.frames)
            .map_err(|e| format!("Batchjournal konnte nicht angelegt werden: {e}"))?;
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
        let mut machine = ScanStateMachine::default();
        let _ = machine.transition(ScanState::AwaitingHolderConfirmation);
        let _ = machine.transition(ScanState::WaitingForHolderDetection);
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
                    self.end_batch(
                        &app,
                        &id,
                        ScanState::BatchFailed,
                        format!("Archivstart fehlgeschlagen: {e}"),
                    );
                    return;
                }
            };
            let transition_error = Arc::new(Mutex::new(None::<String>));
            let error_slot = transition_error.clone();
            let result = self
                .adapter
                .scan(&scan_request, cancelled.clone(), &mut |step| {
                    if cancelled.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Err(error) = machine.transition(step.state) {
                        *error_slot.lock().expect("transition error mutex poisoned") =
                            Some(error.clone());
                        let current = machine.state();
                        emit(
                            &app,
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
                    let mut data = self.data.lock().expect("core mutex poisoned");
                    data.state = step.state;
                    data.progress = step.progress;
                    if let Some(holder_status) = step.holder_status {
                        data.holder_detected = holder_status == HolderStatus::Detected;
                    }
                    let event = ScanEvent::new(
                        id.clone(),
                        step.state,
                        step.progress,
                        format!("Frame {frame}: {}", step.message),
                        step.level,
                    )
                    .with_batch(batch_snapshot(data.batch.as_ref().unwrap()))
                    .with_holder_status(step.holder_status);
                    drop(data);
                    emit(&app, event);
                });
            if cancelled.load(Ordering::SeqCst) {
                let _ = self
                    .archive
                    .mark(&pending, ArchiveStatus::Cancelled, "Batch cancelled");
                break;
            }
            if let Some(error) = transition_error
                .lock()
                .expect("transition error mutex poisoned")
                .take()
            {
                let _ = self.archive.mark(
                    &pending,
                    ArchiveStatus::Failed,
                    format!("Invalid adapter transition: {error}"),
                );
                self.end_batch(
                    &app,
                    &id,
                    ScanState::BatchFailed,
                    format!("Frame {frame}: ungültiger Zustandsübergang: {error}"),
                );
                return;
            }
            let product = match result {
                Ok(p) if machine.state() == ScanState::ClosingScanner => p,
                Ok(_) => {
                    let _ = self.archive.mark(
                        &pending,
                        ArchiveStatus::Failed,
                        "Incomplete mock lifecycle",
                    );
                    self.end_batch(
                        &app,
                        &id,
                        ScanState::BatchFailed,
                        format!("Frame {frame}: unvollständiger Lifecycle"),
                    );
                    return;
                }
                Err(e) => {
                    let status = if e == ScanError::TimedOut {
                        ArchiveStatus::TimedOut
                    } else {
                        ArchiveStatus::Failed
                    };
                    let _ = self.archive.mark(&pending, status, format!("{e:?}"));
                    self.end_batch(
                        &app,
                        &id,
                        ScanState::BatchFailed,
                        format!("Frame {frame} fehlgeschlagen: {e:?}"),
                    );
                    return;
                }
            };
            if let Err(e) = self.archive.finalize(&pending, product) {
                self.end_batch(
                    &app,
                    &id,
                    ScanState::BatchFailed,
                    format!("Frame {frame}: Ausgabe ungültig: {e}"),
                );
                return;
            }
            if let Err(e) = self.archive.update_batch(&id, Some(frame), None, None) {
                self.end_batch(
                    &app,
                    &id,
                    ScanState::BatchFailed,
                    format!("Batchjournal konnte nicht aktualisiert werden: {e}"),
                );
                return;
            }
            if index + 1 < request.frames.len() {
                if machine.transition(ScanState::InterFrameCooldown).is_err() {
                    self.end_batch(
                        &app,
                        &id,
                        ScanState::BatchFailed,
                        "Ungültiger Cooldown-Übergang".into(),
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
                    emit(
                        &app,
                        self.batch_event(
                            ScanState::InterFrameCooldown,
                            &format!("Bereitschaftspause: {remaining} Sekunden"),
                            EventLevel::Info,
                        ),
                    );
                    self.clock.wait_second(&cancelled);
                }
                if cancelled.load(Ordering::SeqCst) {
                    break;
                }
                let _ = machine.transition(ScanState::Initializing);
            } else {
                if machine.transition(ScanState::BatchCompleted).is_err() {
                    self.end_batch(
                        &app,
                        &id,
                        ScanState::BatchFailed,
                        "Batchabschluss-Zustand ungültig".into(),
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
            self.end_batch(
                &app,
                &id,
                ScanState::BatchCancelled,
                "Batch abgebrochen; archivierte Frames bleiben erhalten".into(),
            );
        } else {
            self.end_batch(
                &app,
                &id,
                ScanState::BatchCompleted,
                "Batch erfolgreich abgeschlossen".into(),
            );
        }
    }

    fn end_batch(&self, app: &AppHandle, id: &str, state: ScanState, message: String) {
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
            data.batch = None;
            event
        };
        if let Some(event) = event {
            emit(app, event);
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
        let mut machine = ScanStateMachine::default();
        let result = self.adapter.scan(&request, cancelled.clone(), &mut |step| {
            if cancelled.load(Ordering::SeqCst) {
                emit(
                    &app,
                    ScanEvent::new(
                        id.clone(),
                        ScanState::Cancelling,
                        self.current_progress(),
                        "Adapter-Ereignis nach Abbruchanforderung ignoriert",
                        EventLevel::Warning,
                    ),
                );
                return;
            }
            match machine.transition(step.state) {
                Ok(()) => self.record_and_emit(&app, &id, step),
                Err(error) => {
                    let current = self.snapshot().state;
                    emit(
                        &app,
                        ScanEvent::new(
                            id.clone(),
                            current,
                            self.current_progress(),
                            format!("Ungültiger Adapter-Zustandsübergang verworfen: {error}"),
                            EventLevel::Error,
                        ),
                    );
                }
            }
        });

        let result = if cancelled.load(Ordering::SeqCst) {
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
                let _ = self.archive.mark(
                    &pending,
                    ArchiveStatus::Cancelled,
                    "User requested cancellation",
                );
                (
                    ScanState::Cancelled,
                    self.current_progress(),
                    "Scan kontrolliert abgebrochen".into(),
                    EventLevel::Warning,
                )
            }
            Err(ScanError::TimedOut) => {
                let _ =
                    self.archive
                        .mark(&pending, ArchiveStatus::TimedOut, "Mock process timeout");
                (
                    ScanState::TimedOut,
                    self.current_progress(),
                    "Zeitüberschreitung des Mock-Prozesses".into(),
                    EventLevel::Error,
                )
            }
            Err(ScanError::ProcessExit(code)) => {
                let _ = self.archive.mark(
                    &pending,
                    ArchiveStatus::Failed,
                    format!("Process exit code {code}"),
                );
                (
                    ScanState::Failed,
                    self.current_progress(),
                    format!("Mock-Prozess endete mit Exitcode {code}"),
                    EventLevel::Error,
                )
            }
            Err(ScanError::IncompleteStream) => {
                let _ = self.archive.mark(
                    &pending,
                    ArchiveStatus::Failed,
                    "Missing EOF, byte count, or TIFF confirmation",
                );
                (
                    ScanState::Failed,
                    self.current_progress(),
                    "Unvollständiger Datenstrom trotz Exitcode 0".into(),
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
            emit(
                &app,
                ScanEvent::new(
                    id,
                    terminal,
                    progress,
                    format!("Inkonsistenter Jobabschluss wurde als Fehler markiert: {error}"),
                    EventLevel::Error,
                ),
            );
            return;
        }
        self.finish_job(&id, terminal, progress);
        emit(&app, ScanEvent::new(id, terminal, progress, message, level));
    }

    fn record_and_emit(&self, app: &AppHandle, id: &str, step: AdapterStep) {
        {
            let mut data = self.data.lock().expect("core mutex poisoned");
            data.state = step.state;
            data.progress = step.progress;
            if let Some(holder_status) = step.holder_status {
                data.holder_detected = holder_status == HolderStatus::Detected;
            }
            if let Some(job) = data.jobs.iter_mut().find(|job| job.id == id) {
                job.state = step.state;
            }
        }
        emit(
            app,
            ScanEvent::new(
                id.to_string(),
                step.state,
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
        data.active = None;
        if let Some(job) = data.jobs.iter_mut().find(|job| job.id == id) {
            job.state = terminal;
        }
    }
}

fn ensure_idle(data: &CoreData) -> Result<(), String> {
    if data.active.is_some() || data.batch.is_some() {
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

#[cfg(test)]
mod tests {
    use super::*;

    struct CountingClock(AtomicU64);
    impl BatchClock for CountingClock {
        fn wait_second(&self, _: &AtomicBool) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
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
    fn cooldown_clock_is_injectable_and_counts_seconds() {
        let clock = CountingClock(AtomicU64::new(0));
        for _ in 0..30 {
            clock.wait_second(&AtomicBool::new(false));
        }
        assert_eq!(clock.0.load(Ordering::SeqCst), 30);
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
