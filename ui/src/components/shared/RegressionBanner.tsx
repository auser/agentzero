import { useEffect, useRef, useState } from 'react'
import { getBaseUrl, getToken } from '@/lib/api/client'
import { AlertTriangle, X } from 'lucide-react'
import { Button } from '@/components/ui/button'

interface RegressionEvent {
  id: number
  file_path: string
  conflicting_agents: string[]
  correlation_id: string
  receivedAt: Date
}

export function RegressionBanner() {
  const [warnings, setWarnings] = useState<RegressionEvent[]>([])
  const counterRef = useRef(0)

  useEffect(() => {
    const token = getToken()
    if (!token) return

    const url = new URL(`${getBaseUrl()}/v1/events`, window.location.origin)
    url.searchParams.set('token', token)
    url.searchParams.set('topic', 'regression.')

    const es = new EventSource(url.toString())

    es.onmessage = (e: MessageEvent) => {
      try {
        const data = JSON.parse(e.data as string) as {
          payload?: string
        }
        if (!data.payload) return
        const payload = JSON.parse(data.payload) as {
          file_path?: string
          conflicting_agents?: Array<[string, string]>
          correlation_id?: string
        }
        if (!payload.file_path) return

        const warning: RegressionEvent = {
          id: ++counterRef.current,
          file_path: payload.file_path,
          conflicting_agents: (payload.conflicting_agents ?? []).map(
            ([agent]) => agent,
          ),
          correlation_id: payload.correlation_id ?? '',
          receivedAt: new Date(),
        }
        setWarnings((prev) => [warning, ...prev].slice(0, 20))
      } catch {
        // ignore parse errors
      }
    }

    es.onerror = () => es.close()
    return () => es.close()
  }, [])

  if (warnings.length === 0) return null

  return (
    <div className="space-y-2">
      {warnings.map((w) => (
        <div
          key={w.id}
          className="flex items-start gap-3 rounded-lg border border-yellow-800/50 bg-yellow-950/30 px-4 py-3"
        >
          <AlertTriangle className="h-4 w-4 text-yellow-500 mt-0.5 shrink-0" />
          <div className="flex-1 min-w-0">
            <p className="text-sm text-yellow-200">
              File conflict detected:{' '}
              <code className="text-xs bg-yellow-900/50 px-1 rounded">
                {w.file_path}
              </code>
            </p>
            <p className="text-xs text-yellow-400/70 mt-0.5">
              Modified by agents:{' '}
              {w.conflicting_agents.join(', ')}
            </p>
          </div>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6 text-yellow-500/50 hover:text-yellow-400"
            onClick={() => setWarnings((prev) => prev.filter((x) => x.id !== w.id))}
          >
            <X className="h-3.5 w-3.5" />
          </Button>
        </div>
      ))}
    </div>
  )
}
