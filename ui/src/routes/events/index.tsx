import { createFileRoute } from '@tanstack/react-router'
import { useEffect, useRef, useState } from 'react'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Badge } from '@/components/ui/badge'
import { getBaseUrl, getToken } from '@/lib/api/client'
import { Pause, Play, Trash2 } from 'lucide-react'
import { formatDistanceToNow } from 'date-fns'

export const Route = createFileRoute('/events/')({
  component: EventsPage,
})

interface EventEntry {
  id: number
  topic: string
  data: unknown
  receivedAt: Date
}

function EventsPage() {
  const [entries, setEntries] = useState<EventEntry[]>([])
  const [topicFilter, setTopicFilter] = useState('')
  const [paused, setPaused] = useState(false)
  const counterRef = useRef(0)
  const pausedRef = useRef(false)

  useEffect(() => {
    pausedRef.current = paused
  }, [paused])

  useEffect(() => {
    const token = getToken()
    if (!token) return

    const url = new URL(`${getBaseUrl()}/v1/events`, window.location.origin)
    url.searchParams.set('token', token)
    if (topicFilter) url.searchParams.set('topic', topicFilter)

    const es = new EventSource(url.toString())

    es.onmessage = (e: MessageEvent) => {
      if (pausedRef.current) return
      try {
        const data = JSON.parse(e.data as string) as { topic?: string }
        const entry: EventEntry = {
          id: ++counterRef.current,
          topic: data.topic ?? 'unknown',
          data,
          receivedAt: new Date(),
        }
        setEntries((prev) => [entry, ...prev].slice(0, 500))
      } catch {
        // ignore
      }
    }

    es.onerror = () => es.close()
    return () => es.close()
  }, [topicFilter])

  return (
    <div className="space-y-4 h-full flex flex-col max-w-4xl">
      <div className="flex items-center justify-between shrink-0">
        <h1 className="text-xl font-semibold">Events</h1>
        <div className="flex items-center gap-2">
          <Badge variant="outline" className="text-xs">
            {entries.length} events
          </Badge>
          <Button size="sm" variant="outline" onClick={() => setPaused((p) => !p)}>
            {paused ? <Play className="h-4 w-4 mr-2" /> : <Pause className="h-4 w-4 mr-2" />}
            {paused ? 'Resume' : 'Pause'}
          </Button>
          <Button size="sm" variant="outline" onClick={() => setEntries([])}>
            <Trash2 className="h-4 w-4 mr-2" />
            Clear
          </Button>
        </div>
      </div>

      <div className="shrink-0">
        <Input
          value={topicFilter}
          onChange={(e) => { setEntries([]); setTopicFilter(e.target.value) }}
          placeholder="Filter by topic prefix (e.g. job. or agent.)"
          className="max-w-sm"
        />
      </div>

      <ScrollArea className="flex-1 rounded-lg border border-border bg-card font-mono text-xs">
        <div className="p-3 space-y-1.5">
          {entries.length === 0 && (
            <p className="text-muted-foreground text-center py-8 font-sans text-sm">
              {paused ? 'Stream paused' : 'Waiting for events…'}
            </p>
          )}
          {entries.map((entry) => (
            <div key={entry.id} className="rounded border border-border p-2 space-y-1 hover:bg-muted/20">
              <div className="flex items-center justify-between">
                <Badge variant="outline" className="text-xs px-1.5 py-0 h-5">
                  {entry.topic}
                </Badge>
                <span className="text-muted-foreground text-xs">
                  {formatDistanceToNow(entry.receivedAt, { addSuffix: true })}
                </span>
              </div>
              <pre className="text-xs text-muted-foreground overflow-x-auto whitespace-pre-wrap wrap-break-word">
                {JSON.stringify(entry.data, null, 2)}
              </pre>
            </div>
          ))}
        </div>
      </ScrollArea>
    </div>
  )
}
