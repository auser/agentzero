/**
 * Interactive workflow topology visualization powered by workflow-graph WASM.
 */
import { useRef, useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import {
  WorkflowGraphComponent,
  type WorkflowGraphHandle,
  darkTheme,
} from '@auser/workflow-graph-react'
import { topologyApi } from '@/lib/api/topology'
import { topologyToWorkflow } from '@/components/workflows/WorkflowCanvas'
import { renderNode } from '@/components/workflows/NodeRenderer'
import { Button } from '@/components/ui/button'
import { Maximize2, RotateCcw, Network } from 'lucide-react'

export function WorkflowTopology() {
  const graphRef = useRef<WorkflowGraphHandle>(null)
  const navigate = useNavigate()

  const { data: topology } = useQuery({
    queryKey: ['topology'],
    queryFn: () => topologyApi.get(),
    refetchInterval: 3_000,
  })

  const nodes = topology?.nodes ?? []
  const edges = topology?.edges ?? []
  const workflow = topologyToWorkflow(nodes, edges)

  const handleNodeClick = useCallback(
    (_jobId: string) => {
      void navigate({ to: '/agents' })
    },
    [navigate],
  )

  if (nodes.length === 0) {
    return (
      <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm">
        <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
          <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
            <Network className="h-3.5 w-3.5" />
            Workflow Topology
          </h3>
        </div>
        <div className="flex flex-col items-center justify-center py-16 text-muted-foreground">
          <Network className="h-10 w-10 mb-3 opacity-20" />
          <p className="text-sm">No agents configured</p>
          <p className="text-xs text-muted-foreground/60 mt-1">
            Add agents to visualize your workflow topology
          </p>
        </div>
      </div>
    )
  }

  return (
    <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm overflow-hidden">
      <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
          <Network className="h-3.5 w-3.5" />
          Workflow Topology
          <span className="text-[10px] text-muted-foreground/60 normal-case tracking-normal font-normal">
            {nodes.length} agent{nodes.length !== 1 ? 's' : ''} · {edges.length} connection{edges.length !== 1 ? 's' : ''}
          </span>
        </h3>
        <div className="flex gap-0.5">
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground hover:text-foreground"
            onClick={() => graphRef.current?.resetLayout()}
            title="Reset layout"
          >
            <RotateCcw className="h-3.5 w-3.5" />
          </Button>
          <Button
            variant="ghost"
            size="icon"
            className="h-7 w-7 text-muted-foreground hover:text-foreground"
            onClick={() => graphRef.current?.zoomToFit()}
            title="Zoom to fit"
          >
            <Maximize2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
      <div style={{ height: 320 }} className="bg-[#0d1117]">
        <WorkflowGraphComponent
          ref={graphRef}
          workflow={workflow}
          theme={{
            ...darkTheme,
            layout: {
              node_width: 180,
              node_height: 52,
              node_radius: 6,
              h_gap: 60,
              v_gap: 30,
              header_height: 0,
              padding: 24,
              junction_dot_radius: 3.5,
              status_icon_radius: 6,
              status_icon_margin: 8,
            },
          }}
          autoResize
          onNodeClick={handleNodeClick}
          onRenderNode={renderNode}
          onError={(err) => console.error('Workflow graph error:', err)}
        />
      </div>
    </div>
  )
}
