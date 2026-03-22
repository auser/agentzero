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
import { ALL_NODE_DEFINITIONS } from '@/lib/node-definitions'
import { Button } from '@/components/ui/button'
import { Maximize2, RotateCcw, Network, Settings } from 'lucide-react'
import type { DragNodeData } from '@/components/workflows/DraggablePalette'
import { KeySelector, type PendingConnection } from '@/components/workflows/KeySelector'
import { CommandPalette, useCommandPalette } from '@/components/workflows/CommandPalette'
import { CreateAgentDialog } from '@/components/workflows/CreateAgentDialog'
import { ConfigPanel } from '@/components/workflows/ConfigPanel'
import { OverlayLayer } from '@/components/workflows/OverlayLayer'
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
  const containerRef = useRef<HTMLDivElement>(null)
  const [dragOver, setDragOver] = useState(false)
  const [pendingConnection, setPendingConnection] = useState<PendingConnection | null>(null)
  const [createAgentOpen, setCreateAgentOpen] = useState(false)
  const [configPanelOpen, setConfigPanelOpen] = useState(false)
  const cmdK = useCommandPalette()

  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; nodeId?: string } | null>(null)
  const [renaming, setRenaming] = useState<{ nodeId: string; x: number; y: number; name: string } | null>(null)
  const lastClickRef = useRef<{ nodeId: string; time: number } | null>(null)
  const lastMouseRef = useRef<{ x: number; y: number }>({ x: 0, y: 0 })

  const { clear: storeClear } = useWorkflowStore()

  const { data: topology } = useQuery({
    queryKey: ['topology'],
    queryFn: () => topologyApi.get(),
    refetchInterval: 3_000,
  })

  const nodes = topology?.nodes ?? []
  const edges = topology?.edges ?? []
  const workflow = topologyToWorkflow(nodes, edges)

  // Persistence is handled by workflow-graph's built-in persist option
  // No manual save/restore needed

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

  const handleNodeClick = useCallback((nodeId: string) => {
    const now = Date.now()
    const last = lastClickRef.current
    if (last && last.nodeId === nodeId && now - last.time < 400) {
      // Double-click — show inline rename at the click position
      lastClickRef.current = null
      const job = workflow.jobs.find(j => j.id === nodeId)
      const { x, y } = lastMouseRef.current
      setRenaming({ nodeId, x, y: y - 15, name: job?.name ?? nodeId })
    } else {
      lastClickRef.current = { nodeId, time: now }
    }
  }, [workflow.jobs])

  // Drag end is auto-persisted by workflow-graph's persist option
  const handleNodeDragEnd = useCallback(
    () => {
      // Auto-persisted by workflow-graph
    },
    [],
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

  // Ctrl+G / Cmd+G to group selected nodes
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'g') {
        e.preventDefault()
        graphRef.current?.groupSelected('Group').catch(() => {})
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [])

  // Right-click context menu
  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setContextMenu({ x: e.clientX, y: e.clientY })
  }, [])

  const closeContextMenu = useCallback(() => setContextMenu(null), [])

  const handleRenameSubmit = useCallback((newName: string) => {
    if (renaming && newName.trim()) {
      graphRef.current?.updateNode(renaming.nodeId, { name: newName.trim() }).catch(() => {})
    }
    setRenaming(null)
  }, [renaming])

  const handleGroup = useCallback(() => {
    graphRef.current?.groupSelected('Group').catch(() => {})
    setContextMenu(null)
  }, [])

  const handleUngroup = useCallback((nodeId: string) => {
    graphRef.current?.ungroupNode(nodeId).catch(() => {})
    setContextMenu(null)
  }, [])

  const handleToggleCollapse = useCallback((nodeId: string) => {
    graphRef.current?.toggleCollapse(nodeId).catch(() => {})
    setContextMenu(null)
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
          .catch(() => {})
      } catch (err) {
        console.error('Failed to add dropped node:', err)
      }
    },
    [],
  )

  const handleClear = useCallback(() => {
    storeClear()
    localStorage.removeItem('agentzero-workflow-graph')
    window.location.reload()
  }, [storeClear])

  const nodeCount = workflow.jobs.length
  const isEmpty = nodes.length === 0

  if (isEmpty) {
    const emptyClasses = fullHeight
      ? `transition-colors relative h-full flex flex-col ${dragOver ? 'bg-primary/5' : ''}`
      : `rounded-lg border bg-card/80 backdrop-blur-sm transition-colors relative ${dragOver ? 'border-primary/50 bg-primary/5' : 'border-border/50'}`

    return (
      <div
        className={emptyClasses}
        onDragOver={handleDragOver}
        onDragLeave={handleDragLeave}
        onDrop={handleDrop}
      >
        {!fullHeight && (
          <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
            <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
              <Network className="h-3.5 w-3.5" />
              Workflow Topology
            </h3>
          </div>
        )}
        <div className="flex flex-col items-center justify-center flex-1 py-16 text-muted-foreground">
          <Network className="h-10 w-10 mb-3 opacity-20" />
          <p className="text-sm">No agents configured</p>
          <p className="text-xs text-muted-foreground/60 mt-1">
            Drag agents, tools, or channels from the palette to build your workflow
          </p>
        </div>
      </div>
    )
  }

  const containerClasses = fullHeight
    ? `overflow-hidden transition-colors relative h-full flex flex-col ${dragOver ? 'border-primary/50' : ''}`
    : `rounded-lg border bg-card/80 backdrop-blur-sm overflow-hidden transition-colors relative ${dragOver ? 'border-primary/50' : 'border-border/50'}`

  return (
    <div
      ref={containerRef}
      className={containerClasses}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
      onContextMenu={handleContextMenu}
      onMouseDown={(e) => { lastMouseRef.current = { x: e.clientX, y: e.clientY } }}
    >
      {/* Only show header bar in dashboard widget mode, not fullHeight (page has its own toolbar) */}
      {!fullHeight && (
        <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
          <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
            <Network className="h-3.5 w-3.5" />
            Workflow Topology
            <span className="text-[10px] text-muted-foreground/60 normal-case tracking-normal font-normal">
              {nodeCount} node{nodeCount !== 1 ? 's' : ''}
            </span>
          </h3>
          <div className="flex items-center gap-1">
            <button
              onClick={handleClear}
              className="flex items-center gap-1 h-7 px-2 text-[10px] text-muted-foreground/40 hover:text-destructive bg-muted/20 hover:bg-destructive/10 rounded border border-border/30 transition-colors"
              title="Clear saved layout"
            >
              Clear
            </button>
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
      )}
      <WorkflowGraphComponent
        ref={graphRef}
        workflow={workflow}
        className={`w-full bg-background ${fullHeight ? 'flex-1' : ''}`}
        style={fullHeight ? undefined : { height: 320 }}
        theme={THEME}
        persist={{ key: 'agentzero-workflow-graph' }}
        nodeTypes={ALL_NODE_DEFINITIONS}
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

      {/* LangFlow-style field overlays positioned over canvas nodes */}
      <OverlayLayer graphRef={graphRef} containerRef={containerRef} />

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

      {/* Inline rename input (double-click node) */}
      {renaming && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => handleRenameSubmit(renaming.name)} />
          <div
            className="fixed z-50 rounded-md border border-border bg-zinc-900 p-1 shadow-xl shadow-black/50"
            style={{ left: renaming.x - 100, top: renaming.y, transform: 'translateY(-50%)' }}
          >
            <input
              autoFocus
              className="w-[200px] bg-transparent text-sm text-foreground outline-none px-2 py-1"
              defaultValue={renaming.name}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleRenameSubmit((e.target as HTMLInputElement).value)
                if (e.key === 'Escape') setRenaming(null)
              }}
              onBlur={(e) => handleRenameSubmit(e.target.value)}
            />
          </div>
        </>
      )}

      {/* Right-click context menu */}
      {contextMenu && (
        <>
          <div className="fixed inset-0 z-40" onClick={closeContextMenu} onContextMenu={(e) => { e.preventDefault(); closeContextMenu() }} />
          <div
            className="fixed z-50 min-w-[160px] rounded-md border border-border bg-zinc-900 p-1 shadow-xl shadow-black/50"
            style={{ left: contextMenu.x, top: contextMenu.y }}
          >
            <button
              className="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-foreground hover:bg-accent transition-colors"
              onClick={handleGroup}
            >
              <span className="text-muted-foreground">⌘G</span>
              Group Selected
            </button>
            <button
              className="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-foreground hover:bg-accent transition-colors"
              onClick={() => {
                // Ungroup whichever node is selected
                const selected = workflow.jobs.find(j => j.children && j.children.length > 0)
                if (selected) handleUngroup(selected.id)
                else setContextMenu(null)
              }}
            >
              Ungroup
            </button>
            <div className="my-1 h-px bg-border/50" />
            <button
              className="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-foreground hover:bg-accent transition-colors"
              onClick={() => {
                const compound = workflow.jobs.find(j => j.children && j.children.length > 0)
                if (compound) handleToggleCollapse(compound.id)
                else setContextMenu(null)
              }}
            >
              Toggle Collapse
            </button>
            <div className="my-1 h-px bg-border/50" />
            <button
              className="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-foreground hover:bg-accent transition-colors"
              onClick={() => { cmdK.setOpen(true); setContextMenu(null) }}
            >
              <span className="text-muted-foreground">⌘K</span>
              Add Node
            </button>
            <button
              className="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-destructive hover:bg-destructive/10 transition-colors"
              onClick={() => { handleClear(); setContextMenu(null) }}
            >
              Clear All
            </button>
          </div>
        </>
      )}
    </div>
  )
}

/** Detects when the graph finishes initializing by polling for the canvas. */
