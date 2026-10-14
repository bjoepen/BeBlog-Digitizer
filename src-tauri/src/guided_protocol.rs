//! Typed protocol contract shared by the guided UI workflow and the C client.
//! This module only parses client stdout lines; it never starts a process.
#![allow(dead_code)] // Real process start remains hash-gated until independent approval.

use crate::model::ScanState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClientEvent {
    Opened,
    OpenFailed,
    HolderDetected,
    FramePositioned(u8),
    Parameters {
        dpi: u16,
        width: u32,
        height: u32,
        bytes_per_line: u32,
        depth: u8,
        format: String,
        expected_bytes: u64,
    },
    ScanStarted,
    DataChunk {
        chunk: u32,
        total: u64,
    },
    Eof {
        total: u64,
    },
    TiffWritten,
    ScannerClosing,
    Complete,
    Error {
        status: i32,
    },
    ProtocolError,
    Diagnostic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProtocolError {
    InvalidEvent,
    InvalidFields,
    InvalidTransition {
        from: ScanState,
        event: &'static str,
    },
    UserConfirmationRequired,
    InconsistentTransfer,
}

pub(crate) fn parse_client_line(line: &str) -> Result<ClientEvent, ProtocolError> {
    let Some(body) = line.strip_prefix("BEBLOG/1\t") else {
        return Ok(ClientEvent::Diagnostic);
    };
    let mut fields = body.split('\t');
    let name = fields.next().ok_or(ProtocolError::InvalidEvent)?;
    let rest: Vec<_> = fields.collect();
    let event = match (name, rest.as_slice()) {
        ("OPENED", []) => ClientEvent::Opened,
        ("OPEN_FAILED", ["sane_open"]) => ClientEvent::OpenFailed,
        ("HOLDER_DETECTED", []) => ClientEvent::HolderDetected,
        ("FRAME_POSITIONED", [frame]) => ClientEvent::FramePositioned(number(frame)?),
        ("PARAMETERS", [dpi, width, height, stride, depth, format, expected]) => {
            ClientEvent::Parameters {
                dpi: number(dpi)?,
                width: number(width)?,
                height: number(height)?,
                bytes_per_line: number(stride)?,
                depth: number(depth)?,
                format: (*format).to_string(),
                expected_bytes: number(expected)?,
            }
        }
        ("SCAN_STARTED", []) => ClientEvent::ScanStarted,
        ("DATA_CHUNK", [chunk, total]) => ClientEvent::DataChunk {
            chunk: number(chunk)?,
            total: number(total)?,
        },
        ("EOF", [total]) => ClientEvent::Eof {
            total: number(total)?,
        },
        ("TIFF_WRITTEN", []) => ClientEvent::TiffWritten,
        ("SCANNER_CLOSING", []) => ClientEvent::ScannerClosing,
        ("COMPLETE", []) => ClientEvent::Complete,
        ("ERROR", [status]) if status.starts_with("status=") => ClientEvent::Error {
            status: number(&status[7..])?,
        },
        ("PROTOCOL_ERROR", [_]) => ClientEvent::ProtocolError,
        _ => return Err(ProtocolError::InvalidFields),
    };
    Ok(event)
}

fn number<T: std::str::FromStr>(value: &str) -> Result<T, ProtocolError> {
    value.parse().map_err(|_| ProtocolError::InvalidFields)
}

/// Pure reducer for one single-frame guided run. A UI confirmation only sends
/// the continuation token; it never advances holder state by itself.
#[derive(Debug, Clone)]
pub(crate) struct GuidedProtocol {
    state: ScanState,
    user_confirmed: bool,
    transferred: u64,
    expected_bytes: Option<u64>,
    eof_seen: bool,
    tiff_seen: bool,
    failed: bool,
}

impl Default for GuidedProtocol {
    fn default() -> Self {
        Self {
            state: ScanState::OpeningScanner,
            user_confirmed: false,
            transferred: 0,
            expected_bytes: None,
            eof_seen: false,
            tiff_seen: false,
            failed: false,
        }
    }
}

impl GuidedProtocol {
    pub(crate) fn state(&self) -> ScanState {
        self.state
    }
    pub(crate) fn transferred(&self) -> u64 {
        self.transferred
    }
    pub(crate) fn is_failed(&self) -> bool {
        self.failed
    }

    fn reject(&mut self, error: ProtocolError) -> Result<(), ProtocolError> {
        self.failed = true;
        self.state = ScanState::Failed;
        Err(error)
    }

    pub(crate) fn confirm_insertion(&mut self) -> Result<&'static [u8], ProtocolError> {
        if self.state != ScanState::AwaitingHolderConfirmation {
            return Err(ProtocolError::UserConfirmationRequired);
        }
        self.user_confirmed = true;
        self.state = ScanState::WaitingForHolderDetection;
        Ok(b"CONTINUE\n")
    }

    pub(crate) fn observe(&mut self, event: ClientEvent) -> Result<(), ProtocolError> {
        if matches!(event, ClientEvent::Diagnostic)
            || self.failed && matches!(event, ClientEvent::ScannerClosing)
        {
            return Ok(());
        }
        if self.failed {
            return Err(ProtocolError::InvalidTransition {
                from: self.state,
                event: "after-error",
            });
        }
        match event {
            ClientEvent::Opened if self.state == ScanState::OpeningScanner => {
                self.state = ScanState::AwaitingHolderConfirmation
            }
            ClientEvent::OpenFailed if self.state == ScanState::OpeningScanner => {
                self.failed = true;
                self.state = ScanState::Failed;
            }
            ClientEvent::HolderDetected
                if self.state == ScanState::WaitingForHolderDetection && self.user_confirmed =>
            {
                self.state = ScanState::PositioningFrame
            }
            ClientEvent::FramePositioned(frame)
                if (1..=6).contains(&frame) && self.state == ScanState::PositioningFrame =>
            {
                self.state = ScanState::ConfiguringFrame
            }
            ClientEvent::Parameters {
                dpi,
                width,
                height,
                bytes_per_line,
                depth,
                format,
                expected_bytes,
            } if self.state == ScanState::ConfiguringFrame => {
                let Some(row_bytes) = u64::from(width).checked_mul(3) else {
                    return self.reject(ProtocolError::InvalidFields);
                };
                let Some(expected) = u64::from(bytes_per_line).checked_mul(u64::from(height))
                else {
                    return self.reject(ProtocolError::InvalidFields);
                };
                if !matches!(dpi, 600 | 2820)
                    || format != "RGB"
                    || depth != 8
                    || width == 0
                    || height == 0
                    || row_bytes != u64::from(bytes_per_line)
                    || expected != expected_bytes
                    || expected > 512 * 1024 * 1024
                {
                    return self.reject(ProtocolError::InvalidFields);
                }
                self.expected_bytes = Some(expected_bytes);
            }
            ClientEvent::ScanStarted
                if self.state == ScanState::ConfiguringFrame && self.expected_bytes.is_some() =>
            {
                self.state = ScanState::Transferring
            }
            ClientEvent::DataChunk { chunk, total } if self.state == ScanState::Transferring => {
                if self.transferred.checked_add(chunk as u64) != Some(total) {
                    return self.reject(ProtocolError::InconsistentTransfer);
                }
                if self.expected_bytes.is_some_and(|expected| total > expected) {
                    return self.reject(ProtocolError::InconsistentTransfer);
                }
                self.transferred = total;
            }
            ClientEvent::Eof { total } if self.state == ScanState::Transferring => {
                if total != self.transferred || self.expected_bytes != Some(total) {
                    return self.reject(ProtocolError::InconsistentTransfer);
                }
                self.eof_seen = true;
                self.state = ScanState::Finalizing;
            }
            ClientEvent::TiffWritten if self.state == ScanState::Finalizing && self.eof_seen => {
                self.tiff_seen = true
            }
            ClientEvent::ScannerClosing
                if self.state == ScanState::Finalizing && self.tiff_seen =>
            {
                self.state = ScanState::ClosingScanner
            }
            ClientEvent::Complete if self.state == ScanState::ClosingScanner => {
                self.state = ScanState::Completed
            }
            ClientEvent::Error { .. } | ClientEvent::ProtocolError => {
                self.failed = true;
                self.state = ScanState::Failed;
            }
            other => {
                return self.reject(ProtocolError::InvalidTransition {
                    from: self.state,
                    event: event_name(&other),
                })
            }
        }
        Ok(())
    }
}

