//! Bot Memory Module - Qdrant Vector Storage for Trading Decisions
//!
//! Stores embeddings of trading decisions, outcomes, and market states
//! for semantic search and pattern recognition.

use anyhow::Result;
use chrono::{DateTime, Utc};
use qdrant_client::prelude::*;
use qdrant_client::qdrant::{
    vectors_config::Config, CreateCollection, Distance, PointStruct, SearchPoints, VectorParams,
    VectorsConfig,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const COLLECTION_NAME: &str = "trading_decisions";
const VECTOR_SIZE: u64 = 1536; // OpenAI ada-002 embedding size

/// Memory store for trading bot decisions and outcomes
pub struct BotMemory {
    client: QdrantClient,
    collection_name: String,
}

impl BotMemory {
    /// Create a new memory store
    pub async fn new(url: &str, collection_name: Option<&str>) -> Result<Self> {
        let client = QdrantClient::from_url(url).build()?;
        let collection_name = collection_name.unwrap_or(COLLECTION_NAME).to_string();

        // Create collection if it doesn't exist
        let collections = client.list_collections().await?;
        let exists = collections
            .collections
            .iter()
            .any(|c| c.name == collection_name);

        if !exists {
            client
                .create_collection(&CreateCollection {
                    collection_name: collection_name.clone(),
                    vectors_config: Some(VectorsConfig {
                        config: Some(Config::Params(VectorParams {
                            size: VECTOR_SIZE,
                            distance: Distance::Cosine.into(),
                            ..Default::default()
                        })),
                    }),
                    ..Default::default()
                })
                .await?;
        }

        Ok(Self {
            client,
            collection_name,
        })
    }

    /// Store a trading decision with its embedding
    pub async fn store_decision(&self, memory: &DecisionMemory) -> Result<()> {
        let point = PointStruct::new(
            memory.id.to_string(),
            memory.embedding.clone(),
            serde_json::to_value(memory)?.try_into()?,
        );

        self.client
            .upsert_points(&self.collection_name, None, vec![point], None)
            .await?;

        Ok(())
    }

    /// Search for similar decisions
    pub async fn search_similar(
        &self,
        embedding: Vec<f32>,
        limit: u64,
    ) -> Result<Vec<DecisionMemory>> {
        let results = self
            .client
            .search_points(&SearchPoints {
                collection_name: self.collection_name.clone(),
                vector: embedding,
                limit,
                with_payload: Some(true.into()),
                ..Default::default()
            })
            .await?;

        let memories: Vec<DecisionMemory> = results
            .result
            .into_iter()
            .filter_map(|point| {
                point
                    .payload
                    .get("memory")
                    .and_then(|v| serde_json::from_value(v.clone().into()).ok())
            })
            .collect();

        Ok(memories)
    }

    /// Store a bot debate/discussion
    pub async fn store_debate(&self, debate: &BotDebate) -> Result<()> {
        let point = PointStruct::new(
            debate.id.to_string(),
            debate.embedding.clone(),
            serde_json::to_value(debate)?.try_into()?,
        );

        self.client
            .upsert_points(&self.collection_name, None, vec![point], None)
            .await?;

        Ok(())
    }

    /// Get recent decisions for a market
    pub async fn get_market_history(
        &self,
        market_id: &str,
        limit: usize,
    ) -> Result<Vec<DecisionMemory>> {
        // Use filter to find decisions for this market
        let filter = qdrant_client::qdrant::Filter {
            must: vec![qdrant_client::qdrant::Condition {
                condition_one_of: Some(
                    qdrant_client::qdrant::condition::ConditionOneOf::Field(
                        qdrant_client::qdrant::FieldCondition {
                            key: "market_id".to_string(),
                            r#match: Some(qdrant_client::qdrant::Match {
                                match_value: Some(
                                    qdrant_client::qdrant::r#match::MatchValue::Keyword(
                                        market_id.to_string(),
                                    ),
                                ),
                            }),
                            ..Default::default()
                        },
                    ),
                ),
            }],
            ..Default::default()
        };

        let results = self
            .client
            .scroll(
                &qdrant_client::qdrant::ScrollPoints {
                    collection_name: self.collection_name.clone(),
                    filter: Some(filter),
                    limit: Some(limit as u32),
                    with_payload: Some(true.into()),
                    with_vectors: Some(false.into()),
                    ..Default::default()
                },
            )
            .await?;

        let memories: Vec<DecisionMemory> = results
            .result
            .into_iter()
            .filter_map(|point| {
                serde_json::from_value(
                    point
                        .payload
                        .into_iter()
                        .collect::<serde_json::Map<String, serde_json::Value>>()
                        .into(),
                )
                .ok()
            })
            .collect();

        Ok(memories)
    }
}

