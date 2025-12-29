//! CSV compatibility codec.
//!
//! # Power of Ten Compliance
//! - Rule 2: Bounded parsing (max tokens per line).
//! - Rule 3: Minimized allocations (reuse buffers where possible).
//! - Rule 5: Assertions on parsing.
//!
//! # Input Format
//! - New order: `N, user, symbol, price, qty, side, userOrderId`
//! - Cancel: `C, user, userOrderId`
//! - Flush: `F`
//! - Query TOB: `Q, symbol`
//!
//! # Output Format (symbol-aware)
//! - Ack: `A, userId, userOrderId, symbol`
//! - CancelAck: `C, userId, userOrderId, symbol`
//! - Trade: `T, symbol, buyUser, buyOrder, sellUser, sellOrder, price, qty`
//! - TopOfBook: `B, symbol, side, price, qty` or `B, symbol, side, -, -`
//! - Reject: `R, userId, userOrderId, symbol, reason`

use engine_core::{
    Cancel, InputMessage, NewOrder, OutputMessage, Side, Symbol, TopOfBookQuery,
};

/// Maximum tokens per CSV line (prevents unbounded parsing).
const MAX_TOKENS: usize = 16;

/// Maximum line length.
const MAX_LINE_LENGTH: usize = 256;

// =============================================================================
// Input Parsing
// =============================================================================

/// Parse a single CSV line into an `InputMessage`.
///
/// Returns `None` for blank lines or comments (starting with `#`).
pub fn parse_input_line(line: &str) -> Option<InputMessage> {
    debug_assert!(line.len() <= MAX_LINE_LENGTH, "line too long for CSV parsing");

    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    // Parse into fixed-size token array (no heap allocation for small lines)
    let mut tokens: [&str; MAX_TOKENS] = [""; MAX_TOKENS];
    let token_count = split_csv_line(trimmed, &mut tokens);

    if token_count == 0 {
        return None;
    }

    let msg_type = tokens[0].chars().next()?;

    match msg_type {
        'N' => parse_new_order(&tokens[..token_count]),
        'C' => parse_cancel(&tokens[..token_count]),
        'F' => {
            if token_count == 1 {
                Some(InputMessage::Flush)
            } else {
                None
            }
        }
        'Q' => parse_query_tob(&tokens[..token_count]),
        _ => None,
    }
}

/// Split CSV line into tokens without heap allocation.
/// Returns number of tokens written.
fn split_csv_line<'a>(line: &'a str, tokens: &mut [&'a str; MAX_TOKENS]) -> usize {
    let mut count = 0;

    for part in line.split(',') {
        if count >= MAX_TOKENS {
            break;
        }
        tokens[count] = part.trim();
        count += 1;
    }

    count
}

fn parse_new_order(tokens: &[&str]) -> Option<InputMessage> {
    // N, user, symbol, price, qty, side, userOrderId
    if tokens.len() != 7 {
        return None;
    }

    let user_id: u32 = tokens[1].parse().ok()?;
    let symbol = Symbol::from_str(tokens[2]);
    let price: u32 = tokens[3].parse().ok()?;
    let quantity: u32 = tokens[4].parse().ok()?;

    if quantity == 0 {
        return None;
    }

    let side_char = tokens[5].chars().next()?;
    let side = match side_char {
        'B' => Side::Buy,
        'S' => Side::Sell,
        _ => return None,
    };

    let user_order_id: u32 = tokens[6].parse().ok()?;

    if user_order_id == 0 {
        return None;
    }

    Some(InputMessage::NewOrder(NewOrder::new(
        user_id,
        user_order_id,
        symbol,
        price,
        quantity,
        side,
    )))
}

fn parse_cancel(tokens: &[&str]) -> Option<InputMessage> {
    // C, user, userOrderId
    if tokens.len() != 3 {
        return None;
    }

    let user_id: u32 = tokens[1].parse().ok()?;
    let user_order_id: u32 = tokens[2].parse().ok()?;

    Some(InputMessage::Cancel(Cancel::new(user_id, user_order_id)))
}

fn parse_query_tob(tokens: &[&str]) -> Option<InputMessage> {
    // Q, symbol
    if tokens.len() != 2 {
        return None;
    }

    let symbol = Symbol::from_str(tokens[1]);
    if symbol.is_empty() {
        return None;
    }

    Some(InputMessage::QueryTopOfBook(TopOfBookQuery::new(symbol)))
}

// =============================================================================
// Output Formatting
// =============================================================================

/// Format buffer for CSV output (avoids repeated allocation).
pub struct CsvFormatBuffer {
    buf: String,
}

