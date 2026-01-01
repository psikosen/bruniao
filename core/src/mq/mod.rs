//! RabbitMQ Message Queue Module
//!
//! High-performance async message broker for:
//! - Trading signal distribution
//! - Order execution events
//! - Bot debate triggers
//! - Risk alerts
//! - Cross-service communication

use anyhow::{Context, Result};
use futures_util::StreamExt;
use lapin::{
    options::*,
    publisher_confirm::Confirmation,
    types::{AMQPValue, FieldTable, ShortString},
    BasicProperties, Channel, Connection, ConnectionProperties, Consumer,
};
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument, warn};

/// Exchange names
pub mod exchanges {
    pub const TRADING: &str = "trading";
    pub const SIGNALS: &str = "signals";
    pub const ORDERS: &str = "orders";
    pub const RISK: &str = "risk";
    pub const DEBATES: &str = "debates";
    pub const EVENTS: &str = "events";
}

/// Queue names
pub mod queues {
    pub const TRADING_DECISIONS: &str = "trading.decisions";
    pub const ORDER_EXECUTION: &str = "orders.execution";
    pub const ORDER_FILLS: &str = "orders.fills";
    pub const RISK_ALERTS: &str = "risk.alerts";
    pub const DEBATE_REQUESTS: &str = "debates.requests";
    pub const DEBATE_RESULTS: &str = "debates.results";
    pub const MARKET_UPDATES: &str = "market.updates";
    pub const STRATEGY_SIGNALS: &str = "strategy.signals";
}

/// Routing keys
pub mod routing {
    pub const DECISION_NEW: &str = "decision.new";
    pub const DECISION_EXECUTED: &str = "decision.executed";
    pub const ORDER_PLACED: &str = "order.placed";
    pub const ORDER_FILLED: &str = "order.filled";
    pub const ORDER_CANCELLED: &str = "order.cancelled";
    pub const RISK_WARNING: &str = "risk.warning";
    pub const RISK_CRITICAL: &str = "risk.critical";
    pub const RISK_KILL_SWITCH: &str = "risk.kill_switch";
    pub const DEBATE_START: &str = "debate.start";
    pub const DEBATE_MESSAGE: &str = "debate.message";
    pub const DEBATE_CONSENSUS: &str = "debate.consensus";
    pub const MARKET_ORDERBOOK: &str = "market.orderbook";
    pub const MARKET_TRADE: &str = "market.trade";
    pub const SIGNAL_MM: &str = "signal.market_making";
    pub const SIGNAL_ARB: &str = "signal.arbitrage";
}

/// Trading decision message
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct TradingDecisionMessage {
    pub decision_id: String,
    pub decision_type: String,  // "market_making" | "arbitrage"
    pub market_id: String,
    pub token_id: String,
    pub side: String,           // "buy" | "sell"
    pub price: String,
    pub size: String,
    pub order_type: String,     // "gtc" | "fok" | "ioc"
    pub confidence: f64,
    pub reason: String,
    pub timestamp: i64,
}

/// Order execution message
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct OrderExecutionMessage {
    pub order_id: String,
    pub decision_id: String,
    pub market_id: String,
    pub token_id: String,
    pub side: String,
    pub price: String,
    pub size: String,
    pub status: String,         // "pending" | "open" | "filled" | "cancelled" | "failed"
    pub filled_size: String,
    pub avg_fill_price: String,
    pub timestamp: i64,
}

/// Risk alert message
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RiskAlertMessage {
    pub alert_id: String,
    pub alert_type: String,     // "warning" | "critical" | "kill_switch"
    pub metric: String,
    pub current_value: String,
    pub threshold: String,
    pub message: String,
    pub timestamp: i64,
}

/// Debate request message
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct DebateRequestMessage {
    pub debate_id: String,
    pub topic: String,
    pub market_id: String,
    pub context: String,
    pub urgency: String,        // "low" | "medium" | "high"
    pub max_rounds: u32,
    pub participants: Vec<String>,
    pub timestamp: i64,
}

/// Debate result message
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct DebateResultMessage {
    pub debate_id: String,
    pub topic: String,
    pub consensus: String,
    pub confidence: f64,
    pub decision: Option<TradingDecisionMessage>,
    pub rounds_completed: u32,
    pub duration_ms: u64,
    pub timestamp: i64,
}

