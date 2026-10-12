#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedLog {
    Initialized,
    ScannerOpened,
    HolderDetected,
    FrameConfigured,
    ScanOptionConfigured,
    ScanStarted,
    /// `bytes` is the size of this one `sane_read()` call, never a running total.
    ReadChunk {
        bytes: u64,
    },
    Eof {
        final_chunk_bytes: u64,
    },
    CaptureComplete {
        total_bytes: u64,
        terminal_status: i32,
    },
    TiffWritten {
        path: String,
    },
    ClientExit {
        code: i32,
    },
    ReadFailed {
        status: i32,
        detail: String,
    },
    Malformed {
        stream: LogStream,
        line: String,
    },
    Unknown {
        stream: LogStream,
        line: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferError {
    Overflow,
    ExceedsExpected { total: u64, expected: u64 },
    ReportedTotalMismatch { observed: u64, reported: u64 },
}

/// Accumulates per-call read sizes without confusing them with the client's
/// separate `capture ended: total=...` report.
#[derive(Debug, Clone, Copy)]
pub struct TransferAccumulator {
    total: u64,
    expected: u64,
}

impl TransferAccumulator {
    pub fn new(expected: u64) -> Self {
        Self { total: 0, expected }
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn record_chunk(&mut self, bytes: u64) -> Result<u64, TransferError> {
        let total = self
            .total
            .checked_add(bytes)
            .ok_or(TransferError::Overflow)?;
        if total > self.expected {
            return Err(TransferError::ExceedsExpected {
                total,
                expected: self.expected,
            });
        }
        self.total = total;
        Ok(total)
    }

    pub fn verify_reported_total(&self, reported: u64) -> Result<(), TransferError> {
        if self.total == reported {
            Ok(())
        } else {
            Err(TransferError::ReportedTotalMismatch {
                observed: self.total,
                reported,
            })
        }
    }
}

#[derive(Default)]
pub struct SidecarLogParser;

impl SidecarLogParser {
    pub fn parse(&self, stream: LogStream, original_line: &str) -> ParsedLog {
        let line = strip_client_timestamp(original_line);
        if line.starts_with("sane_init -> 0") {
            return ParsedLog::Initialized;
        }
        if line.starts_with("sane_open") && line.contains("-> 0") {
            return ParsedLog::ScannerOpened;
        }
        if line.contains("frame descriptor range=1..6") {
            return ParsedLog::HolderDetected;
        }
        if line.starts_with("sane_control_option(frame, SET=") && line.contains("-> 0") {
            return ParsedLog::FrameConfigured;
        }
        if (line.starts_with("sane_control_option(mode,")
            || line.starts_with("sane_control_option(resolution,"))
            && line.contains("-> 0")
        {
            return ParsedLog::ScanOptionConfigured;
        }
        if line.starts_with("sane_start") && line.contains("-> 0") {
            return ParsedLog::ScanStarted;
        }
        if line.starts_with("sane_read -> ") {
            return parse_read_line(stream, line);
        }
        if let Some(rest) = line.strip_prefix("capture ended: total=") {
            let parsed = rest.split_once(" terminal=").and_then(|(total, terminal)| {
                total.parse::<u64>().ok().zip(
                    terminal
                        .split_whitespace()
                        .next()
                        .and_then(|status| status.parse::<i32>().ok()),
                )
            });
            return match parsed {
                Some((total_bytes, terminal_status)) => ParsedLog::CaptureComplete {
                    total_bytes,
                    terminal_status,
                },
                None => ParsedLog::Malformed {
                    stream,
                    line: original_line.to_string(),
                },
            };
        }
        if let Some(rest) = line.strip_prefix("TIFF ") {
            if let Some((path, "written")) = rest.rsplit_once(" -> ") {
                return ParsedLog::TiffWritten {
                    path: path.to_string(),
                };
            }
        }
        if let Some(rest) = line.strip_prefix("client exit -> ") {
            if let Some(code) = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse().ok())
            {
                return ParsedLog::ClientExit { code };
            }
            return ParsedLog::Malformed {
                stream,
                line: original_line.to_string(),
            };
        }
        ParsedLog::Unknown {
            stream,
            line: original_line.to_string(),
        }
    }
}

fn parse_read_line(stream: LogStream, line: &str) -> ParsedLog {
    let malformed = || ParsedLog::Malformed {
        stream,
        line: line.to_string(),
    };
    let Some((status, detail)) = line
        .strip_prefix("sane_read -> ")
        .and_then(|rest| rest.split_once(' '))
    else {
        return malformed();
    };
    let Ok(status) = status.parse::<i32>() else {
        return malformed();
    };
    let Some(bytes) = detail
        .split("bytes=")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .and_then(|value| value.trim_end_matches(',').parse::<u64>().ok())
    else {
        return malformed();
    };
    match status {
        0 => ParsedLog::ReadChunk { bytes },
        5 => ParsedLog::Eof {
            final_chunk_bytes: bytes,
        },
        _ => ParsedLog::ReadFailed {
            status,
            detail: detail.to_string(),
        },
    }
}

fn strip_client_timestamp(line: &str) -> &str {
    let Some((prefix, remainder)) = line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
    else {
        return line;
    };
    let bytes = prefix.as_bytes();
    if bytes.len() == 23
        && bytes.get(4) == Some(&b'-')
        && bytes.get(7) == Some(&b'-')
        && bytes.get(10) == Some(&b'T')
        && bytes.get(13) == Some(&b':')
        && bytes.get(16) == Some(&b':')
        && bytes.get(19) == Some(&b'.')
    {
        remainder
    } else {
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamped_original_read_chunks_as_individual_chunk_sizes() {
        let parser = SidecarLogParser;
        let fixture = include_str!("../tests/fixtures/research-005-frame1-client-stderr.fixture");
        let chunks = fixture
            .lines()
            .filter_map(|line| match parser.parse(LogStream::Stderr, line) {
                ParsedLog::ReadChunk { bytes } => Some(bytes),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(chunks, vec![16_384, 16_384, 16_384, 16_384, 12_792]);
        assert_eq!(chunks[0], chunks[1]);
        assert_eq!(chunks[1], chunks[2]);
    }

    #[test]
    fn accumulator_sums_chunks_and_checks_the_client_total() {
        let mut transfer = TransferAccumulator::new(1_438_200);
        assert_eq!(transfer.record_chunk(16_384), Ok(16_384));
        assert_eq!(transfer.record_chunk(16_384), Ok(32_768));
        assert_eq!(transfer.record_chunk(16_384), Ok(49_152));
        assert_eq!(transfer.record_chunk(12_792), Ok(61_944));
        assert_eq!(transfer.verify_reported_total(61_944), Ok(()));
        assert_eq!(
            transfer.verify_reported_total(61_943),
            Err(TransferError::ReportedTotalMismatch {
                observed: 61_944,
                reported: 61_943
            })
        );
    }

    #[test]
    fn accumulated_original_frame_profile_matches_client_reported_total() {
        // Research-005 frame1.client.log has 87 `bytes=16384` read calls
        // (source lines 54-140), then one `bytes=12792` call (line 141).
        let mut transfer = TransferAccumulator::new(1_438_200);
        for _ in 0..87 {
            transfer.record_chunk(16_384).unwrap();
        }
        transfer.record_chunk(12_792).unwrap();
        assert_eq!(transfer.total(), 1_438_200);
        transfer.verify_reported_total(1_438_200).unwrap();
    }

    #[test]
    fn accumulator_rejects_overflow_and_data_above_profile() {
        let mut overflow = TransferAccumulator::new(u64::MAX);
        overflow.record_chunk(u64::MAX).unwrap();
        assert_eq!(overflow.record_chunk(1), Err(TransferError::Overflow));

        let mut too_large = TransferAccumulator::new(10);
        assert_eq!(
            too_large.record_chunk(11),
            Err(TransferError::ExceedsExpected {
                total: 11,
                expected: 10
            })
        );
    }

    #[test]
    fn parses_eof_total_tiff_and_process_exit_from_original_lines() {
        let parser = SidecarLogParser;
        let fixture = include_str!("../tests/fixtures/research-005-frame1-client-stderr.fixture");
        let parsed = fixture
            .lines()
            .map(|line| parser.parse(LogStream::Stderr, line))
            .collect::<Vec<_>>();
        assert!(parsed.contains(&ParsedLog::Eof {
            final_chunk_bytes: 0
        }));
        assert!(parsed.contains(&ParsedLog::CaptureComplete {
            total_bytes: 1_438_200,
            terminal_status: 5
        }));
        assert!(parsed
            .iter()
            .any(|event| matches!(event, ParsedLog::TiffWritten { .. })));
        assert!(parsed.contains(&ParsedLog::ClientExit { code: 0 }));
    }

    #[test]
    fn retains_stream_for_unknown_stderr_and_never_classifies_it_as_error() {
        let parsed = SidecarLogParser.parse(
            LogStream::Stderr,
            "[2026-10-11T08:31:35.713] SANE version code=0x1040000",
        );
        assert_eq!(
            parsed,
            ParsedLog::Unknown {
                stream: LogStream::Stderr,
                line: "[2026-10-11T08:31:35.713] SANE version code=0x1040000".into()
            }
        );
    }

    #[test]
    fn malformed_known_lines_are_not_silently_downgraded_to_unknown() {
        assert!(matches!(
            SidecarLogParser.parse(LogStream::Stderr, "sane_read -> 0 (Success), bytes=NaN"),
            ParsedLog::Malformed { .. }
        ));
        assert!(matches!(
            SidecarLogParser.parse(LogStream::Stdout, "capture ended: total=oops terminal=5"),
            ParsedLog::Malformed { .. }
        ));
    }

    #[test]
    fn parses_read_calls_from_either_original_process_stream() {
        for stream in [LogStream::Stdout, LogStream::Stderr] {
            assert_eq!(
                SidecarLogParser.parse(stream, "sane_read -> 0 (Success), bytes=16"),
                ParsedLog::ReadChunk { bytes: 16 }
            );
        }
    }
}
