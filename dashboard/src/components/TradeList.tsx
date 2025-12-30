import { format } from 'date-fns'

interface Trade {
  id: string
  market: string
  side: 'buy' | 'sell'
  price: number
  size: number
  pnl?: number
  timestamp: Date
  type: 'market_making' | 'arbitrage' | 'signal'
}

const mockTrades: Trade[] = [
  { id: '1', market: 'ETH > $4000 Jan', side: 'buy', price: 0.55, size: 2, pnl: 0.12, timestamp: new Date(Date.now() - 60000), type: 'market_making' },
  { id: '2', market: 'ETH > $4000 Jan', side: 'sell', price: 0.57, size: 2, pnl: 0.04, timestamp: new Date(Date.now() - 120000), type: 'market_making' },
  { id: '3', market: 'BTC > $100k Feb', side: 'buy', price: 0.32, size: 3, timestamp: new Date(Date.now() - 180000), type: 'arbitrage' },
  { id: '4', market: 'BTC > $100k Feb', side: 'buy', price: 0.65, size: 3, timestamp: new Date(Date.now() - 180000), type: 'arbitrage' },
  { id: '5', market: 'Fed Rate Cut', side: 'sell', price: 0.78, size: 1.5, pnl: -0.15, timestamp: new Date(Date.now() - 300000), type: 'signal' },
]

export default function TradeList() {
  return (
    <div className="space-y-3">
      {mockTrades.map((trade) => (
        <div key={trade.id} className="flex items-center justify-between p-3 bg-poly-darker rounded-lg">
          <div className="flex items-center gap-3">
            <div className={`w-2 h-2 rounded-full ${
              trade.side === 'buy' ? 'bg-poly-green' : 'bg-poly-red'
            }`} />
            <div>
              <p className="text-sm font-medium">{trade.market}</p>
              <p className="text-xs text-gray-500">
                {trade.side.toUpperCase()} @ {trade.price.toFixed(2)} x {trade.size}
              </p>
            </div>
          </div>
          <div className="text-right">
            {trade.pnl !== undefined && (
              <p className={`text-sm font-medium ${
                trade.pnl >= 0 ? 'text-poly-green' : 'text-poly-red'
              }`}>
                {trade.pnl >= 0 ? '+' : ''}{trade.pnl.toFixed(2)}
              </p>
            )}
            <p className="text-xs text-gray-500">
              {format(trade.timestamp, 'HH:mm:ss')}
            </p>
          </div>
        </div>
      ))}
    </div>
  )
}
