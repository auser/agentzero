import { createFileRoute } from '@tanstack/react-router'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { runsApi } from '@/lib/api/runs'
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
import { RegressionBanner } from '@/components/shared/RegressionBanner'
import { Button } from '@/components/ui/button'
import { Link } from '@tanstack/react-router'
import { MessageSquare, PlayCircle, AlertTriangle, GitBranch } from 'lucide-react'
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

  const estopMutation = useMutation({
    mutationFn: runsApi.estop,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ['runs'] }),
  })

  return (
    <div className="space-y-4 max-w-6xl">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Dashboard</h1>
        <div className="flex gap-2">
          <Link to="/chat">
            <Button variant="outline" size="sm">
              <MessageSquare className="h-4 w-4 mr-2" />
              New Chat
            </Button>
          </Link>
          <Link to="/runs">
            <Button variant="outline" size="sm">
              <PlayCircle className="h-4 w-4 mr-2" />
              Submit Run
            </Button>
          </Link>
          <Button variant="outline" size="sm" disabled>
            <GitBranch className="h-4 w-4 mr-2" />
            Create Workflow
          </Button>
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

      {/* System health bar */}
      <SystemHealthBar />

      {/* Workflow topology (hero) */}
      <WorkflowTopology />

      {/* Two-column layout: left = agents + runs, right = schedules + channels */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <div className="space-y-4">
          <AgentStatusPanel />
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
