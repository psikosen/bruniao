"""
Market Making Strategy for Polymarket

Implements a micro market maker that posts tiny limit orders inside the spread
with inventory management and risk controls.
"""

import asyncio
from dataclasses import dataclass
from decimal import Decimal
from typing import Optional, List, Dict
from datetime import datetime
import structlog

logger = structlog.get_logger()


@dataclass
class Quote:
    """A bid or ask quote."""
    price: Decimal
    size: Decimal
    side: str  # 'buy' or 'sell'


@dataclass
class MarketState:
    """Current state of a market."""
    token_id: str
    best_bid: Decimal
    best_ask: Decimal
    bid_size: Decimal
    ask_size: Decimal
    spread: Decimal
    mid_price: Decimal


@dataclass
class InventoryState:
    """Current inventory position."""
    position: Decimal
    max_position: Decimal
    skew: Decimal  # -1 to 1, normalized


class MicroMarketMaker:
    """
    Micro market maker for small capital ($100).

    Strategy:
    - Post small quotes slightly inside the spread
    - Manage inventory to avoid getting "run over"
    - Apply skew when inventory is imbalanced
    """

    def __init__(
        self,
        quote_size: Decimal = Decimal("2"),  # $2 quotes
        spread_ticks: int = 1,
        max_position: Decimal = Decimal("10"),
        skew_threshold: Decimal = Decimal("0.5"),
    ):
        self.quote_size = quote_size
        self.spread_ticks = spread_ticks
        self.max_position = max_position
        self.skew_threshold = skew_threshold
        self.tick_size = Decimal("0.01")

        # Active quotes by token_id
        self.active_quotes: Dict[str, List[Quote]] = {}

    def calculate_quotes(
        self,
        market: MarketState,
        inventory: InventoryState,
    ) -> List[Quote]:
        """
        Calculate bid/ask quotes based on market state and inventory.

        Returns list of quotes to place.
        """
        quotes = []

        # Calculate target prices
        # Improve best prices by 1 tick
        bid_price = market.best_bid + self.tick_size
        ask_price = market.best_ask - self.tick_size

        # Don't cross the market
        if bid_price >= ask_price:
            bid_price = market.best_bid
            ask_price = market.best_ask

        # Apply inventory skew
        # Positive skew = long, want to sell more -> lower ask, widen bid
        # Negative skew = short, want to buy more -> lower bid, widen ask
        skew_adjustment = inventory.skew * self.tick_size * 2
        bid_price -= skew_adjustment
        ask_price -= skew_adjustment

        # Clamp to valid range
        min_price = Decimal("0.01")
        max_price = Decimal("0.99")
        bid_price = max(min_price, min(max_price, bid_price))
        ask_price = max(min_price, min(max_price, ask_price))

        # Determine sizes based on inventory
        bid_size = self.quote_size
        ask_size = self.quote_size

        # Reduce size on side where we're overleveraged
        if inventory.skew > self.skew_threshold:
            bid_size = self.quote_size / 2
        elif inventory.skew < -self.skew_threshold:
            ask_size = self.quote_size / 2

        # Check if we can add to position
        if inventory.position + bid_size <= inventory.max_position:
            quotes.append(Quote(
                price=bid_price,
                size=bid_size,
                side='buy',
            ))

        if inventory.position - ask_size >= -inventory.max_position:
            quotes.append(Quote(
                price=ask_price,
                size=ask_size,
                side='sell',
            ))

        return quotes

    def should_update_quotes(
        self,
        token_id: str,
        new_quotes: List[Quote],
    ) -> bool:
        """Check if we need to update quotes."""
        if token_id not in self.active_quotes:
            return True

        old_quotes = self.active_quotes[token_id]
        if len(old_quotes) != len(new_quotes):
            return True

        # Check if prices have changed significantly
        for old, new in zip(
            sorted(old_quotes, key=lambda q: q.side),
            sorted(new_quotes, key=lambda q: q.side),
        ):
            if abs(old.price - new.price) >= self.tick_size:
                return True

        return False

    def update_active_quotes(self, token_id: str, quotes: List[Quote]):
        """Update the active quotes cache."""
        self.active_quotes[token_id] = quotes


@dataclass
class ArbitrageOpportunity:
    """Detected complement arbitrage opportunity."""
    yes_token_id: str
    no_token_id: str
    yes_price: Decimal
    no_price: Decimal
    total_cost: Decimal
    profit_margin: Decimal
    executable_size: Decimal


