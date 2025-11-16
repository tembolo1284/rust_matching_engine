//! Single-symbol order book with price-time priority.
//!
//!" This is the Rust analogue of your C++ `OrderBook`:
//! - One instance per symbol.
//! - Bids: descending by price (best = highest).
//! - Asks: ascending by price (best = lowest).
//! - FIFO (time-priority) within each price level.
//!
//! Differences vs the C++ implementation:
//! - For simplicity and safety, cancellation currently does a linear
//!   search over the relevant side instead of storing raw iterators
//!   (which are tricky to model safely in Rust without arenas/unsafe).
//!   Semantics are the same; complexity is slightly higher for cancels.

use std::collections::{BTreeMap, VecDeque};

use crate::messages::{NewOrder, OutputMessage};
use crate::order::Order;
use crate::order_type::OrderType;
use crate::side::Side;
use crate::top_of_book::TopOfBookSnapshot;

/// Single-symbol order book.
#[derive(Debug)]
pub struct OrderBook {
    symbol: String,

    /// Bids: price -> FIFO queue of orders at that price.
    ///
    /// We use `BTreeMap` so keys are sorted ascending; we treat the
    /// highest key as best bid.
    bids: BTreeMap<u32, VecDeque<Order>>,

    /// Asks: price -> FIFO queue of orders at that price.
    ///
    /// We use `BTreeMap` so keys are sorted ascending; we treat the
    /// lowest key as best ask.
    asks: BTreeMap<u32, VecDeque<Order>>,

    /// Cache of previous top-of-book for change detection.
    prev_best_bid_price: u32,
    prev_best_bid_qty: u32,
    prev_best_ask_price: u32,
    prev_best_ask_qty: u32,
}

impl OrderBook {
    /// Create a new order book for the given symbol.
    pub fn new(symbol: impl Into<String>) -> Self {
        OrderBook {
            symbol: symbol.into(),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            prev_best_bid_price: 0,
            prev_best_bid_qty: 0,
            prev_best_ask_price: 0,
            prev_best_ask_qty: 0,
        }
    }

    /// Returns the symbol of this book.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Process a new order, returning output messages:
    /// - Ack
    /// - Trades
    /// - Top-of-book changes
    ///
    /// This matches the behavior of your C++ `addOrder`.
    pub fn add_order(&mut self, msg: &NewOrder) -> Vec<OutputMessage> {
        let mut outputs = Vec::new();

        // Create an internal order with timestamp.
        let mut order = Order::from_new_order_now(msg);

        // Ack.
        outputs.push(OutputMessage::ack(
            order.user_id,
            order.user_order_id,
            self.symbol.clone(),
        ));

        // Match against the opposing side.
        let trade_outputs = self.match_order(&mut order);
        outputs.extend(trade_outputs);

        // If there's remaining quantity and it's a limit order, add to book.
        if order.remaining_qty > 0 && order.order_type == OrderType::Limit {
            self.add_to_book(order);
        }

        // Emit top-of-book changes (if any).
        let tob_outputs = self.check_top_of_book_changes();
        outputs.extend(tob_outputs);

        outputs
    }

    /// Cancel an order by `(user_id, user_order_id)`.
    ///
    /// Semantics mirror C++ `cancelOrder`:
    /// - If the order exists, remove it from the book and emit:
    ///   - CancelAck
    ///   - Top-of-book changes (if affected).
    /// - If it doesn't exist, still emit a CancelAck.
    ///
    /// Note: We don't get the symbol here; the `MatchingEngine` routes
    /// cancel to the correct `OrderBook` based on its own mapping, just
    /// like your C++ engine.
    pub fn cancel_order(&mut self, user_id: u32, user_order_id: u32) -> Vec<OutputMessage> {
        let mut outputs = Vec::new();

        // Helper lambda: try to remove from a side (bids or asks).
        fn remove_from_side(
            _side: Side,
            _book_symbol: &str,
            levels: &mut BTreeMap<u32, VecDeque<Order>>,
            user_id: u32,
            user_order_id: u32,
        ) -> bool {
            // Iterate over all price levels; in practice the depth is usually small.
            let mut empty_prices = Vec::new();

            for (price, orders) in levels.iter_mut() {
                let mut idx = 0;
                while idx < orders.len() {
                    if let Some(o) = orders.get(idx) {
                        if o.user_id == user_id && o.user_order_id == user_order_id {
                            // Found the order; remove it.
                            orders.remove(idx);
                            if orders.is_empty() {
                                empty_prices.push(*price);
                            }
                            return true;
                        }
                    }
                    idx += 1;
                }
            }

            // Clean up any now-empty price levels.
            for p in empty_prices {
                levels.remove(&p);
            }

            false
        }

        // Try bids then asks.
        let mut found = remove_from_side(Side::Buy, &self.symbol, &mut self.bids, user_id, user_order_id);
        if !found {
            found = remove_from_side(Side::Sell, &self.symbol, &mut self.asks, user_id, user_order_id);
        }

        // Always emit CancelAck, even if not found (matches your C++ behavior).
        outputs.push(OutputMessage::cancel_ack(
            user_id,
            user_order_id,
            self.symbol.clone(),
        ));

        // If we actually removed something, TOB may have changed.
        if found {
            let tob_outputs = self.check_top_of_book_changes();
            outputs.extend(tob_outputs);
        }

        outputs
    }

