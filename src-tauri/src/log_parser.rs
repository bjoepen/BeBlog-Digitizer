#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedLog {
    Initialized,
    ScannerOpened,
    HolderDetected,
    FrameConfigured,
    ScanOptionConfigured,
    ScanStarted,
    Data { bytes: u64 },
    Eof,
    CaptureComplete { bytes: u64 },
    TiffWritten,
    Unknown(String),
}

#[derive(Default)]
pub struct SidecarLogParser;

impl SidecarLogParser {
    pub fn parse(&self, line: &str) -> ParsedLog {
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
        if line.contains("End of file reached") && line.starts_with("sane_read") {
            return ParsedLog::Eof;
        }
        if let Some(rest) = line.strip_prefix("capture ended: total=") {
            if let Some(value) = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse().ok())
            {
                return ParsedLog::CaptureComplete { bytes: value };
            }
        }
        if line.starts_with("sane_read -> 0") {
            if let Some(value) = line
                .split("bytes=")
                .nth(1)
                .and_then(|value| value.trim().parse().ok())
            {
                return ParsedLog::Data { bytes: value };
            }
        }
        if line.starts_with("TIFF ") && line.ends_with("-> written") {
            return ParsedLog::TiffWritten;
        }
        ParsedLog::Unknown(line.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_eof_and_byte_count() {
        let parser = SidecarLogParser;
        assert_eq!(
            parser.parse("sane_read -> 5 (End of file reached), bytes=0"),
            ParsedLog::Eof
        );
        assert_eq!(
            parser.parse("capture ended: total=1438200 terminal=5"),
            ParsedLog::CaptureComplete { bytes: 1_438_200 }
        );
    }

    #[test]
    fn preserves_unknown_messages_without_failing() {
        let parser = SidecarLogParser;
        assert_eq!(
            parser.parse("new backend message"),
            ParsedLog::Unknown("new backend message".into())
        );
    }

    #[test]
    fn malformed_numbers_are_unknown() {
        assert!(matches!(
            SidecarLogParser.parse("capture ended: total=not-a-number"),
            ParsedLog::Unknown(_)
        ));
    }
}
