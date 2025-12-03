//! Network connection handler with multi-protocol support.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Result};
use bytes::BytesMut;
use engine_core::{InputMessage, OutputMessage, Symbol};
use engine_protocol::{binary_codec, fix_codec};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

use crate::types::{Protocol, Transport};

/// Messages from network to app.
#[derive(Debug)]
pub enum NetworkEvent {
    Connected,
    Disconnected,
    Message(OutputMessage),
    Error(String),
    LatencySample { _order_id: u32, latency_us: u64 },
}

/// Engine connection with auto-protocol support.
pub struct EngineConnection {
    server_addr: String,
    transport: Transport,
    protocol: Protocol,
    tcp_stream: Option<TcpStream>,
    udp_socket: Option<UdpSocket>,
    event_tx: Sender<NetworkEvent>,
    _read_buffer: BytesMut,
    write_buffer: BytesMut,
    pending_orders: Arc<RwLock<HashMap<u32, Instant>>>,
}

impl EngineConnection {
    pub fn new(
        server_addr: &str,
        transport: Transport,
        protocol: Protocol,
        event_tx: Sender<NetworkEvent>,
    ) -> Self {
        Self {
            server_addr: server_addr.to_string(),
            transport,
            protocol,
            tcp_stream: None,
            udp_socket: None,
            event_tx,
            _read_buffer: BytesMut::with_capacity(65536),
            write_buffer: BytesMut::with_capacity(65536),
            pending_orders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        info!("Connecting to {} via {:?}...", self.server_addr, self.transport);

        match self.transport {
            Transport::Tcp => {
                let stream = TcpStream::connect(&self.server_addr).await?;
                stream.set_nodelay(true)?;
                self.tcp_stream = Some(stream);
            }
            Transport::Udp => {
                let socket = UdpSocket::bind("0.0.0.0:0").await?;
                socket.connect(&self.server_addr).await?;
                self.udp_socket = Some(socket);
            }
        }

        info!("Connected successfully using {:?} protocol", self.protocol);
        let _ = self.event_tx.send(NetworkEvent::Connected).await;
        Ok(())
    }

    pub async fn send(&mut self, msg: InputMessage) -> Result<()> {
        // Track order for latency measurement
        if let InputMessage::NewOrder(ref order) = msg {
            let mut pending = self.pending_orders.write().await;
            pending.insert(order.user_order_id, Instant::now());
        }

        match self.transport {
            Transport::Tcp => self.send_tcp(&msg).await,
            Transport::Udp => self.send_udp(&msg).await,
        }
    }

    async fn send_tcp(&mut self, msg: &InputMessage) -> Result<()> {
        let stream = self.tcp_stream.as_mut().ok_or_else(|| anyhow!("Not connected"))?;

        self.write_buffer.clear();

        match self.protocol {
            Protocol::Csv => {
                let line = format_input_csv(msg);
                let data = format!("{}\n", line);
                stream.write_all(data.as_bytes()).await?;
            }
            Protocol::Binary => {
                let mut encoder = binary_codec::BinaryEncoder::new();
                let frame = encoder.encode_input(msg)?;
                // Length prefix + frame
                let len = (frame.len() as u32).to_be_bytes();
                stream.write_all(&len).await?;
                stream.write_all(frame).await?;
            }
            Protocol::Fix => {
                let mut encoder = fix_codec::FixEncoder::new(
                    fix_codec::FixVersion::Fix44,
                    "CLIENT",
                    "ENGINE",
                );
                let frame = encoder.encode_input(msg)?;
                stream.write_all(&frame).await?;
            }
        }

        stream.flush().await?;
        debug!("Sent: {:?}", msg);
        Ok(())
    }

    async fn send_udp(&mut self, msg: &InputMessage) -> Result<()> {
        let socket = self.udp_socket.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let data = match self.protocol {
            Protocol::Csv => {
                let line = format_input_csv(msg);
                format!("{}\n", line).into_bytes()
            }
            Protocol::Binary => {
                let mut encoder = binary_codec::BinaryEncoder::new();
                let frame = encoder.encode_input(msg)?;
                let mut data = Vec::with_capacity(4 + frame.len());
                data.extend_from_slice(&(frame.len() as u32).to_be_bytes());
                data.extend_from_slice(frame);
                data
            }
            Protocol::Fix => {
                return Err(anyhow!("FIX not supported over UDP"));
            }
        };

        socket.send(&data).await?;
        debug!("Sent UDP: {:?}", msg);
        Ok(())
    }

    pub async fn run(&mut self, mut msg_rx: Receiver<InputMessage>) {
        let mut heartbeat = interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    // Send heartbeat (query for non-existent symbol)
                    let query = InputMessage::QueryTopOfBook(
                        engine_core::TopOfBookQuery::new(Symbol::from_str("PING"))
                    );
                    if let Err(e) = self.send(query).await {
                        warn!("Heartbeat failed: {}", e);
                        self.handle_disconnect().await;
                    }
                }

                Some(msg) = msg_rx.recv() => {
                    if let Err(e) = self.send(msg).await {
                        error!("Send failed: {}", e);
                        self.handle_disconnect().await;
                    }
                }

                result = self.recv_message() => {
                    match result {
                        Ok(Some(msg)) => {
                            // Calculate latency if this is an ack
                            if let OutputMessage::Ack(ref ack) = msg {
                                let mut pending = self.pending_orders.write().await;
                                if let Some(sent_time) = pending.remove(&ack.user_order_id) {
                                    let latency_us = sent_time.elapsed().as_micros() as u64;
                                    let _ = self.event_tx.send(NetworkEvent::LatencySample {
                                        _order_id: ack.user_order_id,
                                        latency_us,
                                    }).await;
                                }
                            }

                            let _ = self.event_tx.send(NetworkEvent::Message(msg)).await;
                        }
                        Ok(None) => {
                            self.handle_disconnect().await;
                        }
                        Err(e) => {
                            error!("Recv error: {}", e);
                            let _ = self.event_tx.send(NetworkEvent::Error(e.to_string())).await;
                        }
                    }
                }
            }
        }
    }

    async fn recv_message(&mut self) -> Result<Option<OutputMessage>> {
        match self.transport {
            Transport::Tcp => self.recv_tcp().await,
            Transport::Udp => self.recv_udp().await,
        }
    }

    async fn recv_tcp(&mut self) -> Result<Option<OutputMessage>> {
        let stream = self.tcp_stream.as_mut().ok_or_else(|| anyhow!("Not connected"))?;

        match self.protocol {
            Protocol::Csv => {
                // Read line
                let mut line = String::new();
                let mut reader = BufReader::new(stream);
                let n = reader.read_line(&mut line).await?;
                if n == 0 {
                    return Ok(None);
                }
                // Parse CSV - we need to handle output format
                Ok(parse_output_csv(&line))
            }
            Protocol::Binary => {
                // Read length prefix
                let mut len_buf = [0u8; 4];
                stream.read_exact(&mut len_buf).await?;
                let len = u32::from_be_bytes(len_buf) as usize;

                // Read frame
                let mut frame = vec![0u8; len];
                stream.read_exact(&mut frame).await?;

                let msg = binary_codec::decode_output(&frame)?;
                Ok(Some(msg))
            }
            Protocol::Fix => {
                // Read FIX message (simplified)
                let mut buf = Vec::with_capacity(4096);
                let mut temp = [0u8; 1];
                
                loop {
                    stream.read_exact(&mut temp).await?;
                    buf.push(temp[0]);
                    
                    // Check for complete message (ends with 10=XXX|)
                    if buf.len() >= 7 && buf.ends_with(&[0x01]) {
                        if let Some(pos) = buf.windows(3).rposition(|w| w == b"10=") {
                            if pos + 7 <= buf.len() {
                                break;
                            }
                        }
                    }
                    
                    if buf.len() > 8192 {
                        return Err(anyhow!("FIX message too large"));
                    }
                }

                let decoder = fix_codec::FixDecoder::new(fix_codec::FixVersion::Fix44);
                let msg = decoder.decode_output(&buf)?;
                Ok(Some(msg))
            }
        }
    }

    async fn recv_udp(&mut self) -> Result<Option<OutputMessage>> {
        let socket = self.udp_socket.as_ref().ok_or_else(|| anyhow!("Not connected"))?;

        let mut buf = [0u8; 65536];
        let n = socket.recv(&mut buf).await?;
        if n == 0 {
            return Ok(None);
        }

        let data = &buf[..n];

        match self.protocol {
            Protocol::Csv => {
                let line = String::from_utf8_lossy(data);
                Ok(parse_output_csv(&line))
            }
            Protocol::Binary => {
                // Skip length prefix if present
                let frame = if data.len() > 4 && &data[4..8] == b"MENG" {
                    &data[4..]
                } else {
                    data
                };
                let msg = binary_codec::decode_output(frame)?;
                Ok(Some(msg))
            }
            Protocol::Fix => {
                Err(anyhow!("FIX not supported over UDP"))
            }
        }
    }

    async fn handle_disconnect(&mut self) {
        warn!("Disconnected from server");
        let _ = self.event_tx.send(NetworkEvent::Disconnected).await;
        
        self.tcp_stream = None;
        self.udp_socket = None;

        // Attempt reconnection
        tokio::time::sleep(Duration::from_secs(1)).await;
        
        if let Err(e) = self.connect().await {
            error!("Reconnection failed: {}", e);
        }
    }
}

