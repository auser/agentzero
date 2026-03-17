import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api/client'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Check, X } from 'lucide-react'

export const Route = createFileRoute('/approvals/')({
  component: ApprovalsPage,
})

interface ApprovalItem {
  id: string
  tool: string
  args: unknown
  agent_id?: string
  requested_at?: string
}

interface ApprovalsResponse {
  data: ApprovalItem[]
  total: number
}

function ApprovalsPage() {
  const qc = useQueryClient()

  const { data, isPending, error } = useQuery({
    queryKey: ['approvals'],
    queryFn: () => api.get<ApprovalsResponse>('/v1/approvals'),
    refetchInterval: 5_000,
    retry: false,
  })

  const approveMutation = useMutation({
    mutationFn: (id: string) => api.post(`/v1/approvals/${id}/approve`),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['approvals'] }),
  })

  const denyMutation = useMutation({
    mutationFn: (id: string) => api.post(`/v1/approvals/${id}/deny`),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['approvals'] }),
  })

  return (
    <div className="space-y-4 max-w-3xl">
      <div className="flex items-center gap-3">
        <h1 className="text-xl font-semibold">Approvals</h1>
        {data && data.total > 0 && (
          <Badge className="bg-yellow-500/20 text-yellow-400 border-yellow-500/30">
            {data.total} pending
          </Badge>
        )}
      </div>

      {error && (
        <div className="rounded-lg border border-border p-4">
          <p className="text-sm font-medium">Approvals endpoint not yet available</p>
          <p className="text-xs text-muted-foreground mt-1">
            Add <code className="bg-muted px-1 rounded">GET /v1/approvals</code> to the gateway.
          </p>
        </div>
      )}

      {isPending && <p className="text-sm text-muted-foreground">Loading…</p>}

      {!error && !isPending && data?.data.length === 0 && (
        <div className="rounded-lg border border-border p-8 text-center">
          <p className="text-sm text-muted-foreground">No pending approvals</p>
        </div>
      )}

      <div className="space-y-3">
        {data?.data.map((item) => (
          <div key={item.id} className="rounded-lg border border-yellow-800/30 bg-yellow-950/10 p-4 space-y-3">
            <div className="flex items-start justify-between gap-4">
              <div className="space-y-1 min-w-0">
                <div className="flex items-center gap-2">
                  <Badge variant="outline" className="font-mono text-xs">{item.tool}</Badge>
                  {item.agent_id && (
                    <span className="text-xs text-muted-foreground">{item.agent_id}</span>
                  )}
                </div>
                <pre className="text-xs font-mono bg-muted rounded p-2 overflow-x-auto max-h-24 whitespace-pre-wrap break-all">
                  {JSON.stringify(item.args, null, 2)}
                </pre>
              </div>
              <div className="flex gap-2 shrink-0">
                <Button
                  size="sm"
                  variant="outline"
                  className="border-emerald-800/50 text-emerald-400 hover:bg-emerald-950/50"
                  onClick={() => approveMutation.mutate(item.id)}
                  disabled={approveMutation.isPending}
                >
                  <Check className="h-4 w-4 mr-1.5" />
                  Approve
                </Button>
                <Button
                  size="sm"
                  variant="outline"
                  className="border-red-800/50 text-red-400 hover:bg-red-950/50"
                  onClick={() => denyMutation.mutate(item.id)}
                  disabled={denyMutation.isPending}
                >
                  <X className="h-4 w-4 mr-1.5" />
                  Deny
                </Button>
              </div>
            </div>
          </div>
        ))}
      </div>
    </div>
  )
}
