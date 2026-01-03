//! DragonflyDB/Redis Cache Module
//!
//! High-performance distributed caching layer for:
//! - Orderbook snapshots (sub-millisecond reads)
//! - Market data caching
//! - Session state management
//! - Rate limiting
//! - Cross-service communication

use anyhow::{Context, Result};
use deadpool_redis::{Config, Pool, Runtime};
use redis::AsyncCommands;
use rust_decimal::Decimal;
use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, info, instrument};

/// Cache key prefixes for different data types
pub mod keys {
    pub const ORDERBOOK: &str = "ob";
    pub const MARKET: &str = "mkt";
    pub const POSITION: &str = "pos";
    pub const ORDER: &str = "ord";
    pub const STRATEGY: &str = "strat";
    pub const RATE_LIMIT: &str = "rl";
    pub const SESSION: &str = "sess";
    pub const LOCK: &str = "lock";
    pub const PUBSUB: &str = "ps";
    pub const METRICS: &str = "metrics";
}

/// Cache TTL presets (in seconds)
pub mod ttl {
    pub const ORDERBOOK: u64 = 5;           // Orderbooks refresh frequently
    pub const MARKET_DATA: u64 = 60;        // Market metadata is stable
    pub const POSITION: u64 = 30;           // Positions need regular updates
    pub const ORDER: u64 = 300;             // Orders live longer
    pub const RATE_LIMIT: u64 = 60;         // Rate limit windows
    pub const SESSION: u64 = 3600;          // Session state (1 hour)
    pub const LOCK: u64 = 30;               // Distributed locks
    pub const STRATEGY_STATE: u64 = 10;     // Strategy state is volatile
}

/// Cached orderbook snapshot for ultra-fast reads
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CachedOrderBook {
    pub token_id: String,
    pub bids: Vec<(Decimal, Decimal)>,  // (price, size)
    pub asks: Vec<(Decimal, Decimal)>,
    pub mid_price: Decimal,
    pub spread: Decimal,
    pub timestamp: i64,
    pub sequence: u64,
}

/// Cached market data
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CachedMarket {
    pub market_id: String,
    pub condition_id: String,
    pub question: String,
    pub yes_token_id: String,
    pub no_token_id: String,
    pub volume_24h: Decimal,
    pub liquidity: Decimal,
    pub last_price_yes: Decimal,
    pub last_price_no: Decimal,
    pub active: bool,
    pub updated_at: i64,
}

/// Cached position for quick PnL calculations
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CachedPosition {
    pub market_id: String,
    pub token_id: String,
    pub side: String,
    pub size: Decimal,
    pub avg_price: Decimal,
    pub unrealized_pnl: Decimal,
    pub realized_pnl: Decimal,
    pub updated_at: i64,
}

/// Strategy state cache for cross-instance coordination
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct CachedStrategyState {
    pub strategy_id: String,
    pub last_decision_time: i64,
    pub active_orders: u32,
    pub inventory_skew: Decimal,
    pub last_mm_quotes: Vec<(String, Decimal, Decimal)>,  // (token_id, bid, ask)
    pub arb_opportunities_found: u32,
    pub updated_at: i64,
}

/// High-performance cache client wrapping DragonflyDB
#[derive(Clone)]
pub struct CacheClient {
    pool: Pool,
    prefix: String,
}

impl CacheClient {
    /// Create a new cache client from URL
    pub async fn new(url: &str) -> Result<Self> {
        Self::with_prefix(url, "bruniao").await
    }

    /// Create a new cache client with custom prefix
    pub async fn with_prefix(url: &str, prefix: &str) -> Result<Self> {
        let cfg = Config::from_url(url);
        let pool = cfg
            .create_pool(Some(Runtime::Tokio1))
            .context("Failed to create DragonflyDB connection pool")?;

        // Test connection
        let mut conn = pool.get().await.context("Failed to connect to DragonflyDB")?;
        let pong: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .context("DragonflyDB ping failed")?;

        if pong != "PONG" {
            anyhow::bail!("Unexpected PING response: {}", pong);
        }

        info!(url = url, prefix = prefix, "Connected to DragonflyDB");

        Ok(Self {
            pool,
            prefix: prefix.to_string(),
        })
    }

    /// Build a cache key with prefix
    fn key(&self, namespace: &str, id: &str) -> String {
        format!("{}:{}:{}", self.prefix, namespace, id)
    }

    /// Set a value with TTL
    #[instrument(skip(self, value), fields(key = %key))]
    pub async fn set<T: Serialize>(&self, namespace: &str, key: &str, value: &T, ttl_secs: u64) -> Result<()> {
        let full_key = self.key(namespace, key);
        let serialized = serde_json::to_string(value)?;

        let mut conn = self.pool.get().await?;
        conn.set_ex::<_, _, ()>(&full_key, &serialized, ttl_secs)
            .await
            .context("Failed to set cache value")?;

        debug!(key = %full_key, ttl = ttl_secs, "Cache SET");
        Ok(())
    }