/// Market update message
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct MarketUpdateMessage {
    pub update_type: String,    // "orderbook" | "trade" | "status"
    pub market_id: String,
    pub token_id: String,
    pub data: serde_json::Value,
    pub sequence: u64,
    pub timestamp: i64,
}

/// High-performance message queue client
#[derive(Clone)]
pub struct MessageQueue {
    connection: Arc<Connection>,
    channel: Arc<RwLock<Channel>>,
    consumer_tag_counter: Arc<RwLock<u64>>,
}

impl MessageQueue {
    /// Create a new message queue client from URL
    pub async fn new(url: &str) -> Result<Self> {
        let connection = Connection::connect(url, ConnectionProperties::default())
            .await
            .context("Failed to connect to RabbitMQ")?;

        let channel = connection
            .create_channel()
            .await
            .context("Failed to create RabbitMQ channel")?;

        info!(url = url, "Connected to RabbitMQ");

        Ok(Self {
            connection: Arc::new(connection),
            channel: Arc::new(RwLock::new(channel)),
            consumer_tag_counter: Arc::new(RwLock::new(0)),
        })
    }

    /// Initialize all exchanges and queues
    pub async fn setup(&self) -> Result<()> {
        let channel = self.channel.read().await;

        // Declare topic exchanges
        for exchange in [
            exchanges::TRADING,
            exchanges::SIGNALS,
            exchanges::ORDERS,
            exchanges::RISK,
            exchanges::DEBATES,
            exchanges::EVENTS,
        ] {
            channel
                .exchange_declare(
                    exchange,
                    lapin::ExchangeKind::Topic,
                    ExchangeDeclareOptions {
                        durable: true,
                        ..Default::default()
                    },
                    FieldTable::default(),
                )
                .await
                .context(format!("Failed to declare exchange: {}", exchange))?;

            debug!(exchange, "Declared exchange");
        }

        // Declare queues with appropriate settings
        let queue_configs = [
            (queues::TRADING_DECISIONS, exchanges::TRADING, vec![routing::DECISION_NEW]),
            (queues::ORDER_EXECUTION, exchanges::ORDERS, vec![routing::ORDER_PLACED, routing::ORDER_FILLED, routing::ORDER_CANCELLED]),
            (queues::ORDER_FILLS, exchanges::ORDERS, vec![routing::ORDER_FILLED]),
            (queues::RISK_ALERTS, exchanges::RISK, vec![routing::RISK_WARNING, routing::RISK_CRITICAL, routing::RISK_KILL_SWITCH]),
            (queues::DEBATE_REQUESTS, exchanges::DEBATES, vec![routing::DEBATE_START]),
            (queues::DEBATE_RESULTS, exchanges::DEBATES, vec![routing::DEBATE_CONSENSUS]),
            (queues::MARKET_UPDATES, exchanges::EVENTS, vec![routing::MARKET_ORDERBOOK, routing::MARKET_TRADE]),
            (queues::STRATEGY_SIGNALS, exchanges::SIGNALS, vec![routing::SIGNAL_MM, routing::SIGNAL_ARB]),
        ];

        for (queue, exchange, routing_keys) in queue_configs {
            // Declare queue
            channel
                .queue_declare(
                    queue,
                    QueueDeclareOptions {
                        durable: true,
                        ..Default::default()
                    },
                    FieldTable::default(),
                )
                .await
                .context(format!("Failed to declare queue: {}", queue))?;

            // Bind queue to exchange with routing keys
            for routing_key in routing_keys {
                channel
                    .queue_bind(
                        queue,
                        exchange,
                        routing_key,
                        QueueBindOptions::default(),
                        FieldTable::default(),
                    )
                    .await
                    .context(format!("Failed to bind queue {} to {}", queue, routing_key))?;
            }

            debug!(queue, exchange, "Declared and bound queue");
        }

        info!("Message queue setup complete");
        Ok(())
    }

    /// Publish a message to an exchange
    #[instrument(skip(self, message), fields(exchange = %exchange, routing_key = %routing_key))]
    pub async fn publish<T: Serialize>(
        &self,
        exchange: &str,
        routing_key: &str,
        message: &T,
    ) -> Result<()> {
        self.publish_with_options(exchange, routing_key, message, MessageOptions::default()).await
    }