    /// Flush/clear the entire order book.
    pub fn flush(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.prev_best_bid_price = 0;
        self.prev_best_bid_qty = 0;
        self.prev_best_ask_price = 0;
        self.prev_best_ask_qty = 0;
    }

    /// Get best bid price (0 if none).
    pub fn best_bid_price(&self) -> u32 {
        self.bids
            .keys()
            .next_back()
            .copied()
            .unwrap_or(0)
    }

    /// Get best ask price (0 if none).
    pub fn best_ask_price(&self) -> u32 {
        self.asks
            .keys()
            .next()
            .copied()
            .unwrap_or(0)
    }

    /// Get total quantity at best bid (0 if none).
    pub fn best_bid_quantity(&self) -> u32 {
        match self.bids.keys().next_back().copied() {
            Some(price) => {
                if let Some(orders) = self.bids.get(&price) {
                    Self::total_quantity_at_price(orders)
                } else {
                    0
                }
            }
            None => 0,
        }
    }

    /// Get total quantity at best ask (0 if none).
    pub fn best_ask_quantity(&self) -> u32 {
        match self.asks.keys().next().copied() {
            Some(price) => {
                if let Some(orders) = self.asks.get(&price) {
                    Self::total_quantity_at_price(orders)
                } else {
                    0
                }
            }
            None => 0,
        }
    }

