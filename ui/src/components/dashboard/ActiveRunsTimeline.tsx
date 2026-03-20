/**
 * Active runs timeline showing real-time run progress.
 */
import { useQuery } from '@tanstack/react-query'
import { runsApi } from '@/lib/api/runs'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { StatusBadge } from '@/components/shared/StatusBadge'
import { CostDisplay } from '@/components/shared/CostDisplay'
import { Link } from '@tanstack/react-router'
import { PlayCircle } from 'lucide-react'
import { formatDistanceToNow } from 'date-fns'

export function ActiveRunsTimeline() {
  const { data: activeRuns } = useQuery({
    queryKey: ['runs', { status: 'running' }],
    queryFn: () => runsApi.list('running'),
    refetchInterval: 3_000,
  })

  const { data: recentRuns } = useQuery({
    queryKey: ['runs'],
    queryFn: () => runsApi.list(),
  })

  // Show active runs first, then recent completed (up to 5 total)
  const active = activeRuns?.data ?? []
  const recent = (recentRuns?.data ?? [])
    .filter((r) => r.status !== 'running')
    .slice(0, Math.max(0, 5 - active.length))
  const combined = [...active, ...recent]

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm flex items-center gap-1.5">
            <PlayCircle className="h-3.5 w-3.5" />
            Runs
            {active.length > 0 && (
              <span className="text-[10px] bg-green-500/20 text-green-400 px-1.5 py-0.5 rounded-full">
                {active.length} active
              </span>
            )}
          </CardTitle>
          <Link to="/runs" className="text-xs text-primary hover:underline">
            View all
          </Link>
        </div>
      </CardHeader>
      <CardContent>
        {combined.length === 0 ? (
          <p className="text-sm text-muted-foreground text-center py-4">No runs yet</p>
        ) : (
          <div className="space-y-1.5">
            {combined.map((run) => (
              <div
                key={run.run_id}
                className={`flex items-center justify-between py-1.5 px-2 rounded ${
                  run.status === 'running' ? 'bg-green-500/5 border border-green-500/20' : 'border-b border-border last:border-0'
                }`}
              >
                <div className="flex items-center gap-2 min-w-0 flex-1 mr-3">
                  <StatusBadge status={run.status} />
                  <span className="text-xs font-mono text-muted-foreground truncate">
                    {run.agent_id}
                  </span>
                </div>
                <div className="flex items-center gap-2 shrink-0">
                  <CostDisplay
                    microdollars={run.cost_microdollars}
                    className="text-xs text-muted-foreground"
                  />
                  <span className="text-[10px] text-muted-foreground">
                    {formatDistanceToNow(new Date(run.accepted_at), { addSuffix: true })}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}
