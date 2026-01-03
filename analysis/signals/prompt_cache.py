"""
Prompt Caching for LLM Interactions

Implements efficient caching for Claude/OpenAI API calls to reduce
latency and costs for repetitive analysis patterns.

Based on: https://ngrok.com/blog/prompt-caching
"""

import hashlib
import json
import time
from dataclasses import dataclass
from typing import Optional, Dict, Any, List
from datetime import datetime, timedelta
import asyncio
import structlog

logger = structlog.get_logger()

# Try to import LLM clients
try:
    from anthropic import AsyncAnthropic

    HAS_ANTHROPIC = True
except ImportError:
    HAS_ANTHROPIC = False

try:
    from openai import AsyncOpenAI

    HAS_OPENAI = True
except ImportError:
    HAS_OPENAI = False


@dataclass
class CacheEntry:
    """A cached prompt response."""

    key: str
    response: str
    model: str
    tokens_used: int
    created_at: datetime
    ttl_seconds: int
    hit_count: int = 0

    def is_expired(self) -> bool:
        return datetime.utcnow() > self.created_at + timedelta(seconds=self.ttl_seconds)


@dataclass
class CacheStats:
    """Cache performance statistics."""

    hits: int = 0
    misses: int = 0
    evictions: int = 0
    total_tokens_saved: int = 0

    @property
    def hit_rate(self) -> float:
        total = self.hits + self.misses
        return self.hits / total if total > 0 else 0.0


class PromptCache:
    """
    In-memory cache for LLM prompt responses.

    Supports:
    - TTL-based expiration
    - LRU eviction
    - Semantic key generation
    - Token savings tracking
    """

    def __init__(
        self,
        max_entries: int = 1000,
        default_ttl: int = 3600,  # 1 hour
    ):
        self.max_entries = max_entries
        self.default_ttl = default_ttl
        self.cache: Dict[str, CacheEntry] = {}
        self.stats = CacheStats()
        self._lock = asyncio.Lock()

    def _generate_key(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str,
        temperature: float,
    ) -> str:
        """Generate a cache key from prompt parameters."""
        content = json.dumps(
            {
                "system": system_prompt,
                "user": user_prompt,
                "model": model,
                "temperature": temperature,
            },
            sort_keys=True,
        )
        return hashlib.sha256(content.encode()).hexdigest()[:32]

    async def get(self, key: str) -> Optional[str]:
        """Get a cached response."""
        async with self._lock:
            entry = self.cache.get(key)
            if entry is None:
                self.stats.misses += 1
                return None

            if entry.is_expired():
                del self.cache[key]
                self.stats.misses += 1
                self.stats.evictions += 1
                return None

            entry.hit_count += 1
            self.stats.hits += 1
            self.stats.total_tokens_saved += entry.tokens_used

            logger.debug("cache_hit", key=key, hit_count=entry.hit_count)
            return entry.response

    async def set(
        self,
        key: str,
        response: str,
        model: str,
        tokens_used: int,
        ttl: Optional[int] = None,
    ):
        """Set a cached response."""
        async with self._lock:
            # Evict if at capacity (LRU)
            if len(self.cache) >= self.max_entries:
                await self._evict_lru()

            self.cache[key] = CacheEntry(
                key=key,
                response=response,
                model=model,
                tokens_used=tokens_used,
                created_at=datetime.utcnow(),
                ttl_seconds=ttl or self.default_ttl,
            )

    async def _evict_lru(self):
        """Evict the least recently used entry."""
        if not self.cache:
            return

        # Find entry with lowest hit count and oldest creation
        lru_key = min(
            self.cache.keys(),
            key=lambda k: (self.cache[k].hit_count, self.cache[k].created_at),
        )
        del self.cache[lru_key]
        self.stats.evictions += 1

    def get_stats(self) -> CacheStats:
        """Get cache statistics."""
        return self.stats


