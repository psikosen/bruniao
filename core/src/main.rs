//! Bruniao - Polymarket Trading Infrastructure
//!
//! A high-performance trading bot for Polymarket with market making,
//! arbitrage detection, and AI-powered strategy analysis.
//!
//! Now with DragonflyDB caching and RabbitMQ messaging for maximum speed!

mod api;
pub mod cache;
mod executor;
pub mod models;
pub mod mq;
mod orderbook;
mod risk;
mod strategy;
mod ws;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::cache::CacheClient;
use crate::executor::OrderExecutor;
use crate::mq::MessageQueue;
use crate::orderbook::OrderBookManager;
use crate::risk::RiskManager;
use crate::strategy::StrategyEngine;
use crate::ws::WebSocketManager;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to configuration file
    #[arg(short, long, default_value = "config/config.yaml")]
    config: PathBuf,

    /// Enable dry-run mode (no real orders)
    #[arg(short, long)]
    dry_run: bool,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, default_value = "info")]
    log_level: String,
}

/// Application state shared across components
pub struct AppState {
    pub config: models::Config,
    pub orderbook_manager: Arc<OrderBookManager>,
    pub risk_manager: Arc<RwLock<RiskManager>>,
    pub strategy_engine: Arc<RwLock<StrategyEngine>>,
    pub order_executor: Arc<OrderExecutor>,
    pub cache: Option<Arc<CacheClient>>,
    pub mq: Option<Arc<MessageQueue>>,
    pub dry_run: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    let log_level = match args.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    tracing_subscriber::registry()
        .with(fmt::layer().json())
        .with(EnvFilter::from_default_env().add_directive(log_level.into()))
        .init();

    info!("Starting Bruniao Trading Bot v{}", env!("CARGO_PKG_VERSION"));
    info!("Dry run mode: {}", args.dry_run);

    // Load configuration
    let config = models::Config::load(&args.config)?;
    info!("Loaded configuration from {:?}", args.config);

    // Initialize components
    let orderbook_manager = Arc::new(OrderBookManager::new());
    let risk_manager = Arc::new(RwLock::new(RiskManager::new(&config.risk)));
    let order_executor = Arc::new(OrderExecutor::new(&config, args.dry_run).await?);
    let strategy_engine = Arc::new(RwLock::new(StrategyEngine::new(&config.strategy)));

    // Initialize DragonflyDB cache (optional - graceful degradation)
    let cache = match std::env::var("DRAGONFLY_URL") {
        Ok(url) => {
            match CacheClient::new(&url).await {
                Ok(client) => {
                    info!("DragonflyDB cache connected");
                    Some(Arc::new(client))
                }
                Err(e) => {
                    warn!("DragonflyDB unavailable, running without distributed cache: {}", e);
                    None
                }
            }
        }
        Err(_) => {
            warn!("DRAGONFLY_URL not set, running without distributed cache");
            None
        }
    };

    // Initialize RabbitMQ message queue (optional - graceful degradation)
    let mq = match std::env::var("RABBITMQ_URL") {
        Ok(url) => {
            match MessageQueue::new(&url).await {
                Ok(queue) => {
                    // Setup exchanges and queues
                    if let Err(e) = queue.setup().await {
                        warn!("Failed to setup RabbitMQ topology: {}", e);
                    }
                    info!("RabbitMQ message queue connected");
                    Some(Arc::new(queue))
                }
                Err(e) => {
                    warn!("RabbitMQ unavailable, running without message queue: {}", e);
                    None
                }
            }
        }
        Err(_) => {
            warn!("RABBITMQ_URL not set, running without message queue");
            None
        }
    };

    let state = Arc::new(AppState {
        config: config.clone(),
        orderbook_manager: orderbook_manager.clone(),
        risk_manager: risk_manager.clone(),
        strategy_engine: strategy_engine.clone(),
        order_executor: order_executor.clone(),
        cache,
        mq,
        dry_run: args.dry_run,
    });

    // Start WebSocket connection for orderbook updates
    let ws_manager = WebSocketManager::new(
        &config.polymarket.ws_url,
        orderbook_manager.clone(),
    );

    // Spawn WebSocket handler
    let ws_handle = tokio::spawn(async move {
        if let Err(e) = ws_manager.run().await {
            tracing::error!("WebSocket error: {:?}", e);
        }
    });

    // Spawn strategy loop
    let state_clone = state.clone();
    let strategy_handle = tokio::spawn(async move {
        run_strategy_loop(state_clone).await;
    });

    // Spawn risk monitoring loop
    let state_clone = state.clone();
    let risk_handle = tokio::spawn(async move {
        run_risk_monitor(state_clone).await;
    });

    info!("All components started. Trading bot is running.");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received. Cleaning up...");

    // Cancel all open orders before shutdown
    if !args.dry_run {
        order_executor.cancel_all_orders().await?;
    }

    ws_handle.abort();
    strategy_handle.abort();
    risk_handle.abort();

    info!("Bruniao shutdown complete.");
    Ok(())
}

