//! Order type (Market vs Limit).
//!
//! Mirrors your C++ `OrderType`:
//! ```cpp
//! enum class OrderType {
//!     MARKET,  // price = 0
//!     LIMIT    // price > 0
//! };
//! ```

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OrderType {
    Market,
    Limit,
}

