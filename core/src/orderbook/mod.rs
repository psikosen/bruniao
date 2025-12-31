//! Orderbook management module
//!
//! Maintains real-time orderbook state from WebSocket updates

use crate::models::{BookLevel, BookLevelRaw, OrderBook};
use dashmap::DashMap;
use rust_decimal::Decimal;
use std::str::FromStr;
use tracing::{debug, warn};

/// Thread-safe orderbook manager
pub struct OrderBookManager {
    /// Map of token_id -> OrderBook
    books: DashMap<String, OrderBook>,
}

impl OrderBookManager {
    pub fn new() -> Self {
        Self {
            books: DashMap::new(),
        }
    }

    /// Get orderbook for a token
    pub fn get_book(&self, token_id: &str) -> Option<OrderBook> {
        self.books.get(token_id).map(|r| r.clone())
    }

    /// Get all tracked token IDs
    pub fn get_tracked_tokens(&self) -> Vec<String> {
        self.books.iter().map(|r| r.key().clone()).collect()
    }

    /// Apply a full book snapshot
    pub fn apply_snapshot(
        &self,
        token_id: &str,
        bids: Vec<BookLevelRaw>,
        asks: Vec<BookLevelRaw>,
    ) {
        let parsed_bids = Self::parse_levels(bids);
        let parsed_asks = Self::parse_levels(asks);

        let book = OrderBook {
            token_id: token_id.to_string(),
            bids: parsed_bids,
            asks: parsed_asks,
            last_update: Some(chrono::Utc::now()),
        };

        self.books.insert(token_id.to_string(), book);
        debug!("Applied snapshot for token {}", token_id);
    }

    /// Apply a delta update to the orderbook
    pub fn apply_delta(
        &self,
        token_id: &str,
        bid_deltas: Vec<BookLevelRaw>,
        ask_deltas: Vec<BookLevelRaw>,
    ) {
        if let Some(mut book) = self.books.get_mut(token_id) {
            // Apply bid deltas
            for delta in bid_deltas {
                if let (Ok(price), Ok(size)) = (
                    Decimal::from_str(&delta.price),
                    Decimal::from_str(&delta.size),
                ) {
                    Self::apply_level_delta(&mut book.bids, price, size, true);
                }
            }

            // Apply ask deltas
            for delta in ask_deltas {
                if let (Ok(price), Ok(size)) = (
                    Decimal::from_str(&delta.price),
                    Decimal::from_str(&delta.size),
                ) {
                    Self::apply_level_delta(&mut book.asks, price, size, false);
                }
            }

            book.last_update = Some(chrono::Utc::now());
        } else {
            warn!("Received delta for unknown token: {}", token_id);
        }
    }

    /// Parse raw book levels into typed BookLevel
    fn parse_levels(raw: Vec<BookLevelRaw>) -> Vec<BookLevel> {
        raw.into_iter()
            .filter_map(|l| {
                let price = Decimal::from_str(&l.price).ok()?;
                let size = Decimal::from_str(&l.size).ok()?;
                Some(BookLevel { price, size })
            })
            .collect()
    }

    /// Apply a single level delta
    /// size = 0 means remove the level
    fn apply_level_delta(
        levels: &mut Vec<BookLevel>,
        price: Decimal,
        size: Decimal,
        is_bid: bool,
    ) {
        // Find existing level at this price
        let pos = levels.iter().position(|l| l.price == price);

        if size.is_zero() {
            // Remove level
            if let Some(idx) = pos {
                levels.remove(idx);
            }
        } else if let Some(idx) = pos {
            // Update existing level
            levels[idx].size = size;
        } else {
            // Insert new level
            let new_level = BookLevel { price, size };

            // Find insertion point to maintain sorted order
            // Bids: descending by price
            // Asks: ascending by price
            let insert_pos = if is_bid {
                levels.iter().position(|l| l.price < price).unwrap_or(levels.len())
            } else {
                levels.iter().position(|l| l.price > price).unwrap_or(levels.len())
            };

            levels.insert(insert_pos, new_level);
        }
    }

    /// Check if complement arbitrage exists between YES and NO tokens
    /// Returns the profit margin if sum of best asks < 1.00 - threshold
    pub fn check_complement_arbitrage(
        &self,
        yes_token_id: &str,
        no_token_id: &str,
        threshold: Decimal,
    ) -> Option<ArbitrageOpportunity> {
        let yes_book = self.get_book(yes_token_id)?;
        let no_book = self.get_book(no_token_id)?;

        let yes_ask = yes_book.best_ask()?;
        let no_ask = no_book.best_ask()?;

        let total_cost = yes_ask.price + no_ask.price;
        let one = Decimal::from(1);
        let profit_margin = one - total_cost;

        // Only return if profit exceeds threshold
        if profit_margin > threshold {
            // Executable size is min of both sides
            let executable_size = yes_ask.size.min(no_ask.size);

            Some(ArbitrageOpportunity {
                yes_token_id: yes_token_id.to_string(),
                no_token_id: no_token_id.to_string(),
                yes_price: yes_ask.price,
                no_price: no_ask.price,
                profit_margin,
                executable_size,
            })
        } else {
            None
        }
    }

