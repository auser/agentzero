import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { modelsApi } from '@/lib/api/models'
import { Button } from '@/components/ui/button'
import { RefreshCw } from 'lucide-react'

export const Route = createFileRoute('/models/')({
  component: ModelsPage,
})

function ModelsPage() {
  const qc = useQueryClient()

  const { data, isPending, isFetching } = useQuery({
    queryKey: ['models'],
    queryFn: () => modelsApi.list(),
  })

  // Group by provider
  const byProvider: Record<string, NonNullable<typeof data>['data']> = {}
  for (const m of data?.data ?? []) {
    const key = m.owned_by || 'unknown'
    ;(byProvider[key] ??= []).push(m)
  }

  return (
    <div className="space-y-4 max-w-3xl">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Models</h1>
        <Button
          size="sm"
          variant="outline"
          disabled={isFetching}
          onClick={() => void qc.invalidateQueries({ queryKey: ['models'] })}
        >
          <RefreshCw className={`h-4 w-4 mr-2 ${isFetching ? 'animate-spin' : ''}`} />
          Refresh
        </Button>
      </div>

      {isPending && <p className="text-sm text-muted-foreground">Loading…</p>}

      {Object.entries(byProvider).map(([provider, models]) => (
        <div key={provider} className="space-y-2">
          <h2 className="text-sm font-medium text-muted-foreground capitalize">{provider}</h2>
          <div className="rounded-lg border border-border divide-y divide-border">
            {models.map((m) => (
              <div key={m.id} className="flex items-center justify-between px-4 py-2.5">
                <span className="text-sm font-mono">{m.id}</span>
              </div>
            ))}
          </div>
        </div>
      ))}

      {!isPending && data?.data.length === 0 && (
        <p className="text-sm text-muted-foreground">No models found. Check your provider configuration.</p>
      )}
    </div>
  )
}
