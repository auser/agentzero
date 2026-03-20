/**
 * Channel connection status indicators.
 */
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api/client'
import { Link } from '@tanstack/react-router'
import { Radio, ChevronRight } from 'lucide-react'

interface ConfigResponse {
  channels?: {
    [key: string]: { enabled?: boolean }
  }
}

export function ChannelStatus() {
  const { data } = useQuery({
    queryKey: ['config'],
    queryFn: () => api.get<ConfigResponse>('/v1/config'),
    retry: false,
  })

  const channels: { name: string; connected: boolean }[] = []
  if (data?.channels) {
    for (const [name, config] of Object.entries(data.channels)) {
      channels.push({ name, connected: config.enabled !== false })
    }
  }

  if (channels.length === 0) {
    return null
  }

  return (
    <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm">
      <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
          <Radio className="h-3.5 w-3.5" />
          Channels
        </h3>
        <Link
          to="/channels"
          className="text-xs text-primary hover:text-primary/80 flex items-center gap-0.5 transition-colors"
        >
          Configure <ChevronRight className="h-3 w-3" />
        </Link>
      </div>
      <div className="p-3">
        <div className="flex flex-wrap gap-2">
          {channels.map((ch) => (
            <div
              key={ch.name}
              className="flex items-center gap-1.5 text-xs px-2.5 py-1.5 rounded-md bg-muted/30 border border-border/30 hover:bg-muted/50 transition-colors"
            >
              <span
                className={`h-1.5 w-1.5 rounded-full ${
                  ch.connected
                    ? 'bg-emerald-500 shadow-[0_0_4px_rgba(34,197,94,0.5)]'
                    : 'bg-zinc-600'
                }`}
              />
              <span className="capitalize">{ch.name}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