    /// Publish a message with options
    pub async fn publish_with_options<T: Serialize>(
        &self,
        exchange: &str,
        routing_key: &str,
        message: &T,
        options: MessageOptions,
    ) -> Result<()> {
        let channel = self.channel.read().await;
        let payload = serde_json::to_vec(message)?;

        let mut headers = FieldTable::default();
        if let Some(priority) = options.priority {
            headers.insert(
                ShortString::from("priority"),
                AMQPValue::ShortUInt(priority as u16),
            );
        }

        let properties = BasicProperties::default()
            .with_content_type(ShortString::from("application/json"))
            .with_delivery_mode(if options.persistent { 2 } else { 1 })
            .with_headers(headers);

        let confirm = channel
            .basic_publish(
                exchange,
                routing_key,
                BasicPublishOptions::default(),
                &payload,
                properties,
            )
            .await
            .context("Failed to publish message")?
            .await
            .context("Failed to confirm message")?;

        match confirm {
            Confirmation::NotRequested => {
                debug!(exchange, routing_key, "Message published (no confirm)");
            }
            Confirmation::Ack(_) => {
                debug!(exchange, routing_key, "Message published and confirmed");
            }
            Confirmation::Nack(_) => {
                warn!(exchange, routing_key, "Message was not acknowledged");
            }
        }

        Ok(())
    }

