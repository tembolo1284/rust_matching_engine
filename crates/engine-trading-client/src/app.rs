//! Application state and logic.

use chrono::Local;
use engine_core::{Cancel, InputMessage, NewOrder, OutputMessage, Side, Symbol};
use indexmap::IndexMap;
use std::collections::VecDeque;
use tokio::sync::mpsc::Sender;

use crate::network::NetworkEvent;
use crate::types::{Order, OrderBookState, OrderStatus, Position, Protocol, Trade, Transport};

/// Input mode for the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Editing,
}

/// Active panel in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    OrderBook,
    Orders,
    Trades,
    OrderEntry,
    LoadTest,
}

/// Main application state.
pub struct App {
    // Connection state
    pub connected: bool,
    pub user_id: u32,
    pub transport: Transport,
    pub protocol: Protocol,
    pub server_addr: String,

    // UI state
    pub input_mode: InputMode,
    pub current_panel: Panel,
    pub should_quit: bool,
    pub show_help: bool,
    pub show_chart: bool,
    pub show_depth: bool,
    pub status_message: Option<String>,

    // Trading state
    pub current_symbol: Symbol,
    pub order_books: IndexMap<Symbol, OrderBookState>,
    pub my_orders: IndexMap<u32, Order>,
    pub recent_trades: VecDeque<Trade>,
    pub positions: IndexMap<Symbol, Position>,

    // Order entry
    pub order_side: Option<Side>,
    pub order_price_input: String,
    pub order_qty_input: String,
    pub is_market_order: bool,

    // Selection state
    pub selected_order_index: usize,
    pub selected_bid_index: usize,
    pub selected_ask_index: usize,
    pub selected_scenario_index: usize,

    // Input buffer
    pub input_buffer: String,
    pub input_cursor: usize,

    // Statistics
    pub total_trades: u64,
    pub total_volume: u64,
    pub message_count: u64,

    // Order ID counter
    pub next_order_id: u32,

    // Network sender
    pub msg_tx: Option<Sender<InputMessage>>,

    // Latency tracking
    pub last_latency_us: u64,
    pub avg_latency_us: f64,
    pub latency_samples: u64,
}

impl App {
    pub fn new(user_id: u32, symbol: &str) -> Self {
        let current_symbol = Symbol::from_str(symbol);
        let mut order_books = IndexMap::new();
        order_books.insert(current_symbol, OrderBookState::default());

        Self {
            connected: false,
            user_id,
            transport: Transport::Tcp,
            protocol: Protocol::Csv,
            server_addr: String::new(),

            input_mode: InputMode::Normal,
            current_panel: Panel::OrderBook,
            should_quit: false,
            show_help: false,
            show_chart: false,
            show_depth: true,
            status_message: None,

            current_symbol,
            order_books,
            my_orders: IndexMap::new(),
            recent_trades: VecDeque::with_capacity(100),
            positions: IndexMap::new(),

            order_side: None,
            order_price_input: String::new(),
            order_qty_input: String::new(),
            is_market_order: false,

            selected_order_index: 0,
            selected_bid_index: 0,
            selected_ask_index: 0,
            selected_scenario_index: 0,

            input_buffer: String::new(),
            input_cursor: 0,

            total_trades: 0,
            total_volume: 0,
            message_count: 0,

            next_order_id: 1000,

            msg_tx: None,

            last_latency_us: 0,
            avg_latency_us: 0.0,
            latency_samples: 0,
        }
    }

