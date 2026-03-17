import { useEffect, useRef, useState } from 'react'
import { getBaseUrl, getToken } from '@/lib/api/client'

export interface RunStreamState {
  chunks: string[]
  isDone: boolean
  error: string | null
}

/**
 * Streams run output via SSE from /v1/runs/:runId/stream.
 * Automatically closes when the run finishes or an error occurs.
 */
export function useRunStream(runId: string | null): RunStreamState {
  const [chunks, setChunks] = useState<string[]>([])
  const [isDone, setIsDone] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const esRef = useRef<EventSource | null>(null)

  useEffect(() => {
    if (!runId) return

    const token = getToken()
    const url = new URL(`${getBaseUrl()}/v1/runs/${runId}/stream`, window.location.origin)
    if (token) url.searchParams.set('token', token)

    const es = new EventSource(url.toString())
    esRef.current = es

    es.onmessage = (e: MessageEvent) => {
      const data = e.data as string
      if (data === '[DONE]') {
        setIsDone(true)
        es.close()
        return
      }
      setChunks((prev) => [...prev, data])
    }

    es.onerror = () => {
      setError('Stream connection lost')
      setIsDone(true)
      es.close()
    }

    return () => {
      es.close()
      esRef.current = null
    }
  }, [runId])

  // Reset when runId changes
  useEffect(() => {
    setChunks([])
    setIsDone(false)
    setError(null)
  }, [runId])

  return { chunks, isDone, error }
}