/// Format an input message as CSV.
fn format_input_csv(msg: &InputMessage) -> String {
    match msg {
        InputMessage::NewOrder(o) => {
            let side = match o.side {
                engine_core::Side::Buy => 'B',
                engine_core::Side::Sell => 'S',
            };
            format!(
                "N, {}, {}, {}, {}, {}, {}",
                o.user_id, o.symbol, o.price, o.quantity, side, o.user_order_id
            )
        }
        InputMessage::Cancel(c) => {
            format!("C, {}, {}", c.user_id, c.user_order_id)
        }
        InputMessage::Flush => "F".to_string(),
        InputMessage::QueryTopOfBook(q) => {
            format!("Q, {}", q.symbol)
        }
    }
}

/// Parse CSV output message.
fn parse_output_csv(line: &str) -> Option<OutputMessage> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    let parts: Vec<&str> = trimmed.split(',').map(|s| s.trim()).collect();
    if parts.is_empty() {
        return None;
    }

    match parts[0] {
        "A" if parts.len() >= 3 => {
            let user_id: u32 = parts[1].parse().ok()?;
            let user_order_id: u32 = parts[2].parse().ok()?;
            let symbol = if parts.len() >= 4 {
                Symbol::from_str(parts[3])
            } else {
                Symbol::from_str("")
            };
            Some(OutputMessage::ack(user_id, user_order_id, symbol))
        }
        "C" if parts.len() >= 3 => {
            let user_id: u32 = parts[1].parse().ok()?;
            let user_order_id: u32 = parts[2].parse().ok()?;
            let symbol = if parts.len() >= 4 {
                Symbol::from_str(parts[3])
            } else {
                Symbol::from_str("")
            };
            Some(OutputMessage::cancel_ack(user_id, user_order_id, symbol))
        }
        "T" if parts.len() >= 7 => {
            // T, symbol, buy_uid, buy_oid, sell_uid, sell_oid, price, qty
            let symbol = Symbol::from_str(parts[1]);
            let user_id_buy: u32 = parts[2].parse().ok()?;
            let user_order_id_buy: u32 = parts[3].parse().ok()?;
            let user_id_sell: u32 = parts[4].parse().ok()?;
            let user_order_id_sell: u32 = parts[5].parse().ok()?;
            let price: u32 = parts[6].parse().ok()?;
            let quantity: u32 = parts[7].parse().ok()?;
            Some(OutputMessage::trade(
                symbol,
                user_id_buy,
                user_order_id_buy,
                user_id_sell,
                user_order_id_sell,
                price,
                quantity,
            ))
        }
        "B" if parts.len() >= 4 => {
            // B, symbol, side, price, qty  OR  B, symbol, side, -, -
            let symbol = Symbol::from_str(parts[1]);
            let side = match parts[2] {
                "B" => engine_core::Side::Buy,
                "S" => engine_core::Side::Sell,
                _ => return None,
            };
            
            if parts[3] == "-" {
                Some(OutputMessage::top_of_book_eliminated(symbol, side))
            } else {
                let price: u32 = parts[3].parse().ok()?;
                let qty: u32 = parts[4].parse().ok()?;
                Some(OutputMessage::top_of_book(symbol, side, price, qty))
            }
        }
        _ => None,
    }
}
