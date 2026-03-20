/**
 * Channel connection status indicators.
 */
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api/client'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Link } from '@tanstack/react-router'
import { Radio } from 'lucide-react'

interface ChannelInfo {
  name: string
  type: string
  connected: boolean
}

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

  // Derive channel list from config
  const channels: ChannelInfo[] = []
  if (data?.channels) {
    for (const [name, config] of Object.entries(data.channels)) {
      channels.push({
        name,
        type: name,
        connected: config.enabled !== false,
      })
    }
  }

  if (channels.length === 0) {
    return null
  }

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm flex items-center gap-1.5">
            <Radio className="h-3.5 w-3.5" />
            Channels
          </CardTitle>
          <Link to="/channels" className="text-xs text-primary hover:underline">
            Configure
          </Link>
        </div>
      </CardHeader>
      <CardContent>
        <div className="flex flex-wrap gap-2">
          {channels.map((ch) => (
            <div
              key={ch.name}
              className="flex items-center gap-1.5 text-xs px-2 py-1 rounded bg-muted/50"
            >
              <span
                className={`h-1.5 w-1.5 rounded-full ${ch.connected ? 'bg-green-500' : 'bg-gray-500'}`}
              />
              <span className="capitalize">{ch.name}</span>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}
