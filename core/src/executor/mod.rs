//! Order execution module
//!
//! Handles order placement, cancellation, and position tracking

use crate::api::PolymarketClient;
use crate::models::{Config, Order, OrderStatus, Position, Side, TradingDecision};
use anyhow::Result;
use parking_lot::RwLock;
use rust_decimal::Decimal;
use std::collections::HashMap;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Order executor for placing and managing orders
pub struct OrderExecutor {
    client: PolymarketClient,
    dry_run: bool,
    /// Active orders by order ID
    orders: RwLock<HashMap<Uuid, Order>>,
    /// Positions by market ID
    positions: RwLock<HashMap<String, Position>>,
    /// Order history for analysis
    order_history: RwLock<Vec<Order>>,
}

impl OrderExecutor {
    pub async fn new(config: &Config, dry_run: bool) -> Result<Self> {
        let client = PolymarketClient::new(&config.polymarket).await?;

        Ok(Self {
            client,
            dry_run,
            orders: RwLock::new(HashMap::new()),
            positions: RwLock::new(HashMap::new()),
            order_history: RwLock::new(Vec::new()),
        })
    }

    /// Execute a trading decision
    pub async fn execute(&self, decision: TradingDecision) -> Result<Order> {
        let mut order = decision.order;

        info!(
            decision_id = %decision.decision_id,
            order_id = %order.id,
            market = %order.market_id,
            side = ?order.side,
            price = %order.price,
            size = %order.size,
            reason = %decision.reason,
            "Executing trading decision"
        );

        if self.dry_run {
            info!("DRY RUN: Would place order {:?}", order);
            order.status = OrderStatus::Open;
            self.orders.write().insert(order.id, order.clone());
            return Ok(order);
        }

        // Place order via API
        match self.client.place_order(&order).await {
            Ok(_response) => {
                order.status = OrderStatus::Open;
                info!(
                    order_id = %order.id,
                    "Order placed successfully"
                );
                self.orders.write().insert(order.id, order.clone());
                Ok(order)
            }
            Err(e) => {
                order.status = OrderStatus::Rejected;
                error!(
                    order_id = %order.id,
                    error = ?e,
                    "Failed to place order"
                );
                self.order_history.write().push(order.clone());
                Err(e)
            }
        }
    }

    /// Cancel a specific order
    pub async fn cancel_order(&self, order_id: Uuid) -> Result<()> {
        if self.dry_run {
            info!("DRY RUN: Would cancel order {}", order_id);
            if let Some(mut order) = self.orders.write().remove(&order_id) {
                order.status = OrderStatus::Cancelled;
                self.order_history.write().push(order);
            }
            return Ok(());
        }

        self.client.cancel_order(order_id).await?;

        if let Some(mut order) = self.orders.write().remove(&order_id) {
            order.status = OrderStatus::Cancelled;
            self.order_history.write().push(order);
        }

        info!(order_id = %order_id, "Order cancelled");
        Ok(())
    }

    /// Cancel all open orders
    pub async fn cancel_all_orders(&self) -> Result<()> {
        let order_ids: Vec<Uuid> = self.orders.read().keys().cloned().collect();

        info!("Cancelling {} open orders", order_ids.len());

        for order_id in order_ids {
            if let Err(e) = self.cancel_order(order_id).await {
                warn!(order_id = %order_id, error = ?e, "Failed to cancel order");
            }
        }

        Ok(())
    }

    /// Get all open orders
    pub fn get_open_orders(&self) -> Vec<Order> {
        self.orders.read().values().cloned().collect()
    }

    /// Get orders for a specific market
    pub fn get_market_orders(&self, market_id: &str) -> Vec<Order> {
        self.orders
            .read()
            .values()
            .filter(|o| o.market_id == market_id)
            .cloned()
            .collect()
    }

    /// Get current positions
    pub async fn get_positions(&self) -> Result<Vec<Position>> {
        // In a real implementation, this would fetch from the API
        Ok(self.positions.read().values().cloned().collect())
    }

    /// Update position from a fill
    pub fn update_position_from_fill(
        &self,
        market_id: &str,
        token_id: &str,
        token_type: crate::models::TokenType,
        side: Side,
        price: Decimal,
        size: Decimal,
    ) {
        let mut positions = self.positions.write();

        let position = positions.entry(market_id.to_string()).or_insert(Position {
            market_id: market_id.to_string(),
            token_id: token_id.to_string(),
            token_type,
            size: Decimal::ZERO,
            average_entry_price: Decimal::ZERO,
            current_price: price,
            unrealized_pnl: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
        });

        match side {
            Side::Buy => {
                // Average up the entry price
                let new_cost = position.average_entry_price * position.size + price * size;
                let new_size = position.size + size;
                if !new_size.is_zero() {
                    position.average_entry_price = new_cost / new_size;
                }
                position.size = new_size;
            }
            Side::Sell => {
                // Realize P&L
                let realized = (price - position.average_entry_price) * size;
                position.realized_pnl += realized;
                position.size -= size;

                // Clean up closed positions
                if position.size.is_zero() {
                    positions.remove(market_id);
                }
            }
        }
    }

    /// Get order by ID
    pub fn get_order(&self, order_id: Uuid) -> Option<Order> {
        self.orders.read().get(&order_id).cloned()
    }

    /// Check if we have open orders in a market
    pub fn has_open_orders(&self, market_id: &str) -> bool {
        self.orders
            .read()
            .values()
            .any(|o| o.market_id == market_id)
    }

    /// Get order history
    pub fn get_order_history(&self) -> Vec<Order> {
        self.order_history.read().clone()
    }

    /// Replace an existing order with a new one
    pub async fn replace_order(&self, old_order_id: Uuid, new_order: Order) -> Result<Order> {
        // Cancel old order first
        self.cancel_order(old_order_id).await?;

        // Place new order
        let decision = TradingDecision {
            decision_id: Uuid::new_v4(),
            decision_type: crate::models::DecisionType::MarketMaking,
            order: new_order,
            reason: "Order replacement".to_string(),
            confidence: 1.0,
            timestamp: chrono::Utc::now(),
        };

        self.execute(decision).await
    }

    /// Sync orders with the exchange
    pub async fn sync_orders(&self) -> Result<()> {
        if self.dry_run {
            return Ok(());
        }

        let exchange_orders = self.client.get_open_orders().await?;

        let mut orders = self.orders.write();

        // Remove orders that are no longer on the exchange
        let exchange_ids: std::collections::HashSet<_> =
            exchange_orders.iter().map(|o| o.id).collect();

        orders.retain(|id, _order| {
            if !exchange_ids.contains(id) {
                debug!(order_id = %id, "Order no longer on exchange");
                false
            } else {
                true
            }
        });

        // Update statuses
        for exchange_order in exchange_orders {
            if let Some(local_order) = orders.get_mut(&exchange_order.id) {
                local_order.status = exchange_order.status;
                local_order.filled_size = exchange_order.filled_size;
            }
        }

        Ok(())
    }
}
