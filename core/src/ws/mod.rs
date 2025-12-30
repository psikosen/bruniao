//! WebSocket module for real-time orderbook updates

use crate::models::WsMessage;
use crate::orderbook::OrderBookManager;
use anyhow::Result;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::sync::Arc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

/// WebSocket manager for Polymarket CLOB
pub struct WebSocketManager {
    url: String,
    orderbook_manager: Arc<OrderBookManager>,
}

impl WebSocketManager {
    pub fn new(url: &str, orderbook_manager: Arc<OrderBookManager>) -> Self {
        Self {
            url: url.to_string(),
            orderbook_manager,
        }
    }

    /// Run the WebSocket connection loop
    pub async fn run(&self) -> Result<()> {
        loop {
            match self.connect_and_process().await {
                Ok(_) => {
                    info!("WebSocket connection closed normally");
                }
                Err(e) => {
                    error!("WebSocket error: {:?}", e);
                }
            }

            // Reconnect after delay
            warn!("Reconnecting WebSocket in 5 seconds...");
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    }

    async fn connect_and_process(&self) -> Result<()> {
        info!("Connecting to WebSocket: {}", self.url);

        let (ws_stream, _) = connect_async(&self.url).await?;
        let (mut write, mut read) = ws_stream.split();

        info!("WebSocket connected successfully");

        // Subscribe to markets
        let tracked_tokens = self.orderbook_manager.get_tracked_tokens();
        if !tracked_tokens.is_empty() {
            let sub_msg = SubscribeMessage {
                msg_type: "subscribe".to_string(),
                channel: "book".to_string(),
                assets: tracked_tokens,
            };
            let msg_text = serde_json::to_string(&sub_msg)?;
            write.send(Message::Text(msg_text)).await?;
            debug!("Sent subscription message");
        }

        // Heartbeat task
        let heartbeat_interval = tokio::time::Duration::from_secs(30);
        let mut heartbeat = tokio::time::interval(heartbeat_interval);

        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            self.handle_message(&text).await;
                        }
                        Some(Ok(Message::Ping(data))) => {
                            write.send(Message::Pong(data)).await?;
                        }
                        Some(Ok(Message::Close(_))) => {
                            info!("Received close frame");
                            break;
                        }
                        Some(Err(e)) => {
                            error!("WebSocket read error: {:?}", e);
                            break;
                        }
                        None => {
                            info!("WebSocket stream ended");
                            break;
                        }
                        _ => {}
                    }
                }
                _ = heartbeat.tick() => {
                    write.send(Message::Ping(vec![])).await?;
                }
            }
        }

        Ok(())
    }

    async fn handle_message(&self, text: &str) {
        match serde_json::from_str::<WsMessage>(text) {
            Ok(WsMessage::BookSnapshot(snapshot)) => {
                debug!("Received book snapshot for {}", snapshot.asset_id);
                self.orderbook_manager.apply_snapshot(
                    &snapshot.asset_id,
                    snapshot.bids,
                    snapshot.asks,
                );
            }
            Ok(WsMessage::BookDelta(delta)) => {
                debug!("Received book delta for {}", delta.asset_id);
                self.orderbook_manager.apply_delta(
                    &delta.asset_id,
                    delta.bids,
                    delta.asks,
                );
            }
            Ok(WsMessage::Trade(trade)) => {
                debug!(
                    "Trade: {} {} @ {} size {}",
                    trade.asset_id, trade.side, trade.price, trade.size
                );
            }
            Ok(WsMessage::Error { message }) => {
                error!("WebSocket error message: {}", message);
            }
            Err(e) => {
                warn!("Failed to parse WebSocket message: {:?}", e);
                debug!("Raw message: {}", text);
            }
        }
    }

    /// Subscribe to additional markets
    pub async fn subscribe(&self, token_ids: Vec<String>) -> Result<()> {
        // This would need access to the write half of the WebSocket
        // In a real implementation, you'd use a channel to send subscribe requests
        info!("Subscribe request for {:?}", token_ids);
        Ok(())
    }
}

#[derive(Serialize)]
struct SubscribeMessage {
    #[serde(rename = "type")]
    msg_type: String,
    channel: String,
    assets: Vec<String>,
}
