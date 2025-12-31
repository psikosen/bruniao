//! Bruniao - Polymarket Trading Infrastructure
//!
//! A high-performance trading bot for Polymarket with market making,
//! arbitrage detection, and AI-powered strategy analysis.

mod api;
mod executor;
mod models;
mod orderbook;
mod risk;
mod strategy;
mod ws;

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, Level};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::executor::OrderExecutor;
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

    let state = Arc::new(AppState {
        config: config.clone(),
        orderbook_manager: orderbook_manager.clone(),
        risk_manager: risk_manager.clone(),
        strategy_engine: strategy_engine.clone(),
        order_executor: order_executor.clone(),
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
            let risk_manager = state.risk_manager.write().await;
            match risk_manager.validate_order(&decision) {
                Ok(_) => {
                    drop(risk_manager);
                    if let Err(e) = state.order_executor.execute(decision).await {
                        tracing::error!("Order execution error: {:?}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("Order rejected by risk manager: {:?}", e);
                }
            }
        }
    }
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

            // Cancel all orders
            if !state.dry_run {
                if let Err(e) = state.order_executor.cancel_all_orders().await {
                    tracing::error!("Failed to cancel orders: {:?}", e);
                }
            }
        }
    }
}
