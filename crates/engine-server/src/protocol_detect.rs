//! Protocol detection for incoming connections.

use crate::types::Protocol;
use engine_protocol::wire_types::MAGIC_BYTES;

/// Detect protocol from the first few bytes of a connection.
///
/// Returns the detected protocol and whether to consume the peeked bytes.
pub fn detect_protocol(first_bytes: &[u8]) -> Protocol {
    if first_bytes.is_empty() {
        return Protocol::Csv; // Default
    }

    // Check for binary magic bytes "MENG"
    if first_bytes.len() >= 4 && first_bytes[0..4] == MAGIC_BYTES {
        return Protocol::Binary;
    }

    // Check for FIX (starts with "8=FIX")
    if first_bytes.len() >= 5 {
        if &first_bytes[0..2] == b"8=" {
            return Protocol::Fix;
        }
    }

    // Check for CSV commands
    match first_bytes[0] {
        b'N' | b'C' | b'F' | b'Q' | b'#' => Protocol::Csv,
        _ => {
            // If first byte is printable ASCII, assume CSV
            if first_bytes[0].is_ascii_graphic() || first_bytes[0].is_ascii_whitespace() {
                Protocol::Csv
            } else {
                Protocol::Binary
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_csv() {
        assert_eq!(detect_protocol(b"N, 1, IBM"), Protocol::Csv);
        assert_eq!(detect_protocol(b"C, 1, 100"), Protocol::Csv);
        assert_eq!(detect_protocol(b"F"), Protocol::Csv);
        assert_eq!(detect_protocol(b"# comment"), Protocol::Csv);
    }

    #[test]
    fn test_detect_binary() {
        assert_eq!(detect_protocol(&[0xBE, 0xEF, 0x01, 0x00]), Protocol::Binary);
    }

    #[test]
    fn test_detect_fix() {
        assert_eq!(detect_protocol(b"8=FIX.4.4"), Protocol::Fix);
    }
}
