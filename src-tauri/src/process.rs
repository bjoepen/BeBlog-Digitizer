use crate::model::SimulationMode;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessEvent {
    Started { program: &'static str },
    Stdout(String),
    Stderr(String),
    Exited { code: i32 },
    Cancelled,
    TimedOut,
    CleanedUp,
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
    Cancelled,
    TimedOut,
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
#[derive(Default)]
pub struct RecordedProcessRunner;

impl RecordedProcessRunner {
    fn wait(cancelled: &AtomicBool, deadline: Instant, duration: Duration) -> ProcessOutcome {
        let until = Instant::now() + duration;
        while Instant::now() < until {
            if cancelled.load(Ordering::SeqCst) {
                return ProcessOutcome::Cancelled;
            }
            if Instant::now() >= deadline {
                return ProcessOutcome::TimedOut;
            }
            thread::sleep(Duration::from_millis(10));
        }
        ProcessOutcome::Exited(0)
    }
}

impl ProcessRunner for RecordedProcessRunner {
    fn run(
        &self,
        request: &ProcessRequest,
        cancelled: Arc<AtomicBool>,
        emit: &mut dyn FnMut(ProcessEvent),
    ) -> ProcessOutcome {
        emit(ProcessEvent::Started {
            program: "recorded-minolta-fixture",
        });
        let deadline = Instant::now() + request.timeout;
        let mut outcome = ProcessOutcome::Exited(0);
        let mut lines = recorded_client_lines(request.frame);
        lines.insert(
            2,
            "diagnostic: future backend message is intentionally unknown".into(),
        );
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

        for (index, line) in lines.into_iter().enumerate() {
            if request.simulation == SimulationMode::Timeout && index == 3 {
                outcome = Self::wait(&cancelled, Instant::now(), Duration::from_millis(30));
                if outcome == ProcessOutcome::Exited(0) {
                    outcome = ProcessOutcome::TimedOut;
                }
                break;
            }
            outcome = Self::wait(&cancelled, deadline, Duration::from_millis(20));
            if outcome != ProcessOutcome::Exited(0) {
                break;
            }
            emit(ProcessEvent::Stdout(line));
            if request.simulation == SimulationMode::Error && index == 8 {
                emit(ProcessEvent::Stderr("simulated sidecar I/O failure".into()));
                outcome = ProcessOutcome::Exited(1);
                break;
            }
        }

        match outcome {
            ProcessOutcome::Exited(code) => emit(ProcessEvent::Exited { code }),
            ProcessOutcome::Cancelled => emit(ProcessEvent::Cancelled),
            ProcessOutcome::TimedOut => emit(ProcessEvent::TimedOut),
        }
        // A real runner must reap its child before emitting this final event.
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
            RecordedProcessRunner.run(
                &request(mode),
                Arc::new(AtomicBool::new(false)),
                &mut |event| events.push(event),
            );
            assert_eq!(events.last(), Some(&ProcessEvent::CleanedUp));
        }
    }

    #[test]
    fn cancellation_is_reported_and_cleaned_up() {
        let cancelled = Arc::new(AtomicBool::new(true));
        let mut events = Vec::new();
        let result =
            RecordedProcessRunner.run(&request(SimulationMode::Success), cancelled, &mut |event| {
                events.push(event)
            });
        assert_eq!(result, ProcessOutcome::Cancelled);
        assert!(events.contains(&ProcessEvent::Cancelled));
        assert_eq!(events.last(), Some(&ProcessEvent::CleanedUp));
    }

    #[test]
    fn timeout_is_reported_and_cleaned_up() {
        let mut events = Vec::new();
        let result = RecordedProcessRunner.run(
            &request(SimulationMode::Timeout),
            Arc::new(AtomicBool::new(false)),
            &mut |event| events.push(event),
        );
        assert_eq!(result, ProcessOutcome::TimedOut);
        assert_eq!(events.last(), Some(&ProcessEvent::CleanedUp));
    }
}