    /// Subscribe to a queue
    pub async fn subscribe(&self, queue: &str) -> Result<Consumer> {
        let channel = self.channel.read().await;
        let mut counter = self.consumer_tag_counter.write().await;
        *counter += 1;
        let consumer_tag = format!("bruniao-consumer-{}", *counter);

        let consumer = channel
            .basic_consume(
                queue,
                &consumer_tag,
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .context(format!("Failed to subscribe to queue: {}", queue))?;

        info!(queue, consumer_tag, "Subscribed to queue");
        Ok(consumer)
    }

    /// Create a message handler for a queue
    pub async fn handle_messages<T, F, Fut>(
        &self,
        queue: &str,
        handler: F,
    ) -> Result<()>
    where
        T: DeserializeOwned + Send + 'static,
        F: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send,
    {
        let mut consumer = self.subscribe(queue).await?;
        let channel = self.channel.clone();

        tokio::spawn(async move {
            while let Some(delivery) = consumer.next().await {
                match delivery {
                    Ok(delivery) => {
                        match serde_json::from_slice::<T>(&delivery.data) {
                            Ok(message) => {
                                if let Err(e) = handler(message).await {
                                    error!(error = %e, "Message handler error");
                                    // Nack the message so it can be reprocessed
                                    if let Err(e) = delivery.nack(BasicNackOptions::default()).await {
                                        error!(error = %e, "Failed to nack message");
                                    }
                                } else {
                                    // Ack the message
                                    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
                                        error!(error = %e, "Failed to ack message");
                                    }
                                }
                            }
                            Err(e) => {
                                error!(error = %e, "Failed to deserialize message");
                                // Reject the message (don't requeue malformed messages)
                                if let Err(e) = delivery.reject(BasicRejectOptions { requeue: false }).await {
                                    error!(error = %e, "Failed to reject message");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Consumer error");
                    }
                }
            }
        });

        Ok(())
    }

    // ==================== Trading Messages ====================

    /// Publish a trading decision
    pub async fn publish_trading_decision(&self, decision: &TradingDecisionMessage) -> Result<()> {
        self.publish_with_options(
            exchanges::TRADING,
            routing::DECISION_NEW,
            decision,
            MessageOptions {
                persistent: true,
                priority: Some(8),
            },
        )
        .await
    }

    /// Publish order execution update
    pub async fn publish_order_execution(&self, order: &OrderExecutionMessage) -> Result<()> {
        let routing_key = match order.status.as_str() {
            "open" | "pending" => routing::ORDER_PLACED,
            "filled" => routing::ORDER_FILLED,
            "cancelled" => routing::ORDER_CANCELLED,
            _ => routing::ORDER_PLACED,
        };

        self.publish_with_options(
            exchanges::ORDERS,
            routing_key,
            order,
            MessageOptions {
                persistent: true,
                priority: Some(7),
            },
        )
        .await
    }

    // ==================== Risk Messages ====================

    /// Publish risk alert
    pub async fn publish_risk_alert(&self, alert: &RiskAlertMessage) -> Result<()> {
        let routing_key = match alert.alert_type.as_str() {
            "warning" => routing::RISK_WARNING,
            "critical" => routing::RISK_CRITICAL,
            "kill_switch" => routing::RISK_KILL_SWITCH,
            _ => routing::RISK_WARNING,
        };

        self.publish_with_options(
            exchanges::RISK,
            routing_key,
            alert,
            MessageOptions {
                persistent: true,
                priority: if alert.alert_type == "kill_switch" { Some(10) } else { Some(9) },
            },
        )
        .await
    }

    // ==================== Debate Messages ====================

    /// Request a bot debate
    pub async fn request_debate(&self, request: &DebateRequestMessage) -> Result<()> {
        self.publish_with_options(
            exchanges::DEBATES,
            routing::DEBATE_START,
            request,
            MessageOptions {
                persistent: true,
                priority: match request.urgency.as_str() {
                    "high" => Some(9),
                    "medium" => Some(5),
                    _ => Some(3),
                },
            },
        )
        .await
    }

    /// Publish debate result
    pub async fn publish_debate_result(&self, result: &DebateResultMessage) -> Result<()> {
        self.publish_with_options(
            exchanges::DEBATES,
            routing::DEBATE_CONSENSUS,
            result,
            MessageOptions {
                persistent: true,
                priority: Some(7),
            },
        )
        .await
    }

    // ==================== Market Messages ====================

    /// Publish market update
    pub async fn publish_market_update(&self, update: &MarketUpdateMessage) -> Result<()> {
        let routing_key = match update.update_type.as_str() {
            "orderbook" => routing::MARKET_ORDERBOOK,
            "trade" => routing::MARKET_TRADE,
            _ => routing::MARKET_ORDERBOOK,
        };

        self.publish(exchanges::EVENTS, routing_key, update).await
    }

    // ==================== Strategy Signals ====================

    /// Publish strategy signal (market making or arbitrage)
    pub async fn publish_strategy_signal(&self, signal_type: &str, signal: &serde_json::Value) -> Result<()> {
        let routing_key = match signal_type {
            "market_making" => routing::SIGNAL_MM,
            "arbitrage" => routing::SIGNAL_ARB,
            _ => routing::SIGNAL_MM,
        };

        self.publish(exchanges::SIGNALS, routing_key, signal).await
    }

    // ==================== Health Check ====================

    /// Check if message queue is healthy
    pub async fn health_check(&self) -> Result<bool> {
        Ok(self.connection.status().connected())
    }

    /// Get queue statistics
    pub async fn queue_stats(&self, queue: &str) -> Result<QueueStats> {
        let channel = self.channel.read().await;
        let queue_state = channel
            .queue_declare(
                queue,
                QueueDeclareOptions {
                    passive: true,  // Don't create, just check
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await?;

        Ok(QueueStats {
            name: queue.to_string(),
            message_count: queue_state.message_count(),
            consumer_count: queue_state.consumer_count(),
        })
    }
}

/// Message publishing options
#[derive(Debug, Clone, Default)]
pub struct MessageOptions {
    pub persistent: bool,
    pub priority: Option<u8>,
}

/// Queue statistics
#[derive(Debug, Clone)]
pub struct QueueStats {
    pub name: String,
    pub message_count: u32,
    pub consumer_count: u32,
}

/// Trait for message handlers
#[async_trait::async_trait]
pub trait MessageHandler<T> {
    async fn handle(&self, message: T) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        let decision = TradingDecisionMessage {
            decision_id: "test-123".to_string(),
            decision_type: "market_making".to_string(),
            market_id: "market-456".to_string(),
            token_id: "token-789".to_string(),
            side: "buy".to_string(),
            price: "0.55".to_string(),
            size: "10.0".to_string(),
            order_type: "gtc".to_string(),
            confidence: 0.85,
            reason: "Good spread".to_string(),
            timestamp: 1234567890,
        };

        let json = serde_json::to_string(&decision).unwrap();
        assert!(json.contains("market_making"));
        assert!(json.contains("0.55"));
    }
}
