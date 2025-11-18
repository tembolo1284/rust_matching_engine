// crates/engine-trading-client/src/network.rs

use anyhow::Result;
use bytes::{BufMut, BytesMut};
use engine_core::{InputMessage, OutputMessage};
use engine_protocol::binary_codec;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc::{UnboundedSender, UnboundedReceiver};
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

pub struct EngineConnection {
    server_addr: String,
    stream: Option<TcpStream>,
    read_buffer: BytesMut,
    write_buffer: BytesMut,
    tx: UnboundedSender<OutputMessage>,
    reconnect_attempts: u32,
}

impl EngineConnection {
    pub fn new(server_addr: &str, tx: UnboundedSender<OutputMessage>) -> Self {
        Self {
            server_addr: server_addr.to_string(),
            stream: None,
            read_buffer: BytesMut::with_capacity(65536),
            write_buffer: BytesMut::with_capacity(65536),
            tx,
            reconnect_attempts: 0,
        }
    }

    pub async fn connect(&mut self) -> Result<()> {
        info!("Connecting to {}...", self.server_addr);
        
        match TcpStream::connect(&self.server_addr).await {
            Ok(stream) => {
                stream.set_nodelay(true)?;
                self.stream = Some(stream);
                self.reconnect_attempts = 0;
                info!("Connected successfully");
                Ok(())
            }
            Err(e) => {
                error!("Connection failed: {}", e);
                Err(e.into())
            }
        }
    }

    pub async fn send(&mut self, msg: InputMessage) -> Result<()> {
        let stream = self.stream.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        // Encode message
        self.write_buffer.clear();
        let mut payload = Vec::new();
        binary_codec::encode_input(&msg, &mut payload)?;
        
        // Add length prefix
        let len = payload.len() as u32;
        self.write_buffer.put_u32_le(len);
        self.write_buffer.extend_from_slice(&payload);
        
        // Send
        stream.write_all(&self.write_buffer).await?;
        stream.flush().await?;
        
        debug!("Sent message: {:?}", msg);
        Ok(())
    }

    pub async fn run(&mut self, mut rx: mpsc::UnboundedReceiver<InputMessage>) {
        let mut heartbeat = interval(Duration::from_secs(30));
        
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if let Err(e) = self.send_heartbeat().await {
                        warn!("Heartbeat failed: {}", e);
                        self.handle_disconnect().await;
                    }
                }
                
                Some(msg) = rx.recv() => {
                    // Send messages from the app
                    if let Err(e) = self.send(msg).await {
                        error!("Failed to send message: {}", e);
                        self.handle_disconnect().await;
                    }
                }
                
                result = self.read_message() => {
                    match result {
                        Ok(Some(msg)) => {
                            debug!("Received from server: {:?}", msg);
                            if let Err(e) = self.tx.send(msg) {
                                error!("Failed to send message to app: {}", e);
                            }
                        }
                        Ok(None) => {
                            self.handle_disconnect().await;
                        }
                        Err(e) => {
                            error!("Read error: {}", e);
                            self.handle_disconnect().await;
                        }
                    }
                }
            }
        }
    }

    async fn read_message(&mut self) -> Result<Option<OutputMessage>> {
        let stream = self.stream.as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        // Read length prefix
        while self.read_buffer.len() < 4 {
            let mut buf = [0u8; 1024];
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                return Ok(None); // Connection closed
            }
            self.read_buffer.extend_from_slice(&buf[..n]);
        }

        let len = u32::from_le_bytes([
            self.read_buffer[0],
            self.read_buffer[1],
            self.read_buffer[2],
            self.read_buffer[3],
        ]) as usize;

        // Read message body
        while self.read_buffer.len() < 4 + len {
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).await?;
            if n == 0 {
                return Ok(None);
            }
            self.read_buffer.extend_from_slice(&buf[..n]);
        }

        // Extract and decode message
        let msg_bytes = self.read_buffer.split_to(4 + len);
        let msg = binary_codec::decode_output(&msg_bytes[4..])?;
        
        Ok(Some(msg))
    }

    async fn send_heartbeat(&mut self) -> Result<()> {
        // Send a query to keep connection alive
        use engine_core::TopOfBookQuery;
        let query = InputMessage::QueryTopOfBook(TopOfBookQuery {
            symbol: "HEARTBEAT".to_string(),
        });
        self.send(query).await
    }

    async fn handle_disconnect(&mut self) {
        warn!("Connection lost, attempting to reconnect...");
        self.stream = None;
        self.reconnect_attempts += 1;
        
        // Exponential backoff
        let delay = Duration::from_millis(1000 * (2_u64.pow(self.reconnect_attempts.min(5))));
        tokio::time::sleep(delay).await;
        
        if let Err(e) = self.connect().await {
            error!("Reconnection failed: {}", e);
        } else {
            info!("Reconnected successfully");
        }
    }
}
