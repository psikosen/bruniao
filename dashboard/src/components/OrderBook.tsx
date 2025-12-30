interface Level {
  price: number
  size: number
}

interface OrderBookData {
  market: string
  bids: Level[]
  asks: Level[]
  spread: number
}

const mockOrderBook: OrderBookData = {
  market: 'ETH > $4000 Jan',
  bids: [
    { price: 0.54, size: 150 },
    { price: 0.53, size: 200 },
    { price: 0.52, size: 350 },
    { price: 0.51, size: 500 },
    { price: 0.50, size: 800 },
  ],
  asks: [
    { price: 0.56, size: 120 },
    { price: 0.57, size: 180 },
    { price: 0.58, size: 300 },
    { price: 0.59, size: 450 },
    { price: 0.60, size: 700 },
  ],
  spread: 0.02,
}

export default function OrderBook() {
  const maxSize = Math.max(
    ...mockOrderBook.bids.map(l => l.size),
    ...mockOrderBook.asks.map(l => l.size),
  )

  return (
    <div>
      {/* Market Header */}
      <div className="flex justify-between items-center mb-4">
        <h3 className="font-semibold">{mockOrderBook.market}</h3>
        <span className="text-sm text-gray-400">
          Spread: {(mockOrderBook.spread * 100).toFixed(1)}%
        </span>
      </div>

      <div className="grid grid-cols-2 gap-4">
        {/* Bids */}
        <div>
          <div className="flex justify-between text-xs text-gray-500 mb-2 px-2">
            <span>Price</span>
            <span>Size</span>
          </div>
          {mockOrderBook.bids.map((level, idx) => (
            <div key={idx} className="relative flex justify-between py-1 px-2">
              <div
                className="absolute inset-0 bg-poly-green/20"
                style={{ width: `${(level.size / maxSize) * 100}%` }}
              />
              <span className="relative text-poly-green">{level.price.toFixed(2)}</span>
              <span className="relative text-gray-400">{level.size.toFixed(0)}</span>
            </div>
          ))}
        </div>

        {/* Asks */}
        <div>
          <div className="flex justify-between text-xs text-gray-500 mb-2 px-2">
            <span>Price</span>
            <span>Size</span>
          </div>
          {mockOrderBook.asks.map((level, idx) => (
            <div key={idx} className="relative flex justify-between py-1 px-2">
              <div
                className="absolute inset-0 right-0 bg-poly-red/20"
                style={{ width: `${(level.size / maxSize) * 100}%`, marginLeft: 'auto' }}
              />
              <span className="relative text-poly-red">{level.price.toFixed(2)}</span>
              <span className="relative text-gray-400">{level.size.toFixed(0)}</span>
            </div>
          ))}
        </div>
      </div>

      {/* My Orders Indicator */}
      <div className="mt-4 pt-4 border-t border-gray-700">
        <p className="text-xs text-gray-500 mb-2">My Active Orders</p>
        <div className="flex gap-2">
          <span className="bg-poly-green/20 text-poly-green text-xs px-2 py-1 rounded">
            BID @ 0.55 x $2
          </span>
          <span className="bg-poly-red/20 text-poly-red text-xs px-2 py-1 rounded">
            ASK @ 0.57 x $2
          </span>
        </div>
      </div>
    </div>
  )
}
