use crate::model::ScanState;

#[derive(Debug)]
pub struct ScanStateMachine {
    state: ScanState,
}

impl Default for ScanStateMachine {
    fn default() -> Self {
        Self {
            state: ScanState::Idle,
        }
    }
}

impl ScanStateMachine {
    pub fn state(&self) -> ScanState {
        self.state
    }

    pub fn transition(&mut self, next: ScanState) -> Result<(), String> {
        use ScanState::*;
        let allowed = matches!(
            (self.state, next),
            (Initializing, Initializing)
                | (OpeningScanner, OpeningScanner)
                | (WaitingForHolder, WaitingForHolder)
                | (ConfiguringFrame, ConfiguringFrame)
                | (StartingScan, StartingScan)
                | (Finalizing, Finalizing)
                | (Idle, Initializing)
                | (Idle, Cancelling)
                | (Idle, Failed)
                | (Idle, TimedOut)
                | (Initializing, OpeningScanner)
                | (OpeningScanner, WaitingForHolder)
                | (WaitingForHolder, ConfiguringFrame)
                | (ConfiguringFrame, StartingScan)
                | (StartingScan, Transferring)
                | (Transferring, Transferring)
                | (Transferring, Finalizing)
                | (Finalizing, Completed)
                | (
                    Initializing
                        | OpeningScanner
                        | WaitingForHolder
                        | ConfiguringFrame
                        | StartingScan
                        | Transferring
                        | Finalizing,
                    Cancelling
                )
                | (Cancelling, Cancelled)
                | (
                    Initializing
                        | OpeningScanner
                        | WaitingForHolder
                        | ConfiguringFrame
                        | StartingScan
                        | Transferring
                        | Finalizing
                        | Cancelling,
                    Failed
                )
                | (
                    Initializing
                        | OpeningScanner
                        | WaitingForHolder
                        | ConfiguringFrame
                        | StartingScan
                        | Transferring
                        | Finalizing
                        | Cancelling,
                    TimedOut
                )
                | (Completed | Cancelled | Failed | TimedOut, Idle)
        );
        if !allowed {
            return Err(format!(
                "invalid scan transition: {:?} -> {:?}",
                self.state, next
            ));
        }
        self.state = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ScanState::*;

    #[test]
    fn accepts_research_009_success_path() {
        let mut sm = ScanStateMachine::default();
        for state in [
            Initializing,
            OpeningScanner,
            WaitingForHolder,
            ConfiguringFrame,
            StartingScan,
            Transferring,
            Finalizing,
            Completed,
        ] {
            sm.transition(state).unwrap();
        }
        assert_eq!(sm.state(), Completed);
    }

    #[test]
    fn rejects_skipping_scan_start() {
        let mut sm = ScanStateMachine::default();
        sm.transition(Initializing).unwrap();
        assert!(sm.transition(Transferring).is_err());
    }

    #[test]
    fn invalid_transition_is_reported_without_mutating_state() {
        let mut sm = ScanStateMachine::default();
        let error = sm.transition(Completed).unwrap_err();
        assert!(error.contains("Idle -> Completed"));
        assert_eq!(sm.state(), Idle);
    }

    #[test]
    fn supports_controlled_cancellation() {
        let mut sm = ScanStateMachine::default();
        sm.transition(Initializing).unwrap();
        sm.transition(Cancelling).unwrap();
        sm.transition(Cancelled).unwrap();
        assert!(sm.state().terminal());
    }

    #[test]
    fn timeout_and_process_failure_are_valid_from_active_states() {
        let mut timed_out = ScanStateMachine::default();
        for state in [
            Initializing,
            OpeningScanner,
            WaitingForHolder,
            ConfiguringFrame,
        ] {
            timed_out.transition(state).unwrap();
        }
        timed_out.transition(TimedOut).unwrap();

        let mut failed = ScanStateMachine::default();
        for state in [Initializing, OpeningScanner, WaitingForHolder] {
            failed.transition(state).unwrap();
        }
        failed.transition(Failed).unwrap();
        assert_eq!(failed.state(), Failed);
    }
}
