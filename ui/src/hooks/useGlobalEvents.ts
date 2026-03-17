import { useEffect } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { getBaseUrl, getToken } from '@/lib/api/client'

/**
 * Subscribes to the global SSE event bus at /v1/events.
 * Invalidates TanStack Query caches based on event topic prefix.
 * Mount once in Shell.tsx.
 */
export function useGlobalEvents() {
  const queryClient = useQueryClient()

  useEffect(() => {
    const token = getToken()
    if (!token) return

    const url = new URL(`${getBaseUrl()}/v1/events`, window.location.origin)
    url.searchParams.set('token', token)

    const es = new EventSource(url.toString())

    es.onmessage = (e: MessageEvent) => {
      try {
        const event = JSON.parse(e.data as string) as { topic?: string }
        const topic = event.topic ?? ''

        if (topic.startsWith('job.') || topic.startsWith('run.')) {
          void queryClient.invalidateQueries({ queryKey: ['runs'] })
        }
        if (topic.startsWith('agent.')) {
          void queryClient.invalidateQueries({ queryKey: ['agents'] })
        }
        if (topic.startsWith('approval.')) {
          void queryClient.invalidateQueries({ queryKey: ['approvals'] })
        }
        if (topic.startsWith('cron.')) {
          void queryClient.invalidateQueries({ queryKey: ['cron'] })
        }
      } catch {
        // ignore malformed events
      }
    }

    es.onerror = () => {
      es.close()
    }

    return () => es.close()
  }, [queryClient])
}
