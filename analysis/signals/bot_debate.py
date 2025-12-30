"""
Bot Debate System

Implements a multi-agent debate system where trading bots discuss
and deliberate on strategies, trades, and market conditions.

The debate transcript is stored in Qdrant for future reference
and learning.
"""

import asyncio
from dataclasses import dataclass, field
from typing import List, Optional, Dict, Any
from datetime import datetime
from enum import Enum
import uuid
import json
import structlog

from .prompt_cache import CachedLLMClient, BOT_DEBATE_SYSTEM

logger = structlog.get_logger()


class BotRole(Enum):
    """Roles that bots can take in debates."""
    MARKET_MAKER = "market_maker"
    RISK_MANAGER = "risk_manager"
    ANALYST = "analyst"
    CONTRARIAN = "contrarian"
    MODERATOR = "moderator"


@dataclass
class BotPersona:
    """A bot's personality and role in debates."""
    id: str
    name: str
    role: BotRole
    traits: List[str]
    bias: Optional[str] = None  # e.g., "bullish", "risk-averse"

    def get_system_prompt(self, topic: str) -> str:
        """Get the system prompt for this bot."""
        traits_str = ", ".join(self.traits)
        return BOT_DEBATE_SYSTEM.format(
            bot_name=self.name,
            bot_role=f"{self.role.value} ({traits_str})",
            topic=topic,
        )


@dataclass
class DebateMessage:
    """A single message in a debate."""
    id: str
    participant_id: str
    participant_name: str
    content: str
    reasoning: Optional[str]
    confidence: float
    timestamp: datetime

    def to_dict(self) -> Dict[str, Any]:
        return {
            "id": self.id,
            "participant_id": self.participant_id,
            "participant_name": self.participant_name,
            "content": self.content,
            "reasoning": self.reasoning,
            "confidence": self.confidence,
            "timestamp": self.timestamp.isoformat(),
        }


@dataclass
class Debate:
    """A complete debate session."""
    id: str
    topic: str
    market_id: Optional[str]
    context: Dict[str, Any]
    participants: List[BotPersona]
    messages: List[DebateMessage] = field(default_factory=list)
    consensus: Optional[str] = None
    started_at: datetime = field(default_factory=datetime.utcnow)
    ended_at: Optional[datetime] = None

    def to_dict(self) -> Dict[str, Any]:
        return {
            "id": self.id,
            "topic": self.topic,
            "market_id": self.market_id,
            "context": self.context,
            "participants": [
                {"id": p.id, "name": p.name, "role": p.role.value}
                for p in self.participants
            ],
            "messages": [m.to_dict() for m in self.messages],
            "consensus": self.consensus,
            "started_at": self.started_at.isoformat(),
            "ended_at": self.ended_at.isoformat() if self.ended_at else None,
        }


# Default bot personas
DEFAULT_BOTS = [
    BotPersona(
        id="mm_alpha",
        name="Alpha",
        role=BotRole.MARKET_MAKER,
        traits=["spread-focused", "inventory-conscious", "profit-seeking"],
    ),
    BotPersona(
        id="risk_guardian",
        name="Guardian",
        role=BotRole.RISK_MANAGER,
        traits=["risk-averse", "drawdown-focused", "conservative"],
        bias="risk-averse",
    ),
    BotPersona(
        id="analyst_sage",
        name="Sage",
        role=BotRole.ANALYST,
        traits=["data-driven", "pattern-seeking", "probabilistic"],
    ),
    BotPersona(
        id="contrarian_maverick",
        name="Maverick",
        role=BotRole.CONTRARIAN,
        traits=["skeptical", "contrarian", "devil's-advocate"],
        bias="contrarian",
    ),
]


