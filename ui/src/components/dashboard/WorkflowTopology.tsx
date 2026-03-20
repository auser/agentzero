/**
 * Interactive workflow topology visualization powered by workflow-graph WASM.
 * Replaces the old canvas-based TopologyGraph with type-aware node rendering.
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
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Maximize2, RotateCcw } from 'lucide-react'

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
    (jobId: string) => {
      void navigate({ to: '/agents' })
      // Could navigate to specific agent: /agents?selected=jobId
      void jobId
    },
    [navigate],
  )

  if (nodes.length === 0) {
    return (
      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Agent Topology</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="flex items-center justify-center py-12 text-sm text-muted-foreground">
            No agents configured. Add agents to see the workflow topology.
          </div>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm">Agent Topology</CardTitle>
          <div className="flex gap-1">
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              onClick={() => graphRef.current?.resetLayout()}
              title="Reset layout"
            >
              <RotateCcw className="h-3.5 w-3.5" />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              className="h-7 w-7"
              onClick={() => graphRef.current?.zoomToFit()}
              title="Zoom to fit"
            >
              <Maximize2 className="h-3.5 w-3.5" />
            </Button>
          </div>
        </div>
      </CardHeader>
      <CardContent>
        <div style={{ height: 280, background: '#0d1117', borderRadius: 8 }}>
          <WorkflowGraphComponent
            ref={graphRef}
            workflow={workflow}
            theme={{
              ...darkTheme,
              layout: {
                node_width: 180,
                node_height: 52,
                h_gap: 60,
                v_gap: 30,
                padding: 20,
              },
            }}
            autoResize
            onNodeClick={handleNodeClick}
            onRenderNode={renderNode}
            onError={(err) => console.error('Workflow graph error:', err)}
          />
        </div>
      </CardContent>
    </Card>
  )
}