impl CsvFormatBuffer {
    /// Create a new format buffer with default capacity.
    pub fn new() -> Self {
        CsvFormatBuffer {
            buf: String::with_capacity(128),
        }
    }

    /// Create with specific capacity.
    pub fn with_capacity(cap: usize) -> Self {
        CsvFormatBuffer {
            buf: String::with_capacity(cap),
        }
    }

    /// Format an output message, returning the formatted string.
    /// Reuses internal buffer.
    pub fn format(&mut self, msg: &OutputMessage) -> &str {
        self.buf.clear();
        format_output_into(msg, &mut self.buf);
        &self.buf
    }

    /// Format using legacy format (no symbol in some messages).
    pub fn format_legacy(&mut self, msg: &OutputMessage) -> &str {
        self.buf.clear();
        format_output_legacy_into(msg, &mut self.buf);
        &self.buf
    }
}

impl Default for CsvFormatBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// Format an `OutputMessage` as a CSV line (symbol-aware format).
pub fn format_output_csv(msg: &OutputMessage) -> String {
    let mut buf = String::with_capacity(64);
    format_output_into(msg, &mut buf);
    buf
}

/// Format into existing String buffer.
pub fn format_output_into(msg: &OutputMessage, buf: &mut String) {
    use std::fmt::Write;

    match msg {
        OutputMessage::Ack(a) => {
            let _ = write!(buf, "A, {}, {}, {}", a.user_id, a.user_order_id, a.symbol);
        }
        OutputMessage::CancelAck(c) => {
            let _ = write!(buf, "C, {}, {}, {}", c.user_id, c.user_order_id, c.symbol);
        }
        OutputMessage::Trade(t) => {
            let _ = write!(
                buf,
                "T, {}, {}, {}, {}, {}, {}, {}",
                t.symbol,
                t.user_id_buy,
                t.user_order_id_buy,
                t.user_id_sell,
                t.user_order_id_sell,
                t.price,
                t.quantity
            );
        }
        OutputMessage::TopOfBook(t) => {
            let side_char = match t.side {
                Side::Buy => 'B',
                Side::Sell => 'S',
            };
            if t.is_eliminated() {
                let _ = write!(buf, "B, {}, {}, -, -", t.symbol, side_char);
            } else {
                let _ = write!(
                    buf,
                    "B, {}, {}, {}, {}",
                    t.symbol, side_char, t.price, t.total_quantity
                );
            }
        }
        OutputMessage::Reject(r) => {
            let _ = write!(
                buf,
                "R, {}, {}, {}, {:?}",
                r.user_id, r.user_order_id, r.symbol, r.reason
            );
        }
    }
}

/// Legacy format (compatible with original C++ format).
pub fn format_output_legacy(msg: &OutputMessage) -> String {
    let mut buf = String::with_capacity(64);
    format_output_legacy_into(msg, &mut buf);
    buf
}

/// Format legacy into existing buffer.
pub fn format_output_legacy_into(msg: &OutputMessage, buf: &mut String) {
    use std::fmt::Write;

    match msg {
        OutputMessage::Ack(a) => {
            let _ = write!(buf, "A, {}, {}", a.user_id, a.user_order_id);
        }
        OutputMessage::CancelAck(c) => {
            let _ = write!(buf, "C, {}, {}", c.user_id, c.user_order_id);
        }
        OutputMessage::Trade(t) => {
            let _ = write!(
                buf,
                "T, {}, {}, {}, {}, {}, {}",
                t.user_id_buy,
                t.user_order_id_buy,
                t.user_id_sell,
                t.user_order_id_sell,
                t.price,
                t.quantity
            );
        }
        OutputMessage::TopOfBook(t) => {
            let side_char = match t.side {
                Side::Buy => 'B',
                Side::Sell => 'S',
            };
            if t.is_eliminated() {
                let _ = write!(buf, "B, {}, -, -", side_char);
            } else {
                let _ = write!(buf, "B, {}, {}, {}", side_char, t.price, t.total_quantity);
            }
        }
        OutputMessage::Reject(r) => {
            let _ = write!(buf, "R, {}, {}, {:?}", r.user_id, r.user_order_id, r.reason);
        }
    }
}

// =============================================================================
// Streaming Parser
// =============================================================================

/// Streaming CSV parser that processes lines one at a time.
#[derive(Debug, Default)]
pub struct CsvParser {
    /// Partial line buffer for streaming input.
    line_buf: String,
}

impl CsvParser {
    /// Create a new CSV parser.
    pub fn new() -> Self {
        CsvParser {
            line_buf: String::with_capacity(MAX_LINE_LENGTH),
        }
    }