/// Main strategy execution loop
async fn run_strategy_loop(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_millis(
        state.config.strategy.loop_interval_ms,
    ));

    loop {
        interval.tick().await;

        // Check if risk limits allow trading
        let risk_manager = state.risk_manager.read().await;
        if risk_manager.is_kill_switch_active() {
            tracing::warn!("Kill switch active - skipping strategy tick");
            continue;
        }
        drop(risk_manager);

        // Get strategy decisions
        let mut strategy_engine = state.strategy_engine.write().await;
        let decisions = strategy_engine.tick(&state.orderbook_manager).await;
        drop(strategy_engine);

        // Execute decisions through risk manager
        for decision in decisions {
            let mut risk_manager = state.risk_manager.write().await;
            match risk_manager.validate_order(&decision) {
                Ok(_) => {
                    drop(risk_manager);

                    // Publish decision to message queue (if available)
                    if let Some(ref mq) = state.mq {
                        let msg = mq::TradingDecisionMessage {
                            decision_id: decision.decision_id.to_string(),
                            decision_type: match decision.decision_type {
                                models::DecisionType::MarketMaking => "market_making".to_string(),
                                models::DecisionType::Arbitrage => "arbitrage".to_string(),
                            },
                            market_id: decision.order.market_id.clone(),
                            token_id: decision.order.token_id.clone(),
                            side: match decision.order.side {
                                models::Side::Buy => "buy".to_string(),
                                models::Side::Sell => "sell".to_string(),
                            },
                            price: decision.order.price.to_string(),
                            size: decision.order.size.to_string(),
                            order_type: match decision.order.order_type {
                                models::OrderType::GTC => "gtc".to_string(),
                                models::OrderType::FOK => "fok".to_string(),
                                models::OrderType::IOC => "ioc".to_string(),
                            },
                            confidence: decision.confidence,
                            reason: decision.reason.clone(),
                            timestamp: decision.timestamp.timestamp(),
                        };

                        if let Err(e) = mq.publish_trading_decision(&msg).await {
                            tracing::warn!("Failed to publish decision to MQ: {:?}", e);
                        }
                    }

                    // Execute the order
                    if let Err(e) = state.order_executor.execute(decision).await {
                        tracing::error!("Order execution error: {:?}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("Order rejected by risk manager: {:?}", e);
                }
            }
        }

        // Cache orderbook snapshots periodically (for cross-service access)
        if let Some(ref cache) = state.cache {
            if let Err(e) = cache_orderbook_snapshots(&state.orderbook_manager, cache).await {
                tracing::debug!("Failed to cache orderbooks: {:?}", e);
            }
        }
    }
}

/// Cache orderbook snapshots to DragonflyDB
async fn cache_orderbook_snapshots(
    orderbook_manager: &OrderBookManager,
    cache: &CacheClient,
) -> anyhow::Result<()> {
    use crate::cache::CachedOrderBook;

    let snapshots: Vec<CachedOrderBook> = orderbook_manager
        .get_all_books()
        .iter()
        .map(|book| {
            CachedOrderBook {
                token_id: book.token_id.clone(),
                bids: book.bids.iter().map(|l| (l.price, l.size)).collect(),
                asks: book.asks.iter().map(|l| (l.price, l.size)).collect(),
                mid_price: book.mid_price(),
                spread: book.spread(),
                timestamp: chrono::Utc::now().timestamp(),
                sequence: book.sequence,
            }
        })
        .collect();

    if !snapshots.is_empty() {
        cache.cache_orderbooks(&snapshots).await?;
    }

    Ok(())
}

/// Risk monitoring loop - checks for breaches and triggers kill switch
async fn run_risk_monitor(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));

    loop {
        interval.tick().await;

        let mut risk_manager = state.risk_manager.write().await;

        // Update P&L and check limits
        if let Err(e) = risk_manager.update_positions(&state.order_executor).await {
            tracing::error!("Risk monitor error: {:?}", e);
        }

        // Check for drawdown breach
        if risk_manager.check_drawdown_breach() {
            tracing::error!("DRAWDOWN LIMIT BREACHED - Activating kill switch");
            risk_manager.activate_kill_switch();

            // Publish kill switch alert to message queue
            if let Some(ref mq) = state.mq {
                let alert = mq::RiskAlertMessage {
                    alert_id: uuid::Uuid::new_v4().to_string(),
                    alert_type: "kill_switch".to_string(),
                    metric: "daily_drawdown".to_string(),
                    current_value: risk_manager.get_daily_pnl().to_string(),
                    threshold: state.config.risk.daily_drawdown_limit.to_string(),
                    message: "Daily drawdown limit breached - kill switch activated".to_string(),
                    timestamp: chrono::Utc::now().timestamp(),
                };

                if let Err(e) = mq.publish_risk_alert(&alert).await {
                    tracing::error!("Failed to publish risk alert: {:?}", e);
                }
            }

            // Cancel all orders
            if !state.dry_run {
                if let Err(e) = state.order_executor.cancel_all_orders().await {
                    tracing::error!("Failed to cancel orders: {:?}", e);
                }
            }
        }

        // Cache risk metrics to DragonflyDB
        if let Some(ref cache) = state.cache {
            let _ = cache.set_metric("daily_pnl", risk_manager.get_daily_pnl().to_f64().unwrap_or(0.0)).await;
            let _ = cache.set_metric("position_count", risk_manager.get_position_count() as f64).await;
            let _ = cache.set_metric("kill_switch_active", if risk_manager.is_kill_switch_active() { 1.0 } else { 0.0 }).await;
        }
    }
}
