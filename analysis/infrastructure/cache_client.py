"""
DragonflyDB/Redis Cache Client

High-performance distributed caching for:
- Prompt/LLM response caching
- Market data caching
- Session state management
- Cross-service data sharing
"""

import asyncio
import json
import hashlib
import os
from datetime import datetime
from typing import Any, Optional, TypeVar, Generic
from dataclasses import dataclass, asdict
from decimal import Decimal

import redis.asyncio as redis
import structlog

logger = structlog.get_logger()

T = TypeVar('T')


# Cache key prefixes
class CacheKeys:
    ORDERBOOK = "ob"
    MARKET = "mkt"
    POSITION = "pos"
    ORDER = "ord"
    STRATEGY = "strat"
    RATE_LIMIT = "rl"
    SESSION = "sess"
    LOCK = "lock"
    PUBSUB = "ps"
    METRICS = "metrics"
    PROMPT = "prompt"
    DEBATE = "debate"
    EMBEDDING = "emb"


# Cache TTL presets (in seconds)
class CacheTTL:
    ORDERBOOK = 5
    MARKET_DATA = 60
    POSITION = 30
    ORDER = 300
    RATE_LIMIT = 60
    SESSION = 3600
    LOCK = 30
    STRATEGY_STATE = 10
    PROMPT_CACHE = 3600      # LLM responses
    DEBATE_CACHE = 1800      # Bot debates (30 min)
    EMBEDDING_CACHE = 86400  # Embeddings (24 hours)


@dataclass
class CachedPromptResponse:
    """Cached LLM response"""
    prompt_hash: str
    model: str
    response: str
    tokens_used: int
    temperature: float
    cached_at: int
    ttl: int


@dataclass
class CachedDebate:
    """Cached bot debate"""
    debate_id: str
    topic: str
    market_id: str
    consensus: str
    confidence: float
    participants: list[str]
    messages_count: int
    duration_ms: int
    cached_at: int


@dataclass
class CachedMarketData:
    """Cached market data snapshot"""
    market_id: str
    condition_id: str
    question: str
    yes_token_id: str
    no_token_id: str
    yes_price: str
    no_price: str
    volume_24h: str
    liquidity: str
    active: bool
    updated_at: int