class CachedLLMClient:
    """
    LLM client with prompt caching support.

    Implements caching strategies optimized for trading bot analysis:
    - Market analysis prompts (high cache potential)
    - Decision explanations (medium cache potential)
    - Real-time signals (low cache potential)
    """

    def __init__(
        self,
        provider: str = "anthropic",  # or "openai"
        api_key: Optional[str] = None,
        cache: Optional[PromptCache] = None,
    ):
        self.provider = provider
        self.cache = cache or PromptCache()

        if provider == "anthropic" and HAS_ANTHROPIC:
            self.client = AsyncAnthropic(api_key=api_key)
        elif provider == "openai" and HAS_OPENAI:
            self.client = AsyncOpenAI(api_key=api_key)
        else:
            raise ValueError(f"Unsupported provider: {provider}")

    async def complete(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str = "claude-3-haiku-20240307",
        temperature: float = 0.0,
        max_tokens: int = 1024,
        use_cache: bool = True,
        cache_ttl: Optional[int] = None,
    ) -> str:
        """
        Get a completion with optional caching.

        Low temperature (0.0) is recommended for cacheable responses.
        """
        # Generate cache key
        cache_key = self.cache._generate_key(
            system_prompt, user_prompt, model, temperature
        )

        # Check cache
        if use_cache:
            cached = await self.cache.get(cache_key)
            if cached:
                return cached

        # Make API call
        start_time = time.time()

        if self.provider == "anthropic":
            response = await self._anthropic_complete(
                system_prompt, user_prompt, model, temperature, max_tokens
            )
        else:
            response = await self._openai_complete(
                system_prompt, user_prompt, model, temperature, max_tokens
            )

        elapsed_ms = (time.time() - start_time) * 1000
        logger.info(
            "llm_completion",
            model=model,
            elapsed_ms=round(elapsed_ms, 2),
            cached=False,
        )

        # Cache response
        if use_cache and temperature == 0.0:
            # Estimate tokens (rough approximation)
            tokens_used = len(response.split()) * 1.3
            await self.cache.set(
                cache_key, response, model, int(tokens_used), cache_ttl
            )

        return response

    async def _anthropic_complete(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str,
        temperature: float,
        max_tokens: int,
    ) -> str:
        """Make Anthropic API call."""
        response = await self.client.messages.create(
            model=model,
            max_tokens=max_tokens,
            temperature=temperature,
            system=system_prompt,
            messages=[{"role": "user", "content": user_prompt}],
        )
        return response.content[0].text

    async def _openai_complete(
        self,
        system_prompt: str,
        user_prompt: str,
        model: str,
        temperature: float,
        max_tokens: int,
    ) -> str:
        """Make OpenAI API call."""
        response = await self.client.chat.completions.create(
            model=model,
            max_tokens=max_tokens,
            temperature=temperature,
            messages=[
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt},
            ],
        )
        return response.choices[0].message.content


# Pre-defined system prompts for trading analysis (cacheable prefixes)

MARKET_ANALYSIS_SYSTEM = """You are a trading analyst for prediction markets.
Your role is to analyze market conditions and provide actionable insights.

Key metrics to consider:
- Spread (best ask - best bid)
- Depth (size at each price level)
- Recent trade flow (buys vs sells)
- Time to resolution
- Information asymmetry signals

Respond with concise, structured analysis."""

DECISION_EXPLANATION_SYSTEM = """You are explaining a trading bot's decision.
Describe the reasoning behind a specific trade in clear, educational terms.

Include:
- Market context at decision time
- Strategy that triggered the trade
- Risk assessment
- Expected outcome

Be concise but thorough."""

BOT_DEBATE_SYSTEM = """You are a trading bot participating in a strategy debate.
Your persona: {bot_name} - {bot_role}

Engage constructively with other bots' perspectives.
Base arguments on data and logic, not emotion.
Acknowledge valid counterpoints.
Aim for consensus when possible.

Current debate topic: {topic}"""


class TradingAnalyzer:
    """
    High-level trading analysis with prompt caching.
    """

    def __init__(self, llm_client: CachedLLMClient):
        self.llm = llm_client

    async def analyze_market(
        self,
        market_id: str,
        question: str,
        best_bid: float,
        best_ask: float,
        bid_size: float,
        ask_size: float,
        recent_trades: List[Dict[str, Any]],
    ) -> str:
        """Analyze market conditions."""
        user_prompt = f"""Market: {market_id}
Question: {question}

Current State:
- Best Bid: {best_bid:.4f} (size: {bid_size:.2f})
- Best Ask: {best_ask:.4f} (size: {ask_size:.2f})
- Spread: {(best_ask - best_bid):.4f}

Recent Trades (last 10):
{json.dumps(recent_trades[-10:], indent=2)}

Provide analysis of current market conditions and any notable patterns."""

        return await self.llm.complete(
            system_prompt=MARKET_ANALYSIS_SYSTEM,
            user_prompt=user_prompt,
            use_cache=True,
            cache_ttl=300,  # 5 minute cache for market analysis
        )

    async def explain_decision(
        self,
        decision_type: str,
        side: str,
        price: float,
        size: float,
        reason: str,
        market_context: Dict[str, Any],
    ) -> str:
        """Generate human-readable explanation of a trading decision."""
        user_prompt = f"""Trading Decision to Explain:
- Type: {decision_type}
- Side: {side}
- Price: {price:.4f}
- Size: {size:.2f}
- Bot's Reason: {reason}

Market Context:
{json.dumps(market_context, indent=2)}

Explain this decision in clear terms."""

        return await self.llm.complete(
            system_prompt=DECISION_EXPLANATION_SYSTEM,
            user_prompt=user_prompt,
            use_cache=True,
            cache_ttl=3600,  # 1 hour cache for explanations
        )
