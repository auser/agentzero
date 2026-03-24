import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { runsApi } from '@/lib/api/runs'
import { healthApi } from '@/lib/api/health'
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
import { RegressionBanner } from '@/components/shared/RegressionBanner'
import { Button } from '@/components/ui/button'
import { Link } from '@tanstack/react-router'
import { MessageSquare, PlayCircle, AlertTriangle, GitBranch, WifiOff } from 'lucide-react'
import { useState } from 'react'
import { WorkflowTopology } from '@/components/dashboard/WorkflowTopology'
import { SystemHealthBar } from '@/components/dashboard/SystemHealthBar'
import { AgentStatusPanel } from '@/components/dashboard/AgentStatusPanel'
import { ActiveRunsTimeline } from '@/components/dashboard/ActiveRunsTimeline'
import { ScheduleOverview } from '@/components/dashboard/ScheduleOverview'
import { ChannelStatus } from '@/components/dashboard/ChannelStatus'

export const Route = createFileRoute('/dashboard/')({
  component: DashboardPage,
})

function DashboardPage() {
  const [estopOpen, setEstopOpen] = useState(false)
  const queryClient = useQueryClient()

  const { data: health, isError: gatewayDown } = useQuery({
    queryKey: ['health'],
    queryFn: () => healthApi.get(),
    refetchInterval: 5_000,
    retry: false,
  })

  const estopMutation = useMutation({
    mutationFn: runsApi.estop,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['runs'] }),
  })

  if (gatewayDown && !health) {
    return (
      <div className="flex flex-col items-center justify-center h-[60vh] text-center">
        <WifiOff className="h-16 w-16 text-muted-foreground/20 mb-4" />
        <h2 className="text-lg font-semibold mb-1">Gateway Offline</h2>
        <p className="text-sm text-muted-foreground mb-4 max-w-md">
          Cannot connect to the AgentZero gateway. Make sure it&apos;s running and accessible.
        </p>
        <code className="text-xs text-muted-foreground/60 bg-muted/30 px-3 py-1.5 rounded font-mono">
          agentzero gateway --port 42617
        </code>
      </div>
    )
  }

  return (
    <div className="space-y-5 max-w-7xl">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-lg font-semibold tracking-tight">Dashboard</h1>
          <p className="text-xs text-muted-foreground mt-0.5">
            Monitor your agent workflows and system health
          </p>
        </div>
        <div className="flex gap-2">
          <Link to="/chat">
            <Button variant="outline" size="sm" className="h-8 text-xs">
              <MessageSquare className="h-3.5 w-3.5 mr-1.5" />
              Chat
            </Button>
          </Link>
          <Link to="/runs">
            <Button variant="outline" size="sm" className="h-8 text-xs">
              <PlayCircle className="h-3.5 w-3.5 mr-1.5" />
              Run
            </Button>
          </Link>
          <Button variant="outline" size="sm" className="h-8 text-xs" disabled>
            <GitBranch className="h-3.5 w-3.5 mr-1.5" />
            Workflow
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="h-8 text-xs border-red-800/40 text-red-400 hover:bg-red-950/30 hover:border-red-700/50"
            onClick={() => setEstopOpen(true)}
          >
            <AlertTriangle className="h-3.5 w-3.5 mr-1.5" />
            E-Stop
          </Button>
        </div>
      </div>

      <RegressionBanner />

      {/* Metrics row */}
      <SystemHealthBar />

      {/* Workflow snapshot (read-only preview) */}
      <div className="rounded-lg border border-border/50 bg-card overflow-hidden">
        <div className="flex items-center justify-between px-4 py-2 border-b border-border/50">
          <span className="text-sm font-medium text-muted-foreground">Workflow Topology</span>
          <Link to="/workflows" className="text-xs text-primary hover:underline">
            Open Editor →
          </Link>
        </div>
        <div style={{ height: 300 }}>
          <WorkflowTopology readOnly />
        </div>
      </div>

      {/* Middle row: agents */}
      <AgentStatusPanel />

      {/* Bottom row: runs (wide) + schedules & channels (narrow) */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <div className="lg:col-span-2">
          <ActiveRunsTimeline />
        </div>
        <div className="space-y-4">
          <ScheduleOverview />
          <ChannelStatus />
        </div>
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
