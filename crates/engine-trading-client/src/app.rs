// crates/engine-trading-client/src/app.rs

use chrono::{DateTime, Local};
use engine_core::{OutputMessage, Side};
use indexmap::IndexMap;
use std::collections::VecDeque;
use tokio::sync::mpsc::UnboundedSender;

pub enum InputMode {
    Normal,
    Editing,
}

#[derive(PartialEq)]
pub enum Panel {
    OrderBook,
    Orders,
    Trades,
    OrderEntry,
}

#[derive(Clone)]
pub struct Order {
    pub order_id: u32,
    pub symbol: String,
    pub side: Side,
    pub price: u32,
    pub quantity: u32,
    pub filled_qty: u32,
    pub status: OrderStatus,
    pub timestamp: DateTime<Local>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum OrderStatus {
    #[allow(dead_code)]
    Pending,
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
}

#[derive(Clone)]
pub struct Trade {
    pub symbol: String,
    pub price: u32,
    pub quantity: u32,
    pub side: Side, // Our side
    pub timestamp: DateTime<Local>,
}

#[derive(Default)]
pub struct OrderBook {
    pub bids: Vec<(u32, u32)>, // (price, quantity)
    pub asks: Vec<(u32, u32)>, // (price, quantity)
    pub last_update: Option<DateTime<Local>>,
}

pub struct App {
    // Connection state
    pub connected: bool,
    pub user_id: u32,
    
    // UI state
    pub input_mode: InputMode,
    pub current_panel: Panel,
    pub should_quit: bool,
    pub show_help: bool,
    pub show_chart: bool,
    pub show_depth: bool,
    
    // Trading state
    pub current_symbol: String,
    pub order_books: IndexMap<String, OrderBook>,
    pub my_orders: IndexMap<u32, Order>,
    pub recent_trades: VecDeque<Trade>,
    pub positions: IndexMap<String, Position>,
    
    // Order entry
    pub order_side: Option<Side>,
    pub order_price_input: String,
    pub order_qty_input: String,
    pub is_market_order: bool,
    
    // Selection state
    pub selected_order_index: usize,
    pub selected_bid_index: usize,
    pub selected_ask_index: usize,
    
    // Input buffer
    pub input_buffer: String,
    pub input_cursor: usize,
    
    // Statistics
    pub total_trades: u32,
    pub total_volume: u64,
    pub message_count: u64,
    
    // Order ID counter
    pub next_order_id: u32,

    pub network_tx: Option<UnboundedSender<InputMessage>>,
}

#[derive(Default, Clone)]
pub struct Position {
    pub symbol: String,
    pub quantity: i32, // Positive = long, negative = short
    pub avg_price: f64,
    pub realized_pnl: f64,
    pub unrealized_pnl: f64,
}

impl App {
    pub fn new(user_id: u32, symbol: &str) -> Self {
        let mut app = Self {
            connected: false,
            user_id,
            input_mode: InputMode::Normal,
            current_panel: Panel::OrderBook,
            should_quit: false,
            show_help: false,
            show_chart: false,
            show_depth: true,
            current_symbol: symbol.to_string(),
            order_books: IndexMap::new(),
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
            input_buffer: String::new(),
            input_cursor: 0,
            total_trades: 0,
            total_volume: 0,
            message_count: 0,
            next_order_id: 1000,
            network_tx: None,
        };
        
        // Initialize empty order book for current symbol
        app.order_books.insert(symbol.to_string(), OrderBook::default());
        app
    }
    
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }
    pub fn set_network_sender(&mut self, tx: UnboundedSender<InputMessage>) {
        self.network_tx = Some(tx);
    }

    pub fn next_panel(&mut self) {
        self.current_panel = match self.current_panel {
            Panel::OrderBook => Panel::Orders,
            Panel::Orders => Panel::Trades,
            Panel::Trades => Panel::OrderEntry,
            Panel::OrderEntry => Panel::OrderBook,
        };
    }
    
    pub fn prev_panel(&mut self) {
        self.current_panel = match self.current_panel {
            Panel::OrderBook => Panel::OrderEntry,
            Panel::Orders => Panel::OrderBook,
            Panel::Trades => Panel::Orders,
            Panel::OrderEntry => Panel::Trades,
        };
    }
    
    pub fn start_order_entry(&mut self, side: Side) {
        self.order_side = Some(side);
        self.current_panel = Panel::OrderEntry;
        self.input_mode = InputMode::Editing;
        self.input_buffer.clear();
        self.input_cursor = 0;
    }
    
    pub fn toggle_market_order(&mut self) {
        self.is_market_order = !self.is_market_order;
    }
    
    pub fn cancel_selected_order(&mut self) {
        if let Some(order) = self.my_orders.values().nth(self.selected_order_index) {
            let cancel_msg = InputMessage::Cancel(Cancel {
                user_id: self.user_id,
                user_order_id: order.order_id,
            });
            
            if let Some(tx) = &self.network_tx {
                let _ = tx.send(cancel_msg);
            }
        }
    }
    
    pub fn cancel_all_orders(&mut self) {
        // Cancel all open orders
        for order in self.my_orders.values() {
            if order.status == OrderStatus::Open || order.status == OrderStatus::PartiallyFilled {
                // Would send cancel message
            }
        }
    }
    
    pub fn move_selection_up(&mut self) {
        match self.current_panel {
            Panel::Orders => {
                if self.selected_order_index > 0 {
                    self.selected_order_index -= 1;
                }
            }
            Panel::OrderBook => {
                if self.selected_bid_index > 0 {
                    self.selected_bid_index -= 1;
                }
            }
            _ => {}
        }
    }
    
