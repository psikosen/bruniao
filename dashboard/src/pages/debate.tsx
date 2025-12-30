import { useState, useEffect, useRef } from 'react'
import Head from 'next/head'
import Layout from '@/components/Layout'
import BotAvatar from '@/components/BotAvatar'
import DebateMessage from '@/components/DebateMessage'
import { format } from 'date-fns'

interface Bot {
  id: string
  name: string
  role: string
  avatar: string
  color: string
}

interface Message {
  id: string
  botId: string
  botName: string
  content: string
  reasoning?: string
  confidence: number
  timestamp: Date
}

interface Debate {
  id: string
  topic: string
  marketId?: string
  status: 'active' | 'concluded'
  consensus?: string
  messages: Message[]
  startedAt: Date
}

const BOTS: Bot[] = [
  { id: 'mm_alpha', name: 'Alpha', role: 'Market Maker', avatar: '🤖', color: 'text-poly-green' },
  { id: 'risk_guardian', name: 'Guardian', role: 'Risk Manager', avatar: '🛡️', color: 'text-yellow-400' },
  { id: 'analyst_sage', name: 'Sage', role: 'Analyst', avatar: '📊', color: 'text-poly-blue' },
  { id: 'contrarian_maverick', name: 'Maverick', role: 'Contrarian', avatar: '🎯', color: 'text-purple-400' },
]

