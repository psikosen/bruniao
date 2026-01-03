"""
Event-Driven Debate Worker

Listens for debate requests from RabbitMQ and processes them
using the multi-agent debate system. Results are published back
to the message queue for consumption by other services.

This worker enables:
- Async debate processing (non-blocking trading)
- Scalable debate workers (horizontal scaling)
- Cross-service debate triggers
- Cached debate results in DragonflyDB
"""

import asyncio
import signal
from datetime import datetime
from typing import Optional
import structlog

# Import infrastructure
from infrastructure import (
    CacheClient,
    CachedDebate,
    MessageQueueClient,
    DebateRequestMessage,
    DebateResultMessage,
    TradingDecisionMessage,
    get_cache,
    get_mq,
    close_cache,
    close_mq,
)

# Import debate system
from signals.bot_debate import (
    DebateOrchestrator,
    DEFAULT_BOTS,
    Debate,
)
from signals.prompt_cache import CachedLLMClient

logger = structlog.get_logger()


class DebateWorker:
    """
    Event-driven debate worker.

    Listens for debate requests on RabbitMQ and:
    1. Runs multi-agent debates
    2. Caches results to DragonflyDB
    3. Publishes results back to RabbitMQ
    4. Optionally generates trading decisions from debate conclusions
    """

    def __init__(
        self,
        mq: MessageQueueClient,
        cache: CacheClient,
        llm_client: CachedLLMClient,
        max_concurrent_debates: int = 5,
    ):
        self.mq = mq
        self.cache = cache
        self.llm = llm_client
        self.max_concurrent = max_concurrent_debates
        self.orchestrator = DebateOrchestrator(llm_client, DEFAULT_BOTS)
        self._running = False
        self._active_debates = 0
        self._semaphore = asyncio.Semaphore(max_concurrent_debates)

    async def start(self) -> None:
        """Start the debate worker"""
        self._running = True
        logger.info("debate_worker_starting", max_concurrent=self.max_concurrent)

        # Subscribe to debate requests
        await self.mq.subscribe_debate_requests(self._handle_debate_request)

        logger.info("debate_worker_started")

        # Keep running
        while self._running:
            await asyncio.sleep(1)

    async def stop(self) -> None:
        """Stop the debate worker gracefully"""
        logger.info("debate_worker_stopping", active_debates=self._active_debates)
        self._running = False

        # Wait for active debates to complete
        while self._active_debates > 0:
            await asyncio.sleep(0.5)

        logger.info("debate_worker_stopped")

    async def _handle_debate_request(
        self,
        request: DebateRequestMessage,
    ) -> None:
        """Handle an incoming debate request"""
        async with self._semaphore:
            self._active_debates += 1
            start_time = datetime.utcnow()

            logger.info(
                "debate_request_received",
                debate_id=request.debate_id,
                topic=request.topic,
                market_id=request.market_id,
                urgency=request.urgency,
            )

            try:
                # Check cache for existing debate
                cached = await self.cache.get_cached_debate(request.debate_id)
                if cached:
                    logger.info(
                        "debate_cache_hit",
                        debate_id=request.debate_id,
                    )
                    # Return cached result
                    result = DebateResultMessage(
                        debate_id=cached.debate_id,
                        topic=cached.topic,
                        consensus=cached.consensus,
                        confidence=cached.confidence,
                        decision=None,
                        rounds_completed=(
                            cached.messages_count // len(request.participants)
                            if request.participants
                            else 0
                        ),
                        duration_ms=0,  # From cache
                        timestamp=cached.cached_at,
                    )
                    await self.mq.publish_debate_result(result)
                    return

                # Run the debate
                result = await self._run_debate(request)
                duration_ms = int(
                    (datetime.utcnow() - start_time).total_seconds() * 1000
                )
                result.duration_ms = duration_ms

                # Cache the result
                cached_debate = CachedDebate(
                    debate_id=result.debate_id,
                    topic=result.topic,
                    market_id=request.market_id,
                    consensus=result.consensus,
                    confidence=result.confidence,
                    participants=request.participants,
                    messages_count=result.rounds_completed * len(request.participants),
                    duration_ms=duration_ms,
                    cached_at=int(datetime.utcnow().timestamp()),
                )
                await self.cache.cache_debate(cached_debate)

                # Publish result
                await self.mq.publish_debate_result(result)

                # If debate produced a trading decision, publish it too
                if result.decision:
                    decision_msg = TradingDecisionMessage(**result.decision)
                    await self.mq.publish_trading_decision(decision_msg)

                logger.info(
                    "debate_completed",
                    debate_id=request.debate_id,
                    duration_ms=duration_ms,
                    confidence=result.confidence,
                    has_decision=result.decision is not None,
                )

                # Update metrics
                await self.cache.incr_metric("debates_completed")
                await self.cache.set_metric("last_debate_duration_ms", duration_ms)

            except Exception as e:
                logger.error(
                    "debate_failed",
                    debate_id=request.debate_id,
                    error=str(e),
                )

                # Publish error result
                duration_ms = int(
                    (datetime.utcnow() - start_time).total_seconds() * 1000
                )
                error_result = DebateResultMessage(
                    debate_id=request.debate_id,
                    topic=request.topic,
                    consensus=f"Debate failed: {str(e)}",
                    confidence=0.0,
                    decision=None,
                    rounds_completed=0,
                    duration_ms=duration_ms,
                    timestamp=int(datetime.utcnow().timestamp()),
                )
                await self.mq.publish_debate_result(error_result)

                # Update error metrics
                await self.cache.incr_metric("debates_failed")

            finally:
                self._active_debates -= 1

    async def _run_debate(
        self,
        request: DebateRequestMessage,
    ) -> DebateResultMessage:
        """Run a debate and return the result"""
        # Parse context
        import json

        try:
            context = (
                json.loads(request.context)
                if isinstance(request.context, str)
                else request.context
            )
        except (json.JSONDecodeError, TypeError):
            context = {"raw_context": request.context}

        # Select participants
        participants = [
            bot
            for bot in DEFAULT_BOTS
            if bot.id in request.participants or not request.participants
        ][:4]

        # Create debate
        debate = Debate(
            id=request.debate_id,
            topic=request.topic,
            market_id=request.market_id,
            context=context,
            participants=participants,
        )

        # Set max rounds based on urgency
        max_rounds = {
            "high": 2,  # Fast decision
            "medium": 3,  # Balanced
            "low": 5,  # Thorough
        }.get(request.urgency, request.max_rounds)

        self.orchestrator.max_rounds = max_rounds

        # Run the debate
        completed_debate = await self.orchestrator.run_debate(debate)

        # Analyze consensus to determine trading decision
        trading_decision = await self._extract_trading_decision(
            completed_debate,
            request.market_id,
        )

        # Calculate confidence from debate
        if completed_debate.messages:
            avg_confidence = sum(m.confidence for m in completed_debate.messages) / len(
                completed_debate.messages
            )
        else:
            avg_confidence = 0.5

        return DebateResultMessage(
            debate_id=request.debate_id,
            topic=request.topic,
            consensus=completed_debate.consensus or "No consensus reached",
            confidence=avg_confidence,
            decision=trading_decision,
            rounds_completed=(
                len(completed_debate.messages) // len(participants)
                if participants
                else 0
            ),
            duration_ms=0,  # Will be set by caller
            timestamp=int(datetime.utcnow().timestamp()),
        )

    async def _extract_trading_decision(
        self,
        debate: Debate,
        market_id: str,
    ) -> Optional[dict]:
        """
        Extract a trading decision from debate consensus.

        Returns a TradingDecisionMessage dict if the debate
        resulted in an actionable trading recommendation.
        """
        if not debate.consensus:
            return None

        consensus_lower = debate.consensus.lower()

        # Look for actionable signals
        buy_signals = ["buy", "enter long", "bullish", "take position"]
        sell_signals = ["sell", "exit", "close", "short"]
        hold_signals = ["wait", "hold", "no action", "skip"]

        buy_score = sum(1 for s in buy_signals if s in consensus_lower)
        sell_score = sum(1 for s in sell_signals if s in consensus_lower)
        hold_score = sum(1 for s in hold_signals if s in consensus_lower)

        if hold_score >= max(buy_score, sell_score):
            return None  # No action recommended

        if buy_score > sell_score:
            side = "buy"
            confidence = buy_score / len(buy_signals)
        else:
            side = "sell"
            confidence = sell_score / len(sell_signals)

        # Only create decision if confidence is high enough
        if confidence < 0.5:
            return None

        import uuid

        return {
            "decision_id": str(uuid.uuid4()),
            "decision_type": "debate_consensus",
            "market_id": market_id,
            "token_id": "",  # Would need market data to determine
            "side": side,
            "price": "0",  # Market order
            "size": "1",  # Minimum size
            "order_type": "gtc",
            "confidence": confidence,
            "reason": debate.consensus,
            "timestamp": int(datetime.utcnow().timestamp()),
        }


async def main():
    """Main entry point for debate worker"""
    logger.info("Starting debate worker...")

    # Initialize infrastructure
    cache = await get_cache()
    mq = await get_mq()

    # Initialize LLM client with caching
    from anthropic import AsyncAnthropic

    anthropic_client = AsyncAnthropic()
    llm_client = CachedLLMClient(
        anthropic_client=anthropic_client,
        cache=cache,
    )

    # Create worker
    worker = DebateWorker(
        mq=mq,
        cache=cache,
        llm_client=llm_client,
    )

    # Handle shutdown gracefully
    loop = asyncio.get_event_loop()

    def shutdown_handler():
        logger.info("Shutdown signal received")
        asyncio.create_task(worker.stop())

    for sig in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(sig, shutdown_handler)

    try:
        await worker.start()
    finally:
        await close_cache()
        await close_mq()


if __name__ == "__main__":
    asyncio.run(main())
