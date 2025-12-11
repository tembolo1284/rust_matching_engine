//! Protocol detection for incoming connections.

use crate::types::Protocol;
use engine_protocol::wire_types::MAGIC_BYTE;

/// Detect protocol from the first few bytes of a connection.
///
/// Detection rules:
/// - First byte is 'M' (0x4D) → Binary
/// - First bytes are "8=" → FIX
/// - First byte is N, C, F, Q, or # → CSV
/// - Other printable ASCII → CSV
/// - Otherwise → Binary (fallback)
pub fn detect_protocol(first_bytes: &[u8]) -> Protocol {
    if first_bytes.is_empty() {
        return Protocol::Csv; // Default
    }

    // Check for binary magic byte 'M' (0x4D)
    // But 'M' could also be a CSV command, so check second byte
    // Binary: [0x4D, msg_type (0-3 or 10-13), version, ...]
    // If second byte is a valid msg_type, it's binary
    if first_bytes[0] == MAGIC_BYTE && first_bytes.len() >= 2 {
        let msg_type = first_bytes[1];
        // Valid input types: 0-3, valid output types: 10-13
        if msg_type <= 3 || (10..=13).contains(&msg_type) {
            return Protocol::Binary;
        }
    }

    // Check for FIX (starts with "8=FIX")
    if first_bytes.len() >= 2 && &first_bytes[0..2] == b"8=" {
        return Protocol::Fix;
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
        // Magic byte 'M' followed by valid msg_type
        assert_eq!(detect_protocol(&[MAGIC_BYTE, 0, 1, 0]), Protocol::Binary); // NewOrder
        assert_eq!(detect_protocol(&[MAGIC_BYTE, 1, 1, 0]), Protocol::Binary); // Cancel
        assert_eq!(detect_protocol(&[MAGIC_BYTE, 2, 1, 0]), Protocol::Binary); // Flush
        assert_eq!(detect_protocol(&[MAGIC_BYTE, 3, 1, 0]), Protocol::Binary); // QueryTOB
        assert_eq!(detect_protocol(&[MAGIC_BYTE, 10, 1, 0]), Protocol::Binary); // Ack
        
        // Non-printable first byte
        assert_eq!(detect_protocol(&[0xBE, 0xEF, 0x01, 0x00]), Protocol::Binary);
    }

    #[test]
    fn test_detect_fix() {
        assert_eq!(detect_protocol(b"8=FIX.4.4"), Protocol::Fix);
        assert_eq!(detect_protocol(b"8=FIX.4.2"), Protocol::Fix);
    }

    #[test]
    fn test_ambiguous_m() {
        // 'M' followed by something that's NOT a valid msg_type
        // This would be rare in practice but we handle it
        assert_eq!(detect_protocol(b"My order"), Protocol::Csv);
    }
}