    /// Get a value
    #[instrument(skip(self), fields(key = %key))]
    pub async fn get<T: DeserializeOwned>(&self, namespace: &str, key: &str) -> Result<Option<T>> {
        let full_key = self.key(namespace, key);

        let mut conn = self.pool.get().await?;
        let value: Option<String> = conn.get(&full_key).await?;

        match value {
            Some(s) => {
                let deserialized = serde_json::from_str(&s)?;
                debug!(key = %full_key, "Cache HIT");
                Ok(Some(deserialized))
            }
            None => {
                debug!(key = %full_key, "Cache MISS");
                Ok(None)
            }
        }
    }

    /// Delete a key
    pub async fn delete(&self, namespace: &str, key: &str) -> Result<bool> {
        let full_key = self.key(namespace, key);
        let mut conn = self.pool.get().await?;
        let deleted: u32 = conn.del(&full_key).await?;
        Ok(deleted > 0)
    }

    /// Set if not exists (for distributed locking)
    pub async fn setnx(&self, namespace: &str, key: &str, value: &str, ttl_secs: u64) -> Result<bool> {
        let full_key = self.key(namespace, key);
        let mut conn = self.pool.get().await?;

        let result: bool = redis::cmd("SET")
            .arg(&full_key)
            .arg(value)
            .arg("NX")
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut conn)
            .await
            .unwrap_or(false);