class DebateOrchestrator:
    """
    Orchestrates debates between trading bots.

    Debates can be:
    - Pre-trade: Should we enter this position?
    - Post-trade: Was this trade good? What can we learn?
    - Strategic: How should we adjust our parameters?
    """

    def __init__(
        self,
        llm_client: CachedLLMClient,
        bots: Optional[List[BotPersona]] = None,
        max_rounds: int = 5,
    ):
        self.llm = llm_client
        self.bots = bots or DEFAULT_BOTS
        self.max_rounds = max_rounds
        self.active_debates: Dict[str, Debate] = {}

    async def start_debate(
        self,
        topic: str,
        context: Dict[str, Any],
        market_id: Optional[str] = None,
        participants: Optional[List[str]] = None,
    ) -> Debate:
        """
        Start a new debate on a topic.

        Args:
            topic: The debate topic/question
            context: Relevant market/trading context
            market_id: Optional market this debate is about
            participants: Optional list of bot IDs to include
        """
        debate_id = str(uuid.uuid4())[:8]

        # Select participants
        if participants:
            debate_bots = [b for b in self.bots if b.id in participants]
        else:
            debate_bots = self.bots[:4]  # Default to first 4 bots

        debate = Debate(
            id=debate_id,
            topic=topic,
            market_id=market_id,
            context=context,
            participants=debate_bots,
        )

        self.active_debates[debate_id] = debate

        logger.info(
            "debate_started",
            debate_id=debate_id,
            topic=topic,
            participants=[b.name for b in debate_bots],
        )

        return debate

    async def run_debate(
        self,
        debate: Debate,
        on_message: Optional[callable] = None,
    ) -> Debate:
        """
        Run a complete debate session.

        Args:
            debate: The debate to run
            on_message: Optional callback for each message (for streaming)
        """
        context_str = json.dumps(debate.context, indent=2)

        for round_num in range(self.max_rounds):
            logger.info("debate_round", debate_id=debate.id, round=round_num + 1)

            # Each bot takes a turn
            for bot in debate.participants:
                # Build conversation history
                history = self._build_history(debate.messages)

                # Generate bot response
                response = await self._generate_bot_response(
                    bot=bot,
                    topic=debate.topic,
                    context=context_str,
                    history=history,
                    round_num=round_num,
                )

                message = DebateMessage(
                    id=str(uuid.uuid4())[:8],
                    participant_id=bot.id,
                    participant_name=bot.name,
                    content=response["content"],
                    reasoning=response.get("reasoning"),
                    confidence=response.get("confidence", 0.7),
                    timestamp=datetime.utcnow(),
                )

                debate.messages.append(message)

                if on_message:
                    await on_message(message)

                # Small delay between responses
                await asyncio.sleep(0.1)

            # Check for early consensus
            if await self._check_consensus(debate):
                break

        # Generate final consensus
        debate.consensus = await self._generate_consensus(debate)
        debate.ended_at = datetime.utcnow()

        logger.info(
            "debate_ended",
            debate_id=debate.id,
            rounds=round_num + 1,
            consensus=debate.consensus,
        )

        return debate

    async def _generate_bot_response(
        self,
        bot: BotPersona,
        topic: str,
        context: str,
        history: str,
        round_num: int,
    ) -> Dict[str, Any]:
        """Generate a response from a bot."""
        system_prompt = bot.get_system_prompt(topic)

        user_prompt = f"""Context:
{context}

Previous Discussion:
{history if history else "(This is the opening of the debate)"}

Round {round_num + 1}: Share your perspective on the topic.
{"Focus on opening positions." if round_num == 0 else "Build on or challenge previous points."}

Respond in JSON format:
{{
    "content": "Your main argument (2-3 sentences)",
    "reasoning": "Brief supporting logic",
    "confidence": 0.0 to 1.0
}}"""

        response_text = await self.llm.complete(
            system_prompt=system_prompt,
            user_prompt=user_prompt,
            temperature=0.7,  # Some creativity in debates
            use_cache=False,  # Debates should be fresh
        )

        try:
            return json.loads(response_text)
        except json.JSONDecodeError:
            return {"content": response_text, "confidence": 0.5}

    def _build_history(self, messages: List[DebateMessage]) -> str:
        """Build conversation history string."""
        if not messages:
            return ""

        lines = []
        for msg in messages[-10:]:  # Last 10 messages
            lines.append(f"{msg.participant_name}: {msg.content}")

        return "\n".join(lines)

    async def _check_consensus(self, debate: Debate) -> bool:
        """Check if bots have reached consensus."""
        if len(debate.messages) < len(debate.participants) * 2:
            return False

        # Simple heuristic: check if last round had agreement signals
        last_round = debate.messages[-len(debate.participants):]
        agreement_words = ["agree", "consensus", "correct", "valid point"]

        agreement_count = sum(
            1 for msg in last_round
            if any(word in msg.content.lower() for word in agreement_words)
        )

        return agreement_count >= len(debate.participants) * 0.75

    async def _generate_consensus(self, debate: Debate) -> str:
        """Generate final consensus summary."""
        history = self._build_history(debate.messages)

        prompt = f"""Summarize the consensus (or lack thereof) from this debate:

Topic: {debate.topic}

Discussion:
{history}

Provide a 1-2 sentence summary of the conclusion or key disagreements."""

        return await self.llm.complete(
            system_prompt="You are a neutral debate moderator summarizing discussion outcomes.",
            user_prompt=prompt,
            temperature=0.0,
            use_cache=True,
        )

    def get_debate(self, debate_id: str) -> Optional[Debate]:
        """Get an active or completed debate."""
        return self.active_debates.get(debate_id)


class PreTradeDebate:
    """
    Specialized debate for pre-trade decisions.

    Used when bots need to decide whether to enter a position.
    """

    def __init__(self, orchestrator: DebateOrchestrator):
        self.orchestrator = orchestrator

    async def should_trade(
        self,
        market_id: str,
        side: str,
        price: float,
        size: float,
        market_context: Dict[str, Any],
    ) -> Dict[str, Any]:
        """
        Debate whether to execute a trade.

        Returns: {
            "should_trade": bool,
            "confidence": float,
            "reasoning": str,
            "debate_id": str
        }
        """
        topic = f"Should we {side} {size} units at {price:.4f}?"
        context = {
            "market_id": market_id,
            "proposed_trade": {"side": side, "price": price, "size": size},
            "market": market_context,
        }

        debate = await self.orchestrator.start_debate(
            topic=topic,
            context=context,
            market_id=market_id,
        )

        await self.orchestrator.run_debate(debate)

        # Analyze consensus
        positive_signals = ["yes", "proceed", "buy", "sell", "execute"]
        negative_signals = ["no", "wait", "skip", "risk", "avoid"]

        consensus_lower = debate.consensus.lower() if debate.consensus else ""

        positive_score = sum(1 for s in positive_signals if s in consensus_lower)
        negative_score = sum(1 for s in negative_signals if s in consensus_lower)

        should_trade = positive_score > negative_score
        confidence = abs(positive_score - negative_score) / max(len(positive_signals), 1)

        return {
            "should_trade": should_trade,
            "confidence": min(confidence, 1.0),
            "reasoning": debate.consensus or "No consensus reached",
            "debate_id": debate.id,
            "debate": debate.to_dict(),
        }