class ComplementArbitrageDetector:
    """
    Detects when YES + NO tokens can be bought for less than $1.

    This is the "safest" strategy but opportunities are rare and
    competition is fierce.
    """

    def __init__(
        self,
        min_profit_margin: Decimal = Decimal("0.02"),  # 2% minimum profit
        max_size: Decimal = Decimal("10"),  # $10 max per arb
    ):
        self.min_profit_margin = min_profit_margin
        self.max_size = max_size

    def check_opportunity(
        self,
        yes_market: MarketState,
        no_market: MarketState,
    ) -> Optional[ArbitrageOpportunity]:
        """
        Check if there's an arbitrage opportunity.

        Returns opportunity if YES_ask + NO_ask < 1.00 - threshold
        """
        total_cost = yes_market.best_ask + no_market.best_ask
        profit_margin = Decimal("1.00") - total_cost

        if profit_margin < self.min_profit_margin:
            return None

        # Executable size is minimum of both sides
        executable_size = min(
            yes_market.ask_size,
            no_market.ask_size,
            self.max_size,
        )

        logger.info(
            "arbitrage_opportunity",
            yes_price=str(yes_market.best_ask),
            no_price=str(no_market.best_ask),
            total_cost=str(total_cost),
            profit_margin=str(profit_margin),
            executable_size=str(executable_size),
        )

        return ArbitrageOpportunity(
            yes_token_id=yes_market.token_id,
            no_token_id=no_market.token_id,
            yes_price=yes_market.best_ask,
            no_price=no_market.best_ask,
            total_cost=total_cost,
            profit_margin=profit_margin,
            executable_size=executable_size,
        )


class StrategyOrchestrator:
    """
    Orchestrates multiple strategies and manages execution.
    """

    def __init__(
        self,
        market_maker: MicroMarketMaker,
        arb_detector: ComplementArbitrageDetector,
        dry_run: bool = True,
    ):
        self.market_maker = market_maker
        self.arb_detector = arb_detector
        self.dry_run = dry_run
        self.running = False

    async def run(
        self,
        markets: Dict[str, tuple],  # condition_id -> (yes_token, no_token)
        get_market_state,  # Callable to get market state
        get_inventory,  # Callable to get inventory
        execute_order,  # Callable to execute orders
    ):
        """
        Main strategy loop.
        """
        self.running = True
        logger.info("strategy_orchestrator_started")

        while self.running:
            try:
                for condition_id, (yes_token, no_token) in markets.items():
                    # Get market states
                    yes_state = await get_market_state(yes_token)
                    no_state = await get_market_state(no_token)

                    if yes_state is None or no_state is None:
                        continue

                    # Check for arbitrage first (higher priority)
                    arb = self.arb_detector.check_opportunity(yes_state, no_state)
                    if arb:
                        await self._execute_arbitrage(arb, execute_order)
                        continue

                    # Generate market making quotes
                    inventory = await get_inventory(condition_id)
                    yes_quotes = self.market_maker.calculate_quotes(yes_state, inventory)

                    if self.market_maker.should_update_quotes(yes_token, yes_quotes):
                        await self._execute_quotes(yes_token, yes_quotes, execute_order)
                        self.market_maker.update_active_quotes(yes_token, yes_quotes)

                await asyncio.sleep(0.1)  # 100ms tick

            except Exception as e:
                logger.error("strategy_error", error=str(e))
                await asyncio.sleep(1)

    async def _execute_arbitrage(self, arb: ArbitrageOpportunity, execute_order):
        """Execute an arbitrage opportunity."""
        if self.dry_run:
            logger.info("dry_run_arbitrage", arb=arb)
            return

        # Execute both legs simultaneously using FOK orders
        await asyncio.gather(
            execute_order(
                token_id=arb.yes_token_id,
                side='buy',
                price=arb.yes_price,
                size=arb.executable_size,
                order_type='FOK',
            ),
            execute_order(
                token_id=arb.no_token_id,
                side='buy',
                price=arb.no_price,
                size=arb.executable_size,
                order_type='FOK',
            ),
        )

    async def _execute_quotes(self, token_id: str, quotes: List[Quote], execute_order):
        """Execute market making quotes."""
        if self.dry_run:
            logger.info("dry_run_quotes", token_id=token_id, quotes=quotes)
            return

        for quote in quotes:
            await execute_order(
                token_id=token_id,
                side=quote.side,
                price=quote.price,
                size=quote.size,
                order_type='GTC',
            )

    def stop(self):
        """Stop the strategy loop."""
        self.running = False
        logger.info("strategy_orchestrator_stopped")
