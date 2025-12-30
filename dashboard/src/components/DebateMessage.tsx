import { format } from 'date-fns'
import BotAvatar from './BotAvatar'

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

interface DebateMessageProps {
  message: Message
  bot: Bot
}

export default function DebateMessage({ message, bot }: DebateMessageProps) {
  return (
    <div className="flex gap-4">
      <BotAvatar bot={bot} />
      <div className="flex-1">
        <div className="flex items-center gap-3 mb-1">
          <span className={`font-semibold ${bot.color}`}>{message.botName}</span>
          <span className="text-xs text-gray-500">{bot.role}</span>
          <span className="text-xs text-gray-600">
            {format(message.timestamp, 'HH:mm:ss')}
          </span>
        </div>
        <div className="bg-poly-darker rounded-lg p-4">
          <p className="text-gray-200">{message.content}</p>
          {message.reasoning && (
            <p className="text-sm text-gray-500 mt-2 pt-2 border-t border-gray-700">
              <span className="text-gray-400">Reasoning:</span> {message.reasoning}
            </p>
          )}
          <div className="flex items-center gap-2 mt-3">
            <span className="text-xs text-gray-500">Confidence:</span>
            <div className="flex-1 h-1.5 bg-gray-700 rounded-full max-w-[100px]">
              <div
                className="h-full bg-poly-blue rounded-full"
                style={{ width: `${message.confidence * 100}%` }}
              />
            </div>
            <span className="text-xs text-gray-400">{(message.confidence * 100).toFixed(0)}%</span>
          </div>
        </div>
      </div>
    </div>
  )
}
