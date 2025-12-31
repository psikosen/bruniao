//! Strategy engine module
//!
//! Implements market making and arbitrage strategies

use crate::models::{
    DecisionType, Order, OrderType, Side, StrategyConfig, TokenType, TradingDecision,
};
use crate::orderbook::{MarketMakingQuotes, OrderBookManager};
use rust_decimal::Decimal;
use std::collections::HashMap;
use tracing::{debug, info};
use uuid::Uuid;

/// Market configuration for trading
#[derive(Debug, Clone)]
pub struct MarketConfig {
    pub condition_id: String,
    pub yes_token_id: String,
    pub no_token_id: String,
    pub enabled: bool,
}

/// Strategy engine for generating trading decisions
pub struct StrategyEngine {
    config: StrategyConfig,
    markets: HashMap<String, MarketConfig>,
    /// Track last quote prices to avoid unnecessary updates
    last_quotes: HashMap<String, (Decimal, Decimal)>,
}

impl StrategyEngine {
    pub fn new(config: &StrategyConfig) -> Self {
        Self {
            config: config.clone(),
            markets: HashMap::new(),
            last_quotes: HashMap::new(),
        }
    }

    /// Register a market for trading
    pub fn register_market(&mut self, market: MarketConfig) {
        info!(
            condition_id = %market.condition_id,
            "Registering market for trading"
        );
        self.markets.insert(market.condition_id.clone(), market);
    }

    /// Main strategy tick - generates trading decisions
    pub async fn tick(&mut self, orderbook: &OrderBookManager) -> Vec<TradingDecision> {
        let mut decisions = Vec::new();

        // Run arbitrage detection first (higher priority)
        decisions.extend(self.check_arbitrage_opportunities(orderbook));

        // Run market making
        decisions.extend(self.generate_market_making_quotes(orderbook));

        decisions
    }

    /// Check for complement arbitrage opportunities
    fn check_arbitrage_opportunities(
        &self,
        orderbook: &OrderBookManager,
    ) -> Vec<TradingDecision> {
        let mut decisions = Vec::new();

        for market in self.markets.values() {
            if !market.enabled {
                continue;
            }

            if let Some(arb) = orderbook.check_complement_arbitrage(
                &market.yes_token_id,
                &market.no_token_id,
                self.config.arb_threshold,
            ) {
                info!(
                    market = %market.condition_id,
                    profit_margin = %arb.profit_margin,
                    size = %arb.executable_size,
                    "Arbitrage opportunity detected"
                );

                // Generate buy orders for both YES and NO
                let size = arb.executable_size.min(self.config.mm_quote_size);

                let yes_order = Order::new(
                    market.condition_id.clone(),
                    market.yes_token_id.clone(),
                    TokenType::Yes,
                    Side::Buy,
                    arb.yes_price,
                    size,
                    OrderType::FOK, // Use FOK to ensure we get both legs
                );

                let no_order = Order::new(
                    market.condition_id.clone(),
                    market.no_token_id.clone(),
                    TokenType::No,
                    Side::Buy,
                    arb.no_price,
                    size,
                    OrderType::FOK,
                );

                decisions.push(TradingDecision {
                    decision_id: Uuid::new_v4(),
                    decision_type: DecisionType::Arbitrage,
                    order: yes_order,
                    reason: format!(
                        "Complement arb: YES@{} + NO@{} = {}, margin {}",
                        arb.yes_price,
                        arb.no_price,
                        arb.yes_price + arb.no_price,
                        arb.profit_margin
                    ),
                    confidence: 0.95,
                    timestamp: chrono::Utc::now(),
                });

                decisions.push(TradingDecision {
                    decision_id: Uuid::new_v4(),
                    decision_type: DecisionType::Arbitrage,
                    order: no_order,
                    reason: format!(
                        "Complement arb: YES@{} + NO@{} = {}, margin {}",
                        arb.yes_price,
                        arb.no_price,
                        arb.yes_price + arb.no_price,
                        arb.profit_margin
                    ),
                    confidence: 0.95,
                    timestamp: chrono::Utc::now(),
                });
            }
        }

        decisions
    }