class CacheClient:
    """High-performance async cache client for DragonflyDB"""

    def __init__(
        self,
        url: Optional[str] = None,
        prefix: str = "bruniao",
        pool_size: int = 10,
    ):
        self.url = url or os.getenv("DRAGONFLY_URL", "redis://localhost:6379")
        self.prefix = prefix
        self.pool_size = pool_size
        self._pool: Optional[redis.ConnectionPool] = None
        self._client: Optional[redis.Redis] = None
        self._stats = {"hits": 0, "misses": 0, "sets": 0}

    async def connect(self) -> None:
        """Connect to DragonflyDB"""
        self._pool = redis.ConnectionPool.from_url(
            self.url,
            max_connections=self.pool_size,
            decode_responses=True,
        )
        self._client = redis.Redis(connection_pool=self._pool)

        # Test connection
        pong = await self._client.ping()
        if pong:
            logger.info("dragonfly_connected", url=self.url)
        else:
            raise ConnectionError("Failed to connect to DragonflyDB")

    async def close(self) -> None:
        """Close connection"""
        if self._client:
            await self._client.close()
        if self._pool:
            await self._pool.disconnect()

    def _key(self, namespace: str, key: str) -> str:
        """Build a cache key with prefix"""
        return f"{self.prefix}:{namespace}:{key}"

    async def set(
        self,
        namespace: str,
        key: str,
        value: Any,
        ttl: int = 3600,
    ) -> None:
        """Set a value with TTL"""
        full_key = self._key(namespace, key)
        serialized = json.dumps(value, default=str)
        await self._client.setex(full_key, ttl, serialized)
        self._stats["sets"] += 1
        logger.debug("cache_set", key=full_key, ttl=ttl)

    async def get(self, namespace: str, key: str) -> Optional[Any]:
        """Get a value"""
        full_key = self._key(namespace, key)
        value = await self._client.get(full_key)

        if value:
            self._stats["hits"] += 1
            logger.debug("cache_hit", key=full_key)
            return json.loads(value)
        else:
            self._stats["misses"] += 1
            logger.debug("cache_miss", key=full_key)
            return None

    async def delete(self, namespace: str, key: str) -> bool:
        """Delete a key"""
        full_key = self._key(namespace, key)
        deleted = await self._client.delete(full_key)
        return deleted > 0

    async def exists(self, namespace: str, key: str) -> bool:
        """Check if key exists"""
        full_key = self._key(namespace, key)
        return await self._client.exists(full_key) > 0

    async def incr(self, namespace: str, key: str) -> int:
        """Increment a counter"""
        full_key = self._key(namespace, key)
        return await self._client.incr(full_key)

    async def incr_with_ttl(self, namespace: str, key: str, ttl: int) -> int:
        """Increment with TTL (for rate limiting)"""
        full_key = self._key(namespace, key)
        pipe = self._client.pipeline()
        pipe.incr(full_key)
        pipe.expire(full_key, ttl)
        results = await pipe.execute()
        return results[0]

    # ==================== Rate Limiting ====================

    async def check_rate_limit(
        self,
        key: str,
        max_requests: int,
        window_secs: int,
    ) -> bool:
        """Check if rate limit allows request"""
        count = await self.incr_with_ttl(CacheKeys.RATE_LIMIT, key, window_secs)
        return count <= max_requests

    # ==================== Distributed Locking ====================

    async def acquire_lock(
        self,
        lock_name: str,
        holder_id: str,
        ttl: int = 30,
    ) -> bool:
        """Acquire a distributed lock"""
        full_key = self._key(CacheKeys.LOCK, lock_name)
        acquired = await self._client.set(
            full_key, holder_id, nx=True, ex=ttl
        )
        return acquired is not None

    async def release_lock(self, lock_name: str, holder_id: str) -> bool:
        """Release a lock (only if we hold it)"""
        full_key = self._key(CacheKeys.LOCK, lock_name)
        current_holder = await self._client.get(full_key)
        if current_holder == holder_id:
            await self._client.delete(full_key)
            return True
        return False

    # ==================== Prompt Caching ====================

    def _hash_prompt(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str,
        temperature: float,
    ) -> str:
        """Generate a hash for prompt caching"""
        content = f"{system_prompt}|{user_prompt}|{model}|{temperature}"
        return hashlib.sha256(content.encode()).hexdigest()[:32]

    async def get_cached_prompt(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str,
        temperature: float = 0.0,
    ) -> Optional[CachedPromptResponse]:
        """Get cached LLM response"""
        prompt_hash = self._hash_prompt(system_prompt, user_prompt, model, temperature)
        data = await self.get(CacheKeys.PROMPT, prompt_hash)
        if data:
            return CachedPromptResponse(**data)
        return None

    async def cache_prompt_response(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str,
        response: str,
        tokens_used: int,
        temperature: float = 0.0,
        ttl: int = CacheTTL.PROMPT_CACHE,
    ) -> None:
        """Cache an LLM response"""
        prompt_hash = self._hash_prompt(system_prompt, user_prompt, model, temperature)
        cached = CachedPromptResponse(
            prompt_hash=prompt_hash,
            model=model,
            response=response,
            tokens_used=tokens_used,
            temperature=temperature,
            cached_at=int(datetime.utcnow().timestamp()),
            ttl=ttl,
        )
        await self.set(CacheKeys.PROMPT, prompt_hash, asdict(cached), ttl)

    # ==================== Debate Caching ====================

    async def get_cached_debate(
        self,
        debate_id: str,
    ) -> Optional[CachedDebate]:
        """Get cached debate result"""
        data = await self.get(CacheKeys.DEBATE, debate_id)
        if data:
            return CachedDebate(**data)
        return None

    async def cache_debate(
        self,
        debate: CachedDebate,
        ttl: int = CacheTTL.DEBATE_CACHE,
    ) -> None:
        """Cache a debate result"""
        await self.set(CacheKeys.DEBATE, debate.debate_id, asdict(debate), ttl)

    # ==================== Market Data Caching ====================

    async def cache_market_data(
        self,
        market: CachedMarketData,
        ttl: int = CacheTTL.MARKET_DATA,
    ) -> None:
        """Cache market data"""
        await self.set(CacheKeys.MARKET, market.market_id, asdict(market), ttl)

    async def get_market_data(
        self,
        market_id: str,
    ) -> Optional[CachedMarketData]:
        """Get cached market data"""
        data = await self.get(CacheKeys.MARKET, market_id)
        if data:
            return CachedMarketData(**data)
        return None

    async def batch_cache_markets(
        self,
        markets: list[CachedMarketData],
        ttl: int = CacheTTL.MARKET_DATA,
    ) -> None:
        """Batch cache multiple markets"""
        pipe = self._client.pipeline()
        for market in markets:
            key = self._key(CacheKeys.MARKET, market.market_id)
            pipe.setex(key, ttl, json.dumps(asdict(market), default=str))
        await pipe.execute()
        logger.debug("batch_cached_markets", count=len(markets))

    # ==================== Pub/Sub ====================

    async def publish(self, channel: str, message: Any) -> int:
        """Publish a message to a channel"""
        full_channel = self._key(CacheKeys.PUBSUB, channel)
        serialized = json.dumps(message, default=str)
        return await self._client.publish(full_channel, serialized)

    async def subscribe(self, channel: str):
        """Subscribe to a channel (returns async iterator)"""
        full_channel = self._key(CacheKeys.PUBSUB, channel)
        pubsub = self._client.pubsub()
        await pubsub.subscribe(full_channel)
        return pubsub

    # ==================== Metrics ====================

    async def incr_metric(self, metric: str, delta: int = 1) -> int:
        """Increment a metric counter"""
        full_key = self._key(CacheKeys.METRICS, metric)
        return await self._client.incrby(full_key, delta)

    async def set_metric(self, metric: str, value: float) -> None:
        """Set a metric gauge"""
        full_key = self._key(CacheKeys.METRICS, metric)
        await self._client.set(full_key, str(value))

    async def get_metric(self, metric: str) -> Optional[float]:
        """Get a metric value"""
        full_key = self._key(CacheKeys.METRICS, metric)
        value = await self._client.get(full_key)
        if value:
            return float(value)
        return None

    # ==================== Health & Stats ====================

    async def health_check(self) -> bool:
        """Check if cache is healthy"""
        try:
            return await self._client.ping()
        except Exception:
            return False

    def get_stats(self) -> dict:
        """Get cache statistics"""
        total = self._stats["hits"] + self._stats["misses"]
        hit_rate = self._stats["hits"] / total if total > 0 else 0.0
        return {
            **self._stats,
            "hit_rate": hit_rate,
        }


