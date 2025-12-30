//! Risk management module
//!
//! Enforces trading limits and implements kill-switch functionality

use crate::executor::OrderExecutor;
use crate::models::{Order, Position, RiskConfig, Side, TradingDecision};
use anyhow::Result;
use rust_decimal::Decimal;
use std::collections::HashMap;
use thiserror::Error;
use tracing::{info, warn};

#[derive(Error, Debug)]
pub enum RiskError {
    #[error("Order size {size} exceeds maximum {max}")]
    OrderSizeExceeded { size: Decimal, max: Decimal },

    #[error("Position size would exceed limit: current {current}, order {order}, max {max}")]
    PositionLimitExceeded {
        current: Decimal,
        order: Decimal,
        max: Decimal,
    },

    #[error("Maximum open positions ({max}) reached")]
    MaxPositionsReached { max: usize },

    #[error("Kill switch is active - no trading allowed")]
    KillSwitchActive,

    #[error("Insufficient liquidity: available {available}, required {required}")]
    InsufficientLiquidity { available: Decimal, required: Decimal },

    #[error("Daily drawdown limit breached: {current} > {limit}")]
    DrawdownLimitBreached { current: Decimal, limit: Decimal },
}

/// Risk manager for enforcing trading limits
pub struct RiskManager {
    config: RiskConfig,
    positions: HashMap<String, Position>,
    daily_pnl: Decimal,
    starting_balance: Decimal,
    kill_switch_active: bool,
}

impl RiskManager {
    pub fn new(config: &RiskConfig) -> Self {
        Self {
            config: config.clone(),
            positions: HashMap::new(),
            daily_pnl: Decimal::ZERO,
            starting_balance: Decimal::from(100), // Starting capital
            kill_switch_active: false,
        }
    }

    /// Check if kill switch is active
    pub fn is_kill_switch_active(&self) -> bool {
        self.kill_switch_active
    }

    /// Activate the kill switch
    pub fn activate_kill_switch(&mut self) {
        warn!("KILL SWITCH ACTIVATED - All trading halted");
        self.kill_switch_active = true;
    }

    /// Deactivate the kill switch (manual reset)
    pub fn deactivate_kill_switch(&mut self) {
        info!("Kill switch deactivated - Trading resumed");
        self.kill_switch_active = false;
    }

    /// Validate an order against risk limits
    pub fn validate_order(&self, decision: &TradingDecision) -> Result<(), RiskError> {
        let order = &decision.order;

        // Check kill switch
        if self.kill_switch_active && self.config.kill_switch_enabled {
            return Err(RiskError::KillSwitchActive);
        }

        // Check order size
        let order_notional = order.price * order.size;
        if order_notional > self.config.max_order_size {
            return Err(RiskError::OrderSizeExceeded {
                size: order_notional,
                max: self.config.max_order_size,
            });
        }

        // Check position limit
        let current_position = self
            .positions
            .get(&order.market_id)
            .map(|p| p.notional_value())
            .unwrap_or(Decimal::ZERO);

        let new_position = match order.side {
            Side::Buy => current_position + order_notional,
            Side::Sell => current_position - order_notional,
        };

        if new_position.abs() > self.config.max_position_per_market {
            return Err(RiskError::PositionLimitExceeded {
                current: current_position,
                order: order_notional,
                max: self.config.max_position_per_market,
            });
        }

        // Check max open positions
        if order.side == Side::Buy && !self.positions.contains_key(&order.market_id) {
            if self.positions.len() >= self.config.max_open_positions {
                return Err(RiskError::MaxPositionsReached {
                    max: self.config.max_open_positions,
                });
            }
        }

        Ok(())
    }

    /// Update positions from executor state
    pub async fn update_positions(&mut self, executor: &OrderExecutor) -> Result<()> {
        let positions = executor.get_positions().await?;
        self.positions = positions
            .into_iter()
            .map(|p| (p.market_id.clone(), p))
            .collect();

        // Calculate daily P&L
        self.daily_pnl = self
            .positions
            .values()
            .map(|p| p.unrealized_pnl + p.realized_pnl)
            .sum();

        Ok(())
    }

