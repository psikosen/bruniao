"""
RabbitMQ Message Queue Client

High-performance async message broker for:
- Trading signal distribution
- Bot debate orchestration
- Cross-service events
- Async task processing
"""

import asyncio
import json
import os
import uuid
from datetime import datetime
from typing import Any, Callable, Coroutine, Optional, TypeVar
from dataclasses import dataclass, asdict

import aio_pika
from aio_pika import ExchangeType, Message, DeliveryMode
from aio_pika.abc import AbstractRobustConnection, AbstractChannel, AbstractQueue
import structlog

logger = structlog.get_logger()

T = TypeVar('T')


# Exchange names
class Exchanges:
    TRADING = "trading"
    SIGNALS = "signals"
    ORDERS = "orders"
    RISK = "risk"
    DEBATES = "debates"
    EVENTS = "events"
    ANALYSIS = "analysis"


# Queue names
class Queues:
    TRADING_DECISIONS = "trading.decisions"
    ORDER_EXECUTION = "orders.execution"
    ORDER_FILLS = "orders.fills"
    RISK_ALERTS = "risk.alerts"
    DEBATE_REQUESTS = "debates.requests"
    DEBATE_RESULTS = "debates.results"
    MARKET_UPDATES = "market.updates"
    STRATEGY_SIGNALS = "strategy.signals"
    ANALYSIS_REQUESTS = "analysis.requests"
    ANALYSIS_RESULTS = "analysis.results"


# Routing keys
class RoutingKeys:
    DECISION_NEW = "decision.new"
    DECISION_EXECUTED = "decision.executed"
    ORDER_PLACED = "order.placed"
    ORDER_FILLED = "order.filled"
    ORDER_CANCELLED = "order.cancelled"
    RISK_WARNING = "risk.warning"
    RISK_CRITICAL = "risk.critical"
    RISK_KILL_SWITCH = "risk.kill_switch"
    DEBATE_START = "debate.start"
    DEBATE_MESSAGE = "debate.message"
    DEBATE_CONSENSUS = "debate.consensus"
    MARKET_ORDERBOOK = "market.orderbook"
    MARKET_TRADE = "market.trade"
    SIGNAL_MM = "signal.market_making"
    SIGNAL_ARB = "signal.arbitrage"
    ANALYSIS_REQUEST = "analysis.request"
    ANALYSIS_COMPLETE = "analysis.complete"


@dataclass
class TradingDecisionMessage:
    """Trading decision from strategy engine"""
    decision_id: str
    decision_type: str  # "market_making" | "arbitrage"
    market_id: str
    token_id: str
    side: str  # "buy" | "sell"
    price: str
    size: str
    order_type: str  # "gtc" | "fok" | "ioc"
    confidence: float
    reason: str
    timestamp: int


@dataclass
class DebateRequestMessage:
    """Request for bot debate"""
    debate_id: str
    topic: str
    market_id: str
    context: str
    urgency: str  # "low" | "medium" | "high"
    max_rounds: int
    participants: list[str]
    timestamp: int


@dataclass
class DebateResultMessage:
    """Result of bot debate"""
    debate_id: str
    topic: str
    consensus: str
    confidence: float
    decision: Optional[dict]
    rounds_completed: int
    duration_ms: int
    timestamp: int


@dataclass
class RiskAlertMessage:
    """Risk management alert"""
    alert_id: str
    alert_type: str  # "warning" | "critical" | "kill_switch"
    metric: str
    current_value: str
    threshold: str
    message: str
    timestamp: int


@dataclass
class MarketAnalysisRequest:
    """Request for market analysis"""
    request_id: str
    market_id: str
    analysis_type: str  # "sentiment" | "technical" | "fundamental"
    context: dict
    priority: int
    timestamp: int


@dataclass
class MarketAnalysisResult:
    """Result of market analysis"""
    request_id: str
    market_id: str
    analysis_type: str
    result: dict
    confidence: float
    duration_ms: int
    timestamp: int


