use crate::model::SimulationMode;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    Started { program: &'static str },
    StartFailed(String),
    Stdout(String),
    Stderr(String),
    StdoutClosed,
    StderrClosed,
    Exited { code: i32 },
    Cancelled,
    TimedOut,
    UnexpectedEnd(String),
    CleanedUp,
    CleanupFailed(String),
}

#[derive(Debug, Clone)]
pub struct ProcessRequest {
    pub frame: u8,
    pub simulation: SimulationMode,
    pub timeout: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessOutcome {
    Exited(i32),
    StartFailed(String),
    Cancelled,
    TimedOut,
    UnexpectedEnd(String),
    CleanupFailed(String),
}

pub trait ProcessRunner: Send + Sync + 'static {
    fn run(
        &self,
        request: &ProcessRequest,
        cancelled: Arc<AtomicBool>,
        emit: &mut dyn FnMut(ProcessEvent),
    ) -> ProcessOutcome;
}

/// Hardware-free process double. It never invokes std::process or a shell.
#[derive(Debug, Default, Clone, Copy)]
#[allow(dead_code)] // Faults are selectable only from hardware-free unit tests.
pub(crate) enum RecordedFault {
    #[default]
    None,
    StartFailure,
    CleanupFailure,
    MissingCleanupEvent,
    IncompleteData,
    MissingTiff,
    StateRegression,
    UnexpectedEnd,
    LateOutput,
    CancelAfterStdout(usize),
}

#[derive(Default)]
pub struct RecordedProcessRunner {
    fault: RecordedFault,
}

impl RecordedProcessRunner {
    #[allow(dead_code)] // Test-only fault injection for lifecycle regression coverage.
    pub(crate) fn with_fault(fault: RecordedFault) -> Self {
        Self { fault }
    }
}

impl ProcessRunner for RecordedProcessRunner {
    fn run(
        &self,
        request: &ProcessRequest,
        cancelled: Arc<AtomicBool>,
        emit: &mut dyn FnMut(ProcessEvent),
    ) -> ProcessOutcome {
        // The recorded runner is deterministic: timeout is selected by the scenario,
        // never by sleeping against wall-clock time.
        let _timeout_budget = request.timeout;
        if cancelled.load(Ordering::SeqCst) {
            emit(ProcessEvent::Cancelled);
            return ProcessOutcome::Cancelled;
        }
        if matches!(self.fault, RecordedFault::StartFailure) {
            let message = "recorded process start failure".to_string();
            emit(ProcessEvent::StartFailed(message.clone()));
            return ProcessOutcome::StartFailed(message);
        }

        emit(ProcessEvent::Started {
            program: "recorded-minolta-fixture",
        });
        let mut outcome = ProcessOutcome::Exited(0);
        let mut lines = recorded_client_lines(request.frame);
        lines.insert(
            2,
            "diagnostic: future backend message is intentionally unknown".into(),
        );
        if matches!(self.fault, RecordedFault::StateRegression) {
            lines.insert(3, "sane_init -> 0 (Success)".into());
        }
        let start_index = lines
            .iter()
            .position(|line| line.starts_with("sane_start"))
            .unwrap_or(0)
            + 1;
        for (offset, line) in [
            "sane_read -> 0 (Success), bytes=458752",
            "sane_read -> 0 (Success), bytes=983040",
            "sane_read -> 0 (Success), bytes=1438200",
        ]
        .into_iter()
        .enumerate()
        {
            lines.insert(start_index + offset, line.into());
        }
        if request.simulation == SimulationMode::Incomplete {
            lines
                .retain(|line| !line.contains("End of file reached") && !line.starts_with("TIFF "));
        }
        if matches!(self.fault, RecordedFault::IncompleteData) {
            lines.retain(|line| !line.ends_with("bytes=1438200"));
        }
        if matches!(self.fault, RecordedFault::MissingTiff) {
            lines.retain(|line| !line.starts_with("TIFF "));
        }

        let mut stdout_events = 0;
        for (index, line) in lines.into_iter().enumerate() {
            if request.simulation == SimulationMode::Timeout && index == 3 {
                outcome = ProcessOutcome::TimedOut;
                break;
            }
            if cancelled.load(Ordering::SeqCst) {
                outcome = ProcessOutcome::Cancelled;
                break;
            }
            emit(ProcessEvent::Stdout(line));
            stdout_events += 1;
            if matches!(self.fault, RecordedFault::CancelAfterStdout(after) if stdout_events == after)
            {
                cancelled.store(true, Ordering::SeqCst);
                outcome = ProcessOutcome::Cancelled;
                break;
            }
            if request.simulation == SimulationMode::Error && index == 8 {
                emit(ProcessEvent::Stderr("simulated sidecar I/O failure".into()));
                outcome = ProcessOutcome::Exited(1);
                break;
            }
        }

        emit(ProcessEvent::StdoutClosed);
        emit(ProcessEvent::StderrClosed);
        if matches!(self.fault, RecordedFault::UnexpectedEnd) {
            let message = "recorded process ended without an exit status".to_string();
            emit(ProcessEvent::UnexpectedEnd(message.clone()));
            outcome = ProcessOutcome::UnexpectedEnd(message);
        }
        match outcome {
            ProcessOutcome::Exited(code) => emit(ProcessEvent::Exited { code }),
            ProcessOutcome::Cancelled => emit(ProcessEvent::Cancelled),
            ProcessOutcome::TimedOut => emit(ProcessEvent::TimedOut),
            ProcessOutcome::StartFailed(_)
            | ProcessOutcome::UnexpectedEnd(_)
            | ProcessOutcome::CleanupFailed(_) => {}
        }
        if matches!(self.fault, RecordedFault::LateOutput) {
            emit(ProcessEvent::Stdout(
                "TIFF late-output.tiff -> written".into(),
            ));
        }
        if matches!(self.fault, RecordedFault::CleanupFailure) {
            let message = "recorded process cleanup failure".to_string();
            emit(ProcessEvent::CleanupFailed(message.clone()));
            return ProcessOutcome::CleanupFailed(message);
        }
        if matches!(self.fault, RecordedFault::MissingCleanupEvent) {
            return outcome;
        }
        // This event stands for successful resource release/reaping in the fake.
        emit(ProcessEvent::CleanedUp);
        outcome
    }
}

