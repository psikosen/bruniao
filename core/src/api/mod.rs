//! Polymarket API client module
//!
//! Implements L1/L2 authentication and order management

use crate::models::{Market, Order, OrderStatus, PolymarketConfig, Position, Side, TokenType};
use anyhow::{anyhow, Result};
use chrono::Utc;
use ethers::prelude::*;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tracing::{debug, info};
use uuid::Uuid;

/// Polymarket CLOB API client
pub struct PolymarketClient {
    http_client: Client,
    clob_url: String,
    gamma_url: String,
    api_key: Option<String>,
    api_secret: Option<String>,
    api_passphrase: Option<String>,
    wallet: Option<LocalWallet>,
}

impl PolymarketClient {
    pub async fn new(config: &PolymarketConfig) -> Result<Self> {
        let http_client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        // Parse wallet from private key if provided
        let wallet = if let Some(pk) = &config.private_key {
            Some(pk.parse::<LocalWallet>()?)
        } else {
            None
        };

        Ok(Self {
            http_client,
            clob_url: config.clob_url.clone(),
            gamma_url: config.gamma_url.clone(),
            api_key: config.api_key.clone(),
            api_secret: config.api_secret.clone(),
            api_passphrase: config.api_passphrase.clone(),
            wallet,
        })
    }

    /// Derive L2 API credentials from L1 wallet signature
    /// This is the Polymarket authentication flow
    pub async fn derive_api_key(&mut self) -> Result<ApiCredentials> {
        let wallet = self.wallet.as_ref().ok_or_else(|| anyhow!("No wallet configured"))?;

        // Create the message to sign
        let timestamp = Utc::now().timestamp();
        let nonce = Uuid::new_v4().to_string();
        let message = format!(
            "This message attests that I control the address {} at timestamp {}",
            wallet.address(),
            timestamp
        );

        // Sign with L1 wallet
        let signature = wallet.sign_message(&message).await?;

        // Request API key from CLOB
        let request = DeriveApiKeyRequest {
            message,
            signature: format!("0x{}", hex::encode(signature.to_vec())),
            timestamp,
            nonce,
        };

        let response = self
            .http_client
            .post(format!("{}/auth/derive-api-key", self.clob_url))
            .json(&request)
            .send()
            .await?
            .json::<DeriveApiKeyResponse>()
            .await?;

        // Store credentials
        self.api_key = Some(response.api_key.clone());
        self.api_secret = Some(response.api_secret.clone());
        self.api_passphrase = Some(response.passphrase.clone());

        info!("Successfully derived L2 API credentials");

        Ok(ApiCredentials {
            api_key: response.api_key,
            api_secret: response.api_secret,
            passphrase: response.passphrase,
        })
    }

    /// Place an order on the CLOB
    pub async fn place_order(&self, order: &Order) -> Result<OrderResponse> {
        let api_key = self.api_key.as_ref().ok_or_else(|| anyhow!("No API key"))?;

        let request = CreateOrderRequest {
            token_id: order.token_id.clone(),
            side: match order.side {
                Side::Buy => "BUY".to_string(),
                Side::Sell => "SELL".to_string(),
            },
            price: order.price.to_string(),
            size: order.size.to_string(),
            order_type: match order.order_type {
                crate::models::OrderType::GTC => "GTC".to_string(),
                crate::models::OrderType::GTD { expiration } => {
                    format!("GTD:{}", expiration.timestamp())
                }
                crate::models::OrderType::FOK => "FOK".to_string(),
            },
        };

        let timestamp = Utc::now().timestamp_millis();
        let signature = self.sign_request("POST", "/order", &request, timestamp)?;

        let response = self
            .http_client
            .post(format!("{}/order", self.clob_url))
            .header("POLY-API-KEY", api_key)
            .header("POLY-TIMESTAMP", timestamp.to_string())
            .header("POLY-SIGNATURE", signature)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Order placement failed: {}", error_text));
        }

        let order_response = response.json::<OrderResponse>().await?;
        debug!(order_id = %order_response.order_id, "Order placed");