class MessageQueueClient:
    """High-performance async RabbitMQ client"""

    def __init__(
        self,
        url: Optional[str] = None,
        prefetch_count: int = 10,
    ):
        self.url = url or os.getenv(
            "RABBITMQ_URL",
            "amqp://bruniao:bruniao_secret@localhost:5672/trading"
        )
        self.prefetch_count = prefetch_count
        self._connection: Optional[AbstractRobustConnection] = None
        self._channel: Optional[AbstractChannel] = None
        self._exchanges: dict[str, aio_pika.Exchange] = {}
        self._queues: dict[str, AbstractQueue] = {}
        self._consumers: list = []

    async def connect(self) -> None:
        """Connect to RabbitMQ"""
        self._connection = await aio_pika.connect_robust(self.url)
        self._channel = await self._connection.channel()
        await self._channel.set_qos(prefetch_count=self.prefetch_count)
        logger.info("rabbitmq_connected", url=self.url)

    async def close(self) -> None:
        """Close connection"""
        if self._channel:
            await self._channel.close()
        if self._connection:
            await self._connection.close()

    async def setup(self) -> None:
        """Setup exchanges and queues"""
        # Declare exchanges
        exchange_names = [
            Exchanges.TRADING,
            Exchanges.SIGNALS,
            Exchanges.ORDERS,
            Exchanges.RISK,
            Exchanges.DEBATES,
            Exchanges.EVENTS,
            Exchanges.ANALYSIS,
        ]

        for name in exchange_names:
            exchange = await self._channel.declare_exchange(
                name,
                ExchangeType.TOPIC,
                durable=True,
            )
            self._exchanges[name] = exchange
            logger.debug("exchange_declared", name=name)

        # Declare and bind queues
        queue_bindings = [
            (Queues.TRADING_DECISIONS, Exchanges.TRADING, [RoutingKeys.DECISION_NEW]),
            (Queues.ORDER_EXECUTION, Exchanges.ORDERS, [
                RoutingKeys.ORDER_PLACED,
                RoutingKeys.ORDER_FILLED,
                RoutingKeys.ORDER_CANCELLED,
            ]),
            (Queues.ORDER_FILLS, Exchanges.ORDERS, [RoutingKeys.ORDER_FILLED]),
            (Queues.RISK_ALERTS, Exchanges.RISK, [
                RoutingKeys.RISK_WARNING,
                RoutingKeys.RISK_CRITICAL,
                RoutingKeys.RISK_KILL_SWITCH,
            ]),
            (Queues.DEBATE_REQUESTS, Exchanges.DEBATES, [RoutingKeys.DEBATE_START]),
            (Queues.DEBATE_RESULTS, Exchanges.DEBATES, [RoutingKeys.DEBATE_CONSENSUS]),
            (Queues.ANALYSIS_REQUESTS, Exchanges.ANALYSIS, [RoutingKeys.ANALYSIS_REQUEST]),
            (Queues.ANALYSIS_RESULTS, Exchanges.ANALYSIS, [RoutingKeys.ANALYSIS_COMPLETE]),
        ]

        for queue_name, exchange_name, routing_keys in queue_bindings:
            queue = await self._channel.declare_queue(
                queue_name,
                durable=True,
            )
            self._queues[queue_name] = queue

            exchange = self._exchanges[exchange_name]
            for routing_key in routing_keys:
                await queue.bind(exchange, routing_key)

            logger.debug("queue_declared", name=queue_name, bindings=routing_keys)

        logger.info("rabbitmq_setup_complete")

    async def publish(
        self,
        exchange: str,
        routing_key: str,
        message: Any,
        priority: int = 5,
        persistent: bool = True,
    ) -> None:
        """Publish a message to an exchange"""
        if exchange not in self._exchanges:
            raise ValueError(f"Unknown exchange: {exchange}")

        body = json.dumps(
            asdict(message) if hasattr(message, '__dataclass_fields__') else message,
            default=str,
        ).encode()

        msg = Message(
            body,
            delivery_mode=DeliveryMode.PERSISTENT if persistent else DeliveryMode.NOT_PERSISTENT,
            priority=priority,
            content_type="application/json",
            message_id=str(uuid.uuid4()),
            timestamp=datetime.utcnow(),
        )

        await self._exchanges[exchange].publish(msg, routing_key)
        logger.debug(
            "message_published",
            exchange=exchange,
            routing_key=routing_key,
        )

    async def subscribe(
        self,
        queue: str,
        handler: Callable[[Any], Coroutine[Any, Any, None]],
        message_type: Optional[type] = None,
    ) -> None:
        """Subscribe to a queue with a message handler"""
        if queue not in self._queues:
            raise ValueError(f"Unknown queue: {queue}")

        async def process_message(message: aio_pika.IncomingMessage):
            async with message.process():
                try:
                    body = json.loads(message.body.decode())

                    # Convert to dataclass if type specified
                    if message_type and hasattr(message_type, '__dataclass_fields__'):
                        body = message_type(**body)

                    await handler(body)
                    logger.debug("message_processed", queue=queue)
                except Exception as e:
                    logger.error("message_handler_error", queue=queue, error=str(e))
                    # Message will be requeued due to exception

        await self._queues[queue].consume(process_message)
        logger.info("queue_subscribed", queue=queue)

    # ==================== Trading Messages ====================

    async def publish_trading_decision(
        self,
        decision: TradingDecisionMessage,
    ) -> None:
        """Publish a trading decision"""
        await self.publish(
            Exchanges.TRADING,
            RoutingKeys.DECISION_NEW,
            decision,
            priority=8,
        )

    async def subscribe_trading_decisions(
        self,
        handler: Callable[[TradingDecisionMessage], Coroutine[Any, Any, None]],
    ) -> None:
        """Subscribe to trading decisions"""
        await self.subscribe(
            Queues.TRADING_DECISIONS,
            handler,
            TradingDecisionMessage,
        )

    # ==================== Risk Messages ====================

    async def publish_risk_alert(
        self,
        alert: RiskAlertMessage,
    ) -> None:
        """Publish a risk alert"""
        routing_key = {
            "warning": RoutingKeys.RISK_WARNING,
            "critical": RoutingKeys.RISK_CRITICAL,
            "kill_switch": RoutingKeys.RISK_KILL_SWITCH,
        }.get(alert.alert_type, RoutingKeys.RISK_WARNING)

        priority = 10 if alert.alert_type == "kill_switch" else 9

        await self.publish(
            Exchanges.RISK,
            routing_key,
            alert,
            priority=priority,
        )

    async def subscribe_risk_alerts(
        self,
        handler: Callable[[RiskAlertMessage], Coroutine[Any, Any, None]],
    ) -> None:
        """Subscribe to risk alerts"""
        await self.subscribe(
            Queues.RISK_ALERTS,
            handler,
            RiskAlertMessage,
        )

    # ==================== Debate Messages ====================

    async def request_debate(
        self,
        request: DebateRequestMessage,
    ) -> None:
        """Request a bot debate"""
        priority = {"low": 3, "medium": 5, "high": 9}.get(request.urgency, 5)

        await self.publish(
            Exchanges.DEBATES,
            RoutingKeys.DEBATE_START,
            request,
            priority=priority,
        )

    async def subscribe_debate_requests(
        self,
        handler: Callable[[DebateRequestMessage], Coroutine[Any, Any, None]],
    ) -> None:
        """Subscribe to debate requests"""
        await self.subscribe(
            Queues.DEBATE_REQUESTS,
            handler,
            DebateRequestMessage,
        )

    async def publish_debate_result(
        self,
        result: DebateResultMessage,
    ) -> None:
        """Publish debate result"""
        await self.publish(
            Exchanges.DEBATES,
            RoutingKeys.DEBATE_CONSENSUS,
            result,
            priority=7,
        )

    async def subscribe_debate_results(
        self,
        handler: Callable[[DebateResultMessage], Coroutine[Any, Any, None]],
    ) -> None:
        """Subscribe to debate results"""
        await self.subscribe(
            Queues.DEBATE_RESULTS,
            handler,
            DebateResultMessage,
        )

    # ==================== Analysis Messages ====================

    async def request_analysis(
        self,
        request: MarketAnalysisRequest,
    ) -> None:
        """Request market analysis"""
        await self.publish(
            Exchanges.ANALYSIS,
            RoutingKeys.ANALYSIS_REQUEST,
            request,
            priority=request.priority,
        )

    async def subscribe_analysis_requests(
        self,
        handler: Callable[[MarketAnalysisRequest], Coroutine[Any, Any, None]],
    ) -> None:
        """Subscribe to analysis requests"""
        await self.subscribe(
            Queues.ANALYSIS_REQUESTS,
            handler,
            MarketAnalysisRequest,
        )

    async def publish_analysis_result(
        self,
        result: MarketAnalysisResult,
    ) -> None:
        """Publish analysis result"""
        await self.publish(
            Exchanges.ANALYSIS,
            RoutingKeys.ANALYSIS_COMPLETE,
            result,
            priority=7,
        )

    async def subscribe_analysis_results(
        self,
        handler: Callable[[MarketAnalysisResult], Coroutine[Any, Any, None]],
    ) -> None:
        """Subscribe to analysis results"""
        await self.subscribe(
            Queues.ANALYSIS_RESULTS,
            handler,
            MarketAnalysisResult,
        )

    # ==================== Health Check ====================

    async def health_check(self) -> bool:
        """Check if message queue is healthy"""
        try:
            return self._connection is not None and not self._connection.is_closed
        except Exception:
            return False


