//! Browser integration module for agent web research
//!
//! Provides optional integration with Azul terminal browser
//! for agents to search and browse the web for market research.
//!
//! Enable in config: browser.enabled = true

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Browser configuration
#[derive(Debug, Clone, Deserialize, Default)]
pub struct BrowserConfig {
    /// Enable browser functionality
    #[serde(default)]
    pub enabled: bool,
    /// Path to Azul binary (defaults to "azul" in PATH)
    #[serde(default = "default_azul_path")]
    pub azul_path: String,
    /// Enable JavaScript rendering (requires Chrome/Chromium)
    #[serde(default)]
    pub js_rendering: bool,
    /// AI provider for browser chat (openai, anthropic, ollama)
    #[serde(default = "default_ai_provider")]
    pub ai_provider: String,
    /// Search engine preference
    #[serde(default = "default_search_engine")]
    pub default_search_engine: SearchEngine,
    /// Request timeout in seconds
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_azul_path() -> String {
    "azul".to_string()
}

fn default_ai_provider() -> String {
    "anthropic".to_string()
}

fn default_search_engine() -> SearchEngine {
    SearchEngine::DuckDuckGo
}

fn default_timeout() -> u64 {
    30
}

/// Supported search engines
#[derive(Debug, Clone, Copy, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchEngine {
    #[default]
    DuckDuckGo,
    Google,
    Wikipedia,
    ArXiv,
    PubMed,
    GoogleScholar,
}

impl SearchEngine {
    /// Get the Azul search prefix for this engine
    pub fn prefix(&self) -> &'static str {
        match self {
            SearchEngine::DuckDuckGo => "",
            SearchEngine::Google => "g:",
            SearchEngine::Wikipedia => "w:",
            SearchEngine::ArXiv => "a:",
            SearchEngine::PubMed => "p:",
            SearchEngine::GoogleScholar => "gs:",
        }
    }
}

/// Result from a web search
#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String,
}

/// Result from fetching a page
#[derive(Debug, Clone, Serialize)]
pub struct PageContent {
    pub url: String,
    pub title: String,
    pub content: String,
    pub links: Vec<String>,
}

/// Browser agent for web research
pub struct BrowserAgent {
    config: BrowserConfig,
}

impl BrowserAgent {
    /// Create a new browser agent
    pub fn new(config: &BrowserConfig) -> Result<Self> {
        if !config.enabled {
            return Err(anyhow!("Browser functionality is disabled in config"));
        }

        // Verify Azul is available
        let status = Command::new(&config.azul_path)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();

        match status {
            Ok(s) if s.success() => {
                info!("Azul browser available at {}", config.azul_path);
            }
            _ => {
                warn!(
                    "Azul browser not found at {}. Install from https://github.com/0xSero/Azul",
                    config.azul_path
                );
                return Err(anyhow!("Azul browser not found"));
            }
        }

        Ok(Self {
            config: config.clone(),
        })
    }

    /// Check if browser is available
    pub fn is_available(&self) -> bool {
        self.config.enabled
    }

    /// Search the web using specified engine
    pub async fn search(
        &self,
        query: &str,
        engine: Option<SearchEngine>,
    ) -> Result<Vec<SearchResult>> {
        let engine = engine.unwrap_or(self.config.default_search_engine);
        let search_query = format!("{}{}", engine.prefix(), query);

        info!(query = %query, engine = ?engine, "Performing web search");

        // Use Azul in headless/API mode for search
        let output = tokio::process::Command::new(&self.config.azul_path)
            .args(["--headless", "--search", &search_query, "--format", "json"])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Search failed: {}", stderr));
        }