        Ok(order_response)
    }

    /// Cancel an order
    pub async fn cancel_order(&self, order_id: Uuid) -> Result<()> {
        let api_key = self.api_key.as_ref().ok_or_else(|| anyhow!("No API key"))?;

        let timestamp = Utc::now().timestamp_millis();
        let path = format!("/order/{}", order_id);
        let signature = self.sign_request::<()>("DELETE", &path, &(), timestamp)?;

        let response = self
            .http_client
            .delete(format!("{}{}", self.clob_url, path))
            .header("POLY-API-KEY", api_key)
            .header("POLY-TIMESTAMP", timestamp.to_string())
            .header("POLY-SIGNATURE", signature)
            .send()
            .await?;

        if !response.status().is_success() {
            let error_text = response.text().await?;
            return Err(anyhow!("Order cancellation failed: {}", error_text));
        }

        Ok(())
    }

    /// Get open orders
    pub async fn get_open_orders(&self) -> Result<Vec<Order>> {
        let api_key = self.api_key.as_ref().ok_or_else(|| anyhow!("No API key"))?;

        let timestamp = Utc::now().timestamp_millis();
        let signature = self.sign_request::<()>("GET", "/orders", &(), timestamp)?;

        let response = self
            .http_client
            .get(format!("{}/orders", self.clob_url))
            .header("POLY-API-KEY", api_key)
            .header("POLY-TIMESTAMP", timestamp.to_string())
            .header("POLY-SIGNATURE", signature)
            .send()
            .await?
            .json::<Vec<OrderResponse>>()
            .await?;

        let orders = response
            .into_iter()
            .map(|r| r.into_order())
            .collect::<Result<Vec<_>>>()?;

        Ok(orders)
    }

    /// Get markets from Gamma API
    pub async fn get_markets(&self) -> Result<Vec<Market>> {
        let response = self
            .http_client
            .get(format!("{}/markets", self.gamma_url))
            .send()
            .await?
            .json::<Vec<MarketResponse>>()
            .await?;

        let markets = response.into_iter().map(|r| r.into()).collect();
        Ok(markets)
    }

    /// Get specific market
    pub async fn get_market(&self, condition_id: &str) -> Result<Market> {
        let response = self
            .http_client
            .get(format!("{}/markets/{}", self.gamma_url, condition_id))
            .send()
            .await?
            .json::<MarketResponse>()
            .await?;

        Ok(response.into())
    }

    /// Get user positions
    pub async fn get_positions(&self) -> Result<Vec<Position>> {
        let api_key = self.api_key.as_ref().ok_or_else(|| anyhow!("No API key"))?;

        let timestamp = Utc::now().timestamp_millis();
        let signature = self.sign_request::<()>("GET", "/positions", &(), timestamp)?;

        let response = self
            .http_client
            .get(format!("{}/positions", self.clob_url))
            .header("POLY-API-KEY", api_key)
            .header("POLY-TIMESTAMP", timestamp.to_string())
            .header("POLY-SIGNATURE", signature)
            .send()
            .await?
            .json::<Vec<PositionResponse>>()
            .await?;

        let positions = response.into_iter().map(|r| r.into()).collect();
        Ok(positions)
    }

    /// Get user balance
    pub async fn get_balance(&self) -> Result<Decimal> {
        let api_key = self.api_key.as_ref().ok_or_else(|| anyhow!("No API key"))?;

        let timestamp = Utc::now().timestamp_millis();
        let signature = self.sign_request::<()>("GET", "/balance", &(), timestamp)?;

        let response = self
            .http_client
            .get(format!("{}/balance", self.clob_url))
            .header("POLY-API-KEY", api_key)
            .header("POLY-TIMESTAMP", timestamp.to_string())
            .header("POLY-SIGNATURE", signature)
            .send()
            .await?
            .json::<BalanceResponse>()
            .await?;

        Decimal::from_str(&response.balance).map_err(|e| anyhow!("Invalid balance: {}", e))
    }

    /// Sign a request for L2 authentication
    fn sign_request<T: Serialize + 'static>(
        &self,
        method: &str,
        path: &str,
        body: &T,
        timestamp: i64,
    ) -> Result<String> {
        let api_secret = self
            .api_secret
            .as_ref()
            .ok_or_else(|| anyhow!("No API secret"))?;

        let body_str = if std::any::TypeId::of::<T>() == std::any::TypeId::of::<()>() {
            String::new()
        } else {
            serde_json::to_string(body)?
        };

        let message = format!("{}{}{}{}", timestamp, method, path, body_str);

        // HMAC-SHA256
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        type HmacSha256 = Hmac<Sha256>;
        let mut mac =
            HmacSha256::new_from_slice(api_secret.as_bytes()).expect("HMAC can take any key size");
        mac.update(message.as_bytes());
        let result = mac.finalize();

        Ok(hex::encode(result.into_bytes()))
    }
}