class EventDrivenDebateOrchestrator:
    """
    Event-driven bot debate orchestrator using RabbitMQ.

    This allows debates to be triggered asynchronously from
    the trading engine, processed by the analysis service,
    and results published back.
    """

    def __init__(
        self,
        mq: MessageQueueClient,
        debate_handler: Callable[[DebateRequestMessage], Coroutine[Any, Any, DebateResultMessage]],
    ):
        self.mq = mq
        self.debate_handler = debate_handler
        self._running = False

    async def start(self) -> None:
        """Start listening for debate requests"""
        self._running = True

        async def handle_debate_request(request: DebateRequestMessage):
            logger.info(
                "debate_request_received",
                debate_id=request.debate_id,
                topic=request.topic,
            )

            start_time = datetime.utcnow()

            try:
                # Run the debate
                result = await self.debate_handler(request)

                # Publish result
                await self.mq.publish_debate_result(result)

                logger.info(
                    "debate_completed",
                    debate_id=request.debate_id,
                    duration_ms=result.duration_ms,
                    confidence=result.confidence,
                )
            except Exception as e:
                logger.error(
                    "debate_failed",
                    debate_id=request.debate_id,
                    error=str(e),
                )
                # Publish error result
                error_result = DebateResultMessage(
                    debate_id=request.debate_id,
                    topic=request.topic,
                    consensus=f"Debate failed: {str(e)}",
                    confidence=0.0,
                    decision=None,
                    rounds_completed=0,
                    duration_ms=int((datetime.utcnow() - start_time).total_seconds() * 1000),
                    timestamp=int(datetime.utcnow().timestamp()),
                )
                await self.mq.publish_debate_result(error_result)

        await self.mq.subscribe_debate_requests(handle_debate_request)
        logger.info("debate_orchestrator_started")

    async def stop(self) -> None:
        """Stop the orchestrator"""
        self._running = False


# Singleton instance
_mq_instance: Optional[MessageQueueClient] = None


async def get_mq() -> MessageQueueClient:
    """Get or create message queue client singleton"""
    global _mq_instance
    if _mq_instance is None:
        _mq_instance = MessageQueueClient()
        await _mq_instance.connect()
        await _mq_instance.setup()
    return _mq_instance


async def close_mq() -> None:
    """Close message queue client singleton"""
    global _mq_instance
    if _mq_instance:
        await _mq_instance.close()
        _mq_instance = None