    /// Check if drawdown limit is breached
    pub fn check_drawdown_breach(&self) -> bool {
        if self.starting_balance.is_zero() {
            return false;
        }

        let drawdown = -self.daily_pnl / self.starting_balance;
        drawdown > self.config.daily_drawdown_limit
    }

    /// Get current daily P&L
    pub fn get_daily_pnl(&self) -> Decimal {
        self.daily_pnl
    }

    /// Get current positions
    pub fn get_positions(&self) -> &HashMap<String, Position> {
        &self.positions
    }

    /// Get number of open positions
    pub fn open_position_count(&self) -> usize {
        self.positions.len()
    }

    /// Calculate inventory skew for a market
    /// Returns normalized value: positive = long, negative = short
    pub fn calculate_inventory_skew(&self, market_id: &str) -> Decimal {
        self.positions
            .get(market_id)
            .map(|p| {
                let max_pos = self.config.max_position_per_market;
                if max_pos.is_zero() {
                    Decimal::ZERO
                } else {
                    p.notional_value() / max_pos
                }
            })
            .unwrap_or(Decimal::ZERO)
    }

    /// Check if we can take on more risk in a market
    pub fn can_increase_position(&self, market_id: &str, amount: Decimal) -> bool {
        let current = self
            .positions
            .get(market_id)
            .map(|p| p.notional_value())
            .unwrap_or(Decimal::ZERO);

        (current + amount).abs() <= self.config.max_position_per_market
    }

    /// Reset daily P&L (call at start of day)
    pub fn reset_daily_pnl(&mut self) {
        info!("Daily P&L reset");
        self.daily_pnl = Decimal::ZERO;
    }

    /// Set starting balance
    pub fn set_starting_balance(&mut self, balance: Decimal) {
        self.starting_balance = balance;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Order, OrderType, TokenType};

    fn test_config() -> RiskConfig {
        RiskConfig {
            max_position_per_market: Decimal::from(10),
            max_open_positions: 5,
            daily_drawdown_limit: Decimal::from_str_exact("0.05").unwrap(),
            min_book_size: Decimal::from(5),
            kill_switch_enabled: true,
            max_order_size: Decimal::from(5),
        }
    }

    #[test]
    fn test_order_size_limit() {
        let config = test_config();
        let risk_manager = RiskManager::new(&config);

        let order = Order::new(
            "market1".into(),
            "token1".into(),
            TokenType::Yes,
            Side::Buy,
            Decimal::from_str_exact("0.50").unwrap(),
            Decimal::from(20), // 20 * 0.50 = 10 > max 5
            OrderType::GTC,
        );

        let decision = TradingDecision {
            decision_id: uuid::Uuid::new_v4(),
            decision_type: crate::models::DecisionType::MarketMaking,
            order,
            reason: "test".into(),
            confidence: 1.0,
            timestamp: chrono::Utc::now(),
        };

        let result = risk_manager.validate_order(&decision);
        assert!(matches!(result, Err(RiskError::OrderSizeExceeded { .. })));
    }

    #[test]
    fn test_kill_switch() {
        let config = test_config();
        let mut risk_manager = RiskManager::new(&config);
        risk_manager.activate_kill_switch();

        let order = Order::new(
            "market1".into(),
            "token1".into(),
            TokenType::Yes,
            Side::Buy,
            Decimal::from_str_exact("0.50").unwrap(),
            Decimal::from(1),
            OrderType::GTC,
        );

        let decision = TradingDecision {
            decision_id: uuid::Uuid::new_v4(),
            decision_type: crate::models::DecisionType::MarketMaking,
            order,
            reason: "test".into(),
            confidence: 1.0,
            timestamp: chrono::Utc::now(),
        };

        let result = risk_manager.validate_order(&decision);
        assert!(matches!(result, Err(RiskError::KillSwitchActive)));
    }
}