    /// Parse a complete line.
    #[inline]
    pub fn parse_line(&self, line: &str) -> Option<InputMessage> {
        parse_input_line(line)
    }

    /// Feed bytes and extract complete messages.
    ///
    /// Returns messages for each complete line found.
    /// Incomplete lines are buffered internally.
    pub fn feed(&mut self, data: &[u8], messages: &mut Vec<InputMessage>) {
        for &byte in data {
            if byte == b'\n' || byte == b'\r' {
                if !self.line_buf.is_empty() {
                    if let Some(msg) = parse_input_line(&self.line_buf) {
                        messages.push(msg);
                    }
                    self.line_buf.clear();
                }
            } else if self.line_buf.len() < MAX_LINE_LENGTH {
                self.line_buf.push(byte as char);
            }
            // Silently truncate lines that are too long
        }
    }

    /// Flush any remaining partial line.
    pub fn flush(&mut self) -> Option<InputMessage> {
        if self.line_buf.is_empty() {
            return None;
        }
        let msg = parse_input_line(&self.line_buf);
        self.line_buf.clear();
        msg
    }

    /// Clear internal state.
    pub fn clear(&mut self) {
        self.line_buf.clear();
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_new_order() {
        let line = "N, 1, IBM, 100, 50, B, 1001";
        let msg = parse_input_line(line).unwrap();

        match msg {
            InputMessage::NewOrder(o) => {
                assert_eq!(o.user_id, 1);
                assert_eq!(o.user_order_id, 1001);
                assert_eq!(o.symbol.as_str(), "IBM");
                assert_eq!(o.price, 100);
                assert_eq!(o.quantity, 50);
                assert_eq!(o.side, Side::Buy);
            }
            _ => panic!("Expected NewOrder"),
        }
    }

    #[test]
    fn test_parse_cancel() {
        let line = "C, 42, 100";
        let msg = parse_input_line(line).unwrap();

        match msg {
            InputMessage::Cancel(c) => {
                assert_eq!(c.user_id, 42);
                assert_eq!(c.user_order_id, 100);
            }
            _ => panic!("Expected Cancel"),
        }
    }

    #[test]
    fn test_parse_flush() {
        let line = "F";
        let msg = parse_input_line(line).unwrap();
        assert!(matches!(msg, InputMessage::Flush));
    }

    #[test]
    fn test_parse_query_tob() {
        let line = "Q, AAPL";
        let msg = parse_input_line(line).unwrap();

        match msg {
            InputMessage::QueryTopOfBook(q) => {
                assert_eq!(q.symbol.as_str(), "AAPL");
            }
            _ => panic!("Expected QueryTopOfBook"),
        }
    }

    #[test]
    fn test_parse_comment() {
        assert!(parse_input_line("# This is a comment").is_none());
        assert!(parse_input_line("").is_none());
        assert!(parse_input_line("   ").is_none());
    }

    #[test]
    fn test_format_ack() {
        let ack = OutputMessage::ack(1, 100, Symbol::from_str("IBM"));
        let csv = format_output_csv(&ack);
        assert_eq!(csv, "A, 1, 100, IBM");
    }

    #[test]
    fn test_format_trade() {
        let trade = OutputMessage::trade(
            Symbol::from_str("GOOG"),
            1, 100,
            2, 200,
            5000, 25,
        );
        let csv = format_output_csv(&trade);
        assert_eq!(csv, "T, GOOG, 1, 100, 2, 200, 5000, 25");
    }

    #[test]
    fn test_format_tob_eliminated() {
        let tob = OutputMessage::top_of_book_eliminated(Symbol::from_str("X"), Side::Buy);
        let csv = format_output_csv(&tob);
        assert_eq!(csv, "B, X, B, -, -");
    }

    #[test]
    fn test_format_buffer_reuse() {
        let mut buf = CsvFormatBuffer::new();

        let ack = OutputMessage::ack(1, 1, Symbol::from_str("A"));
        assert_eq!(buf.format(&ack), "A, 1, 1, A");

        let trade = OutputMessage::trade(Symbol::from_str("B"), 2, 2, 3, 3, 100, 10);
        assert_eq!(buf.format(&trade), "T, B, 2, 2, 3, 3, 100, 10");
    }

    #[test]
    fn test_streaming_parser() {
        let mut parser = CsvParser::new();
        let mut messages = Vec::new();

        // Feed partial data
        parser.feed(b"N, 1, IBM, 100, 50, B, 1\n", &mut messages);
        assert_eq!(messages.len(), 1);

        messages.clear();
        parser.feed(b"C, 1, 1\nF\n", &mut messages);
        assert_eq!(messages.len(), 2);
    }
}
