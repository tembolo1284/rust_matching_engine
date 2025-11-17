// crates/engine-protocol/src/csv_codec.rs

//! CSV compatibility codec.
//!
//! This mirrors (and slightly extends) your original C++ CSV parser
//! and formatter, but is built on top of the new symbol-aware messages.
//!
//! Input format (lines → `InputMessage`):
//!
//! - New order:
//!   `N, user(int), symbol(string), price(int), qty(int), side(char B or S), userOrderId(int)`
//!
//! - Cancel:
//!   `C, user(int), userOrderId(int)`
//!
//! - Flush:
//!   `F`
//!
//! - Query top-of-book (NEW):
//!   `Q, symbol(string)`
//!
//! Output format (`OutputMessage` → line) - NEW FORMAT:
//!
//! - Ack:
//!   `A, userId, userOrderId, symbol`
//!
//! - CancelAck:
//!   `C, userId, userOrderId, symbol`
//!
//! - Trade:
//!   `T, symbol, userIdBuy, userOrderIdBuy, userIdSell, userOrderIdSell, price, quantity`
//!
//! - TopOfBook (non-eliminated):
//!   `B, symbol, side(B/S), price, totalQuantity`
//!
//! - TopOfBook (eliminated):
//!   `B, symbol, side(B/S), -, -`

use std::num::ParseIntError;

use engine_core::{Cancel, InputMessage, NewOrder, OutputMessage, Side, TopOfBookQuery};

/// Parse a single CSV line into an `InputMessage`.
///
/// Returns `None` for blank lines or comments (starting with `#`).
pub fn parse_input_line(line: &str) -> Option<InputMessage> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let tokens = split_and_trim(trimmed, ',');
    if tokens.is_empty() {
        return None;
    }

    let msg_type = tokens[0].chars().next().unwrap_or('\0');

    match msg_type {
        'N' => parse_new_order(&tokens),
        'C' => parse_cancel(&tokens),
        'F' => {
            if tokens.len() == 1 {
                Some(InputMessage::Flush)
            } else {
                None
            }
        }
        'Q' => parse_query_tob(&tokens),
        _ => None,
    }
}

fn parse_new_order(tokens: &[String]) -> Option<InputMessage> {
    // N, user, symbol, price, qty, side, userOrderId
    if tokens.len() != 7 {
        return None;
    }

    let user_id = parse_u32(&tokens[1]).ok()?;
    let symbol = tokens[2].clone();
    let price = parse_u32(&tokens[3]).ok()?;
    let quantity = parse_u32(&tokens[4]).ok()?;

    if quantity == 0 {
        return None;
    }

    let side_char = tokens[5].chars().next()?;
    let side = match side_char {
        'B' => Side::Buy,
        'S' => Side::Sell,
        _ => return None,
    };

    let user_order_id = parse_u32(&tokens[6]).ok()?;

    Some(InputMessage::NewOrder(NewOrder {
        user_id,
        symbol,
        price,
        quantity,
        side,
        user_order_id,
    }))
}

fn parse_cancel(tokens: &[String]) -> Option<InputMessage> {
    // C, user, userOrderId
    if tokens.len() != 3 {
        return None;
    }

    let user_id = parse_u32(&tokens[1]).ok()?;
    let user_order_id = parse_u32(&tokens[2]).ok()?;

    Some(InputMessage::Cancel(Cancel {
        user_id,
        user_order_id,
    }))
}

fn parse_query_tob(tokens: &[String]) -> Option<InputMessage> {
    // Q, symbol
    if tokens.len() != 2 {
        return None;
    }

    let symbol = tokens[1].clone();
    Some(InputMessage::QueryTopOfBook(TopOfBookQuery { symbol }))
}

/// Format an `OutputMessage` as a CSV line (NEW, symbol-aware format).
pub fn format_output_csv(msg: &OutputMessage) -> String {
    match msg {
        OutputMessage::Ack(a) => format!("A, {}, {}, {}", a.user_id, a.user_order_id, a.symbol),
        OutputMessage::CancelAck(c) => {
            format!("C, {}, {}, {}", c.user_id, c.user_order_id, c.symbol)
        }
        OutputMessage::Trade(t) => format!(
            "T, {}, {}, {}, {}, {}, {}, {}",
            t.symbol,
            t.user_id_buy,
            t.user_order_id_buy,
            t.user_id_sell,
            t.user_order_id_sell,
            t.price,
            t.quantity
        ),
        OutputMessage::TopOfBook(t) => {
            let side_char = match t.side {
                Side::Buy => 'B',
                Side::Sell => 'S',
            };
            if t.eliminated {
                format!("B, {}, {}, -, -", t.symbol, side_char)
            } else {
                format!(
                    "B, {}, {}, {}, {}",
                    t.symbol, side_char, t.price, t.total_quantity
                )
            }
        }
    }
}

/// Legacy formatter matching the original C++ `output_file.csv` format.
///
/// Old format:
/// - Ack:        `A, userId, userOrderId`
/// - CancelAck:  `C, userId, userOrderId`
/// - Trade:      `T, userIdBuy, userOrderIdBuy, userIdSell, userOrderIdSell, price, quantity`
/// - TopOfBook:  `B, side, price, totalQuantity`
/// - TOB elim:   `B, side, -, -`
pub fn format_output_legacy(msg: &OutputMessage) -> String {
    match msg {
        OutputMessage::Ack(a) => format!("A, {}, {}", a.user_id, a.user_order_id),
        OutputMessage::CancelAck(c) => format!("C, {}, {}", c.user_id, c.user_order_id),
        OutputMessage::Trade(t) => format!(
            "T, {}, {}, {}, {}, {}, {}",
            t.user_id_buy,
            t.user_order_id_buy,
            t.user_id_sell,
            t.user_order_id_sell,
            t.price,
            t.quantity
        ),
        OutputMessage::TopOfBook(t) => {
            let side_char = match t.side {
                Side::Buy => 'B',
                Side::Sell => 'S',
            };
            if t.eliminated {
                format!("B, {}, -, -", side_char)
            } else {
                format!("B, {}, {}, {}", side_char, t.price, t.total_quantity)
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------------

fn split_and_trim(s: &str, delimiter: char) -> Vec<String> {
    s.split(delimiter)
        .map(|tok| tok.trim().to_string())
        .collect()
}

fn parse_u32(s: &str) -> Result<u32, ParseIntError> {
    s.parse::<u32>()
}

