"""
Workers Module

Event-driven background workers for:
- Bot debates
- Market analysis
- Signal processing
"""

from .debate_worker import DebateWorker

__all__ = ["DebateWorker"]
