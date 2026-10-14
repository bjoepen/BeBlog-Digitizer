//! D6 batch planning and journal contract.
//!
//! This module only defines and persists a plan. It does not start scanner
//! processes, advance a plan automatically, or connect to the D1 simulation.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D6BatchRequest {
    /// Ordered and explicit so non-contiguous or offset plans can be rejected.
    #[serde(default = "default_frames")]
    pub frames: Vec<u8>,
}

impl Default for D6BatchRequest {
    fn default() -> Self {
        Self {
            frames: default_frames(),
        }
    }
}

fn default_frames() -> Vec<u8> {
    vec![1, 2, 3, 4]
}

impl D6BatchRequest {
    pub fn validate(&self) -> Result<(), String> {
        if !(2..=6).contains(&self.frames.len()) {
            return Err("D6-B1 akzeptiert Batchgrößen von 2 bis 6 Bildern".into());
        }
        let expected = (1..=self.frames.len() as u8).collect::<Vec<_>>();
        if self.frames != expected {
            return Err("D6-B1 akzeptiert ausschließlich die zusammenhängende Folge 1..N".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct D6BatchId(String);

impl D6BatchId {
    pub(crate) fn new() -> Self {
        Self(format!("d6-batch-{}", uuid::Uuid::new_v4().simple()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct D6FrameId(String);

impl D6FrameId {
    fn for_frame(batch_id: &D6BatchId, frame: u8) -> Self {
        Self(format!("{}-frame-{frame}", batch_id.as_str()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum D6BatchStatus {
    Planned,
    Running,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum D6FrameStatus {
    Planned,
    Running,
    Validated,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D6FrameJournalEntry {
    pub frame_id: D6FrameId,
    pub frame: u8,
    pub status: D6FrameStatus,
    /// Capture association is persisted before the capture can be finalized.
    pub capture_id: Option<String>,
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct D6BatchManifest {
    pub schema_version: u8,
    pub batch_id: D6BatchId,
    pub status: D6BatchStatus,
    pub frames: Vec<D6FrameJournalEntry>,
    pub failure: Option<String>,
}

impl D6BatchManifest {
    pub(crate) fn planned(request: &D6BatchRequest) -> Self {
        let batch_id = D6BatchId::new();
        Self {
            schema_version: 1,
            frames: request
                .frames
                .iter()
                .copied()
                .map(|frame| D6FrameJournalEntry {
                    frame_id: D6FrameId::for_frame(&batch_id, frame),
                    frame,
                    status: D6FrameStatus::Planned,
                    capture_id: None,
                    failure: None,
                })
                .collect(),
            batch_id,
            status: D6BatchStatus::Planned,
            failure: None,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Nicht unterstützte D6-Journalschemaversion".into());
        }
        let batch_id = self.batch_id.as_str();
        if !batch_id.starts_with("d6-batch-")
            || batch_id.len() > 96
            || !batch_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err("Ungültige D6-Batch-ID".into());
        }
        let request = D6BatchRequest {
            frames: self.frames.iter().map(|entry| entry.frame).collect(),
        };
        request.validate()?;
        let mut captures = std::collections::HashSet::new();
        for entry in &self.frames {
            if entry.frame_id != D6FrameId::for_frame(&self.batch_id, entry.frame) {
                return Err("D6-Frame-ID ist nicht an Batch-ID und Bildposition gebunden".into());
            }
            match entry.status {
                D6FrameStatus::Planned if entry.capture_id.is_some() || entry.failure.is_some() => {
                    return Err("Geplanter D6-Frame enthält unerwartete Abschlussdaten".into());
                }
                D6FrameStatus::Running | D6FrameStatus::Validated | D6FrameStatus::Completed
                    if entry.capture_id.is_none() || entry.failure.is_some() =>
                {
                    return Err(
                        "Aktiver/abgeschlossener D6-Frame benötigt Capture-Zuordnung".into(),
                    );
                }
                D6FrameStatus::Failed | D6FrameStatus::Cancelled | D6FrameStatus::Interrupted
                    if entry.failure.is_none() =>
                {
                    return Err("Terminaler D6-Frame benötigt eine Fehler-/Abbruchnotiz".into());
                }
                _ => {}
            }
            if let Some(capture_id) = &entry.capture_id {
                if capture_id.is_empty()
                    || capture_id.len() > 96
                    || !capture_id
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    || !captures.insert(capture_id)
                {
                    return Err("Ungültige oder doppelte D6-Capture-ID".into());
                }
            }
        }
        let mut saw_non_completed = false;
        let mut active_frames = 0;
        for entry in &self.frames {
            if entry.status == D6FrameStatus::Completed {
                if saw_non_completed {
                    return Err(
                        "D6-Journal enthält Completed-Frames außerhalb der Reihenfolge".into(),
                    );
                }
            } else {
                saw_non_completed = true;
            }
            if matches!(
                entry.status,
                D6FrameStatus::Running | D6FrameStatus::Validated
            ) {
                active_frames += 1;
            }
        }
        if active_frames > 1 {
            return Err("D6-Journal darf höchstens einen aktiven Frame enthalten".into());
        }
        if self.status == D6BatchStatus::Completed
            && self
                .frames
                .iter()
                .any(|entry| entry.status != D6FrameStatus::Completed)
        {
            return Err("Abgeschlossener D6-Batch enthält offene Frames".into());
        }
        if self.status == D6BatchStatus::Planned
            && self
                .frames
                .iter()
                .any(|entry| entry.status != D6FrameStatus::Planned)
        {
            return Err("Geplanter D6-Batch enthält bereits gestartete Frames".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_plan_contains_four_ordered_frames() {
        let request = D6BatchRequest::default();
        assert_eq!(request.frames, [1, 2, 3, 4]);
        request.validate().unwrap();
    }

    #[test]
    fn accepts_batch_sizes_two_four_and_six() {
        for count in [2_u8, 4, 6] {
            let request = D6BatchRequest {
                frames: (1..=count).collect(),
            };
            assert!(request.validate().is_ok(), "count={count}");
        }
    }

    #[test]
    fn rejects_out_of_range_counts_and_non_contiguous_frames() {
        for frames in [vec![], vec![1], vec![1, 2, 3, 4, 5, 6, 7]] {
            assert!(D6BatchRequest { frames }.validate().is_err());
        }
        for frames in [vec![2, 3], vec![1, 3], vec![1, 2, 4], vec![1, 2, 3, 5]] {
            assert!(D6BatchRequest { frames }.validate().is_err());
        }
    }

    #[test]
    fn batch_and_frame_ids_are_unique_and_stable_within_a_plan() {
        let request = D6BatchRequest::default();
        let first = D6BatchManifest::planned(&request);
        let second = D6BatchManifest::planned(&request);
        assert_ne!(first.batch_id, second.batch_id);
        let ids = first
            .frames
            .iter()
            .map(|entry| entry.frame_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(ids.len(), 4);
        first.validate().unwrap();
    }
}
