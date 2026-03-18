import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { healthApi } from '@/lib/api/health'
import { agentsApi } from '@/lib/api/agents'
import { runsApi } from '@/lib/api/runs'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { StatusBadge } from '@/components/shared/StatusBadge'
import { CostDisplay } from '@/components/shared/CostDisplay'
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
import { Button } from '@/components/ui/button'
import { Link } from '@tanstack/react-router'
import { MessageSquare, PlayCircle, AlertTriangle, Activity } from 'lucide-react'
import { useState } from 'react'
import { formatDistanceToNow } from 'date-fns'
import { TopologyGraph } from '@/components/dashboard/TopologyGraph'
import { RegressionBanner } from '@/components/shared/RegressionBanner'

export const Route = createFileRoute('/dashboard/')({
  component: DashboardPage,
})

function DashboardPage() {
  const [estopOpen, setEstopOpen] = useState(false)
  const queryClient = useQueryClient()

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

  const estopMutation = useMutation({
    mutationFn: runsApi.estop,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['runs'] }),
  })

  const recentRuns = allRuns?.data.slice(0, 5) ?? []
  const activeAgents = agents?.data.filter((a) => a.status === 'active').length ?? 0
  const totalCost = allRuns?.data.reduce((sum, r) => sum + (r.cost_microdollars ?? 0), 0) ?? 0

  return (
    <div className="space-y-6 max-w-5xl">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Dashboard</h1>
        <div className="flex gap-2">
          <Link to="/chat">
            <Button variant="outline" size="sm">
              <MessageSquare className="h-4 w-4 mr-2" />
              New Chat
            </Button>
          </Link>
          <Button
            variant="outline"
            size="sm"
            className="border-red-800/50 text-red-400 hover:bg-red-950/50"
            onClick={() => setEstopOpen(true)}
          >
            <AlertTriangle className="h-4 w-4 mr-2" />
            E-Stop
          </Button>
        </div>
      </div>

      <RegressionBanner />

      <TopologyGraph />

      {/* Summary cards */}
      <div className="grid grid-cols-2 lg:grid-cols-4 gap-4">
        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-xs text-muted-foreground font-medium uppercase tracking-wide">
              Gateway
            </CardTitle>
          </CardHeader>
          <CardContent>
            {health
              ? <StatusBadge status={health.status} />
              : <StatusBadge status="closed" />}
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-xs text-muted-foreground font-medium uppercase tracking-wide">
              Active Agents
            </CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-2xl font-bold">{activeAgents}</p>
            <p className="text-xs text-muted-foreground">{agents?.total ?? 0} total</p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-xs text-muted-foreground font-medium uppercase tracking-wide flex items-center gap-1">
              <Activity className="h-3 w-3" /> Running
            </CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-2xl font-bold">{activeRuns?.total ?? 0}</p>
            <Link to="/runs" className="text-xs text-primary hover:underline">View runs →</Link>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-2">
            <CardTitle className="text-xs text-muted-foreground font-medium uppercase tracking-wide">
              Total Cost
            </CardTitle>
          </CardHeader>
          <CardContent>
            <p className="text-2xl font-bold">
              <CostDisplay microdollars={totalCost} />
            </p>
          </CardContent>
        </Card>
      </div>

      {/* Recent runs */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <CardTitle className="text-sm">Recent Runs</CardTitle>
            <Link to="/runs" className="text-xs text-primary hover:underline">View all</Link>
          </div>
        </CardHeader>
        <CardContent>
          {recentRuns.length === 0 ? (
            <p className="text-sm text-muted-foreground text-center py-6">No runs yet</p>
          ) : (
            <div className="space-y-2">
              {recentRuns.map((run) => (
                <div key={run.run_id} className="flex items-center justify-between py-2 border-b border-border last:border-0">
                  <div className="space-y-0.5 min-w-0 flex-1 mr-4">
                    <p className="text-xs font-mono text-muted-foreground truncate">{run.run_id}</p>
                    <p className="text-xs text-muted-foreground">{run.agent_id}</p>
                  </div>
                  <div className="flex items-center gap-3 shrink-0">
                    <CostDisplay microdollars={run.cost_microdollars} className="text-xs text-muted-foreground" />
                    <StatusBadge status={run.status} />
                    <span className="text-xs text-muted-foreground">
                      {formatDistanceToNow(new Date(run.accepted_at), { addSuffix: true })}
                    </span>
                  </div>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* Quick actions */}
      <div className="flex gap-3">
        <Link to="/chat">
          <Button>
            <MessageSquare className="h-4 w-4 mr-2" />
            Open Chat
          </Button>
        </Link>
        <Link to="/runs">
          <Button variant="outline">
            <PlayCircle className="h-4 w-4 mr-2" />
            Submit Run
          </Button>
        </Link>
      </div>

      <ConfirmDialog
        open={estopOpen}
        onOpenChange={setEstopOpen}
        title="Emergency Stop"
        description="Cancel all running and pending jobs immediately. This cannot be undone."
        confirmLabel="Stop All"
        destructive
        onConfirm={() => estopMutation.mutate()}
      />
    </div>
  )
}
