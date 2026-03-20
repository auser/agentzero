/**
 * System health metrics row with sparkline trends.
 */
import { useQuery } from '@tanstack/react-query'
import { healthApi } from '@/lib/api/health'
import { agentsApi } from '@/lib/api/agents'
import { runsApi } from '@/lib/api/runs'
import { MetricTile } from './MetricTile'

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
  const costDisplay = `$${(totalCost / 1_000_000).toFixed(2)}`

  // Generate sparkline data from recent runs (cost per run, last 10)
  const costTrend = (allRuns?.data ?? [])
    .slice(0, 10)
    .reverse()
    .map((r) => (r.cost_microdollars ?? 0) / 1_000_000)

  const runTrend = (allRuns?.data ?? [])
    .slice(0, 10)
    .reverse()
    .map((_, i) => i + 1)

  return (
    <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
      <MetricTile
        label="Gateway"
        value={health?.status === 'ok' ? 'Online' : 'Offline'}
        subtext={health?.status === 'ok' ? 'All systems operational' : 'Connection failed'}
        accent={health?.status === 'ok' ? 'green' : 'red'}
      />
      <MetricTile
        label="Agents"
        value={activeAgents}
        subtext={`${totalAgents} total configured`}
        accent="blue"
        trend={runTrend.length > 1 ? runTrend : undefined}
      />
      <MetricTile
        label="Running"
        value={runningCount}
        subtext={`${allRuns?.total ?? 0} total runs`}
        accent={runningCount > 0 ? 'green' : 'default'}
        trend={runTrend.length > 1 ? runTrend : undefined}
      />
      <MetricTile
        label="Total Cost"
        value={costDisplay}
        subtext="Across all runs"
        accent="violet"
        trend={costTrend.length > 1 ? costTrend : undefined}
      />
    </div>
  )
}
