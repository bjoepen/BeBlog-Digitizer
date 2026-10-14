//! One process-wide reservation for every operation that may touch the scanner.
//!
//! The AppCore stores the returned guard for jobs and batches. Discovery holds a
//! guard for the duration of enumeration. Dropping a guard releases only its own
//! reservation, so stale cleanup cannot release a newer owner's lease.

use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScannerAction {
    D1Simulation,
    D5SingleFrame,
    #[allow(dead_code)] // Reserved by the D6-B2 shared lease contract for D6-B3.
    D6Batch,
    Discovery,
}

impl ScannerAction {
    fn label(self) -> &'static str {
        match self {
            Self::D1Simulation => "D1-Simulation",
            Self::D5SingleFrame => "D5-Einzelbildscan",
            Self::D6Batch => "D6-Batch",
            Self::Discovery => "Scannererkennung",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct LeaseRecord {
    id: u64,
    action: ScannerAction,
}

#[derive(Clone, Default)]
pub(crate) struct ScannerLeaseManager {
    state: Arc<Mutex<Option<LeaseRecord>>>,
    next_id: Arc<std::sync::atomic::AtomicU64>,
}

pub(crate) struct ScannerLeaseGuard {
    state: Arc<Mutex<Option<LeaseRecord>>>,
    id: u64,
}

impl ScannerLeaseManager {
    pub(crate) fn acquire(&self, action: ScannerAction) -> Result<ScannerLeaseGuard, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Scanner-Lease ist nicht verfügbar (Mutexfehler)".to_string())?;
        if let Some(owner) = *state {
            return Err(format!(
                "Scanner ist durch {} reserviert; {} kann nicht starten",
                owner.action.label(),
                action.label()
            ));
        }
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            .wrapping_add(1);
        *state = Some(LeaseRecord { id, action });
        Ok(ScannerLeaseGuard {
            state: self.state.clone(),
            id,
        })
    }
}

impl Drop for ScannerLeaseGuard {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            if state.as_ref().is_some_and(|record| record.id == self.id) {
                *state = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Barrier, thread};

    #[test]
    fn lease_is_exclusive_across_all_scanner_actions_and_released_by_owner_drop() {
        let manager = ScannerLeaseManager::default();
        let lease = manager.acquire(ScannerAction::D5SingleFrame).unwrap();
        assert!(manager.acquire(ScannerAction::Discovery).is_err());
        for action in [
            ScannerAction::D1Simulation,
            ScannerAction::D5SingleFrame,
            ScannerAction::D6Batch,
            ScannerAction::Discovery,
        ] {
            assert!(manager.acquire(action).is_err());
        }
        drop(lease);
        let released = manager.acquire(ScannerAction::Discovery).unwrap();
        drop(released);
    }

    #[test]
    fn lease_remains_reserved_while_owner_waits_for_opened_confirmation_and_cleanup() {
        let manager = ScannerLeaseManager::default();
        let lease = manager.acquire(ScannerAction::D6Batch).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let contender_manager = manager.clone();
        let contender_barrier = barrier.clone();
        let contender = thread::spawn(move || {
            contender_barrier.wait();
            contender_manager
                .acquire(ScannerAction::D5SingleFrame)
                .is_err()
        });
        barrier.wait();
        assert!(contender.join().unwrap());
        drop(lease); // Caller drops only after process, readers, and reap are confirmed.
        assert!(manager.acquire(ScannerAction::D5SingleFrame).is_ok());
    }

    #[test]
    fn stale_guard_cannot_release_a_different_reservation() {
        let manager = ScannerLeaseManager::default();
        let lease = manager.acquire(ScannerAction::D5SingleFrame).unwrap();
        let state = lease.state.clone();
        let old_id = lease.id;
        drop(lease);
        let newer = manager.acquire(ScannerAction::D6Batch).unwrap();
        if let Ok(mut slot) = state.lock() {
            *slot = Some(LeaseRecord {
                id: newer.id,
                action: ScannerAction::D6Batch,
            });
        }
        let stale = ScannerLeaseGuard { state, id: old_id };
        drop(stale);
        assert!(manager.acquire(ScannerAction::Discovery).is_err());
    }
}
