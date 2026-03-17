import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { api } from '@/lib/api/client'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Trash2, Search } from 'lucide-react'
import { formatDistanceToNow } from 'date-fns'

export const Route = createFileRoute('/memory/')({
  component: MemoryPage,
})

interface MemoryEntry {
  id: string
  role: string
  content: string
  conversation_id?: string
  created_at?: string
}

interface MemoryListResponse {
  data: MemoryEntry[]
  total: number
}

function MemoryPage() {
  const qc = useQueryClient()
  const [query, setQuery] = useState('')
  const [debouncedQ, setDebouncedQ] = useState('')

  const { data, isPending, error } = useQuery({
    queryKey: ['memory', debouncedQ],
    queryFn: () => api.get<MemoryListResponse>(`/v1/memory${debouncedQ ? `?q=${encodeURIComponent(debouncedQ)}` : ''}`),
    retry: false,
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.delete(`/v1/memory/${id}`),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['memory'] }),
  })

  function handleSearch(e: React.FormEvent) {
    e.preventDefault()
    setDebouncedQ(query)
  }

  return (
    <div className="space-y-4 max-w-3xl">
      <h1 className="text-xl font-semibold">Memory</h1>

      <form onSubmit={handleSearch} className="flex gap-2">
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search memory…"
          className="flex-1"
        />
        <Button type="submit" size="sm" variant="outline">
          <Search className="h-4 w-4" />
        </Button>
      </form>

      {isPending && <p className="text-sm text-muted-foreground">Loading…</p>}

      {error && (
        <div className="rounded-lg border border-border p-4">
          <p className="text-sm font-medium">Memory endpoint not yet available</p>
          <p className="text-xs text-muted-foreground mt-1">
            Add <code className="bg-muted px-1 rounded">GET /v1/memory</code> to the gateway.
          </p>
        </div>
      )}

      {data && (
        <div className="space-y-2">
          <p className="text-xs text-muted-foreground">{data.total} entries</p>
          <ScrollArea className="h-[60vh]">
            <div className="space-y-2 pr-2">
              {data.data.map((entry, i) => (
                <div key={entry.id ?? i} className="rounded-lg border border-border p-3 space-y-1">
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-2">
                      <span className="text-xs font-medium capitalize">{entry.role}</span>
                      {entry.conversation_id && (
                        <span className="text-xs text-muted-foreground font-mono">{entry.conversation_id.slice(0, 8)}</span>
                      )}
                    </div>
                    <div className="flex items-center gap-2">
                      {entry.created_at && (
                        <span className="text-xs text-muted-foreground">
                          {formatDistanceToNow(new Date(entry.created_at), { addSuffix: true })}
                        </span>
                      )}
                      <Button
                        variant="ghost" size="icon" className="h-6 w-6 text-muted-foreground hover:text-destructive"
                        onClick={() => deleteMutation.mutate(entry.id)}
                      >
                        <Trash2 className="h-3 w-3" />
                      </Button>
                    </div>
                  </div>
                  <p className="text-xs text-muted-foreground line-clamp-3">{entry.content}</p>
                </div>
              ))}
              {data.data.length === 0 && (
                <p className="text-sm text-muted-foreground text-center py-8">No memory entries</p>
              )}
            </div>
          </ScrollArea>
        </div>
      )}
    </div>
  )
}
