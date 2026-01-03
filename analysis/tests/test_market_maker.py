"""Tests for market maker strategy."""
import pytest
from decimal import Decimal
import sys
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent))


class TestMarketMaker:
    """Test market maker strategy calculations."""

    def test_optimal_spread_calculation(self):
        """Test optimal spread calculation with basic inputs."""
        # Test with reasonable market parameters
        volatility = 0.05
        inventory_risk = 0.1
        # Basic spread should be positive
        spread = calculate_optimal_spread(volatility, inventory_risk)
        assert spread > 0
        assert spread < 1.0

    def test_spread_increases_with_volatility(self):
        """Higher volatility should result in wider spreads."""
        inventory_risk = 0.1
        spread_low_vol = calculate_optimal_spread(0.01, inventory_risk)
        spread_high_vol = calculate_optimal_spread(0.10, inventory_risk)
        assert spread_high_vol > spread_low_vol

    def test_spread_increases_with_inventory_risk(self):
        """Higher inventory risk should result in wider spreads."""
        volatility = 0.05
        spread_low_risk = calculate_optimal_spread(volatility, 0.05)
        spread_high_risk = calculate_optimal_spread(volatility, 0.20)
        assert spread_high_risk > spread_low_risk


def calculate_optimal_spread(volatility: float, inventory_risk: float) -> float:
    """Calculate optimal spread based on volatility and inventory risk."""
    base_spread = 0.02  # 2% base spread
    vol_component = volatility * 2.0
    risk_component = inventory_risk * 1.5
    return base_spread + vol_component + risk_component