    /// Generate market making quotes
    fn generate_market_making_quotes(
        &mut self,
        orderbook: &OrderBookManager,
    ) -> Vec<TradingDecision> {
        let mut decisions = Vec::new();

        // Collect markets to process (clone to avoid borrow conflicts)
        let markets: Vec<MarketConfig> = self
            .markets
            .values()
            .filter(|m| m.enabled)
            .cloned()
            .collect();

        for market in markets {
            // Generate quotes for YES token
            if let Some(quotes) = orderbook.get_mm_quotes(
                &market.yes_token_id,
                self.config.mm_spread_ticks,
                self.config.mm_quote_size,
                Decimal::ZERO, // Would get inventory skew from risk manager
            ) {
                let decisions_yes = self.create_mm_decisions(&market, &quotes, TokenType::Yes);
                decisions.extend(decisions_yes);
            }

            // Generate quotes for NO token
            if let Some(quotes) = orderbook.get_mm_quotes(
                &market.no_token_id,
                self.config.mm_spread_ticks,
                self.config.mm_quote_size,
                Decimal::ZERO,
            ) {
                let decisions_no = self.create_mm_decisions(&market, &quotes, TokenType::No);
                decisions.extend(decisions_no);
            }
        }

        decisions
    }

    fn create_mm_decisions(
        &mut self,
        market: &MarketConfig,
        quotes: &MarketMakingQuotes,
        token_type: TokenType,
    ) -> Vec<TradingDecision> {
        let mut decisions = Vec::new();
        let token_id = match token_type {
            TokenType::Yes => &market.yes_token_id,
            TokenType::No => &market.no_token_id,
        };

        // Check if quotes have changed significantly
        let should_update = self
            .last_quotes
            .get(token_id)
            .map(|(old_bid, old_ask)| {
                let tick = Decimal::from_str("0.01").unwrap();
                (quotes.bid_price - *old_bid).abs() >= tick
                    || (quotes.ask_price - *old_ask).abs() >= tick
            })
            .unwrap_or(true);

        if !should_update {
            debug!(token = %token_id, "Skipping MM update - prices unchanged");
            return decisions;
        }

        // Update tracked quotes
        self.last_quotes
            .insert(token_id.clone(), (quotes.bid_price, quotes.ask_price));

        // Create bid order
        let bid_order = Order::new(
            market.condition_id.clone(),
            token_id.clone(),
            token_type,
            Side::Buy,
            quotes.bid_price,
            quotes.bid_size,
            OrderType::GTC,
        );

        decisions.push(TradingDecision {
            decision_id: Uuid::new_v4(),
            decision_type: DecisionType::MarketMaking,
            order: bid_order,
            reason: format!("MM bid @ {}", quotes.bid_price),
            confidence: 0.7,
            timestamp: chrono::Utc::now(),
        });

        // Create ask order
        let ask_order = Order::new(
            market.condition_id.clone(),
            token_id.clone(),
            token_type,
            Side::Sell,
            quotes.ask_price,
            quotes.ask_size,
            OrderType::GTC,
        );

        decisions.push(TradingDecision {
            decision_id: Uuid::new_v4(),
            decision_type: DecisionType::MarketMaking,
            order: ask_order,
            reason: format!("MM ask @ {}", quotes.ask_price),
            confidence: 0.7,
            timestamp: chrono::Utc::now(),
        });

        decisions
    }

    /// Clear cached quotes (call when market conditions change significantly)
    pub fn clear_quote_cache(&mut self) {
        self.last_quotes.clear();
    }

    /// Disable a market
    pub fn disable_market(&mut self, condition_id: &str) {
        if let Some(market) = self.markets.get_mut(condition_id) {
            market.enabled = false;
            info!(condition_id = %condition_id, "Market disabled");
        }
    }

    /// Enable a market
    pub fn enable_market(&mut self, condition_id: &str) {
        if let Some(market) = self.markets.get_mut(condition_id) {
            market.enabled = true;
            info!(condition_id = %condition_id, "Market enabled");
        }
    }
}

use std::str::FromStr;

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> StrategyConfig {
        StrategyConfig {
            loop_interval_ms: 100,
            mm_spread_ticks: 1,
            mm_quote_size: Decimal::from(5),
            inventory_skew_threshold: Decimal::from_str("0.5").unwrap(),
            arb_threshold: Decimal::from_str("0.01").unwrap(),
            markets: vec![],
        }
    }

    #[test]
    fn test_market_registration() {
        let config = test_config();
        let mut engine = StrategyEngine::new(&config);

        let market = MarketConfig {
            condition_id: "market1".into(),
            yes_token_id: "yes1".into(),
            no_token_id: "no1".into(),
            enabled: true,
        };

        engine.register_market(market);
        assert!(engine.markets.contains_key("market1"));
    }
}