class CachedLLMClient:
    """LLM client wrapper with DragonflyDB caching"""

    def __init__(
        self,
        cache: CacheClient,
        anthropic_client=None,
        openai_client=None,
    ):
        self.cache = cache
        self.anthropic = anthropic_client
        self.openai = openai_client
        self._tokens_saved = 0

    async def complete(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str = "claude-3-sonnet-20240229",
        temperature: float = 0.0,
        max_tokens: int = 1024,
        use_cache: bool = True,
        cache_ttl: int = CacheTTL.PROMPT_CACHE,
    ) -> tuple[str, bool]:
        """
        Complete a prompt with caching support.
        Returns (response, was_cached)
        """
        # Only cache deterministic responses
        if use_cache and temperature == 0.0:
            cached = await self.cache.get_cached_prompt(
                system_prompt, user_prompt, model, temperature
            )
            if cached:
                self._tokens_saved += cached.tokens_used
                logger.info(
                    "llm_cache_hit",
                    model=model,
                    tokens_saved=cached.tokens_used,
                )
                return cached.response, True

        # Call actual LLM
        if "claude" in model.lower():
            response, tokens = await self._call_anthropic(
                system_prompt, user_prompt, model, temperature, max_tokens
            )
        else:
            response, tokens = await self._call_openai(
                system_prompt, user_prompt, model, temperature, max_tokens
            )

        # Cache the response
        if use_cache and temperature == 0.0:
            await self.cache.cache_prompt_response(
                system_prompt=system_prompt,
                user_prompt=user_prompt,
                model=model,
                response=response,
                tokens_used=tokens,
                temperature=temperature,
                ttl=cache_ttl,
            )

        return response, False

    async def _call_anthropic(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str,
        temperature: float,
        max_tokens: int,
    ) -> tuple[str, int]:
        """Call Anthropic API"""
        message = await self.anthropic.messages.create(
            model=model,
            max_tokens=max_tokens,
            temperature=temperature,
            system=system_prompt,
            messages=[{"role": "user", "content": user_prompt}],
        )
        response = message.content[0].text
        tokens = message.usage.input_tokens + message.usage.output_tokens
        return response, tokens

    async def _call_openai(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str,
        temperature: float,
        max_tokens: int,
    ) -> tuple[str, int]:
        """Call OpenAI API"""
        completion = await self.openai.chat.completions.create(
            model=model,
            max_tokens=max_tokens,
            temperature=temperature,
            messages=[
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
        )
        response = completion.choices[0].message.content
        tokens = completion.usage.total_tokens
        return response, tokens

    @property
    def tokens_saved(self) -> int:
        """Total tokens saved by caching"""
        return self._tokens_saved


# Singleton instance for easy access
_cache_instance: Optional[CacheClient] = None


async def get_cache() -> CacheClient:
    """Get or create cache client singleton"""
    global _cache_instance
    if _cache_instance is None:
        _cache_instance = CacheClient()
        await _cache_instance.connect()
    return _cache_instance


async def close_cache() -> None:
    """Close cache client singleton"""
    global _cache_instance
    if _cache_instance:
        await _cache_instance.close()
        _cache_instance = None
