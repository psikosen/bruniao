interface RiskGaugeProps {
  currentDrawdown: number
  maxDrawdown: number
  positionCount: number
  maxPositions: number
}

export default function RiskGauge({
  currentDrawdown,
  maxDrawdown,
  positionCount,
  maxPositions,
}: RiskGaugeProps) {
  const drawdownPercent = (currentDrawdown / maxDrawdown) * 100
  const positionPercent = (positionCount / maxPositions) * 100

  const getColor = (percent: number) => {
    if (percent < 50) return 'bg-poly-green'
    if (percent < 75) return 'bg-yellow-500'
    return 'bg-poly-red'
  }

  return (
    <div className="space-y-6">
      {/* Drawdown Gauge */}
      <div>
        <div className="flex justify-between mb-2">
          <span className="text-sm text-gray-400">Daily Drawdown</span>
          <span className={`text-sm ${drawdownPercent > 75 ? 'text-poly-red' : 'text-white'}`}>
            {currentDrawdown.toFixed(1)}% / {maxDrawdown}%
          </span>
        </div>
        <div className="h-3 bg-poly-darker rounded-full overflow-hidden">
          <div
            className={`h-full transition-all duration-300 ${getColor(drawdownPercent)}`}
            style={{ width: `${Math.min(drawdownPercent, 100)}%` }}
          />
        </div>
        {drawdownPercent > 75 && (
          <p className="text-xs text-poly-red mt-1">Warning: Approaching limit</p>
        )}
      </div>

      {/* Position Count */}
      <div>
        <div className="flex justify-between mb-2">
          <span className="text-sm text-gray-400">Open Positions</span>
          <span className="text-sm">{positionCount} / {maxPositions}</span>
        </div>
        <div className="h-3 bg-poly-darker rounded-full overflow-hidden">
          <div
            className={`h-full transition-all duration-300 ${getColor(positionPercent)}`}
            style={{ width: `${positionPercent}%` }}
          />
        </div>
      </div>

      {/* Risk Summary */}
      <div className="p-3 bg-poly-darker rounded-lg">
        <div className="flex items-center gap-2 mb-2">
          <div className={`w-3 h-3 rounded-full ${
            drawdownPercent < 50 && positionPercent < 80 ? 'bg-poly-green pulse-green' :
            drawdownPercent > 75 ? 'bg-poly-red pulse-red' : 'bg-yellow-500'
          }`} />
          <span className="text-sm font-medium">
            {drawdownPercent < 50 && positionPercent < 80 ? 'Risk OK' :
             drawdownPercent > 75 ? 'High Risk' : 'Moderate Risk'}
          </span>
        </div>
        <p className="text-xs text-gray-500">
          {drawdownPercent < 50 && positionPercent < 80
            ? 'All risk parameters within normal limits.'
            : drawdownPercent > 75
              ? 'Consider reducing exposure or activating kill switch.'
              : 'Monitor closely. Some limits approaching threshold.'}
        </p>
      </div>
    </div>
  )
}
