import { createFileRoute } from '@tanstack/react-router'
import { useEffect, useRef, useState, useCallback } from 'react'
import { useAuthStore } from '@/store/authStore'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Button } from '@/components/ui/button'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { RefreshCw, Trash2, Maximize2, Minimize2, History } from 'lucide-react'

export const Route = createFileRoute('/canvas/')({
  component: CanvasPage,
})

interface CanvasFrame {
  content_type: string
  content: string
  timestamp: number
}

interface CanvasSummary {
  id: string
  content_type: string | null
  frame_count: number
  created_at: number
}

function CanvasPage() {
  const [canvases, setCanvases] = useState<CanvasSummary[]>([])
  const [selectedId, setSelectedId] = useState<string | undefined>()
  const [currentFrame, setCurrentFrame] = useState<CanvasFrame | null>(null)
  const [history, setHistory] = useState<CanvasFrame[]>([])
  const [showHistory, setShowHistory] = useState(false)
  const [fullscreen, setFullscreen] = useState(false)
  const iframeRef = useRef<HTMLIFrameElement>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const token = useAuthStore((s) => s.token)

  const apiBase = import.meta.env.VITE_API_BASE ?? ''

  // Fetch canvas list
  const fetchCanvases = useCallback(async () => {
    try {
      const res = await fetch(`${apiBase}/api/canvas`, {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
      })
      if (res.ok) {
        const data = await res.json()
        setCanvases(data)
        if (!selectedId && data.length > 0) {
          setSelectedId(data[0].id)
        }
      }
    } catch {
      // silently ignore fetch errors
    }
  }, [apiBase, token, selectedId])

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- async fetch sets state in callback, not synchronously
    void fetchCanvases()
    const interval = setInterval(() => void fetchCanvases(), 5000)
    return () => clearInterval(interval)
  }, [fetchCanvases])

  // Fetch snapshot when canvas changes
  useEffect(() => {
    if (!selectedId) return
    async function fetchSnapshot() {
      try {
        const res = await fetch(`${apiBase}/api/canvas/${selectedId}`, {
          headers: token ? { Authorization: `Bearer ${token}` } : {},
        })
        if (res.ok) {
          const data = await res.json()
          if (data.current) {
            setCurrentFrame(data.current)
          }
        }
      } catch {
        // silently ignore
      }
    }
    void fetchSnapshot()
  }, [selectedId, apiBase, token])

  // Fetch history
  useEffect(() => {
    if (!selectedId || !showHistory) return
    async function fetchHistory() {
      try {
        const res = await fetch(`${apiBase}/api/canvas/${selectedId}/history`, {
          headers: token ? { Authorization: `Bearer ${token}` } : {},
        })
        if (res.ok) {
          const data = await res.json()
          setHistory(data)
        }
      } catch {
        // silently ignore
      }
    }
    void fetchHistory()
  }, [selectedId, showHistory, apiBase, token])

  // WebSocket connection for real-time updates
  useEffect(() => {
    if (!selectedId) return

    const wsProto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    const wsHost = apiBase ? new URL(apiBase).host : window.location.host
    const wsUrl = `${wsProto}//${wsHost}/ws/canvas/${selectedId}${token ? `?token=${token}` : ''}`

    const ws = new WebSocket(wsUrl)
    wsRef.current = ws

    ws.onmessage = (event) => {
      try {
        const frame: CanvasFrame = JSON.parse(event.data)
        setCurrentFrame(frame)
      } catch {
        // ignore parse errors
      }
    }

    ws.onclose = () => {
      // Reconnect after 2s
      setTimeout(() => {
        if (wsRef.current === ws) {
          wsRef.current = null
        }
      }, 2000)
    }

    return () => {
      ws.close()
      wsRef.current = null
    }
  }, [selectedId, apiBase, token])

  // Render content in sandboxed iframe
  useEffect(() => {
    if (!currentFrame || !iframeRef.current) return

    const iframe = iframeRef.current
    let html = currentFrame.content

    if (currentFrame.content_type === 'text/markdown') {
      // Wrap markdown in a pre block (proper rendering would need a markdown lib)
      html = `<pre style="white-space:pre-wrap;font-family:system-ui;padding:1rem">${escapeHtml(currentFrame.content)}</pre>`
    } else if (currentFrame.content_type === 'text/plain') {
      html = `<pre style="white-space:pre-wrap;font-family:monospace;padding:1rem">${escapeHtml(currentFrame.content)}</pre>`
    }
    // text/html and image/svg+xml render directly

    iframe.srcdoc = html
  }, [currentFrame])

  async function handleClear() {
    if (!selectedId) return
    await fetch(`${apiBase}/api/canvas/${selectedId}`, {
      method: 'DELETE',
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    })
    setCurrentFrame(null)
  }

  return (
    <div className={fullscreen ? 'fixed inset-0 z-50 bg-background p-4 flex flex-col' : 'flex flex-col h-full'}>
      {/* Toolbar */}
      <div className="flex items-center gap-3 mb-4">
        <h1 className="text-lg font-semibold mr-4">Canvas</h1>

        <Select value={selectedId} onValueChange={setSelectedId}>
          <SelectTrigger className="w-[200px]">
            <SelectValue placeholder="Select canvas..." />
          </SelectTrigger>
          <SelectContent>
            {canvases.map((c) => (
              <SelectItem key={c.id} value={c.id}>
                {c.id} ({c.frame_count} frames)
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Button variant="outline" size="icon" onClick={() => void fetchCanvases()} title="Refresh list">
          <RefreshCw className="h-4 w-4" />
        </Button>

        <Button variant="outline" size="icon" onClick={() => setShowHistory(!showHistory)} title="Toggle history">
          <History className="h-4 w-4" />
        </Button>

        <Button variant="outline" size="icon" onClick={() => void handleClear()} title="Clear canvas" disabled={!selectedId}>
          <Trash2 className="h-4 w-4" />
        </Button>

        <div className="flex-1" />

        <span className="text-xs text-muted-foreground">
          {currentFrame ? `${currentFrame.content_type} | ${new Date(currentFrame.timestamp * 1000).toLocaleTimeString()}` : 'No content'}
        </span>

        <Button variant="outline" size="icon" onClick={() => setFullscreen(!fullscreen)} title="Toggle fullscreen">
          {fullscreen ? <Minimize2 className="h-4 w-4" /> : <Maximize2 className="h-4 w-4" />}
        </Button>
      </div>

      {/* Content area */}
      <div className="flex flex-1 gap-4 min-h-0">
        {/* Canvas viewer */}
        <div className="flex-1 rounded-lg border border-border overflow-hidden bg-white">
          <iframe
            ref={iframeRef}
            sandbox="allow-scripts"
            title="Canvas viewer"
            className="w-full h-full border-0"
            style={{ minHeight: '400px' }}
          />
        </div>

        {/* History panel */}
        {showHistory && (
          <div className="w-72 rounded-lg border border-border overflow-hidden">
            <div className="px-3 py-2 border-b border-border bg-muted/50">
              <span className="text-sm font-medium">History ({history.length})</span>
            </div>
            <ScrollArea className="h-full">
              <div className="p-2 space-y-2">
                {history.map((frame, i) => (
                  <button
                    key={i}
                    className="w-full text-left p-2 rounded border border-border hover:bg-muted/50 transition-colors"
                    onClick={() => setCurrentFrame(frame)}
                  >
                    <div className="text-xs text-muted-foreground">{frame.content_type}</div>
                    <div className="text-xs text-muted-foreground">
                      {new Date(frame.timestamp * 1000).toLocaleTimeString()}
                    </div>
                    <div className="text-xs truncate mt-1">{frame.content.slice(0, 80)}</div>
                  </button>
                ))}
              </div>
            </ScrollArea>
          </div>
        )}
      </div>
    </div>
  )
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}