    pub fn set_connection_info(&mut self, addr: &str, transport: Transport, protocol: Protocol) {
        self.server_addr = addr.to_string();
        self.transport = transport;
        self.protocol = protocol;
    }

    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
        self.status_message = Some(if connected {
            format!("Connected to {} via {} ({})", self.server_addr, self.transport, self.protocol)
        } else {
            "Disconnected".to_string()
        });
    }

    pub fn set_msg_sender(&mut self, tx: Sender<InputMessage>) {
        self.msg_tx = Some(tx);
    }

    pub fn next_panel(&mut self) {
        self.current_panel = match self.current_panel {
            Panel::OrderBook => Panel::Orders,
            Panel::Orders => Panel::Trades,
            Panel::Trades => Panel::LoadTest,
            Panel::LoadTest => Panel::OrderEntry,
            Panel::OrderEntry => Panel::OrderBook,
        };
    }

    pub fn prev_panel(&mut self) {
        self.current_panel = match self.current_panel {
            Panel::OrderBook => Panel::OrderEntry,
            Panel::Orders => Panel::OrderBook,
            Panel::Trades => Panel::Orders,
            Panel::LoadTest => Panel::Trades,
            Panel::OrderEntry => Panel::LoadTest,
        };
    }

    pub fn toggle_market_order(&mut self) {
        self.is_market_order = !self.is_market_order;
    }

    pub fn get_next_order_id(&mut self) -> u32 {
        let id = self.next_order_id;
        self.next_order_id += 1;
        id
    }

    pub fn start_order_entry(&mut self, side: Side) {
        self.order_side = Some(side);
        self.current_panel = Panel::OrderEntry;
        self.input_mode = InputMode::Editing;
        self.input_buffer.clear();
        self.order_price_input.clear();
        self.order_qty_input.clear();
        self.input_cursor = 0;
    }

    pub fn submit_order(&mut self) {
        let Some(side) = self.order_side else { return };

        let quantity: u32 = self.input_buffer.parse().unwrap_or(0);
        if quantity == 0 {
            self.status_message = Some("Invalid quantity".to_string());
            return;
        }

        let price = if self.is_market_order {
            0
        } else {
            (self.order_price_input.parse::<f64>().unwrap_or(100.0) * 100.0) as u32
        };

        let order_id = self.get_next_order_id();

        let new_order = NewOrder::new(
            self.user_id,
            order_id,
            self.current_symbol,
            price,
            quantity,
            side,
        );

        let order = Order {
            order_id,
            symbol: self.current_symbol,
            side,
            price,
            quantity,
            filled_qty: 0,
            status: OrderStatus::Pending,
            timestamp: Local::now(),
        };
        self.my_orders.insert(order_id, order);

        if let Some(ref tx) = self.msg_tx {
            let _ = tx.try_send(InputMessage::NewOrder(new_order));
        }

        self.input_buffer.clear();
        self.order_price_input.clear();
        self.order_qty_input.clear();
        self.order_side = None;
        self.input_mode = InputMode::Normal;
        self.current_panel = Panel::Orders;
        self.status_message = Some(format!("Order {} submitted", order_id));
    }

    pub fn cancel_selected_order(&mut self) {
        if let Some(order) = self.my_orders.values().nth(self.selected_order_index) {
            let cancel = Cancel::new(self.user_id, order.order_id);

            if let Some(ref tx) = self.msg_tx {
                let _ = tx.try_send(InputMessage::Cancel(cancel));
            }

            self.status_message = Some(format!("Cancel sent for order {}", order.order_id));
        }
    }

    pub fn cancel_all_orders(&mut self) {
        let mut count = 0;
        for order in self.my_orders.values() {
            if order.status == OrderStatus::Open || order.status == OrderStatus::PartiallyFilled {
                let cancel = Cancel::new(self.user_id, order.order_id);
                if let Some(ref tx) = self.msg_tx {
                    let _ = tx.try_send(InputMessage::Cancel(cancel));
                }
                count += 1;
            }
        }
        self.status_message = Some(format!("Sent {} cancel requests", count));
    }

    pub fn move_selection_up(&mut self) {
        match self.current_panel {
            Panel::Orders => {
                self.selected_order_index = self.selected_order_index.saturating_sub(1);
            }
            Panel::OrderBook => {
                self.selected_bid_index = self.selected_bid_index.saturating_sub(1);
            }
            Panel::LoadTest => {
                self.selected_scenario_index = self.selected_scenario_index.saturating_sub(1);
            }
            _ => {}
        }
    }

    pub fn move_selection_down(&mut self) {
        match self.current_panel {
            Panel::Orders => {
                let max = self.my_orders.len().saturating_sub(1);
                self.selected_order_index = (self.selected_order_index + 1).min(max);
            }
            Panel::OrderBook => {
                let book = self.order_books.get(&self.current_symbol);
                if let Some(book) = book {
                    let max = book.bids.len().saturating_sub(1);
                    self.selected_bid_index = (self.selected_bid_index + 1).min(max);
                }
            }
            Panel::LoadTest => {
                let max = 8; // Number of preset scenarios - 1
                self.selected_scenario_index = (self.selected_scenario_index + 1).min(max);
            }
            _ => {}
        }
    }

    pub fn move_selection_left(&mut self) {}
    pub fn move_selection_right(&mut self) {}

    pub fn start_symbol_search(&mut self) {
        self.input_mode = InputMode::Editing;
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    pub fn toggle_chart(&mut self) {
        self.show_chart = !self.show_chart;
    }

    pub fn toggle_depth(&mut self) {
        self.show_depth = !self.show_depth;
    }

    pub fn cancel_input(&mut self) {
        self.input_buffer.clear();
        self.input_cursor = 0;
        self.input_mode = InputMode::Normal;
        self.order_side = None;
    }

    pub fn enter_char(&mut self, c: char) {
        self.input_buffer.insert(self.input_cursor, c);
        self.input_cursor += 1;
    }

    pub fn delete_char(&mut self) {
        if self.input_cursor > 0 {
            self.input_cursor -= 1;
            self.input_buffer.remove(self.input_cursor);
        }
    }

    pub fn submit_input(&mut self) {
        if self.order_side.is_some() {
            self.submit_order();
        } else {
            // Symbol search
            if !self.input_buffer.is_empty() {
                let new_symbol = Symbol::from_str(&self.input_buffer.to_uppercase());
                self.current_symbol = new_symbol;
                self.order_books
                    .entry(new_symbol)
                    .or_insert_with(OrderBookState::default);
                self.status_message = Some(format!("Switched to {}", new_symbol));
            }
            self.input_buffer.clear();
            self.input_mode = InputMode::Normal;
        }
    }

    pub fn handle_network_event(&mut self, event: NetworkEvent) {
        match event {
            NetworkEvent::Connected => {
                self.set_connected(true);
            }
            NetworkEvent::Disconnected => {
                self.set_connected(false);
            }
            NetworkEvent::Message(msg) => {
                self.handle_engine_message(msg);
            }
            NetworkEvent::Error(e) => {
                self.status_message = Some(format!("Error: {}", e));
            }
            NetworkEvent::LatencySample { latency_us, .. } => {
                self.last_latency_us = latency_us;
                self.latency_samples += 1;
                // Running average
                self.avg_latency_us = self.avg_latency_us
                    + (latency_us as f64 - self.avg_latency_us) / self.latency_samples as f64;
            }
        }
    }

    pub fn handle_engine_message(&mut self, msg: OutputMessage) {
        self.message_count += 1;

        match msg {
            OutputMessage::Ack(ack) => {
                if ack.user_id == self.user_id {
                    if let Some(order) = self.my_orders.get_mut(&ack.user_order_id) {
                        order.status = OrderStatus::Open;
                    }
                }
            }
            OutputMessage::Trade(trade) => {
                self.total_trades += 1;
                self.total_volume += trade.quantity as u64;

                // Update our orders if involved
                if trade.user_id_buy == self.user_id {
                    if let Some(order) = self.my_orders.get_mut(&trade.user_order_id_buy) {
                        order.filled_qty += trade.quantity;
                        order.status = if order.filled_qty >= order.quantity {
                            OrderStatus::Filled
                        } else {
                            OrderStatus::PartiallyFilled
                        };
                    }

                    self.recent_trades.push_front(Trade {
                        symbol: trade.symbol,
                        price: trade.price,
                        quantity: trade.quantity,
                        side: Side::Buy,
                        timestamp: Local::now(),
                    });

                    self.update_position(trade.symbol, trade.quantity as i64, trade.price);
                }

                if trade.user_id_sell == self.user_id {
                    if let Some(order) = self.my_orders.get_mut(&trade.user_order_id_sell) {
                        order.filled_qty += trade.quantity;
                        order.status = if order.filled_qty >= order.quantity {
                            OrderStatus::Filled
                        } else {
                            OrderStatus::PartiallyFilled
                        };
                    }

                    self.recent_trades.push_front(Trade {
                        symbol: trade.symbol,
                        price: trade.price,
                        quantity: trade.quantity,
                        side: Side::Sell,
                        timestamp: Local::now(),
                    });

                    self.update_position(trade.symbol, -(trade.quantity as i64), trade.price);
                }

                // Limit recent trades
                while self.recent_trades.len() > 100 {
                    self.recent_trades.pop_back();
                }
            }
            OutputMessage::CancelAck(cancel) => {
                if cancel.user_id == self.user_id {
                    if let Some(order) = self.my_orders.get_mut(&cancel.user_order_id) {
                        order.status = OrderStatus::Cancelled;
                    }
                }
            }
            OutputMessage::TopOfBook(tob) => {
                let book = self
                    .order_books
                    .entry(tob.symbol)
                    .or_insert_with(OrderBookState::default);

                if !tob.eliminated {
                    match tob.side {
                        Side::Buy => {
                            if book.bids.is_empty() {
                                book.bids.push((tob.price, tob.total_quantity));
                            } else {
                                book.bids[0] = (tob.price, tob.total_quantity);
                            }
                        }
                        Side::Sell => {
                            if book.asks.is_empty() {
                                book.asks.push((tob.price, tob.total_quantity));
                            } else {
                                book.asks[0] = (tob.price, tob.total_quantity);
                            }
                        }
                    }
                } else {
                    match tob.side {
                        Side::Buy => book.bids.clear(),
                        Side::Sell => book.asks.clear(),
                    }
                }

                book.last_update = Some(Local::now());
            }
        }
    }

    fn update_position(&mut self, symbol: Symbol, qty_delta: i64, price: u32) {
        let pos = self.positions.entry(symbol).or_insert_with(|| Position {
            symbol,
            ..Default::default()
        });

        let old_qty = pos.quantity;
        pos.quantity += qty_delta;

        // Simple average price calculation
        if (old_qty >= 0 && qty_delta > 0) || (old_qty <= 0 && qty_delta < 0) {
            // Adding to position
            let total_cost = pos.avg_price * old_qty.abs() as f64 + price as f64 * qty_delta.abs() as f64;
            pos.avg_price = total_cost / pos.quantity.abs() as f64;
        } else {
            // Reducing position - realize P&L
            let realized = (price as f64 - pos.avg_price) * qty_delta.abs().min(old_qty.abs()) as f64;
            pos.realized_pnl += if old_qty > 0 { realized } else { -realized };
        }
    }
}
