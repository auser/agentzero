/**
 * Active runs timeline with live status indicators.
 */
import { useQuery } from '@tanstack/react-query'
import { runsApi } from '@/lib/api/runs'
import { StatusBadge } from '@/components/shared/StatusBadge'
import { CostDisplay } from '@/components/shared/CostDisplay'
import { Link } from '@tanstack/react-router'
import { PlayCircle, ChevronRight } from 'lucide-react'
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

  const active = activeRuns?.data ?? []
  const recent = (recentRuns?.data ?? [])
    .filter((r) => r.status !== 'running')
    .slice(0, Math.max(0, 6 - active.length))
  const combined = [...active, ...recent]

  return (
    <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm">
      <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
          <PlayCircle className="h-3.5 w-3.5" />
          Runs
          {active.length > 0 && (
            <span className="text-[10px] font-semibold bg-emerald-500/20 text-emerald-400 px-1.5 py-0.5 rounded-full border border-emerald-500/30 animate-pulse">
              {active.length} live
            </span>
          )}
        </h3>
        <Link
          to="/runs"
          className="text-xs text-primary hover:text-primary/80 flex items-center gap-0.5 transition-colors"
        >
          View all <ChevronRight className="h-3 w-3" />
        </Link>
      </div>
      <div className="p-2">
        {combined.length === 0 ? (
          <p className="text-sm text-muted-foreground text-center py-6">No runs yet</p>
        ) : (
          <div className="space-y-0.5">
            {combined.map((run) => (
              <div
                key={run.run_id}
                className={`flex items-center justify-between px-3 py-2 rounded-md transition-colors ${
                  run.status === 'running'
                    ? 'bg-emerald-500/5 border border-emerald-500/15'
                    : 'hover:bg-muted/30'
                }`}
              >
                <div className="flex items-center gap-2.5 min-w-0 flex-1 mr-3">
                  <StatusBadge status={run.status} />
                  <span className="text-xs font-mono text-muted-foreground truncate">
                    {run.agent_id}
                  </span>
                </div>
                <div className="flex items-center gap-3 shrink-0">
                  <CostDisplay
                    microdollars={run.cost_microdollars}
                    className="text-[11px] text-muted-foreground/70"
                  />
                  <span className="text-[10px] text-muted-foreground/50 w-16 text-right">
                    {formatDistanceToNow(new Date(run.accepted_at), { addSuffix: true })}
                  </span>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