fn event_name(event: &ClientEvent) -> &'static str {
    match event {
        ClientEvent::Opened => "opened",
        ClientEvent::OpenFailed => "open-failed",
        ClientEvent::HolderDetected => "holder-detected",
        ClientEvent::FramePositioned(_) => "frame-positioned",
        ClientEvent::Parameters { .. } => "parameters",
        ClientEvent::ScanStarted => "scan-started",
        ClientEvent::DataChunk { .. } => "data-chunk",
        ClientEvent::Eof { .. } => "eof",
        ClientEvent::TiffWritten => "tiff-written",
        ClientEvent::ScannerClosing => "scanner-closing",
        ClientEvent::Complete => "complete",
        ClientEvent::Error { .. } => "error",
        ClientEvent::ProtocolError => "protocol-error",
        ClientEvent::Diagnostic => "diagnostic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(flow: &mut GuidedProtocol, lines: &[&str]) -> Result<(), ProtocolError> {
        for line in lines {
            flow.observe(parse_client_line(line)?)?;
        }
        Ok(())
    }

    #[test]
    fn requires_open_event_then_explicit_continue_then_actual_holder_event() {
        let mut flow = GuidedProtocol::default();
        assert_eq!(
            flow.confirm_insertion(),
            Err(ProtocolError::UserConfirmationRequired)
        );
        flow.observe(ClientEvent::Opened).unwrap();
        assert_eq!(flow.state(), ScanState::AwaitingHolderConfirmation);
        assert_eq!(flow.confirm_insertion().unwrap(), b"CONTINUE\n");
        assert_eq!(flow.state(), ScanState::WaitingForHolderDetection);
        assert!(flow.observe(ClientEvent::HolderDetected).is_ok());
        assert_eq!(flow.state(), ScanState::PositioningFrame);
    }

    #[test]
    fn simulated_client_event_sequence_completes_with_consistent_bytes() {
        let mut flow = GuidedProtocol::default();
        feed(&mut flow, &["BEBLOG/1\tOPENED"]).unwrap();
        flow.confirm_insertion().unwrap();
        feed(
            &mut flow,
            &[
                "BEBLOG/1\tHOLDER_DETECTED",
                "BEBLOG/1\tFRAME_POSITIONED\t1",
                "BEBLOG/1\tPARAMETERS\t2820\t1200\t1800\t3600\t8\tRGB\t6480000",
                "BEBLOG/1\tSCAN_STARTED",
                "BEBLOG/1\tDATA_CHUNK\t1000\t1000",
                "BEBLOG/1\tDATA_CHUNK\t6479000\t6480000",
                "BEBLOG/1\tEOF\t6480000",
                "BEBLOG/1\tTIFF_WRITTEN",
                "BEBLOG/1\tSCANNER_CLOSING",
                "BEBLOG/1\tCOMPLETE",
            ],
        )
        .unwrap();
        assert_eq!(flow.transferred(), 6_480_000);
        assert_eq!(flow.state(), ScanState::Completed);
    }

    #[test]
    fn unknown_diagnostics_do_not_advance_and_bad_event_order_fails_closed() {
        let mut flow = GuidedProtocol::default();
        flow.observe(parse_client_line("ordinary debug text").unwrap())
            .unwrap();
        assert_eq!(flow.state(), ScanState::OpeningScanner);
        assert!(flow.observe(ClientEvent::HolderDetected).is_err());
        assert_eq!(flow.state(), ScanState::Failed);
        assert!(flow.is_failed());
    }

    #[test]
    fn rejects_bad_byte_totals_and_protocol_errors() {
        let mut flow = GuidedProtocol::default();
        flow.observe(ClientEvent::Opened).unwrap();
        flow.confirm_insertion().unwrap();
        flow.observe(ClientEvent::HolderDetected).unwrap();
        flow.observe(ClientEvent::FramePositioned(1)).unwrap();
        flow.observe(ClientEvent::Parameters {
            dpi: 2820,
            width: 1,
            height: 1,
            bytes_per_line: 3,
            depth: 8,
            format: "RGB".into(),
            expected_bytes: 3,
        })
        .unwrap();
        flow.observe(ClientEvent::ScanStarted).unwrap();
        assert_eq!(
            flow.observe(ClientEvent::DataChunk { chunk: 2, total: 3 }),
            Err(ProtocolError::InconsistentTransfer)
        );
        flow.observe(ClientEvent::ProtocolError).unwrap_err();
    }

    #[test]
    fn open_failure_allows_cleanup_diagnostic_but_never_completion() {
        let mut flow = GuidedProtocol::default();
        flow.observe(ClientEvent::OpenFailed).unwrap();
        flow.observe(ClientEvent::ScannerClosing).unwrap();
        assert_eq!(flow.state(), ScanState::Failed);
        assert!(flow.observe(ClientEvent::Complete).is_err());
    }
}