    /// Get market making quotes for a token
    pub fn get_mm_quotes(
        &self,
        token_id: &str,
        spread_ticks: u32,
        quote_size: Decimal,
        inventory_skew: Decimal,
    ) -> Option<MarketMakingQuotes> {
        let book = self.get_book(token_id)?;
        let best_bid = book.best_bid()?;
        let best_ask = book.best_ask()?;

        // One tick = 0.01
        let tick = Decimal::from_str("0.01").unwrap();
        let _spread_adjustment = tick * Decimal::from(spread_ticks);

        // Base quotes: improve by 1 tick
        let mut bid_price = best_bid.price + tick;
        let mut ask_price = best_ask.price - tick;

        // Don't cross the market
        if bid_price >= ask_price {
            bid_price = best_bid.price;
            ask_price = best_ask.price;
        }

        // Apply inventory skew
        // Positive skew = long, want to sell more -> lower ask, raise bid
        // Negative skew = short, want to buy more -> lower bid, raise ask
        let skew_adjustment = inventory_skew * tick * Decimal::from(2);
        bid_price -= skew_adjustment;
        ask_price -= skew_adjustment;

        // Clamp prices to valid range [0.01, 0.99]
        let min_price = Decimal::from_str("0.01").unwrap();
        let max_price = Decimal::from_str("0.99").unwrap();
        bid_price = bid_price.max(min_price).min(max_price);
        ask_price = ask_price.max(min_price).min(max_price);

        Some(MarketMakingQuotes {
            token_id: token_id.to_string(),
            bid_price,
            bid_size: quote_size,
            ask_price,
            ask_size: quote_size,
        })
    }
}

impl Default for OrderBookManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Detected arbitrage opportunity
#[derive(Debug, Clone)]
pub struct ArbitrageOpportunity {
    pub yes_token_id: String,
    pub no_token_id: String,
    pub yes_price: Decimal,
    pub no_price: Decimal,
    pub profit_margin: Decimal,
    pub executable_size: Decimal,
}

/// Market making quotes
#[derive(Debug, Clone)]
pub struct MarketMakingQuotes {
    pub token_id: String,
    pub bid_price: Decimal,
    pub bid_size: Decimal,
    pub ask_price: Decimal,
    pub ask_size: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orderbook_operations() {
        let manager = OrderBookManager::new();

        // Apply snapshot
        let bids = vec![
            BookLevelRaw { price: "0.50".into(), size: "100".into() },
            BookLevelRaw { price: "0.49".into(), size: "200".into() },
        ];
        let asks = vec![
            BookLevelRaw { price: "0.52".into(), size: "150".into() },
            BookLevelRaw { price: "0.53".into(), size: "250".into() },
        ];

        manager.apply_snapshot("token1", bids, asks);

        let book = manager.get_book("token1").unwrap();
        assert_eq!(book.best_bid().unwrap().price, Decimal::from_str("0.50").unwrap());
        assert_eq!(book.best_ask().unwrap().price, Decimal::from_str("0.52").unwrap());
    }

    #[test]
    fn test_complement_arbitrage() {
        let manager = OrderBookManager::new();

        // YES token: best ask at 0.45
        manager.apply_snapshot(
            "yes_token",
            vec![BookLevelRaw { price: "0.44".into(), size: "100".into() }],
            vec![BookLevelRaw { price: "0.45".into(), size: "50".into() }],
        );

        // NO token: best ask at 0.52
        manager.apply_snapshot(
            "no_token",
            vec![BookLevelRaw { price: "0.51".into(), size: "100".into() }],
            vec![BookLevelRaw { price: "0.52".into(), size: "75".into() }],
        );

        // Total cost: 0.45 + 0.52 = 0.97, profit margin = 0.03
        let threshold = Decimal::from_str("0.01").unwrap();
        let arb = manager.check_complement_arbitrage("yes_token", "no_token", threshold);

        assert!(arb.is_some());
        let arb = arb.unwrap();
        assert_eq!(arb.profit_margin, Decimal::from_str("0.03").unwrap());
        assert_eq!(arb.executable_size, Decimal::from_str("50").unwrap());
    }
}