/// A trading decision stored in memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionMemory {
    pub id: Uuid,
    pub market_id: String,
    pub decision_type: String,
    pub side: String,
    pub price: f64,
    pub size: f64,
    pub reason: String,
    pub confidence: f64,
    pub outcome: Option<DecisionOutcome>,
    pub market_context: MarketContext,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub embedding: Vec<f32>,
}

/// Outcome of a trading decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionOutcome {
    pub filled: bool,
    pub fill_price: Option<f64>,
    pub pnl: Option<f64>,
    pub resolution_time_ms: Option<u64>,
}

/// Market context at time of decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketContext {
    pub best_bid: f64,
    pub best_ask: f64,
    pub spread: f64,
    pub bid_size: f64,
    pub ask_size: f64,
    pub mid_price: f64,
    pub volatility_estimate: Option<f64>,
}

/// A debate/discussion between bots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotDebate {
    pub id: Uuid,
    pub topic: String,
    pub market_id: Option<String>,
    pub participants: Vec<BotParticipant>,
    pub messages: Vec<DebateMessage>,
    pub consensus: Option<String>,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotParticipant {
    pub id: String,
    pub name: String,
    pub role: String, // e.g., "market_maker", "risk_manager", "analyst"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateMessage {
    pub participant_id: String,
    pub content: String,
    pub reasoning: Option<String>,
    pub confidence: f64,
    pub timestamp: DateTime<Utc>,
}

/// Generate text for embedding from a decision
pub fn decision_to_text(decision: &DecisionMemory) -> String {
    format!(
        "Market: {} | Decision: {} {} @ {} size {} | Reason: {} | Context: bid {} ask {} spread {} | Confidence: {:.2}",
        decision.market_id,
        decision.decision_type,
        decision.side,
        decision.price,
        decision.size,
        decision.reason,
        decision.market_context.best_bid,
        decision.market_context.best_ask,
        decision.market_context.spread,
        decision.confidence
    )
}

/// Generate text for embedding from a debate
pub fn debate_to_text(debate: &BotDebate) -> String {
    let messages: Vec<String> = debate
        .messages
        .iter()
        .map(|m| format!("{}: {}", m.participant_id, m.content))
        .collect();

    format!(
        "Topic: {} | Market: {} | Discussion: {} | Consensus: {}",
        debate.topic,
        debate.market_id.as_deref().unwrap_or("general"),
        messages.join(" | "),
        debate.consensus.as_deref().unwrap_or("none")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decision_to_text() {
        let decision = DecisionMemory {
            id: Uuid::new_v4(),
            market_id: "test_market".into(),
            decision_type: "market_making".into(),
            side: "buy".into(),
            price: 0.55,
            size: 10.0,
            reason: "spread opportunity".into(),
            confidence: 0.8,
            outcome: None,
            market_context: MarketContext {
                best_bid: 0.54,
                best_ask: 0.56,
                spread: 0.02,
                bid_size: 100.0,
                ask_size: 150.0,
                mid_price: 0.55,
                volatility_estimate: None,
            },
            timestamp: Utc::now(),
            embedding: vec![],
        };

        let text = decision_to_text(&decision);
        assert!(text.contains("test_market"));
        assert!(text.contains("market_making"));
    }
}