    /// Return a simple snapshot of the current top-of-book.
    pub fn top_of_book_snapshot(&self) -> TopOfBookSnapshot {
        TopOfBookSnapshot::new(
            self.best_bid_price(),
            self.best_bid_quantity(),
            self.best_ask_price(),
            self.best_ask_quantity(),
        )
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /// Match an incoming active order against the opposite side of the book.
    ///
    /// Fills generate Trade events. Any remaining quantity is left in the
    /// `order` object for the caller to potentially add to the book.
    fn match_order(&mut self, order: &mut Order) -> Vec<OutputMessage> {
        let mut outputs = Vec::new();

        match order.side {
            Side::Buy => {
                // Buy order: match against asks (ascending price).
                loop {
                    if order.remaining_qty == 0 || self.asks.is_empty() {
                        break;
                    }

                    // Clone key to satisfy borrow rules.
                    let best_ask_price_opt = self.asks.keys().next().copied();
                    let best_ask_price = match best_ask_price_opt {
                        Some(p) => p,
                        None => break,
                    };

                    // Can we match?
                    let can_match = match order.order_type {
                        OrderType::Market => true,
                        OrderType::Limit => order.price >= best_ask_price,
                    };
                    if !can_match {
                        break;
                    }

                    // Match against all orders at this price level FIFO.
                    if let Some(ask_orders) = self.asks.get_mut(&best_ask_price) {
                        while order.remaining_qty > 0 && !ask_orders.is_empty() {
                            // Safe: we only hold a mutable borrow of ask_orders.
                            if let Some(passive_order) = ask_orders.front_mut() {
                                let trade_qty = order.remaining_qty.min(passive_order.remaining_qty);

                                // Trade price is passive (best_ask_price).
                                outputs.push(OutputMessage::trade(
                                    self.symbol.clone(),
                                    order.user_id,
                                    order.user_order_id,
                                    passive_order.user_id,
                                    passive_order.user_order_id,
                                    best_ask_price,
                                    trade_qty,
                                ));

                                order.fill(trade_qty);
                                passive_order.fill(trade_qty);

                                if passive_order.is_filled() {
                                    ask_orders.pop_front();
                                }
                            } else {
                                break;
                            }
                        }
                    }

                    // Remove empty price level.
                    if let Some(ask_orders) = self.asks.get(&best_ask_price) {
                        if ask_orders.is_empty() {
                            self.asks.remove(&best_ask_price);
                        }
                    }
                }
            }
            Side::Sell => {
                // Sell order: match against bids (descending price).
                loop {
                    if order.remaining_qty == 0 || self.bids.is_empty() {
                        break;
                    }

                    // Best bid is the highest key.
                    let best_bid_price_opt = self.bids.keys().next_back().copied();
                    let best_bid_price = match best_bid_price_opt {
                        Some(p) => p,
                        None => break,
                    };

                    let can_match = match order.order_type {
                        OrderType::Market => true,
                        OrderType::Limit => order.price <= best_bid_price,
                    };
                    if !can_match {
                        break;
                    }

                    if let Some(bid_orders) = self.bids.get_mut(&best_bid_price) {
                        while order.remaining_qty > 0 && !bid_orders.is_empty() {
                            if let Some(passive_order) = bid_orders.front_mut() {
                                let trade_qty = order.remaining_qty.min(passive_order.remaining_qty);

                                // Trade price is passive (best_bid_price).
                                outputs.push(OutputMessage::trade(
                                    self.symbol.clone(),
                                    passive_order.user_id,
                                    passive_order.user_order_id,
                                    order.user_id,
                                    order.user_order_id,
                                    best_bid_price,
                                    trade_qty,
                                ));

                                order.fill(trade_qty);
                                passive_order.fill(trade_qty);

                                if passive_order.is_filled() {
                                    bid_orders.pop_front();
                                }
                            } else {
                                break;
                            }
                        }
                    }

                    if let Some(bid_orders) = self.bids.get(&best_bid_price) {
                        if bid_orders.is_empty() {
                            self.bids.remove(&best_bid_price);
                        }
                    }
                }
            }
        }

        outputs
    }

    /// Add a remaining limit order to the appropriate side of the book.
    fn add_to_book(&mut self, order: Order) {
        match order.side {
            Side::Buy => {
                let entry = self.bids.entry(order.price).or_insert_with(VecDeque::new);
                entry.push_back(order);
            }
            Side::Sell => {
                let entry = self.asks.entry(order.price).or_insert_with(VecDeque::new);
                entry.push_back(order);
            }
        }
    }

    /// Check for top-of-book changes and emit appropriate events.
    fn check_top_of_book_changes(&mut self) -> Vec<OutputMessage> {
        let mut outputs = Vec::new();

        let current_best_bid_price = self.best_bid_price();
        let current_best_bid_qty = self.best_bid_quantity();
        let current_best_ask_price = self.best_ask_price();
        let current_best_ask_qty = self.best_ask_quantity();

        // Bid side changes.
        if current_best_bid_price != self.prev_best_bid_price
            || current_best_bid_qty != self.prev_best_bid_qty
        {
            if current_best_bid_price == 0 {
                outputs.push(OutputMessage::top_of_book_eliminated(
                    self.symbol.clone(),
                    Side::Buy,
                ));
            } else {
                outputs.push(OutputMessage::top_of_book(
                    self.symbol.clone(),
                    Side::Buy,
                    current_best_bid_price,
                    current_best_bid_qty,
                ));
            }

            self.prev_best_bid_price = current_best_bid_price;
            self.prev_best_bid_qty = current_best_bid_qty;
        }

        // Ask side changes.
        if current_best_ask_price != self.prev_best_ask_price
            || current_best_ask_qty != self.prev_best_ask_qty
        {
            if current_best_ask_price == 0 {
                outputs.push(OutputMessage::top_of_book_eliminated(
                    self.symbol.clone(),
                    Side::Sell,
                ));
            } else {
                outputs.push(OutputMessage::top_of_book(
                    self.symbol.clone(),
                    Side::Sell,
                    current_best_ask_price,
                    current_best_ask_qty,
                ));
            }

            self.prev_best_ask_price = current_best_ask_price;
            self.prev_best_ask_qty = current_best_ask_qty;
        }

        outputs
    }

    /// Sum of remaining_qty across all orders at one price level.
    fn total_quantity_at_price(orders: &VecDeque<Order>) -> u32 {
        orders.iter().map(|o| o.remaining_qty).sum()
    }
}

