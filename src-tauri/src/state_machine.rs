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
        let active = matches!(
            self.state,
            Initializing
                | OpeningScanner
                | WaitingForHolder
                | ConfiguringFrame
                | StartingScan
                | Transferring
                | Finalizing
                | Cancelling
                | AwaitingHolderConfirmation
                | WaitingForHolderDetection
                | PositioningFrame
                | Scanning
                | ClosingScanner
                | InterFrameCooldown
        );
        let allowed = matches!(
            (self.state, next),
            (
                Idle,
                Initializing | Cancelling | Failed | TimedOut | AwaitingHolderConfirmation
            ) | (
                AwaitingHolderConfirmation,
                WaitingForHolderDetection | BatchCancelled | Cancelling
            ) | (
                WaitingForHolderDetection,
                Initializing | Cancelling | BatchFailed
            ) | (
                Initializing,
                OpeningScanner | Initializing | Cancelling | Failed | TimedOut | BatchFailed
            ) | (
                OpeningScanner,
                WaitingForHolder | Cancelling | Failed | TimedOut | BatchFailed
            ) | (
                WaitingForHolder,
                PositioningFrame | ConfiguringFrame | Cancelling | Failed | TimedOut | BatchFailed
            ) | (
                PositioningFrame,
                ConfiguringFrame | Cancelling | Failed | TimedOut | BatchFailed
            ) | (
                ConfiguringFrame,
                ConfiguringFrame | StartingScan | Cancelling | Failed | TimedOut | BatchFailed
            ) | (
                StartingScan,
                Scanning | Transferring | Cancelling | Failed | TimedOut | BatchFailed
            ) | (
                Scanning,
                Transferring | Cancelling | Failed | TimedOut | BatchFailed
            ) | (
                Transferring,
                Transferring | Finalizing | Cancelling | Failed | TimedOut | BatchFailed
            ) | (
                Finalizing,
                Finalizing
                    | Completed
                    | ClosingScanner
                    | Cancelling
                    | Failed
                    | TimedOut
                    | BatchFailed
            ) | (
                ClosingScanner,
                Completed
                    | InterFrameCooldown
                    | BatchCompleted
                    | Cancelling
                    | Failed
                    | TimedOut
                    | BatchFailed
            ) | (
                InterFrameCooldown,
                Initializing | Cancelling | BatchCancelled | BatchFailed
            ) | (
                Cancelling,
                Cancelled | BatchCancelled | Failed | BatchFailed
            ) | (
                Completed
                    | Cancelled
                    | Failed
                    | TimedOut
                    | BatchCompleted
                    | BatchCancelled
                    | BatchFailed,
                Idle
            )
        ) || (active && matches!(next, Failed | TimedOut | BatchFailed));
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
    fn accepts_single_scan_cleanup_before_completion() {
        let mut sm = ScanStateMachine::default();
        for state in [
            Initializing,
            OpeningScanner,
            WaitingForHolder,
            PositioningFrame,
            ConfiguringFrame,
            StartingScan,
            Scanning,
            Transferring,
            Finalizing,
            ClosingScanner,
            Completed,
        ] {
            sm.transition(state).unwrap();
        }
        assert_eq!(sm.state(), Completed);
    }

    #[test]
    fn accepts_confirmed_batch_and_cooldown_cycle() {
        let mut sm = ScanStateMachine::default();
        for state in [
            AwaitingHolderConfirmation,
            WaitingForHolderDetection,
            Initializing,
            OpeningScanner,
            WaitingForHolder,
            PositioningFrame,
            ConfiguringFrame,
            StartingScan,
            Scanning,
            Transferring,
            Finalizing,
            ClosingScanner,
            InterFrameCooldown,
            Initializing,
        ] {
            sm.transition(state).unwrap();
        }
    }

    #[test]
    fn rejects_skips_and_unconfirmed_scans() {
        let mut sm = ScanStateMachine::default();
        assert!(sm.transition(Transferring).is_err());
        assert!(sm.transition(Completed).is_err());
        assert_eq!(sm.state(), Idle);
    }

    #[test]
    fn supports_cancel_before_confirmation() {
        let mut sm = ScanStateMachine::default();
        sm.transition(AwaitingHolderConfirmation).unwrap();
        sm.transition(BatchCancelled).unwrap();
        assert!(sm.state().terminal());
    }
}
