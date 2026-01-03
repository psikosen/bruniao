"""
Browser Integration for Trading Bots

Provides Python wrapper for Azul terminal browser integration.
Allows trading agents to search and browse the web for market research.

Enable in config: browser.enabled = true
Requires: https://github.com/0xSero/Azul
"""

import asyncio
import json
import shutil
from dataclasses import dataclass, field
from enum import Enum
from typing import List, Optional, Dict, Any
from datetime import datetime
import structlog

logger = structlog.get_logger()


class SearchEngine(Enum):
    """Supported search engines."""

    DUCKDUCKGO = ""
    GOOGLE = "g:"
    WIKIPEDIA = "w:"
    ARXIV = "a:"
    PUBMED = "p:"
    GOOGLE_SCHOLAR = "gs:"


@dataclass
class SearchResult:
    """Result from a web search."""

    title: str
    url: str
    snippet: str
    source: str


@dataclass
class PageContent:
    """Content fetched from a web page."""

    url: str
    title: str
    content: str
    links: List[str] = field(default_factory=list)


@dataclass
class BrowserConfig:
    """Browser configuration."""

    enabled: bool = False
    azul_path: str = "azul"
    js_rendering: bool = False
    ai_provider: str = "anthropic"
    default_search_engine: SearchEngine = SearchEngine.DUCKDUCKGO
    timeout_seconds: int = 30


class BrowserAgent:
    """
    Browser agent for web research.

    Wraps Azul terminal browser for headless web operations.
    """

    def __init__(self, config: Optional[BrowserConfig] = None):
        self.config = config or BrowserConfig()
        self._available = False

        if self.config.enabled:
            self._check_availability()

    def _check_availability(self) -> bool:
        """Check if Azul browser is available."""
        azul_path = shutil.which(self.config.azul_path)
        if azul_path:
            self._available = True
            logger.info("azul_browser_available", path=azul_path)
        else:
            logger.warning(
                "azul_browser_not_found",
                path=self.config.azul_path,
                install_url="https://github.com/0xSero/Azul",
            )
        return self._available

    @property
    def is_available(self) -> bool:
        """Check if browser is available."""
        return self._available and self.config.enabled

    async def search(
        self,
        query: str,
        engine: Optional[SearchEngine] = None,
    ) -> List[SearchResult]:
        """
        Search the web.

        Args:
            query: Search query
            engine: Search engine to use (defaults to config)

        Returns:
            List of search results
        """
        if not self.is_available:
            logger.warning("browser_not_available")
            return []

        engine = engine or self.config.default_search_engine
        search_query = f"{engine.value}{query}"

        logger.info("web_search", query=query, engine=engine.name)

        try:
            proc = await asyncio.create_subprocess_exec(
                self.config.azul_path,
                "--headless",
                "--search",
                search_query,
                "--format",
                "json",
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )

            stdout, stderr = await asyncio.wait_for(
                proc.communicate(), timeout=self.config.timeout_seconds
            )

            if proc.returncode != 0:
                logger.error("search_failed", error=stderr.decode())
                return []

            try:
                data = json.loads(stdout.decode())
                return [
                    SearchResult(
                        title=r.get("title", ""),
                        url=r.get("url", ""),
                        snippet=r.get("snippet", ""),
                        source=engine.name.lower(),
                    )
                    for r in data
                ]
            except json.JSONDecodeError:
                # Fallback parsing
                return self._parse_raw_output(stdout.decode(), engine)

        except asyncio.TimeoutError:
            logger.error("search_timeout", query=query)
            return []
        except Exception as e:
            logger.error("search_error", error=str(e))
            return []

    async def fetch_page(self, url: str) -> Optional[PageContent]:
        """
        Fetch and parse a web page.

        Args:
            url: URL to fetch

        Returns:
            Page content or None on failure
        """
        if not self.is_available:
            return None

        logger.info("fetch_page", url=url)

        args = [self.config.azul_path, "--headless", "--fetch", url, "--format", "json"]
        if self.config.js_rendering:
            args.append("--js")

        try:
            proc = await asyncio.create_subprocess_exec(
                *args,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )

            stdout, stderr = await asyncio.wait_for(
                proc.communicate(), timeout=self.config.timeout_seconds
            )

            if proc.returncode != 0:
                logger.error("fetch_failed", url=url, error=stderr.decode())
                return None

            data = json.loads(stdout.decode())
            return PageContent(
                url=data.get("url", url),
                title=data.get("title", ""),
                content=data.get("content", ""),
                links=data.get("links", []),
            )

        except Exception as e:
            logger.error("fetch_error", url=url, error=str(e))
            return None

    async def summarize_page(self, url: str, prompt: str) -> Optional[str]:
        """
        Fetch a page and summarize it with AI.

        Args:
            url: URL to fetch
            prompt: Summarization prompt

        Returns:
            Summary text or None on failure
        """
        if not self.is_available:
            return None

        logger.info("summarize_page", url=url)

        try:
            proc = await asyncio.create_subprocess_exec(
                self.config.azul_path,
                "--headless",
                "--fetch",
                url,
                "--ai-summarize",
                prompt,
                "--provider",
                self.config.ai_provider,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )

            stdout, stderr = await asyncio.wait_for(
                proc.communicate(),
                timeout=self.config.timeout_seconds * 2,  # Extra time for AI
            )

            if proc.returncode != 0:
                logger.error("summarize_failed", error=stderr.decode())
                return None

            return stdout.decode().strip()

        except Exception as e:
            logger.error("summarize_error", error=str(e))
            return None

    def _parse_raw_output(
        self, output: str, engine: SearchEngine
    ) -> List[SearchResult]:
        """Parse raw output when JSON fails."""
        results = []
        for line in output.strip().split("\n"):
            if line.startswith("http"):
                results.append(
                    SearchResult(
                        title="",
                        url=line.strip(),
                        snippet="",
                        source=engine.name.lower(),
                    )
                )
        return results


