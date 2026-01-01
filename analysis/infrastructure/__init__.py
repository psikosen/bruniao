"""
Infrastructure Module

High-performance infrastructure clients for:
- DragonflyDB distributed caching
- RabbitMQ message queuing
"""

from .cache_client import (
    CacheClient,
    CacheKeys,
    CacheTTL,
    CachedDebate,
    CachedLLMClient,
    CachedMarketData,
    CachedPromptResponse,
    get_cache,
    close_cache,
)

from .mq_client import (
    MessageQueueClient,
    Exchanges,
    Queues,
    RoutingKeys,
    TradingDecisionMessage,
    DebateRequestMessage,
    DebateResultMessage,
    RiskAlertMessage,
    MarketAnalysisRequest,
    MarketAnalysisResult,
    EventDrivenDebateOrchestrator,
    get_mq,
    close_mq,
)

__all__ = [
    # Cache
    "CacheClient",
    "CacheKeys",
    "CacheTTL",
    "CachedDebate",
    "CachedLLMClient",
    "CachedMarketData",
    "CachedPromptResponse",
    "get_cache",
    "close_cache",
    # Message Queue
    "MessageQueueClient",
    "Exchanges",
    "Queues",
    "RoutingKeys",
    "TradingDecisionMessage",
    "DebateRequestMessage",
    "DebateResultMessage",
    "RiskAlertMessage",
    "MarketAnalysisRequest",
    "MarketAnalysisResult",
    "EventDrivenDebateOrchestrator",
    "get_mq",
    "close_mq",
]
