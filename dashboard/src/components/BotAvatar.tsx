interface Bot {
  id: string
  name: string
  role: string
  avatar: string
  color: string
}

interface BotAvatarProps {
  bot: Bot
  size?: 'sm' | 'md' | 'lg'
}

export default function BotAvatar({ bot, size = 'md' }: BotAvatarProps) {
  const sizeClasses = {
    sm: 'w-8 h-8 text-lg',
    md: 'w-12 h-12 text-2xl',
    lg: 'w-16 h-16 text-3xl',
  }

  return (
    <div
      className={`${sizeClasses[size]} rounded-full bg-poly-darker flex items-center justify-center border-2 border-gray-700`}
      title={`${bot.name} - ${bot.role}`}
    >
      {bot.avatar}
    </div>
  )
}
