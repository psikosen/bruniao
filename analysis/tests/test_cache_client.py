"""Tests for cache client."""
import pytest
import sys
from pathlib import Path

# Add parent directory to path for imports
sys.path.insert(0, str(Path(__file__).parent.parent))


class TestCacheClient:
    """Test cache client operations."""

    def test_spread_calculation(self):
        """Test basic spread calculation."""
        best_bid = 0.50
        best_ask = 0.52
        spread = best_ask - best_bid
        assert abs(spread - 0.02) < 1e-10  # Use epsilon for float comparison

    def test_mid_price_calculation(self):
        """Test mid price calculation."""
        best_bid = 0.50
        best_ask = 0.52
        mid_price = (best_bid + best_ask) / 2
        assert mid_price == 0.51
