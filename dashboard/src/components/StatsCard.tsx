interface StatsCardProps {
  title: string
  value: string
  subtitle?: string
  positive?: boolean
}

export default function StatsCard({ title, value, subtitle, positive }: StatsCardProps) {
  return (
    <div className="bg-poly-dark rounded-xl p-4">
      <p className="text-gray-400 text-sm mb-1">{title}</p>
      <p className={`text-2xl font-bold ${
        positive === undefined ? 'text-white' :
        positive ? 'text-poly-green' : 'text-poly-red'
      }`}>
        {value}
      </p>
      {subtitle && (
        <p className={`text-sm mt-1 ${
          positive === undefined ? 'text-gray-500' :
          positive ? 'text-poly-green/70' : 'text-poly-red/70'
        }`}>
          {subtitle}
        </p>
      )}
    </div>
  )
}
