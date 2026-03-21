/**
 * Interactive workflow topology visualization powered by workflow-graph WASM.
 * Supports drag-drop from the DraggablePalette to add nodes.
 * Shows KeySelector when connecting ports with different types.
 */
import { useRef, useCallback, useState, type DragEvent } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import {
  WorkflowGraphComponent,
  type WorkflowGraphHandle,
  type Job,
  darkTheme,
} from '@auser/workflow-graph-react'
import { topologyApi } from '@/lib/api/topology'
import { topologyToWorkflow } from '@/components/workflows/WorkflowCanvas'
import { Button } from '@/components/ui/button'
import { Maximize2, RotateCcw, Network } from 'lucide-react'
import type { DragNodeData } from '@/components/workflows/DraggablePalette'
import { KeySelector, type PendingConnection } from '@/components/workflows/KeySelector'
import { CommandPalette, useCommandPalette } from '@/components/workflows/CommandPalette'
import { useWorkflowStore } from '@/store/workflowStore'

interface WorkflowTopologyProps {
  /** When true, fills parent height instead of using fixed 320px */
  fullHeight?: boolean
}

export function WorkflowTopology({ fullHeight = false }: WorkflowTopologyProps) {
  const graphRef = useRef<WorkflowGraphHandle>(null)
  const [dragOver, setDragOver] = useState(false)
  const [pendingConnection, setPendingConnection] = useState<PendingConnection | null>(null)
  const cmdK = useCommandPalette()

  const navigate = useNavigate()
  const { addedNodes, addNode: storeAddNode, addEdge: storeAddEdge, clear: storeClear } = useWorkflowStore()

  const { data: topology } = useQuery({
    queryKey: ['topology'],
    queryFn: () => topologyApi.get(),
    refetchInterval: 3_000,
  })

  const nodes = topology?.nodes ?? []
  const edges = topology?.edges ?? []
  const workflow = topologyToWorkflow(nodes, edges)

  // Merge topology nodes with manually added nodes
  const mergedWorkflow = {
    ...workflow,
    jobs: [...workflow.jobs, ...addedNodes],
  }

  // Look up port type for a given node+port
  const getPortType = useCallback(
    (nodeId: string, portId: string): string => {
      const job = mergedWorkflow.jobs.find((j) => j.id === nodeId)
      if (!job?.ports) return ''
      const port = job.ports.find((p) => p.id === portId)
      return port?.port_type ?? ''
    },
    [mergedWorkflow.jobs],
  )

  const handleNodeClick = useCallback(
    (_jobId: string) => {
      // no-op: selection handled internally by the graph
    },
    [],
  )

  const handleConnect = useCallback(
    (fromNodeId: string, fromPortId: string, toNodeId: string, toPortId: string) => {
      console.log('onConnect fired:', { fromNodeId, fromPortId, toNodeId, toPortId })
      const fromType = getPortType(fromNodeId, fromPortId)
      const toType = getPortType(toNodeId, toPortId)
      console.log('Port types:', { fromType, toType, fromNodeId, fromPortId, toNodeId, toPortId })

      // Show key selector for any cross-type connection
      const needsTransform =
        fromType !== toType && fromType !== '' && toType !== ''

      if (needsTransform) {
        setPendingConnection({
          fromNodeId,
          fromPortId,
          fromPortType: fromType,
          toNodeId,
          toPortId,
          toPortType: toType,
        })
      } else if (fromType === '' || toType === '') {
        // Unknown port types — show selector as a fallback
        setPendingConnection({
          fromNodeId,
          fromPortId,
          fromPortType: fromType || 'unknown',
          toNodeId,
          toPortId,
          toPortType: toType || 'unknown',
        })
      } else {
        // Same type — direct connection
        graphRef.current?.addEdge(fromNodeId, toNodeId, fromPortId, toPortId)
        storeAddEdge({ fromNodeId, fromPortId, toNodeId, toPortId })
      }
    },
    [getPortType, storeAddEdge],
  )

  const handleConnectionConfirm = useCallback(
    (conn: PendingConnection, keyPath: string | null) => {
      const metadata = keyPath ? { transform: `$.${keyPath}` } : undefined
      graphRef.current?.addEdge(
        conn.fromNodeId,
        conn.toNodeId,
        conn.fromPortId,
        conn.toPortId,
        metadata,
      ).catch(() => {})
      storeAddEdge({
        fromNodeId: conn.fromNodeId,
        fromPortId: conn.fromPortId,
        toNodeId: conn.toNodeId,
        toPortId: conn.toPortId,
        metadata,
      })
      setPendingConnection(null)
    },
    [],
  )

  const handleConnectionCancel = useCallback(() => {
    setPendingConnection(null)
  }, [])

  // Handle Cmd+K node selection
  const handleCmdKSelect = useCallback(
    (data: DragNodeData) => {
      const newNode: Job = {
        id: data.id,
        name: data.name,
        status: 'queued',
        command: '',
        depends_on: [],
        metadata: data.metadata,
        ports: data.ports,
      }
      storeAddNode(newNode)
      graphRef.current?.addNode(newNode).catch(() => {})
    },
    [storeAddNode],
  )

  // Handle drop at React level (not WASM) to avoid borrow issues
  const handleDragOver = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
    setDragOver(true)
  }, [])

  const handleDragLeave = useCallback(() => {
    setDragOver(false)
  }, [])

  const handleDrop = useCallback(
    (e: DragEvent<HTMLDivElement>) => {
      e.preventDefault()
      setDragOver(false)
      const data = e.dataTransfer.getData('application/workflow-node')
      if (!data) return
      try {
        const nodeData: DragNodeData = JSON.parse(data)
        const newNode: Job = {
          id: nodeData.id,
          name: nodeData.name,
          status: 'queued',
          command: '',
          depends_on: [],
          metadata: nodeData.metadata,
          ports: nodeData.ports,
        }

        // Get drop position relative to canvas
        const canvas = (e.currentTarget as HTMLElement).querySelector('canvas')
        let dropX: number | undefined
        let dropY: number | undefined
        if (canvas) {
          const rect = canvas.getBoundingClientRect()
          dropX = e.clientX - rect.left
          dropY = e.clientY - rect.top
        }

        storeAddNode(newNode)
        graphRef.current?.addNode(newNode, dropX, dropY).catch(() => {
          // Node persisted in store, will appear on next render
        })
      } catch (err) {
        console.error('Failed to add dropped node:', err)
      }
    },
    [],
  )

  const isEmpty = nodes.length === 0 && addedNodes.length === 0

  if (isEmpty) {
    return (
      <div
        className={`rounded-lg border bg-card/80 backdrop-blur-sm transition-colors relative ${
          dragOver ? 'border-primary/50 bg-primary/5' : 'border-border/50'
        }`}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
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
            Drag agents, tools, or channels from the palette to build your workflow
          </p>
        </div>
      </div>
    )
  }

  return (
    <div
      className={`rounded-lg border bg-card/80 backdrop-blur-sm overflow-hidden transition-colors relative ${fullHeight ? 'h-full flex flex-col' : ''} ${
        dragOver ? 'border-primary/50' : 'border-border/50'
      }`}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
          <Network className="h-3.5 w-3.5" />
          Workflow Topology
          <span className="text-[10px] text-muted-foreground/60 normal-case tracking-normal font-normal">
            {mergedWorkflow.jobs.length} node{mergedWorkflow.jobs.length !== 1 ? 's' : ''} · {edges.length} connection{edges.length !== 1 ? 's' : ''}
          </span>
        </h3>
        <div className="flex items-center gap-1">
          {addedNodes.length > 0 && (
            <button
              onClick={storeClear}
              className="flex items-center gap-1 h-7 px-2 text-[10px] text-muted-foreground/40 hover:text-red-400 bg-muted/20 hover:bg-red-500/10 rounded border border-border/30 transition-colors"
              title="Clear added nodes"
            >
              Clear
            </button>
          )}
          <button
            onClick={() => cmdK.setOpen(true)}
            className="flex items-center gap-1.5 h-7 px-2 text-[10px] text-muted-foreground/50 hover:text-muted-foreground bg-muted/20 hover:bg-muted/40 rounded border border-border/30 transition-colors"
          >
            <span>Add node</span>
            <kbd className="text-[9px] bg-muted/30 px-1 py-0.5 rounded">⌘K</kbd>
          </button>
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
      <WorkflowGraphComponent
        ref={graphRef}
        workflow={mergedWorkflow}
        className={`w-full bg-background ${fullHeight ? 'flex-1' : ''}`}
        style={fullHeight ? undefined : { height: 320 }}
        theme={{
          ...darkTheme,
          layout: {
            node_width: 220,
            node_height: 100,
            node_radius: 8,
            h_gap: 80,
            v_gap: 40,
            header_height: 0,
            padding: 30,
            junction_dot_radius: 3.5,
            status_icon_radius: 6,
            status_icon_margin: 8,
          },
        }}
        autoResize
        onNodeClick={handleNodeClick}
        onConnect={handleConnect}
        onError={(err) => console.error('Workflow graph error:', err)}
      />

      {/* Key selector overlay */}
      {pendingConnection && (
        <KeySelector
          connection={pendingConnection}
          onConfirm={handleConnectionConfirm}
          onCancel={handleConnectionCancel}
        />
      )}

      {/* Cmd+K command palette */}
      <CommandPalette
        open={cmdK.open}
        onClose={cmdK.onClose}
        onSelect={handleCmdKSelect}
        onCreateAgent={() => void navigate({ to: '/agents' })}
      />
    </div>
  )
}
