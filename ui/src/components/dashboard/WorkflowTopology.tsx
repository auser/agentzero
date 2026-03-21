/**
 * Interactive workflow topology visualization powered by workflow-graph WASM.
 * Supports drag-drop from the DraggablePalette to add nodes.
 * Shows KeySelector when connecting ports with different types.
 * Persists full graph state via workflow-graph's getState/loadState API.
 */
import { useRef, useCallback, useState, useEffect, type DragEvent } from 'react'
import { useQuery } from '@tanstack/react-query'
// useNavigate removed — agent creation now uses inline dialog
import {
  WorkflowGraphComponent,
  type WorkflowGraphHandle,
  type Job,
  darkTheme,
} from '@auser/workflow-graph-react'
import { topologyApi } from '@/lib/api/topology'
import { topologyToWorkflow } from '@/components/workflows/WorkflowCanvas'
import { Button } from '@/components/ui/button'
import { Maximize2, RotateCcw, Network, Settings } from 'lucide-react'
import type { DragNodeData } from '@/components/workflows/DraggablePalette'
import { KeySelector, type PendingConnection } from '@/components/workflows/KeySelector'
import { CommandPalette, useCommandPalette } from '@/components/workflows/CommandPalette'
import { CreateAgentDialog } from '@/components/workflows/CreateAgentDialog'
import { ConfigPanel } from '@/components/workflows/ConfigPanel'
import { useWorkflowStore } from '@/store/workflowStore'

interface WorkflowTopologyProps {
  fullHeight?: boolean
}

const THEME = {
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
}

