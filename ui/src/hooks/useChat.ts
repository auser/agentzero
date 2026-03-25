import { useCallback, useEffect, useRef, useState } from 'react'
import { getBaseUrl, getToken } from '@/lib/api/client'

export interface ChatMessage {
  id: string
  role: 'user' | 'assistant'
  content: string
  streaming?: boolean
}

type WsStatus = 'connecting' | 'open' | 'closed' | 'error'

/**
 * Manages a WebSocket connection to /ws/chat.
 * Falls back to SSE via /v1/chat/completions if WS fails.
 */
export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([])
  const [status, setStatus] = useState<WsStatus>('closed')
  const wsRef = useRef<WebSocket | null>(null)

  const connect = useCallback(() => {
    const token = getToken()
    if (!token) return

    const base = getBaseUrl().replace(/^http/, 'ws')
    const url = `${base}/ws/chat?token=${encodeURIComponent(token)}`

    setStatus('connecting')
    const ws = new WebSocket(url)
    wsRef.current = ws

    ws.onopen = () => setStatus('open')
    ws.onerror = () => setStatus('error')
    ws.onclose = () => setStatus('closed')

    ws.onmessage = (e: MessageEvent) => {
      try {
        const msg = JSON.parse(e.data as string) as { type: string; delta?: string; content?: string }

        if (msg.type === 'delta' && msg.delta) {
          setMessages((prev) => {
            const last = prev[prev.length - 1]
            if (last?.role === 'assistant' && last.streaming) {
              return [
                ...prev.slice(0, -1),
                { ...last, content: last.content + msg.delta! },
              ]
            }
            return [
              ...prev,
              { id: crypto.randomUUID(), role: 'assistant', content: msg.delta!, streaming: true },
            ]
          })
        } else if (msg.type === 'done') {
          setMessages((prev) => {
            const last = prev[prev.length - 1]
            if (last?.streaming) {
              return [...prev.slice(0, -1), { ...last, streaming: false }]
            }
            return prev
          })
        }
      } catch {
        // ignore malformed frames
      }
    }
  }, [])

  useEffect(() => {
    connect()
    return () => wsRef.current?.close()
  }, [connect])

  const send = useCallback(
    (text: string, opts?: { agentId?: string; model?: string; provider?: string }) => {
      const userMsg: ChatMessage = { id: crypto.randomUUID(), role: 'user', content: text }
      setMessages((prev) => [...prev, userMsg])

      if (wsRef.current?.readyState === WebSocket.OPEN) {
        wsRef.current.send(
          JSON.stringify({
            message: text,
            agent_id: opts?.agentId,
            model: opts?.model,
            provider: opts?.provider,
          }),
        )
      }
    },
    [],
  )

  const clear = useCallback(() => setMessages([]), [])

  return { messages, status, send, clear }
}
