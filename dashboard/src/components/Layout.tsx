import Link from 'next/link'
import { useRouter } from 'next/router'

interface LayoutProps {
  children: React.ReactNode
}

export default function Layout({ children }: LayoutProps) {
  const router = useRouter()

  const navItems = [
    { href: '/', label: 'Dashboard', icon: '📊' },
    { href: '/debate', label: 'Bot Debates', icon: '💬' },
    { href: '/trades', label: 'Trade History', icon: '📜' },
    { href: '/settings', label: 'Settings', icon: '⚙️' },
  ]

  return (
    <div className="min-h-screen">
      {/* Top Navigation */}
      <nav className="bg-poly-darker border-b border-gray-800">
        <div className="max-w-7xl mx-auto px-4">
          <div className="flex items-center justify-between h-16">
            {/* Logo */}
            <Link href="/" className="flex items-center gap-3">
              <span className="text-2xl">🐂</span>
              <span className="text-xl font-bold text-white">Bruniao</span>
            </Link>

            {/* Navigation Links */}
            <div className="flex items-center gap-1">
              {navItems.map((item) => (
                <Link
                  key={item.href}
                  href={item.href}
                  className={`flex items-center gap-2 px-4 py-2 rounded-lg transition-colors ${
                    router.pathname === item.href
                      ? 'bg-poly-blue/20 text-poly-blue'
                      : 'text-gray-400 hover:text-white hover:bg-gray-800'
                  }`}
                >
                  <span>{item.icon}</span>
                  <span>{item.label}</span>
                </Link>
              ))}
            </div>

            {/* Quick Actions */}
            <div className="flex items-center gap-3">
              <button className="px-4 py-2 bg-poly-green/20 text-poly-green rounded-lg hover:bg-poly-green/30 transition-colors">
                Start Bot
              </button>
              <button className="px-4 py-2 bg-red-500/20 text-red-400 rounded-lg hover:bg-red-500/30 transition-colors">
                Kill Switch
              </button>
            </div>
          </div>
        </div>
      </nav>

      {/* Main Content */}
      <main className="max-w-7xl mx-auto px-4 py-8">
        {children}
      </main>
    </div>
  )
}
