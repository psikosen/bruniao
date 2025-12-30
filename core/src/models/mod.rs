//! Data models for the trading infrastructure

use anyhow::Result;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

/// Main configuration structure
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub polymarket: PolymarketConfig,
    pub risk: RiskConfig,
    pub strategy: StrategyConfig,
    pub database: DatabaseConfig,
    pub qdrant: QdrantConfig,
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let settings = config::Config::builder()
            .add_source(config::File::from(path))
            .add_source(config::Environment::with_prefix("BRUNIAO"))
            .build()?;

        Ok(settings.try_deserialize()?)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct PolymarketConfig {
    pub clob_url: String,
    pub gamma_url: String,
    pub ws_url: String,
    pub private_key: Option<String>,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub api_passphrase: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiskConfig {
    /// Maximum position size per market in USD
    pub max_position_per_market: Decimal,
    /// Maximum number of open positions
    pub max_open_positions: usize,
    /// Maximum daily drawdown as a decimal (0.05 = 5%)
    pub daily_drawdown_limit: Decimal,
    /// Minimum top-of-book size to trade
    pub min_book_size: Decimal,
    /// Enable kill switch functionality
    pub kill_switch_enabled: bool,
    /// Maximum order size in USD
    pub max_order_size: Decimal,
}

#[derive(Debug, Clone, Deserialize)]
pub struct StrategyConfig {
    /// Strategy loop interval in milliseconds
    pub loop_interval_ms: u64,
    /// Market making spread in ticks
    pub mm_spread_ticks: u32,
    /// Size of market making quotes
    pub mm_quote_size: Decimal,
    /// Inventory skew threshold
    pub inventory_skew_threshold: Decimal,
    /// Arbitrage threshold (complement must be < this)
    pub arb_threshold: Decimal,
    /// Markets to trade (condition IDs)
    pub markets: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub sqlite_path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QdrantConfig {
    pub url: String,
    pub collection_name: String,
    pub vector_size: usize,
}

/// Order side
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Side {
    Buy,
    Sell,
}

/// Order type for time-in-force
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderType {
    /// Good-Till-Cancelled
    GTC,
    /// Good-Till-Day (expires at specified timestamp)
    GTD { expiration: DateTime<Utc> },
    /// Fill-Or-Kill
    FOK,
}

/// Token type (YES or NO outcome)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenType {
    Yes,
    No,
}

/// Order status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OrderStatus {
    Pending,
    Open,
    Filled,
    PartiallyFilled,
    Cancelled,
    Expired,
    Rejected,
}

/// Represents an order to be placed
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub market_id: String,
    pub token_id: String,
    pub token_type: TokenType,
    pub side: Side,
    pub price: Decimal,
    pub size: Decimal,
    pub order_type: OrderType,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub filled_size: Decimal,
}

impl Order {
    pub fn new(
        market_id: String,
        token_id: String,
        token_type: TokenType,
        side: Side,
        price: Decimal,
        size: Decimal,
        order_type: OrderType,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            market_id,
            token_id,
            token_type,
            side,
            price,
            size,
            order_type,
            status: OrderStatus::Pending,
            created_at: now,
            updated_at: now,
            filled_size: Decimal::ZERO,
        }
    }
}

/// Trading decision from strategy engine
#[derive(Debug, Clone)]
pub struct TradingDecision {
    pub decision_id: Uuid,
    pub decision_type: DecisionType,
    pub order: Order,
    pub reason: String,
    pub confidence: f64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DecisionType {
    MarketMaking,
    Arbitrage,
    InventoryRebalance,
    SignalBased,
}

/// Orderbook level
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookLevel {
    pub price: Decimal,
    pub size: Decimal,
}

/// Full orderbook for a token
#[derive(Debug, Clone, Default)]
pub struct OrderBook {
    pub token_id: String,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
    pub last_update: Option<DateTime<Utc>>,
}

impl OrderBook {
    pub fn best_bid(&self) -> Option<&BookLevel> {
        self.bids.first()
    }

    pub fn best_ask(&self) -> Option<&BookLevel> {
        self.asks.first()
    }

    pub fn spread(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.price - bid.price),
            _ => None,
        }
    }

    pub fn mid_price(&self) -> Option<Decimal> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.price + ask.price) / Decimal::from(2)),
            _ => None,
        }
    }
}

/// Market metadata from Gamma API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Market {
    pub condition_id: String,
    pub question: String,
    pub description: Option<String>,
    pub outcomes: Vec<String>,
    pub tokens: Vec<Token>,
    pub active: bool,
    pub closed: bool,
    pub end_date: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Token {
    pub token_id: String,
    pub outcome: String,
    pub winner: Option<bool>,
}

/// Position in a market
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub market_id: String,
    pub token_id: String,
    pub token_type: TokenType,
    pub size: Decimal,
    pub average_entry_price: Decimal,
    pub current_price: Decimal,
    pub unrealized_pnl: Decimal,
    pub realized_pnl: Decimal,
}

impl Position {
    pub fn notional_value(&self) -> Decimal {
        self.size * self.current_price
    }

    pub fn update_pnl(&mut self, current_price: Decimal) {
        self.current_price = current_price;
        self.unrealized_pnl = self.size * (current_price - self.average_entry_price);
    }
}

/// Trade record for logging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradeRecord {
    pub id: Uuid,
    pub order_id: Uuid,
    pub decision_id: Uuid,
    pub market_id: String,
    pub token_id: String,
    pub side: Side,
    pub price: Decimal,
    pub size: Decimal,
    pub fee: Decimal,
    pub timestamp: DateTime<Utc>,
    pub decision_type: String,
    pub reason: String,
}

/// WebSocket message types from Polymarket
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "book")]
    BookSnapshot(BookSnapshotMsg),
    #[serde(rename = "book_delta")]
    BookDelta(BookDeltaMsg),
    #[serde(rename = "trade")]
    Trade(TradeMsg),
    #[serde(rename = "error")]
    Error { message: String },
}

#[derive(Debug, Clone, Deserialize)]
pub struct BookSnapshotMsg {
    pub asset_id: String,
    pub market: String,
    pub bids: Vec<BookLevelRaw>,
    pub asks: Vec<BookLevelRaw>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BookDeltaMsg {
    pub asset_id: String,
    pub market: String,
    pub bids: Vec<BookLevelRaw>,
    pub asks: Vec<BookLevelRaw>,
    pub timestamp: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BookLevelRaw {
    pub price: String,
    pub size: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TradeMsg {
    pub asset_id: String,
    pub market: String,
    pub side: String,
    pub price: String,
    pub size: String,
    pub timestamp: String,
}