        Ok(result)
    }

    /// Increment a counter (for rate limiting)
    pub async fn incr(&self, namespace: &str, key: &str) -> Result<i64> {
        let full_key = self.key(namespace, key);
        let mut conn = self.pool.get().await?;
        let value: i64 = conn.incr(&full_key, 1).await?;
        Ok(value)
    }

    /// Increment with TTL (for rate limiting windows)
    pub async fn incr_with_ttl(&self, namespace: &str, key: &str, ttl_secs: u64) -> Result<i64> {
        let full_key = self.key(namespace, key);
        let mut conn = self.pool.get().await?;

        // Use MULTI/EXEC for atomic increment + expire
        let (value,): (i64,) = redis::pipe()
            .atomic()
            .incr(&full_key, 1)
            .expire(&full_key, ttl_secs as i64)
            .ignore()
            .query_async(&mut conn)
            .await?;

        Ok(value)
    }

    /// Check rate limit (returns true if allowed)
    pub async fn check_rate_limit(&self, key: &str, max_requests: u32, window_secs: u64) -> Result<bool> {
        let count = self.incr_with_ttl(keys::RATE_LIMIT, key, window_secs).await?;
        Ok(count <= max_requests as i64)
    }

    // ==================== Orderbook Caching ====================

    /// Cache an orderbook snapshot
    pub async fn cache_orderbook(&self, orderbook: &CachedOrderBook) -> Result<()> {
        self.set(keys::ORDERBOOK, &orderbook.token_id, orderbook, ttl::ORDERBOOK).await
    }

    /// Get cached orderbook
    pub async fn get_orderbook(&self, token_id: &str) -> Result<Option<CachedOrderBook>> {
        self.get(keys::ORDERBOOK, token_id).await
    }

    /// Batch cache multiple orderbooks
    pub async fn cache_orderbooks(&self, orderbooks: &[CachedOrderBook]) -> Result<()> {
        let mut conn = self.pool.get().await?;
        let mut pipe = redis::pipe();

        for ob in orderbooks {
            let key = self.key(keys::ORDERBOOK, &ob.token_id);
            let value = serde_json::to_string(ob)?;
            pipe.set_ex(&key, &value, ttl::ORDERBOOK);
        }

        pipe.query_async::<_, ()>(&mut conn).await?;
        debug!(count = orderbooks.len(), "Batch cached orderbooks");
        Ok(())
    }

    // ==================== Market Data Caching ====================

    /// Cache market data
    pub async fn cache_market(&self, market: &CachedMarket) -> Result<()> {
        self.set(keys::MARKET, &market.market_id, market, ttl::MARKET_DATA).await
    }

    /// Get cached market
    pub async fn get_market(&self, market_id: &str) -> Result<Option<CachedMarket>> {
        self.get(keys::MARKET, market_id).await
    }

    // ==================== Position Caching ====================

    /// Cache position
    pub async fn cache_position(&self, position: &CachedPosition) -> Result<()> {
        self.set(keys::POSITION, &position.market_id, position, ttl::POSITION).await
    }

    /// Get cached position
    pub async fn get_position(&self, market_id: &str) -> Result<Option<CachedPosition>> {
        self.get(keys::POSITION, market_id).await
    }

    /// Get all cached positions
    pub async fn get_all_positions(&self) -> Result<Vec<CachedPosition>> {
        let pattern = self.key(keys::POSITION, "*");
        let mut conn = self.pool.get().await?;

        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await?;

        if keys.is_empty() {
            return Ok(vec![]);
        }

        let values: Vec<Option<String>> = conn.mget(&keys).await?;

        let positions: Vec<CachedPosition> = values
            .into_iter()
            .flatten()
            .filter_map(|s| serde_json::from_str(&s).ok())
            .collect();

        Ok(positions)
    }

    // ==================== Strategy State ====================

    /// Cache strategy state
    pub async fn cache_strategy_state(&self, state: &CachedStrategyState) -> Result<()> {
        self.set(keys::STRATEGY, &state.strategy_id, state, ttl::STRATEGY_STATE).await
    }

    /// Get cached strategy state
    pub async fn get_strategy_state(&self, strategy_id: &str) -> Result<Option<CachedStrategyState>> {
        self.get(keys::STRATEGY, strategy_id).await
    }

    // ==================== Distributed Locking ====================

    /// Acquire a distributed lock
    pub async fn acquire_lock(&self, lock_name: &str, holder_id: &str, ttl_secs: u64) -> Result<bool> {
        self.setnx(keys::LOCK, lock_name, holder_id, ttl_secs).await
    }

    /// Release a distributed lock (only if we hold it)
    pub async fn release_lock(&self, lock_name: &str, holder_id: &str) -> Result<bool> {
        let full_key = self.key(keys::LOCK, lock_name);
        let mut conn = self.pool.get().await?;

        // Check if we hold the lock
        let current_holder: Option<String> = conn.get(&full_key).await?;
        if current_holder.as_deref() == Some(holder_id) {
            let deleted: u32 = conn.del(&full_key).await?;
            Ok(deleted > 0)
        } else {
            Ok(false)
        }
    }

    /// Execute with lock (distributed mutex)
    pub async fn with_lock<F, T>(&self, lock_name: &str, holder_id: &str, ttl_secs: u64, f: F) -> Result<Option<T>>
    where
        F: std::future::Future<Output = Result<T>>,
    {
        if self.acquire_lock(lock_name, holder_id, ttl_secs).await? {
            let result = f.await;
            self.release_lock(lock_name, holder_id).await?;
            result.map(Some)
        } else {
            Ok(None)
        }
    }

    // ==================== Pub/Sub for Real-time Updates ====================

    /// Publish a message to a channel
    pub async fn publish<T: Serialize>(&self, channel: &str, message: &T) -> Result<u32> {
        let full_channel = self.key(keys::PUBSUB, channel);
        let serialized = serde_json::to_string(message)?;

        let mut conn = self.pool.get().await?;
        let subscribers: u32 = conn.publish(&full_channel, &serialized).await?;

        debug!(channel = %full_channel, subscribers, "Published message");
        Ok(subscribers)
    }

    // ==================== Metrics ====================

    /// Increment a metric counter
    pub async fn incr_metric(&self, metric: &str, delta: i64) -> Result<i64> {
        let full_key = self.key(keys::METRICS, metric);
        let mut conn = self.pool.get().await?;
        let value: i64 = conn.incr(&full_key, delta).await?;
        Ok(value)
    }

    /// Set a metric gauge
    pub async fn set_metric(&self, metric: &str, value: f64) -> Result<()> {
        let full_key = self.key(keys::METRICS, metric);
        let mut conn = self.pool.get().await?;
        conn.set::<_, _, ()>(&full_key, value.to_string()).await?;
        Ok(())
    }

    /// Get a metric value
    pub async fn get_metric(&self, metric: &str) -> Result<Option<f64>> {
        let full_key = self.key(keys::METRICS, metric);
        let mut conn = self.pool.get().await?;
        let value: Option<String> = conn.get(&full_key).await?;
        Ok(value.and_then(|s| s.parse().ok()))
    }

    // ==================== Health Check ====================

    /// Check if cache is healthy
    pub async fn health_check(&self) -> Result<bool> {
        let mut conn = self.pool.get().await?;
        let pong: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await?;
        Ok(pong == "PONG")
    }

    /// Get cache stats
    pub async fn stats(&self) -> Result<CacheStats> {
        let mut conn = self.pool.get().await?;
        let info: String = redis::cmd("INFO")
            .arg("stats")
            .query_async(&mut conn)
            .await?;

        // Parse basic stats from INFO response
        let mut stats = CacheStats::default();
        for line in info.lines() {
            if let Some((key, value)) = line.split_once(':') {
                match key {
                    "total_connections_received" => stats.total_connections = value.parse().unwrap_or(0),
                    "total_commands_processed" => stats.total_commands = value.parse().unwrap_or(0),
                    "keyspace_hits" => stats.hits = value.parse().unwrap_or(0),
                    "keyspace_misses" => stats.misses = value.parse().unwrap_or(0),
                    _ => {}
                }
            }
        }

        if stats.hits + stats.misses > 0 {
            stats.hit_rate = stats.hits as f64 / (stats.hits + stats.misses) as f64;
        }

        Ok(stats)
    }
}

/// Cache statistics
#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    pub total_connections: u64,
    pub total_commands: u64,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_cache_key_format() {
        let client = CacheClient {
            pool: Config::from_url("redis://localhost")
                .create_pool(Some(Runtime::Tokio1))
                .unwrap(),
            prefix: "test".to_string(),
        };

        assert_eq!(client.key("ob", "token123"), "test:ob:token123");
        assert_eq!(client.key("mkt", "market456"), "test:mkt:market456");
    }
}
