interface Position {
  market: string
  side: 'YES' | 'NO'
  size: number
  entryPrice: number
  currentPrice: number
  pnl: number
  pnlPercent: number
}

const mockPositions: Position[] = [
  { market: 'ETH > $4000 Jan', side: 'YES', size: 5, entryPrice: 0.52, currentPrice: 0.55, pnl: 0.15, pnlPercent: 5.77 },
  { market: 'BTC > $100k Feb', side: 'NO', size: 3, entryPrice: 0.35, currentPrice: 0.32, pnl: 0.09, pnlPercent: 8.57 },
  { market: 'Fed Rate Cut Mar', side: 'YES', size: 2, entryPrice: 0.72, currentPrice: 0.68, pnl: -0.08, pnlPercent: -5.56 },
]

export default function PositionTable() {
  const totalPnl = mockPositions.reduce((sum, p) => sum + p.pnl, 0)

  return (
    <div>
      <table className="w-full">
        <thead>
          <tr className="text-left text-xs text-gray-500 border-b border-gray-700">
            <th className="pb-2">Market</th>
            <th className="pb-2">Side</th>
            <th className="pb-2 text-right">Size</th>
            <th className="pb-2 text-right">Entry</th>
            <th className="pb-2 text-right">Current</th>
            <th className="pb-2 text-right">P&L</th>
          </tr>
        </thead>
        <tbody>
          {mockPositions.map((position, idx) => (
            <tr key={idx} className="border-b border-gray-800">
              <td className="py-3 text-sm">{position.market}</td>
              <td className="py-3">
                <span className={`text-xs px-2 py-0.5 rounded ${
                  position.side === 'YES' ? 'bg-poly-green/20 text-poly-green' : 'bg-poly-red/20 text-poly-red'
                }`}>
                  {position.side}
                </span>
              </td>
              <td className="py-3 text-right text-sm">${position.size.toFixed(2)}</td>
              <td className="py-3 text-right text-sm text-gray-400">{position.entryPrice.toFixed(2)}</td>
              <td className="py-3 text-right text-sm">{position.currentPrice.toFixed(2)}</td>
              <td className={`py-3 text-right text-sm ${position.pnl >= 0 ? 'text-poly-green' : 'text-poly-red'}`}>
                {position.pnl >= 0 ? '+' : ''}{position.pnl.toFixed(2)} ({position.pnlPercent.toFixed(1)}%)
              </td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr className="text-sm font-semibold">
            <td colSpan={5} className="pt-3 text-right text-gray-400">Total P&L:</td>
            <td className={`pt-3 text-right ${totalPnl >= 0 ? 'text-poly-green' : 'text-poly-red'}`}>
              {totalPnl >= 0 ? '+' : ''}${totalPnl.toFixed(2)}
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
  )
}
