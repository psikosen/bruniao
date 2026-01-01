# Bruniao - Polymarket Trading Infrastructure

A production-grade trading bot infrastructure for Polymarket with market making, arbitrage detection, and AI-powered strategy analysis.

## Architecture Overview

```
bruniao/
├── core/                    # Rust trading core (low-latency)
│   ├── src/
│   │   ├── orderbook/       # Real-time orderbook management
│   │   ├── executor/        # Order execution engine
│   │   ├── strategy/        # Trading strategies
│   │   ├── risk/            # Risk management & kill-switch
│   │   ├── browser/         # Azul browser integration
│   │   └── ws/              # WebSocket handlers
│   └── Cargo.toml
├── analysis/                # Python analysis layer
│   ├── strategies/          # Strategy development
│   ├── backtesting/         # Historical testing
│   └── signals/             # Signal generation + browser
├── memory/                  # Qdrant vector storage
│   ├── embeddings/          # Trade/decision embeddings
│   └── retrieval/           # Memory retrieval for bots
├── dashboard/               # Web interface
│   ├── src/
│   │   ├── components/      # React components
│   │   ├── pages/           # Bot debate, trades view
│   │   └── api/             # Backend API
│   └── package.json
├── config/                  # Configuration files
└── docker-compose.yml       # Deployment
```

## Key Features

### Trading Core (Rust)
- WebSocket-based orderbook streaming from `wss://clob.polymarket.com`
- L1/L2 authentication flow for Polymarket CLOB
- Market making with inventory management
- Complement arbitrage detection (YES + NO < 1.00)
- Risk controls: max position size, drawdown limits, kill-switch

### Bot Memory (Qdrant)
- Vector storage for trade decisions and outcomes
- Semantic search over historical trades
- Pattern recognition for strategy improvement
- Cross-session learning

### Web Dashboard
- Real-time trade monitoring
- Bot "debate" interface - agents discuss strategies
- Performance analytics
- Configuration management

### Prompt Caching
- Efficient LLM interactions with cached context
- Reduced latency for repetitive analysis
- Cost optimization for AI-powered decisions

### Browser Research (Optional)
- Integration with [Azul TUI Browser](https://github.com/0xSero/Azul)
- Web search across DuckDuckGo, Google, Wikipedia, arXiv
- AI-powered page summarization for market research
- Toggle on/off via config: `browser.enabled: true/false`

## Configuration

### Risk Parameters (Starting with $100)
```yaml
risk:
  max_position_per_market: 10.00   # $10 max per market
  max_open_positions: 5
  daily_drawdown_limit: 0.05       # 5% max daily loss
  min_book_size: 5.00              # Minimum liquidity required
  kill_switch_enabled: true
```

### Order Types Supported
- **GTC** (Good-Till-Cancelled): Standard limit orders
- **GTD** (Good-Till-Day): Expires at specified UTC timestamp
- **FOK** (Fill-Or-Kill): Market orders requiring immediate complete fill

## API Endpoints

| Service | URL |
|---------|-----|
| CLOB API | https://clob.polymarket.com |
| Gamma API | https://gamma-api.polymarket.com |
| WebSocket | wss://clob.polymarket.com |

## Getting Started

```bash
# Clone and setup
git clone <repo>
cd bruniao

# Build Rust core
cd core && cargo build --release

# Install Python dependencies
cd ../analysis && pip install -r requirements.txt

# Start Qdrant
docker-compose up -d qdrant

# Launch dashboard
cd ../dashboard && npm install && npm run dev

# Run the trading bot
./core/target/release/bruniao --config config/production.yaml
```

## Safety Checks

- [ ] Verify jurisdiction allows Polymarket access
- [ ] Start with dry-run mode enabled
- [ ] Test with minimum position sizes first
- [ ] Monitor logs for unexpected behavior
- [ ] Set up alerts for risk limit breaches

## License

MIT