        let results: Vec<SearchResult> = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|_| self.parse_search_output(&output.stdout));

        debug!(count = results.len(), "Search returned results");
        Ok(results)
    }

    /// Fetch and summarize a web page
    pub async fn fetch_page(&self, url: &str) -> Result<PageContent> {
        info!(url = %url, "Fetching web page");

        let mut args = vec!["--headless", "--fetch", url, "--format", "json"];

        if self.config.js_rendering {
            args.push("--js");
        }

        let output = tokio::process::Command::new(&self.config.azul_path)
            .args(&args)
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Fetch failed: {}", stderr));
        }

        let content: PageContent = serde_json::from_slice(&output.stdout)?;
        Ok(content)
    }

    /// Use AI to summarize page content
    pub async fn summarize_page(&self, url: &str, prompt: &str) -> Result<String> {
        info!(url = %url, "Summarizing page with AI");

        let output = tokio::process::Command::new(&self.config.azul_path)
            .args([
                "--headless",
                "--fetch",
                url,
                "--ai-summarize",
                prompt,
                "--provider",
                &self.config.ai_provider,
            ])
            .output()
            .await?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("Summarization failed: {}", stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Search for market-related news
    pub async fn search_market_news(&self, market_topic: &str) -> Result<Vec<SearchResult>> {
        let query = format!("{} news prediction market", market_topic);
        self.search(&query, Some(SearchEngine::DuckDuckGo)).await
    }

    /// Search academic papers (useful for research markets)
    pub async fn search_academic(&self, topic: &str) -> Result<Vec<SearchResult>> {
        self.search(topic, Some(SearchEngine::ArXiv)).await
    }

    /// Parse raw search output into structured results
    fn parse_search_output(&self, output: &[u8]) -> Vec<SearchResult> {
        let text = String::from_utf8_lossy(output);
        let mut results = Vec::new();

        // Simple line-based parsing fallback
        for line in text.lines() {
            if line.starts_with("http") {
                results.push(SearchResult {
                    title: String::new(),
                    url: line.to_string(),
                    snippet: String::new(),
                    source: "web".to_string(),
                });
            }
        }

        results
    }
}

/// Research query for market analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchQuery {
    pub market_id: String,
    pub question: String,
    pub keywords: Vec<String>,
    pub search_engines: Vec<SearchEngine>,
}

/// Research result aggregating multiple sources
#[derive(Debug, Clone, Serialize)]
pub struct ResearchResult {
    pub query: ResearchQuery,
    pub search_results: Vec<SearchResult>,
    pub page_summaries: Vec<PageSummary>,
    pub synthesis: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageSummary {
    pub url: String,
    pub summary: String,
}

/// Market researcher using browser for information gathering
pub struct MarketResearcher {
    browser: Option<BrowserAgent>,
}

impl MarketResearcher {
    pub fn new(config: &BrowserConfig) -> Self {
        let browser = if config.enabled {
            BrowserAgent::new(config).ok()
        } else {
            None
        };

        Self { browser }
    }

    /// Check if research capabilities are available
    pub fn is_available(&self) -> bool {
        self.browser.is_some()
    }

    /// Research a market question
    pub async fn research(&self, query: ResearchQuery) -> Result<ResearchResult> {
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| anyhow!("Browser not available"))?;

        let mut all_results = Vec::new();

        // Search across multiple engines
        for engine in &query.search_engines {
            for keyword in &query.keywords {
                let search_query = format!("{} {}", query.question, keyword);
                if let Ok(results) = browser.search(&search_query, Some(*engine)).await {
                    all_results.extend(results);
                }
            }
        }

        // Deduplicate by URL
        all_results.sort_by(|a, b| a.url.cmp(&b.url));
        all_results.dedup_by(|a, b| a.url == b.url);

        // Summarize top pages
        let mut summaries = Vec::new();
        for result in all_results.iter().take(3) {
            if let Ok(summary) = browser
                .summarize_page(
                    &result.url,
                    &format!(
                        "Summarize this page's relevance to: {}",
                        query.question
                    ),
                )
                .await
            {
                summaries.push(PageSummary {
                    url: result.url.clone(),
                    summary,
                });
            }
        }

        Ok(ResearchResult {
            query,
            search_results: all_results,
            page_summaries: summaries,
            synthesis: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_engine_prefix() {
        assert_eq!(SearchEngine::DuckDuckGo.prefix(), "");
        assert_eq!(SearchEngine::Wikipedia.prefix(), "w:");
        assert_eq!(SearchEngine::ArXiv.prefix(), "a:");
    }

    #[test]
    fn test_browser_config_defaults() {
        let config: BrowserConfig = serde_json::from_str("{}").unwrap();
        assert!(!config.enabled);
        assert_eq!(config.azul_path, "azul");
    }
}