export function WorkflowTopology({ fullHeight = false }: WorkflowTopologyProps) {
  const graphRef = useRef<WorkflowGraphHandle>(null)
  const [dragOver, setDragOver] = useState(false)
  const [pendingConnection, setPendingConnection] = useState<PendingConnection | null>(null)
  const [createAgentOpen, setCreateAgentOpen] = useState(false)
  const [configPanelOpen, setConfigPanelOpen] = useState(false)
  const cmdK = useCommandPalette()

  const { graphState, saveGraphState, clear: storeClear } = useWorkflowStore()

  const { data: topology } = useQuery({
    queryKey: ['topology'],
    queryFn: () => topologyApi.get(),
    refetchInterval: 3_000,
  })

  const nodes = topology?.nodes ?? []
  const edges = topology?.edges ?? []
  const workflow = topologyToWorkflow(nodes, edges)

  // Track saved positions in a ref so we can re-apply after topology resets
  const savedPositionsRef = useRef<Record<string, [number, number]>>(
    (graphState && typeof graphState === 'object' && graphState.positions) ? graphState.positions : {},
  )

  // Positions are now handled by the initialPositions prop on WorkflowGraphComponent
  // No manual setNodePositions calls needed

  // Save graph state helper — called on user interactions, not on a timer
  const saveCurrentState = useCallback(async () => {
    try {
      const state = await graphRef.current?.getState()
      if (state && state.positions) {
        savedPositionsRef.current = state.positions
        saveGraphState(state)
        console.log('[workflow] state saved', Object.keys(state.positions).length, 'positions')
      } else {
        console.log('[workflow] getState returned', state ? 'no positions' : 'null')
      }
    } catch (e) {
      console.log('[workflow] getState error:', e)
    }
  }, [saveGraphState])

  // Save on Delete/Backspace
  useEffect(() => {
    const handler = async (e: KeyboardEvent) => {
      if (e.key === 'Delete' || e.key === 'Backspace') {
        await new Promise((r) => setTimeout(r, 100))
        await saveCurrentState()
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [saveCurrentState])

  // Look up port type for a given node+port from the current workflow
  const getPortType = useCallback(
    (nodeId: string, portId: string): string => {
      const job = workflow.jobs.find((j) => j.id === nodeId)
      if (!job?.ports) return ''
      const port = job.ports.find((p) => p.id === portId)
      return port?.port_type ?? ''
    },
    [workflow.jobs],
  )

  const handleNodeClick = useCallback(() => {}, [])

  const handleNodeDragEnd = useCallback(
    (_jobId: string, x: number, y: number) => {
      // Save position directly — don't rely on getState which may fail
      savedPositionsRef.current = { ...savedPositionsRef.current, [_jobId]: [x, y] }
      // Persist to localStorage via Zustand
      saveGraphState({
        version: 1,
        workflow: { id: 'saved', name: 'saved', trigger: '', jobs: [] },
        positions: savedPositionsRef.current,
        edges: [],
        zoom: 1,
        pan_x: 0,
        pan_y: 0,
      })
      console.log('[workflow] position saved for', _jobId, 'at', x, y)
    },
    [saveGraphState],
  )

  const handleConnect = useCallback(
    (fromNodeId: string, fromPortId: string, toNodeId: string, toPortId: string) => {
      const fromType = getPortType(fromNodeId, fromPortId)
      const toType = getPortType(toNodeId, toPortId)

      const needsTransform = fromType !== toType && fromType !== '' && toType !== ''

      if (needsTransform) {
        setPendingConnection({
          fromNodeId, fromPortId, fromPortType: fromType,
          toNodeId, toPortId, toPortType: toType,
        })
      } else if (fromType === '' || toType === '') {
        setPendingConnection({
          fromNodeId, fromPortId, fromPortType: fromType || 'unknown',
          toNodeId, toPortId, toPortType: toType || 'unknown',
        })
      } else {
        graphRef.current?.addEdge(fromNodeId, toNodeId, fromPortId, toPortId)
          .then(() => saveCurrentState())
          .catch(() => {})
      }
    },
    [getPortType],
  )

  const handleConnectionConfirm = useCallback(
    (conn: PendingConnection, keyPath: string | null) => {
      const metadata = keyPath ? { transform: `$.${keyPath}` } : undefined
      graphRef.current?.addEdge(
        conn.fromNodeId, conn.toNodeId, conn.fromPortId, conn.toPortId, metadata,
      ).catch(() => {})
      setPendingConnection(null)
    },
    [],
  )

  const handleConnectionCancel = useCallback(() => {
    setPendingConnection(null)
  }, [])

  const handleCmdKSelect = useCallback(
    (data: DragNodeData) => {
      const newNode: Job = {
        id: data.id, name: data.name, status: 'queued', command: '',
        depends_on: [], metadata: data.metadata, ports: data.ports,
      }
      graphRef.current?.addNode(newNode).catch(() => {})
    },
    [],
  )

  const handleDragOver = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
    setDragOver(true)
  }, [])

  const handleDragLeave = useCallback(() => setDragOver(false), [])

  const handleDrop = useCallback(
    (e: DragEvent<HTMLDivElement>) => {
      e.preventDefault()
      setDragOver(false)
      const data = e.dataTransfer.getData('application/workflow-node')
      if (!data) return
      try {
        const nodeData: DragNodeData = JSON.parse(data)
        const newNode: Job = {
          id: nodeData.id, name: nodeData.name, status: 'queued', command: '',
          depends_on: [], metadata: nodeData.metadata, ports: nodeData.ports,
        }
        const canvas = (e.currentTarget as HTMLElement).querySelector('canvas')
        let dropX: number | undefined
        let dropY: number | undefined
        if (canvas) {
          const rect = canvas.getBoundingClientRect()
          dropX = e.clientX - rect.left
          dropY = e.clientY - rect.top
        }
        graphRef.current?.addNode(newNode, dropX, dropY)
          .then(() => saveCurrentState())
          .catch(() => {})
      } catch (err) {
        console.error('Failed to add dropped node:', err)
      }
    },
    [],
  )

  const handleClear = useCallback(() => {
    storeClear()
    window.location.reload()
  }, [storeClear])

  const savedJobCount = (graphState && typeof graphState === 'object' && graphState.workflow?.jobs?.length) ? graphState.workflow.jobs.length : 0
  const nodeCount = workflow.jobs.length + savedJobCount
  const isEmpty = nodes.length === 0 && savedJobCount === 0

  if (isEmpty) {
    return (
      <div
        className={`rounded-lg border bg-card/80 backdrop-blur-sm transition-colors relative ${fullHeight ? 'h-full flex flex-col' : ''} ${
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
            {nodeCount} node{nodeCount !== 1 ? 's' : ''}
          </span>
        </h3>
        <div className="flex items-center gap-1">
          {graphState && (
            <button
              onClick={handleClear}
              className="flex items-center gap-1 h-7 px-2 text-[10px] text-muted-foreground/40 hover:text-destructive bg-muted/20 hover:bg-destructive/10 rounded border border-border/30 transition-colors"
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
          <button
            onClick={() => setConfigPanelOpen((v) => !v)}
            className={`flex items-center gap-1 h-7 px-2 text-[10px] rounded border border-border/30 transition-colors ${
              configPanelOpen
                ? 'text-primary bg-primary/10 border-primary/30'
                : 'text-muted-foreground/50 hover:text-muted-foreground bg-muted/20 hover:bg-muted/40'
            }`}
            title="Quick config"
          >
            <Settings className="h-3 w-3" />
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
        workflow={workflow}
        className={`w-full bg-background ${fullHeight ? 'flex-1' : ''}`}
        style={fullHeight ? undefined : { height: 320 }}
        theme={THEME}
        initialPositions={savedPositionsRef.current}
        autoResize
        onNodeClick={handleNodeClick}
        onNodeDragEnd={handleNodeDragEnd}
        onConnect={handleConnect}
        onError={(err) => console.error('Workflow graph error:', err)}
        loadingSkeleton={
          <div className="flex items-center justify-center h-full text-muted-foreground/30 text-sm">
            Loading graph...
          </div>
        }
      />


      {pendingConnection && (
        <KeySelector
          connection={pendingConnection}
          onConfirm={handleConnectionConfirm}
          onCancel={handleConnectionCancel}
        />
      )}

      {/* Config panel (anchored to toolbar) */}
      <ConfigPanel open={configPanelOpen} onClose={() => setConfigPanelOpen(false)} />

      {/* Cmd+K command palette */}
      <CommandPalette
        open={cmdK.open}
        onClose={cmdK.onClose}
        onSelect={handleCmdKSelect}
        onCreateAgent={() => setCreateAgentOpen(true)}
      />

      {/* Create agent dialog */}
      <CreateAgentDialog
        open={createAgentOpen}
        onClose={() => setCreateAgentOpen(false)}
      />
    </div>
  )
}

/** Detects when the graph finishes initializing by polling for the canvas. */
