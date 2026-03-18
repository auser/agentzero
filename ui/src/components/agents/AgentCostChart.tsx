import { useQuery } from '@tanstack/react-query'
import { agentsApi, type AgentStatsResponse } from '@/lib/api/agents'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { CostDisplay } from '@/components/shared/CostDisplay'
import {
  BarChart,
  Bar,
  XAxis,
  YAxis,
  Tooltip,
  ResponsiveContainer,
  Cell,
} from 'recharts'

interface AgentCostChartProps {
  agentId: string
}

export function AgentCostChart({ agentId }: AgentCostChartProps) {
  const { data: stats, isPending } = useQuery({
    queryKey: ['agents', agentId, 'stats'],
    queryFn: () => agentsApi.stats(agentId),
  })

  if (isPending || !stats) {
    return (
      <Card>
        <CardContent className="py-6 text-center text-sm text-muted-foreground">
          Loading stats...
        </CardContent>
      </Card>
    )
  }

  return (
    <div className="space-y-4">
      <AgentSummaryCards stats={stats} />
      <ToolUsageChart stats={stats} />
    </div>
  )
}

function AgentSummaryCards({ stats }: { stats: AgentStatsResponse }) {
  return (
    <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
      <Card>
        <CardHeader className="pb-1">
          <CardTitle className="text-xs text-muted-foreground uppercase tracking-wide">
            Total Runs
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-xl font-bold">{stats.total_runs}</p>
          <p className="text-xs text-muted-foreground">
            {stats.running_count} running
          </p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader className="pb-1">
          <CardTitle className="text-xs text-muted-foreground uppercase tracking-wide">
            Cost
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-xl font-bold">
            <CostDisplay microdollars={stats.total_cost_microdollars} />
          </p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader className="pb-1">
          <CardTitle className="text-xs text-muted-foreground uppercase tracking-wide">
            Tokens
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-xl font-bold">
            {stats.total_tokens_used.toLocaleString()}
          </p>
        </CardContent>
      </Card>
      <Card>
        <CardHeader className="pb-1">
          <CardTitle className="text-xs text-muted-foreground uppercase tracking-wide">
            Success Rate
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-xl font-bold">
            {stats.total_runs > 0
              ? `${Math.round((stats.completed_count / stats.total_runs) * 100)}%`
              : '--'}
          </p>
          <p className="text-xs text-muted-foreground">
            {stats.failed_count} failed
          </p>
        </CardContent>
      </Card>
    </div>
  )
}

const TOOL_COLORS = [
  '#3b82f6', '#22c55e', '#eab308', '#ef4444', '#8b5cf6',
  '#ec4899', '#14b8a6', '#f97316',
]

function ToolUsageChart({ stats }: { stats: AgentStatsResponse }) {
  const data = Object.entries(stats.tool_usage)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 10)
    .map(([name, count]) => ({ name, count }))

  if (data.length === 0) return null

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">Tool Usage</CardTitle>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={200}>
          <BarChart data={data} layout="vertical" margin={{ left: 80 }}>
            <XAxis type="number" tick={{ fontSize: 11 }} />
            <YAxis
              type="category"
              dataKey="name"
              tick={{ fontSize: 11 }}
              width={75}
            />
            <Tooltip
              contentStyle={{ background: '#1f2937', border: '1px solid #374151', borderRadius: 6 }}
              labelStyle={{ color: '#e5e7eb' }}
            />
            <Bar dataKey="count" radius={[0, 4, 4, 0]}>
              {data.map((_, i) => (
                <Cell key={i} fill={TOOL_COLORS[i % TOOL_COLORS.length]} />
              ))}
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  )
}