// Request/Response types

#[derive(Serialize)]
struct DeriveApiKeyRequest {
    message: String,
    signature: String,
    timestamp: i64,
    nonce: String,
}

#[derive(Deserialize)]
struct DeriveApiKeyResponse {
    api_key: String,
    api_secret: String,
    passphrase: String,
}

pub struct ApiCredentials {
    pub api_key: String,
    pub api_secret: String,
    pub passphrase: String,
}

#[derive(Serialize)]
struct CreateOrderRequest {
    token_id: String,
    side: String,
    price: String,
    size: String,
    order_type: String,
}

#[derive(Deserialize)]
pub struct OrderResponse {
    pub order_id: String,
    pub token_id: String,
    pub side: String,
    pub price: String,
    pub size: String,
    pub filled_size: String,
    pub status: String,
    pub created_at: String,
}

impl OrderResponse {
    fn into_order(self) -> Result<Order> {
        let id = Uuid::parse_str(&self.order_id)?;
        let price = Decimal::from_str(&self.price)?;
        let size = Decimal::from_str(&self.size)?;
        let filled_size = Decimal::from_str(&self.filled_size)?;

        let side = match self.side.as_str() {
            "BUY" => Side::Buy,
            "SELL" => Side::Sell,
            _ => return Err(anyhow!("Invalid side: {}", self.side)),
        };

        let status = match self.status.as_str() {
            "OPEN" => OrderStatus::Open,
            "FILLED" => OrderStatus::Filled,
            "PARTIALLY_FILLED" => OrderStatus::PartiallyFilled,
            "CANCELLED" => OrderStatus::Cancelled,
            _ => OrderStatus::Pending,
        };

        Ok(Order {
            id,
            market_id: String::new(), // Would need to look up from token_id
            token_id: self.token_id,
            token_type: TokenType::Yes, // Would need to determine from context
            side,
            price,
            size,
            order_type: crate::models::OrderType::GTC,
            status,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            filled_size,
        })
    }
}

#[derive(Deserialize)]
struct MarketResponse {
    condition_id: String,
    question: String,
    description: Option<String>,
    outcomes: Vec<String>,
    tokens: Vec<TokenResponse>,
    active: bool,
    closed: bool,
    end_date_iso: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    token_id: String,
    outcome: String,
    winner: Option<bool>,
}

impl From<MarketResponse> for Market {
    fn from(r: MarketResponse) -> Self {
        Market {
            condition_id: r.condition_id,
            question: r.question,
            description: r.description,
            outcomes: r.outcomes,
            tokens: r
                .tokens
                .into_iter()
                .map(|t| crate::models::Token {
                    token_id: t.token_id,
                    outcome: t.outcome,
                    winner: t.winner,
                })
                .collect(),
            active: r.active,
            closed: r.closed,
            end_date: r.end_date_iso.and_then(|s| s.parse().ok()),
        }
    }
}

#[derive(Deserialize)]
struct PositionResponse {
    market_id: String,
    token_id: String,
    side: String,
    size: String,
    avg_price: String,
    current_price: String,
}

impl From<PositionResponse> for Position {
    fn from(r: PositionResponse) -> Self {
        let size = Decimal::from_str(&r.size).unwrap_or(Decimal::ZERO);
        let avg_price = Decimal::from_str(&r.avg_price).unwrap_or(Decimal::ZERO);
        let current_price = Decimal::from_str(&r.current_price).unwrap_or(Decimal::ZERO);

        Position {
            market_id: r.market_id,
            token_id: r.token_id,
            token_type: if r.side == "YES" {
                TokenType::Yes
            } else {
                TokenType::No
            },
            size,
            average_entry_price: avg_price,
            current_price,
            unrealized_pnl: size * (current_price - avg_price),
            realized_pnl: Decimal::ZERO,
        }
    }
}

#[derive(Deserialize)]
struct BalanceResponse {
    balance: String,
}

// Add hmac and sha2 dependencies
