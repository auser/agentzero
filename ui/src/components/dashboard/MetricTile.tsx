/**
 * Compact metric tile with optional sparkline trend.
 */
import { ResponsiveContainer, AreaChart, Area } from 'recharts'

interface MetricTileProps {
  label: string
  value: string | number
  subtext?: string
  trend?: number[] // array of recent values for sparkline
  trendColor?: string
  icon?: React.ReactNode
  accent?: 'green' | 'blue' | 'violet' | 'yellow' | 'red' | 'default'
}

const ACCENT_STYLES = {
  green: 'border-l-emerald-500/60',
  blue: 'border-l-blue-500/60',
  violet: 'border-l-violet-500/60',
  yellow: 'border-l-yellow-500/60',
  red: 'border-l-red-500/60',
  default: 'border-l-border',
} as const

// Tailwind standard palette — hex required for recharts SVG rendering
// These match: green-500, blue-500, violet-500, yellow-500, red-500, gray-500
const SPARKLINE_COLORS = {
  green: '#22c55e',
  blue: '#3b82f6',
  violet: '#8b5cf6',
  yellow: '#eab308',
  red: '#ef4444',
  default: '#6b7280',
} as const

export function MetricTile({
  label,
  value,
  subtext,
  trend,
  icon,
  accent = 'default',
}: MetricTileProps) {
  const sparkColor = SPARKLINE_COLORS[accent]

  return (
    <div
      className={`relative overflow-hidden rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm px-4 py-3 border-l-2 ${ACCENT_STYLES[accent]}`}
    >
      <div className="flex items-start justify-between">
        <div className="space-y-1 z-10">
          <p className="text-[11px] font-medium uppercase tracking-wider text-muted-foreground">
            {label}
          </p>
          <div className="flex items-baseline gap-2">
            {icon && <span className="text-muted-foreground">{icon}</span>}
            <p className="text-2xl font-bold tracking-tight">{value}</p>
          </div>
          {subtext && (
            <p className="text-[11px] text-muted-foreground">{subtext}</p>
          )}
        </div>
        {trend && trend.length > 1 && (
          <div className="w-20 h-10 opacity-60">
            <ResponsiveContainer width="100%" height="100%">
              <AreaChart data={trend.map((v, i) => ({ i, v }))}>
                <defs>
                  <linearGradient id={`grad-${accent}`} x1="0" y1="0" x2="0" y2="1">
                    <stop offset="0%" stopColor={sparkColor} stopOpacity={0.3} />
                    <stop offset="100%" stopColor={sparkColor} stopOpacity={0} />
                  </linearGradient>
                </defs>
                <Area
                  type="monotone"
                  dataKey="v"
                  stroke={sparkColor}
                  strokeWidth={1.5}
                  fill={`url(#grad-${accent})`}
                  dot={false}
                  isAnimationActive={false}
                />
              </AreaChart>
            </ResponsiveContainer>
          </div>
        )}
      </div>
    </div>
  )
}