fn recorded_client_lines(frame: u8) -> Vec<String> {
    const FIXTURE: &str = include_str!("../tests/fixtures/research-009-success.fixture");
    FIXTURE
        .lines()
        .filter(|line| {
            line.starts_with("sane_init")
                || line.starts_with("sane_open")
                || line.starts_with("holder poll")
                || line.starts_with("sane_control_option")
                || line.starts_with("sane_start")
                || line.starts_with("sane_read")
                || line.starts_with("capture ended")
                || line.starts_with("TIFF ")
        })
        .map(|line| {
            if line.starts_with("sane_control_option(frame, SET=") {
                format!("sane_control_option(frame, SET={frame}) -> 0 (Success)")
            } else {
                line.to_string()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(mode: SimulationMode) -> ProcessRequest {
        ProcessRequest {
            frame: 1,
            simulation: mode,
            timeout: Duration::from_secs(1),
        }
    }

    #[test]
    fn cleanup_is_last_for_success_and_failure() {
        for mode in [
            SimulationMode::Success,
            SimulationMode::Error,
            SimulationMode::Incomplete,
        ] {
            let mut events = Vec::new();
            RecordedProcessRunner::default().run(
                &request(mode),
                Arc::new(AtomicBool::new(false)),
                &mut |event| events.push(event),
            );
            assert_eq!(events.last(), Some(&ProcessEvent::CleanedUp));
        }
    }

    #[test]
    fn cancellation_before_start_does_not_claim_a_process_was_started() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let mut events = Vec::new();
        let result = RecordedProcessRunner::default().run(
            &request(SimulationMode::Success),
            cancelled,
            &mut |event| events.push(event),
        );
        assert_eq!(result, ProcessOutcome::Cancelled);
        assert!(events.contains(&ProcessEvent::Cancelled));
        assert!(!events
            .iter()
            .any(|event| matches!(event, ProcessEvent::Started { .. })));
        assert!(!events.contains(&ProcessEvent::CleanedUp));
    }

    #[test]
    fn recorded_process_emits_closed_streams_exit_and_cleanup_in_order() {
        let mut events = Vec::new();
        let outcome = RecordedProcessRunner::default().run(
            &request(SimulationMode::Success),
            Arc::new(AtomicBool::new(false)),
            &mut |event| events.push(event),
        );
        let stdout_closed = events
            .iter()
            .position(|e| *e == ProcessEvent::StdoutClosed)
            .unwrap();
        let stderr_closed = events
            .iter()
            .position(|e| *e == ProcessEvent::StderrClosed)
            .unwrap();
        let exited = events
            .iter()
            .position(|e| matches!(e, ProcessEvent::Exited { code: 0 }))
            .unwrap();
        let cleaned = events
            .iter()
            .position(|e| *e == ProcessEvent::CleanedUp)
            .unwrap();
        assert!(stdout_closed < exited && stderr_closed < exited && exited < cleaned);
        assert_eq!(outcome, ProcessOutcome::Exited(0));
    }

    #[test]
    fn cancellation_during_a_frame_is_deterministic_and_cleans_up() {
        let mut events = Vec::new();
        let cancelled = Arc::new(AtomicBool::new(false));
        let result = RecordedProcessRunner::with_fault(RecordedFault::CancelAfterStdout(3)).run(
            &request(SimulationMode::Success),
            cancelled.clone(),
            &mut |event| events.push(event),
        );
        assert_eq!(result, ProcessOutcome::Cancelled);
        assert!(cancelled.load(Ordering::SeqCst));
        assert!(events
            .iter()
            .any(|e| matches!(e, ProcessEvent::Started { .. })));
        assert_eq!(events.last(), Some(&ProcessEvent::CleanedUp));
    }

    #[test]
    fn timeout_is_reported_and_cleaned_up() {
        let mut events = Vec::new();
        let result = RecordedProcessRunner::default().run(
            &request(SimulationMode::Timeout),
            Arc::new(AtomicBool::new(false)),
            &mut |event| events.push(event),
        );
        assert_eq!(result, ProcessOutcome::TimedOut);
        assert_eq!(events.last(), Some(&ProcessEvent::CleanedUp));
    }
}