export default function DebatePage() {
  const [debates, setDebates] = useState<Debate[]>([])
  const [activeDebate, setActiveDebate] = useState<Debate | null>(null)
  const [newTopic, setNewTopic] = useState('')
  const messagesEndRef = useRef<HTMLDivElement>(null)

  // Mock debate data
  useEffect(() => {
    const mockDebate: Debate = {
      id: 'debate-001',
      topic: 'Should we increase position size in the ETH prediction market?',
      marketId: 'eth-10k-jan',
      status: 'concluded',
      consensus: 'The team agrees to maintain current position sizes due to elevated volatility. Guardian raised valid concerns about drawdown limits.',
      messages: [
        {
          id: 'm1',
          botId: 'mm_alpha',
          botName: 'Alpha',
          content: 'The spread has tightened to 2 cents. With our current size, we are leaving profit on the table. I propose increasing quote size from $2 to $4.',
          reasoning: 'Spread tightening indicates more liquidity, reducing fill risk.',
          confidence: 0.75,
          timestamp: new Date(Date.now() - 300000),
        },
        {
          id: 'm2',
          botId: 'risk_guardian',
          botName: 'Guardian',
          content: 'I disagree. Our daily drawdown is at 2.5% - halfway to the 5% limit. Increasing size now would amplify losses if we continue this trajectory.',
          reasoning: 'Risk-adjusted returns do not justify increased exposure.',
          confidence: 0.85,
          timestamp: new Date(Date.now() - 240000),
        },
        {
          id: 'm3',
          botId: 'analyst_sage',
          botName: 'Sage',
          content: 'Looking at the data: our win rate on this market is 62%, above our 58% average. However, the implied volatility has spiked 15% in the last hour.',
          reasoning: 'Statistical edge exists but environment is becoming riskier.',
          confidence: 0.70,
          timestamp: new Date(Date.now() - 180000),
        },
        {
          id: 'm4',
          botId: 'contrarian_maverick',
          botName: 'Maverick',
          content: 'Perhaps we are overthinking this. If we are profitable and have edge, why not lean into it? The drawdown limit exists precisely for moments like this.',
          reasoning: 'Limits should be used, not feared.',
          confidence: 0.60,
          timestamp: new Date(Date.now() - 120000),
        },
        {
          id: 'm5',
          botId: 'mm_alpha',
          botName: 'Alpha',
          content: 'Valid point from Maverick, but I concede to Guardian on this one. The volatility spike Sage mentioned is concerning. Let us revisit when conditions stabilize.',
          confidence: 0.65,
          timestamp: new Date(Date.now() - 60000),
        },
      ],
      startedAt: new Date(Date.now() - 360000),
    }

    setDebates([mockDebate])
    setActiveDebate(mockDebate)
  }, [])

  const scrollToBottom = () => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }

  useEffect(() => {
    scrollToBottom()
  }, [activeDebate?.messages])

  const startNewDebate = () => {
    if (!newTopic.trim()) return

    const debate: Debate = {
      id: `debate-${Date.now()}`,
      topic: newTopic,
      status: 'active',
      messages: [],
      startedAt: new Date(),
    }

    setDebates([debate, ...debates])
    setActiveDebate(debate)
    setNewTopic('')
  }

  const getBotById = (id: string): Bot => {
    return BOTS.find(b => b.id === id) || BOTS[0]
  }

  return (
    <>
      <Head>
        <title>Bot Debates | Bruniao</title>
      </Head>

      <Layout>
        <div className="flex h-[calc(100vh-8rem)]">
          {/* Sidebar - Debate List */}
          <div className="w-80 bg-poly-dark rounded-xl p-4 mr-6 flex flex-col">
            <h2 className="text-xl font-semibold mb-4">Debates</h2>

            {/* New Debate Input */}
            <div className="mb-4">
              <input
                type="text"
                value={newTopic}
                onChange={(e) => setNewTopic(e.target.value)}
                placeholder="Start a new debate..."
                className="w-full bg-poly-darker border border-gray-700 rounded-lg px-4 py-2 text-white placeholder-gray-500 focus:outline-none focus:border-poly-blue"
                onKeyDown={(e) => e.key === 'Enter' && startNewDebate()}
              />
              <button
                onClick={startNewDebate}
                className="w-full mt-2 bg-poly-blue hover:bg-poly-blue/80 text-white py-2 rounded-lg transition-colors"
              >
                Start Debate
              </button>
            </div>

            {/* Debate List */}
            <div className="flex-1 overflow-y-auto space-y-2">
              {debates.map((debate) => (
                <button
                  key={debate.id}
                  onClick={() => setActiveDebate(debate)}
                  className={`w-full text-left p-3 rounded-lg transition-colors ${
                    activeDebate?.id === debate.id
                      ? 'bg-poly-blue/20 border border-poly-blue'
                      : 'bg-poly-darker hover:bg-gray-800'
                  }`}
                >
                  <div className="flex items-center gap-2 mb-1">
                    <span className={`text-xs px-2 py-0.5 rounded ${
                      debate.status === 'active' ? 'bg-poly-green/20 text-poly-green' : 'bg-gray-600 text-gray-300'
                    }`}>
                      {debate.status}
                    </span>
                    <span className="text-xs text-gray-500">
                      {format(debate.startedAt, 'MMM d, HH:mm')}
                    </span>
                  </div>
                  <p className="text-sm text-gray-300 line-clamp-2">{debate.topic}</p>
                </button>
              ))}
            </div>
          </div>

          {/* Main Content - Active Debate */}
          <div className="flex-1 bg-poly-dark rounded-xl flex flex-col">
            {activeDebate ? (
              <>
                {/* Debate Header */}
                <div className="p-6 border-b border-gray-700">
                  <div className="flex justify-between items-start">
                    <div>
                      <h1 className="text-xl font-semibold mb-2">{activeDebate.topic}</h1>
                      <div className="flex items-center gap-4 text-sm text-gray-400">
                        <span>Started {format(activeDebate.startedAt, 'MMM d, yyyy HH:mm')}</span>
                        {activeDebate.marketId && (
                          <span className="bg-poly-blue/20 text-poly-blue px-2 py-0.5 rounded">
                            {activeDebate.marketId}
                          </span>
                        )}
                      </div>
                    </div>
                    <div className="flex gap-2">
                      {BOTS.map((bot) => (
                        <div key={bot.id} className="relative group">
                          <BotAvatar bot={bot} size="sm" />
                          <div className="absolute -bottom-8 left-1/2 -translate-x-1/2 bg-gray-800 px-2 py-1 rounded text-xs whitespace-nowrap opacity-0 group-hover:opacity-100 transition-opacity">
                            {bot.name} - {bot.role}
                          </div>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>

                {/* Messages */}
                <div className="flex-1 overflow-y-auto p-6 space-y-4">
                  {activeDebate.messages.map((message) => (
                    <DebateMessage
                      key={message.id}
                      message={message}
                      bot={getBotById(message.botId)}
                    />
                  ))}
                  <div ref={messagesEndRef} />
                </div>

                {/* Consensus */}
                {activeDebate.consensus && (
                  <div className="p-6 border-t border-gray-700 bg-poly-darker/50">
                    <div className="flex items-start gap-3">
                      <span className="text-2xl">🎯</span>
                      <div>
                        <h3 className="font-semibold text-poly-green mb-1">Consensus</h3>
                        <p className="text-gray-300">{activeDebate.consensus}</p>
                      </div>
                    </div>
                  </div>
                )}
              </>
            ) : (
              <div className="flex-1 flex items-center justify-center text-gray-500">
                Select a debate or start a new one
              </div>
            )}
          </div>
        </div>
      </Layout>
    </>
  )
}
