//! Protocol detection for incoming connections.

use crate::types::Protocol;
use engine_protocol::wire_types::MAGIC_BYTE;

/// Detect protocol from the first few bytes of a connection.
///
/// Detection rules:
/// - First byte is 'M' (0x4D) AND second byte is valid msg_type → Binary
/// - First bytes are "8=" → FIX
/// - First byte is N, C, F, Q, or # → CSV
/// - Other printable ASCII → CSV
/// - Otherwise → Binary (fallback)
pub fn detect_protocol(first_bytes: &[u8]) -> Protocol {
    if first_bytes.is_empty() {
        return Protocol::Csv; // Default
    }

    // Check for binary magic byte 'M' (0x4D)
    // Binary uses ASCII message types: N, C, F (input) and A, X, T, B (output)
    if first_bytes[0] == MAGIC_BYTE && first_bytes.len() >= 2 {
        let msg_type = first_bytes[1];
        // Valid input types: 'N' (0x4E), 'C' (0x43), 'F' (0x46)
        // Valid output types: 'A' (0x41), 'X' (0x58), 'T' (0x54), 'B' (0x42)
        if matches!(msg_type, b'N' | b'C' | b'F' | b'A' | b'X' | b'T' | b'B') {
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
    fn test_detect_binary_zig_format() {
        // Magic byte 'M' followed by ASCII msg_type (Zig format)
        assert_eq!(detect_protocol(&[MAGIC_BYTE, b'N', 0, 0]), Protocol::Binary); // NewOrder
        assert_eq!(detect_protocol(&[MAGIC_BYTE, b'C', 0, 0]), Protocol::Binary); // Cancel
        assert_eq!(detect_protocol(&[MAGIC_BYTE, b'F']), Protocol::Binary);       // Flush
        assert_eq!(detect_protocol(&[MAGIC_BYTE, b'A', 0, 0]), Protocol::Binary); // Ack
        assert_eq!(detect_protocol(&[MAGIC_BYTE, b'X', 0, 0]), Protocol::Binary); // CancelAck
        assert_eq!(detect_protocol(&[MAGIC_BYTE, b'T', 0, 0]), Protocol::Binary); // Trade
        assert_eq!(detect_protocol(&[MAGIC_BYTE, b'B', 0, 0]), Protocol::Binary); // TopOfBook
        
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