class MarketResearcher:
    """
    Market researcher for trading bots.

    Uses browser to gather information about markets,
    news, and relevant events.
    """

    def __init__(self, browser: Optional[BrowserAgent] = None):
        self.browser = browser or BrowserAgent()

    @property
    def is_available(self) -> bool:
        return self.browser.is_available

    async def research_market(
        self,
        market_question: str,
        keywords: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        """
        Research a prediction market question.

        Args:
            market_question: The market question to research
            keywords: Additional keywords to search

        Returns:
            Research results including search results and summaries
        """
        if not self.is_available:
            return {"error": "Browser not available"}

        keywords = keywords or []
        all_results = []
        summaries = []

        # Search with different engines
        for engine in [SearchEngine.DUCKDUCKGO, SearchEngine.GOOGLE]:
            # Main question search
            results = await self.browser.search(f"{market_question} prediction", engine)
            all_results.extend(results)

            # Keyword searches
            for keyword in keywords[:3]:  # Limit keywords
                kw_results = await self.browser.search(
                    f"{keyword} {market_question}", engine
                )
                all_results.extend(kw_results)

        # Deduplicate by URL
        seen_urls = set()
        unique_results = []
        for r in all_results:
            if r.url not in seen_urls:
                seen_urls.add(r.url)
                unique_results.append(r)

        # Summarize top results
        for result in unique_results[:3]:
            summary = await self.browser.summarize_page(
                result.url, f"Summarize how this page relates to: {market_question}"
            )
            if summary:
                summaries.append(
                    {
                        "url": result.url,
                        "title": result.title,
                        "summary": summary,
                    }
                )

        return {
            "question": market_question,
            "search_results": [
                {"title": r.title, "url": r.url, "snippet": r.snippet}
                for r in unique_results
            ],
            "summaries": summaries,
            "timestamp": datetime.utcnow().isoformat(),
        }

    async def search_news(self, topic: str) -> List[SearchResult]:
        """Search for recent news about a topic."""
        return await self.browser.search(
            f"{topic} news latest", SearchEngine.DUCKDUCKGO
        )

    async def search_academic(self, topic: str) -> List[SearchResult]:
        """Search academic papers (arXiv)."""
        return await self.browser.search(topic, SearchEngine.ARXIV)

    async def quick_lookup(self, query: str) -> Optional[str]:
        """Quick lookup with Wikipedia."""
        results = await self.browser.search(query, SearchEngine.WIKIPEDIA)
        if results:
            return await self.browser.summarize_page(
                results[0].url, f"Provide a brief factual summary about: {query}"
            )
        return None


# Factory function
def create_browser(config_dict: Optional[Dict[str, Any]] = None) -> BrowserAgent:
    """
    Create a browser agent from config dictionary.

    Args:
        config_dict: Configuration dictionary

    Returns:
        Configured BrowserAgent
    """
    if not config_dict:
        return BrowserAgent(BrowserConfig(enabled=False))

    engine_map = {
        "duckduckgo": SearchEngine.DUCKDUCKGO,
        "google": SearchEngine.GOOGLE,
        "wikipedia": SearchEngine.WIKIPEDIA,
        "arxiv": SearchEngine.ARXIV,
        "pubmed": SearchEngine.PUBMED,
        "googlescholar": SearchEngine.GOOGLE_SCHOLAR,
    }

    config = BrowserConfig(
        enabled=config_dict.get("enabled", False),
        azul_path=config_dict.get("azul_path", "azul"),
        js_rendering=config_dict.get("js_rendering", False),
        ai_provider=config_dict.get("ai_provider", "anthropic"),
        default_search_engine=engine_map.get(
            config_dict.get("default_search_engine", "duckduckgo").lower(),
            SearchEngine.DUCKDUCKGO,
        ),
        timeout_seconds=config_dict.get("timeout_seconds", 30),
    )

    return BrowserAgent(config)