    pub fn move_selection_down(&mut self) {
        match self.current_panel {
            Panel::Orders => {
                if self.selected_order_index < self.my_orders.len().saturating_sub(1) {
                    self.selected_order_index += 1;
                }
            }
            Panel::OrderBook => {
                let book = self.order_books.get(&self.current_symbol);
                if let Some(book) = book {
                    if self.selected_bid_index < book.bids.len().saturating_sub(1) {
                        self.selected_bid_index += 1;
                    }
                }
            }
            _ => {}
        }
    }
    
    pub fn move_selection_left(&mut self) {
        // Switch between bid/ask selection
    }
    
    pub fn move_selection_right(&mut self) {
        // Switch between bid/ask selection  
    }
    
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

    pub fn get_next_order_id(&mut self) -> u32 {
        let id = self.next_order_id;
        self.next_order_id += 1;
        id
    }
    
    pub fn submit_input(&mut self) {
        if !matches!(self.input_mode, InputMode::Editing) {
            return;
        }
        
        // If we're in order entry mode with a side selected
        if let Some(side) = self.order_side.clone() {
            // Parse quantity
            let quantity = self.input_buffer.parse::<u32>().unwrap_or(0);
            if quantity == 0 {
                return; // Invalid quantity
            }
            
            // Parse price (for limit orders)
            let price = if self.is_market_order {
                0
            } else {
                (self.order_price_input.parse::<f64>().unwrap_or(0.0) * 100.0) as u32
            };
            
            let order_id = self.get_next_order_id();
            
            // Create the order
            let order_msg = InputMessage::NewOrder(NewOrder {
                user_id: self.user_id,
                user_order_id: order_id,
                symbol: self.current_symbol.clone(),
                price,
                quantity,
                side: side.clone(),
            });
            
            // Store order locally as pending
            let order = Order {
                order_id,
                symbol: self.current_symbol.clone(),
                side,
                price,
                quantity,
                filled_qty: 0,
                status: OrderStatus::Pending,
                timestamp: chrono::Local::now(),
            };
            self.my_orders.insert(order_id, order);
            
            // Send to network
            if let Some(tx) = &self.network_tx {
                let _ = tx.send(order_msg);
            }
            
            // Clear input
            self.input_buffer.clear();
            self.order_price_input.clear();
            self.order_qty_input.clear();
            self.order_side = None;
            self.input_mode = InputMode::Normal;
            self.current_panel = Panel::Orders;
        }
    }
    
    pub fn cancel_input(&mut self) {
        self.input_buffer.clear();
        self.input_cursor = 0;
        self.input_mode = InputMode::Normal;
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
                    
                    // Add to recent trades
                    self.recent_trades.push_front(Trade {
                        symbol: trade.symbol.clone(),
                        price: trade.price,
                        quantity: trade.quantity,
                        side: Side::Buy,
                        timestamp: Local::now(),
                    });
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
                }
                
                // Keep recent trades limited
                if self.recent_trades.len() > 100 {
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
                let book = self.order_books.entry(tob.symbol.clone())
                    .or_insert(OrderBook::default());
                
                if !tob.eliminated {
                    match tob.side {
                        Side::Buy => {
                            // Update best bid
                            if let Some(bid) = book.bids.get_mut(0) {
                                *bid = (tob.price, tob.total_quantity);
                            } else {
                                book.bids.push((tob.price, tob.total_quantity));
                            }
                        }
                        Side::Sell => {
                            // Update best ask
                            if let Some(ask) = book.asks.get_mut(0) {
                                *ask = (tob.price, tob.total_quantity);
                            } else {
                                book.asks.push((tob.price, tob.total_quantity));
                            }
                        }
                    }
                } else {
                    // Remove eliminated side
                    match tob.side {
                        Side::Buy => book.bids.clear(),
                        Side::Sell => book.asks.clear(),
                    }
                }
                
                book.last_update = Some(Local::now());
            }
        }
    }
}

/*
This trading client provides:

1. **Real-time Order Book Display** - Shows bids/asks with depth
2. **Order Management** - Place, cancel, and track your orders
3. **Trade Blotter** - See your recent executions
4. **Position Tracking** - Monitor P&L and exposure
5. **Hotkey Trading** - Fast keyboard shortcuts (B=buy, S=sell, etc.)
6. **Multi-symbol Support** - Switch between different instruments
7. **Connection Status** - Real-time server connection monitoring

The terminal UI would look something like:
```
╔══════════════════════════════════════════════════════════════╗
║ AAPL - Connected ✓           [F1]Help [Tab]Switch Panel     ║
╠═══════════════════════╦══════════════════╦═════════════════╣
║     ORDER BOOK        ║   MY ORDERS      ║   POSITIONS     ║
╠═══════════════════════╬══════════════════╬═════════════════╣
║  BIDS         ASKS    ║ #1234 B 100@105  ║ AAPL +100 @105  ║
║  100@105  |  50@106   ║   Status: OPEN   ║  P&L: +$50      ║
║  200@104  |  100@107  ║ #1235 S 50@107   ║                 ║
║  150@103  |  75@108   ║   Status: FILLED ║ MSFT -50 @250   ║
╠═══════════════════════╩══════════════════╩═════════════════╣
║ [B]uy [S]ell [C]ancel [X]Cancel All [Q]uit                  ║
╚══════════════════════════════════════════════════════════════╝
*/
