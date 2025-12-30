import { useState, useEffect } from 'react'
import Head from 'next/head'
import { useQuery } from '@tanstack/react-query'
import Layout from '@/components/Layout'
import StatsCard from '@/components/StatsCard'
import TradeList from '@/components/TradeList'
import OrderBook from '@/components/OrderBook'
import PositionTable from '@/components/PositionTable'
import RiskGauge from '@/components/RiskGauge'

interface DashboardStats {
  balance: number
  dailyPnl: number
  dailyPnlPercent: number
  openPositions: number
  activeOrders: number
  totalTrades: number
  winRate: number
  killSwitchActive: boolean
}

export default function Dashboard() {
  const [connected, setConnected] = useState(false)

  const { data: stats, isLoading } = useQuery<DashboardStats>({
    queryKey: ['stats'],
    queryFn: async () => {
      const res = await fetch('/api/stats')
      return res.json()
    },
    refetchInterval: 1000,
  })

  // Mock data for demo
  const mockStats: DashboardStats = {
    balance: 97.50,
    dailyPnl: -2.50,
    dailyPnlPercent: -2.5,
    openPositions: 3,
    activeOrders: 6,
    totalTrades: 47,
    winRate: 58.3,
    killSwitchActive: false,
  }

  const displayStats = stats || mockStats

  return (
    <>
      <Head>
        <title>Bruniao Trading Dashboard</title>
        <meta name="description" content="Polymarket Trading Bot Dashboard" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
      </Head>

      <Layout>
        {/* Header */}
        <div className="flex justify-between items-center mb-8">
          <div>
            <h1 className="text-3xl font-bold">Trading Dashboard</h1>
            <p className="text-gray-400 mt-1">Real-time bot monitoring</p>
          </div>
          <div className="flex items-center gap-4">
            <div className={`flex items-center gap-2 px-3 py-1 rounded-full ${
              connected ? 'bg-poly-green/20 text-poly-green' : 'bg-red-500/20 text-red-400'
            }`}>
              <div className={`w-2 h-2 rounded-full ${connected ? 'bg-poly-green pulse-green' : 'bg-red-500'}`} />
              {connected ? 'Connected' : 'Disconnected'}
            </div>
            {displayStats.killSwitchActive && (
              <div className="bg-red-500/20 text-red-400 px-3 py-1 rounded-full flex items-center gap-2">
                <span className="text-xl">⚠️</span>
                Kill Switch Active
              </div>
            )}
          </div>
        </div>

        {/* Stats Grid */}
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-8">
          <StatsCard
            title="Balance"
            value={`$${displayStats.balance.toFixed(2)}`}
            subtitle="USDC"
          />
          <StatsCard
            title="Daily P&L"
            value={`${displayStats.dailyPnl >= 0 ? '+' : ''}$${displayStats.dailyPnl.toFixed(2)}`}
            subtitle={`${displayStats.dailyPnlPercent >= 0 ? '+' : ''}${displayStats.dailyPnlPercent.toFixed(1)}%`}
            positive={displayStats.dailyPnl >= 0}
          />
          <StatsCard
            title="Win Rate"
            value={`${displayStats.winRate.toFixed(1)}%`}
            subtitle={`${displayStats.totalTrades} trades`}
          />
          <StatsCard
            title="Open Positions"
            value={displayStats.openPositions.toString()}
            subtitle={`${displayStats.activeOrders} orders`}
          />
        </div>

        {/* Main Content Grid */}
        <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
          {/* Left Column - Orderbook & Positions */}
          <div className="lg:col-span-2 space-y-6">
            <div className="bg-poly-dark rounded-xl p-6">
              <h2 className="text-xl font-semibold mb-4">Active Markets</h2>
              <OrderBook />
            </div>

            <div className="bg-poly-dark rounded-xl p-6">
              <h2 className="text-xl font-semibold mb-4">Positions</h2>
              <PositionTable />
            </div>
          </div>

          {/* Right Column - Risk & Trades */}
          <div className="space-y-6">
            <div className="bg-poly-dark rounded-xl p-6">
              <h2 className="text-xl font-semibold mb-4">Risk Monitor</h2>
              <RiskGauge
                currentDrawdown={Math.abs(displayStats.dailyPnlPercent)}
                maxDrawdown={5}
                positionCount={displayStats.openPositions}
                maxPositions={5}
              />
            </div>

            <div className="bg-poly-dark rounded-xl p-6">
              <h2 className="text-xl font-semibold mb-4">Recent Trades</h2>
              <TradeList />
            </div>
          </div>
        </div>
      </Layout>
    </>
  )
}
