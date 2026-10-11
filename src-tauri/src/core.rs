use std::{
    sync::{atomic::{AtomicBool, AtomicU64, Ordering}, Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use crate::{
    adapter::{AdapterStep, FakeScannerAdapter, ScannerAdapter},
    model::{AppSnapshot, EventLevel, ScanError, ScanEvent, ScanState, StartScanRequest},
    state_machine::ScanStateMachine,
};

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobSummary {
    pub id: String,
    pub frame: u8,
    pub state: ScanState,
    pub created_at_ms: u128,
}

struct ActiveJob { id: String, cancelled: Arc<AtomicBool> }

struct CoreData {
    active: Option<ActiveJob>,
    state: ScanState,
    progress: u8,
    holder_detected: bool,
    jobs: Vec<JobSummary>,
}

impl Default for CoreData {
    fn default() -> Self {
        Self { active: None, state: ScanState::Idle, progress: 0, holder_detected: false, jobs: Vec::new() }
    }
}

#[derive(Clone)]
pub struct AppCore {
    data: Arc<Mutex<CoreData>>,
    adapter: Arc<dyn ScannerAdapter>,
}

impl Default for AppCore {
    fn default() -> Self {
        Self { data: Arc::new(Mutex::new(CoreData::default())), adapter: Arc::new(FakeScannerAdapter) }
    }
}

impl AppCore {
    pub fn snapshot(&self) -> AppSnapshot {
        let data = self.data.lock().expect("core mutex poisoned");
        AppSnapshot {
            adapter: self.adapter.name(),
            scanner_status: if data.active.is_some() { "busy" } else { "ready" },
            holder_status: if data.holder_detected { "detected" } else { "unknown" },
            active_job_id: data.active.as_ref().map(|job| job.id.clone()),
            state: data.state,
            progress: data.progress,
        }
    }

    pub fn jobs(&self) -> Vec<JobSummary> {
        self.data.lock().expect("core mutex poisoned").jobs.clone()
    }

    pub fn start(&self, app: AppHandle, request: StartScanRequest) -> Result<String, String> {
        if !(1..=4).contains(&request.frame) { return Err("Frame muss zwischen 1 und 4 liegen".into()); }
        let id = format!("scan-{:06}", NEXT_JOB.fetch_add(1, Ordering::Relaxed));
        let cancelled = Arc::new(AtomicBool::new(false));
        {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            if data.active.is_some() { return Err("Ein Scannerjob ist bereits aktiv".into()); }
            data.active = Some(ActiveJob { id: id.clone(), cancelled: cancelled.clone() });
            data.state = ScanState::Initializing;
            data.progress = 0;
            data.holder_detected = false;
            data.jobs.push(JobSummary { id: id.clone(), frame: request.frame, state: ScanState::Initializing, created_at_ms: now_ms() });
        }

        let core = self.clone();
        let job_id = id.clone();
        std::thread::spawn(move || core.run_job(app, job_id, request, cancelled));
        Ok(id)
    }

    pub fn cancel(&self, app: &AppHandle) -> Result<(), String> {
        let (id, progress) = {
            let mut data = self.data.lock().map_err(|_| "Interner Zustandsfehler")?;
            let (id, cancelled) = data.active.as_ref()
                .map(|job| (job.id.clone(), job.cancelled.clone()))
                .ok_or_else(|| "Kein aktiver Scannerjob".to_string())?;
            cancelled.store(true, Ordering::SeqCst);
            data.state = ScanState::Cancelling;
            (id, data.progress)
        };
        emit(app, ScanEvent::new(id, ScanState::Cancelling, progress, "Kontrollierter Abbruch angefordert", EventLevel::Warning));
        Ok(())
    }

    fn run_job(&self, app: AppHandle, id: String, request: StartScanRequest, cancelled: Arc<AtomicBool>) {
        let mut machine = ScanStateMachine::default();
        let result = self.adapter.scan(&request, cancelled.clone(), &mut |step| {
            if machine.transition(step.state).is_ok() {
                self.record_and_emit(&app, &id, step);
            }
        });

        let result = if cancelled.load(Ordering::SeqCst) { Err(ScanError::Cancelled) } else { result };
        let (terminal, progress, message, level) = match result {
            Ok(()) => (ScanState::Completed, 100, "Simulierter Scan erfolgreich abgeschlossen", EventLevel::Info),
            Err(ScanError::Cancelled) => {
                let _ = machine.transition(ScanState::Cancelling);
                (ScanState::Cancelled, self.current_progress(), "Scan kontrolliert abgebrochen", EventLevel::Warning)
            }
            Err(ScanError::TimedOut) => (ScanState::TimedOut, self.current_progress(), "Zeitüberschreitung bei der Filmhaltererkennung", EventLevel::Error),
            Err(ScanError::SimulatedIo) => (ScanState::Failed, self.current_progress(), "Simulierter I/O-Fehler während sane_read", EventLevel::Error),
        };
        let _ = machine.transition(terminal);
        self.finish_job(&id, terminal, progress);
        emit(&app, ScanEvent::new(id, terminal, progress, message, level));
    }

    fn record_and_emit(&self, app: &AppHandle, id: &str, step: AdapterStep) {
        {
            let mut data = self.data.lock().expect("core mutex poisoned");
            data.state = step.state;
            data.progress = step.progress;
            if step.state == ScanState::ConfiguringFrame { data.holder_detected = true; }
            if let Some(job) = data.jobs.iter_mut().find(|job| job.id == id) { job.state = step.state; }
        }
        emit(app, ScanEvent::new(id.to_string(), step.state, step.progress, step.message, step.level));
    }

    fn current_progress(&self) -> u8 { self.data.lock().expect("core mutex poisoned").progress }

    fn finish_job(&self, id: &str, terminal: ScanState, progress: u8) {
        let mut data = self.data.lock().expect("core mutex poisoned");
        data.state = terminal;
        data.progress = progress;
        data.active = None;
        if let Some(job) = data.jobs.iter_mut().find(|job| job.id == id) { job.state = terminal; }
    }
}

impl ScanEvent {
    fn new(job_id: String, state: ScanState, progress: u8, message: impl Into<String>, level: EventLevel) -> Self {
        Self { job_id, state, progress, message: message.into(), level, timestamp_ms: now_ms() }
    }
}

fn now_ms() -> u128 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() }

fn emit(app: &AppHandle, event: ScanEvent) {
    let _ = app.emit("scan-event", event);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_core_is_fake_and_idle() {
        let core = AppCore::default();
        let snapshot = core.snapshot();
        assert_eq!(snapshot.adapter, "fake");
        assert_eq!(snapshot.scanner_status, "ready");
        assert_eq!(snapshot.state, ScanState::Idle);
    }
}
