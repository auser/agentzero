/**
 * Compact system health bar showing gateway, agent, and run status at a glance.
 */
import { useQuery } from '@tanstack/react-query'
import { healthApi } from '@/lib/api/health'
import { agentsApi } from '@/lib/api/agents'
import { runsApi } from '@/lib/api/runs'
import { CostDisplay } from '@/components/shared/CostDisplay'

function HealthDot({ ok }: { ok: boolean }) {
  return (
    <span
      className={`inline-block h-2 w-2 rounded-full ${ok ? 'bg-green-500' : 'bg-red-500'}`}
    />
  )
}

export function SystemHealthBar() {
  const { data: health } = useQuery({
    queryKey: ['health'],
    queryFn: () => healthApi.get(),
    refetchInterval: 30_000,
    retry: false,
  })

  const { data: agents } = useQuery({
    queryKey: ['agents'],
    queryFn: () => agentsApi.list(),
  })

  const { data: activeRuns } = useQuery({
    queryKey: ['runs', { status: 'running' }],
    queryFn: () => runsApi.list('running'),
    refetchInterval: 5_000,
  })

  const { data: allRuns } = useQuery({
    queryKey: ['runs'],
    queryFn: () => runsApi.list(),
  })

  const activeAgents = agents?.data.filter((a) => a.status === 'active').length ?? 0
  const totalAgents = agents?.total ?? 0
  const runningCount = activeRuns?.total ?? 0
  const totalCost = allRuns?.data.reduce((sum, r) => sum + (r.cost_microdollars ?? 0), 0) ?? 0

  return (
    <div className="flex items-center gap-6 px-4 py-2.5 bg-muted/30 rounded-lg text-xs">
      <div className="flex items-center gap-1.5">
        <HealthDot ok={health?.status === 'ok'} />
        <span className="text-muted-foreground">Gateway</span>
        <span className="font-medium">{health?.status ?? 'unknown'}</span>
      </div>

      <div className="h-3 w-px bg-border" />

      <div className="flex items-center gap-1.5">
        <span className="text-muted-foreground">Agents</span>
        <span className="font-medium">
          {activeAgents}/{totalAgents}
        </span>
      </div>

      <div className="h-3 w-px bg-border" />

      <div className="flex items-center gap-1.5">
        <span className="text-muted-foreground">Running</span>
        <span className={`font-medium ${runningCount > 0 ? 'text-green-400' : ''}`}>
          {runningCount}
        </span>
      </div>

      <div className="h-3 w-px bg-border" />

      <div className="flex items-center gap-1.5">
        <span className="text-muted-foreground">Cost</span>
        <span className="font-medium">
          <CostDisplay microdollars={totalCost} />
        </span>
      </div>
    </div>
  )
}
