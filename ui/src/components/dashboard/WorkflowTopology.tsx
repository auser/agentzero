/**
 * Workflow topology visualization powered by ReactFlow.
 * Supports drag-drop from palette, port-to-port connections,
 * Cmd+K command palette, and localStorage persistence.
 */
import { useCallback, useState, useRef, useEffect, type DragEvent } from 'react'
import { useQuery } from '@tanstack/react-query'
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  useNodesState,
  useEdgesState,
  useReactFlow,
  addEdge,
  ReactFlowProvider,
  type Connection,
  type Node,
  type Edge,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'

import { topologyApi } from '@/lib/api/topology'
import { workflowsApi } from '@/lib/api/workflows'
import { topologyToReactFlow } from '@/components/workflows/WorkflowCanvas'
import { AgentNode } from '@/components/workflows/AgentNode'
import { ProviderNode } from '@/components/workflows/ProviderNode'
import type { DragNodeData } from '@/components/workflows/DraggablePalette'
// KeySelector removed — isValidConnection enforces port type matching
import { CommandPalette, useCommandPalette } from '@/components/workflows/CommandPalette'
import { CreateAgentDialog } from '@/components/workflows/CreateAgentDialog'
import { ConfigPanel } from '@/components/workflows/ConfigPanel'
import { getDefinition, portsForType } from '@/lib/node-definitions'
import { portTypeColor } from '@/lib/workflow-types'

interface WorkflowTopologyProps {
  fullHeight?: boolean
}

const DEFAULT_WORKFLOW_NAME = 'default'

const nodeTypes = {
  agent: AgentNode,
  tool: AgentNode,
  channel: AgentNode,
  schedule: AgentNode,
  gate: AgentNode,
  subagent: AgentNode,
  role: AgentNode,
  provider: ProviderNode,
}

function WorkflowTopologyInner({ fullHeight = false }: WorkflowTopologyProps) {
  const reactFlowInstance = useReactFlow()
  const [nodes, setNodes, onNodesChange] = useNodesState([])
  const [edges, setEdges, onEdgesChange] = useEdgesState([])
  const [createAgentOpen, setCreateAgentOpen] = useState(false)
  const [configPanelOpen, setConfigPanelOpen] = useState(false)
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null)
  const cmdK = useCommandPalette()
  const saveTimerRef = useRef<ReturnType<typeof setTimeout>>()
  const initializedRef = useRef(false)
  const workflowIdRef = useRef<string | null>(null)
  const lastKnownUpdatedAtRef = useRef<number>(0)
  const isDirtyRef = useRef(false)
  const isSyncingRef = useRef(false)

  // Topology API polling
  const { data: topology } = useQuery({
    queryKey: ['topology'],
    queryFn: () => topologyApi.get(),
    refetchInterval: 3_000,
  })

  // Initialize: load from /v1/workflows API, or create from topology
  useEffect(() => {
    if (initializedRef.current) return

    const init = async () => {
      try {
        // Try loading existing workflows from the server
        const list = await workflowsApi.list('layout')
        const existing = list.data.find((w) => w.name === DEFAULT_WORKFLOW_NAME) ?? list.data[0]

        if (existing?.layout?.nodes && (existing.layout.nodes as unknown[]).length > 0) {
          setNodes(existing.layout.nodes as Node[])
          setEdges((existing.layout.edges ?? []) as Edge[])
          workflowIdRef.current = existing.workflow_id
          lastKnownUpdatedAtRef.current = existing.updated_at
          initializedRef.current = true
          return
        }

        // No saved workflow — create from topology if available
        if (topology?.nodes && topology.nodes.length > 0) {
          const { nodes: rfNodes, edges: rfEdges } = topologyToReactFlow(
            topology.nodes, topology.edges ?? [],
          )
          setNodes(rfNodes)
          setEdges(rfEdges)

          // Create a workflow on the server
          try {
            const created = await workflowsApi.create({
              name: DEFAULT_WORKFLOW_NAME,
              description: 'Default workflow layout',
              layout: { nodes: rfNodes, edges: rfEdges },
            })
            workflowIdRef.current = created.workflow_id
            lastKnownUpdatedAtRef.current = created.updated_at
          } catch {
            // Server might not be running — fall back to local-only
          }
          initializedRef.current = true
        }
      } catch {
        // API not available — try localStorage fallback
        try {
          const raw = localStorage.getItem('agentzero-workflow-reactflow')
          if (raw) {
            const saved = JSON.parse(raw)
            if (saved?.nodes?.length > 0) {
              setNodes(saved.nodes)
              setEdges(saved.edges ?? [])
              initializedRef.current = true
              return
            }
          }
        } catch { /* ignore */ }

        // Last resort: create from topology
        if (topology?.nodes && topology.nodes.length > 0) {
          const { nodes: rfNodes, edges: rfEdges } = topologyToReactFlow(
            topology.nodes, topology.edges ?? [],
          )
          setNodes(rfNodes)
          setEdges(rfEdges)
          initializedRef.current = true
        }
      }
    }

    init()
  }, [topology, setNodes, setEdges])

  // Update node statuses from topology poll (without resetting positions)
  useEffect(() => {
    if (!initializedRef.current || !topology?.nodes) return
    setNodes((prev) => {
      const statusMap = new Map(topology.nodes.map((n) => [n.agent_id, n.status]))
      return prev.map((node) => {
        const newStatus = statusMap.get(node.id)
        if (newStatus && (node.data as Record<string, unknown>).status !== newStatus) {
          return { ...node, data: { ...node.data, status: newStatus } }
        }
        return node
      })
    })
  }, [topology, setNodes])

  // Debounced persistence — save to API, fallback to localStorage
  const persistState = useCallback(() => {
    isDirtyRef.current = true
    clearTimeout(saveTimerRef.current)
    saveTimerRef.current = setTimeout(async () => {
      const currentNodes = reactFlowInstance.getNodes()
      const currentEdges = reactFlowInstance.getEdges()
      const layout = { nodes: currentNodes, edges: currentEdges }

      // Save to API if we have a workflow ID
      if (workflowIdRef.current) {
        try {
          const updated = await workflowsApi.update(workflowIdRef.current, { layout })
          lastKnownUpdatedAtRef.current = updated.updated_at
          isDirtyRef.current = false
        } catch {
          // API failed — save to localStorage as fallback
          try {
            localStorage.setItem('agentzero-workflow-reactflow', JSON.stringify(layout))
          } catch { /* full */ }
        }
      } else {
        // No workflow ID — save to localStorage
        try {
          localStorage.setItem('agentzero-workflow-reactflow', JSON.stringify(layout))
        } catch { /* full */ }
      }
    }, 800)
  }, [reactFlowInstance])

  // Save on node/edge changes
  const handleNodesChange: typeof onNodesChange = useCallback((changes) => {
    onNodesChange(changes)
    persistState()
  }, [onNodesChange, persistState])

  const handleEdgesChange: typeof onEdgesChange = useCallback((changes) => {
    onEdgesChange(changes)
    persistState()
  }, [onEdgesChange, persistState])

  // Port-to-port connection with type checking
  // Connection handler — only called for valid (type-matched) connections
  // since isValidConnection blocks mismatched types
  const handleConnect = useCallback((connection: Connection) => {
    const sourceNode = reactFlowInstance.getNode(connection.source)
    if (!sourceNode) return

    const sourceData = sourceNode.data as Record<string, unknown>
    const sourceDef = getDefinition((sourceData.nodeType as string) ?? '')
    const sourcePort = sourceDef?.outputs?.find((p) => p.id === connection.sourceHandle)
    const color = portTypeColor(sourcePort?.port_type ?? '')

    setEdges((eds) => addEdge({
      ...connection,
      style: { stroke: color, strokeWidth: 2 },
    }, eds))
    persistState()
  }, [reactFlowInstance, setEdges, persistState])

  // (KeySelector removed — isValidConnection handles type enforcement)

  // Drag-drop from palette
  const handleDragOver = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
  }, [])

  const handleDrop = useCallback(
    (e: DragEvent<HTMLDivElement>) => {
      e.preventDefault()
      const data = e.dataTransfer.getData('application/workflow-node')
      if (!data) return
      try {
        const nodeData: DragNodeData = JSON.parse(data)
        const position = reactFlowInstance.screenToFlowPosition({
          x: e.clientX,
          y: e.clientY,
        })
        const ports = portsForType(nodeData.nodeType)
        const newNode: Node = {
          id: nodeData.id,
          type: nodeData.nodeType,
          position,
          data: {
            name: nodeData.name,
            nodeType: nodeData.nodeType,
            status: 'queued',
            metadata: nodeData.metadata ?? {},
          },
        }
        setNodes((nds) => [...nds, newNode])
        persistState()
      } catch (err) {
        console.error('Failed to add dropped node:', err)
      }
    },
    [reactFlowInstance, setNodes, persistState],
  )

  // Cmd+K node addition
  const handleCmdKSelect = useCallback(
    (data: DragNodeData) => {
      const viewport = reactFlowInstance.getViewport()
      const position = reactFlowInstance.screenToFlowPosition({
        x: window.innerWidth / 2,
        y: window.innerHeight / 2,
      })
      const newNode: Node = {
        id: data.id,
        type: data.nodeType,
        position,
        data: {
          name: data.name,
          nodeType: data.nodeType,
          status: 'queued',
          metadata: data.metadata ?? {},
        },
      }
      setNodes((nds) => [...nds, newNode])
      persistState()
    },
    [reactFlowInstance, setNodes, persistState],
  )

  // Clear all
  const handleClear = useCallback(() => {
    setNodes([])
    setEdges([])
    localStorage.removeItem('agentzero-workflow-reactflow')
    if (workflowIdRef.current) {
      workflowsApi.update(workflowIdRef.current, { layout: { nodes: [], edges: [] } }).catch(() => {})
    }
  }, [setNodes, setEdges])

  // Context menu
  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setContextMenu({ x: e.clientX, y: e.clientY })
  }, [])

  // Cross-browser sync: poll for remote changes
  useEffect(() => {
    const intervalId = setInterval(async () => {
      if (!workflowIdRef.current || !initializedRef.current || isDirtyRef.current || isSyncingRef.current) return

      try {
        const list = await workflowsApi.list()
        const remote = list.data.find((w) => w.workflow_id === workflowIdRef.current)
        if (!remote || remote.updated_at <= lastKnownUpdatedAtRef.current) return
        if (isDirtyRef.current) return

        isSyncingRef.current = true
        try {
          const full = await workflowsApi.get(workflowIdRef.current!, 'layout')
          if (!isDirtyRef.current && full.layout?.nodes) {
            setNodes(full.layout.nodes as Node[])
            setEdges((full.layout.edges ?? []) as Edge[])
            lastKnownUpdatedAtRef.current = full.updated_at
          }
        } finally {
          isSyncingRef.current = false
        }
      } catch {
        // Network error — skip this cycle
      }
    }, 5_000)

    return () => clearInterval(intervalId)
  }, [setNodes, setEdges])

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'k') {
        e.preventDefault()
        cmdK.setOpen(true)
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [cmdK])

  return (
    <div
      className={fullHeight ? 'h-full flex flex-col' : ''}
      onContextMenu={handleContextMenu}
    >
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodesChange={handleNodesChange}
        onEdgesChange={handleEdgesChange}
        onConnect={handleConnect}
        onDrop={handleDrop}
        onDragOver={handleDragOver}
        fitView
        deleteKeyCode={['Backspace', 'Delete']}
        edgesReconnectable
        elementsSelectable
        selectNodesOnDrag={false}
        className="bg-background"
        colorMode="dark"
        defaultEdgeOptions={{ style: { strokeWidth: 2 }, selectable: true, focusable: true }}
        edgesFocusable
        isValidConnection={(connection) => {
          // Enforce port type matching
          const src = reactFlowInstance.getNode(connection.source)
          const tgt = reactFlowInstance.getNode(connection.target)
          if (!src || !tgt) return false
          const srcDef = getDefinition((src.data as Record<string, unknown>).nodeType as string)
          const tgtDef = getDefinition((tgt.data as Record<string, unknown>).nodeType as string)
          const srcPort = srcDef?.outputs?.find((p) => p.id === connection.sourceHandle)
          const tgtPort = tgtDef?.inputs?.find((p) => p.id === connection.targetHandle)
          if (!srcPort || !tgtPort) return true // allow unknown ports
          return srcPort.port_type === tgtPort.port_type // only same type
        }}
        style={fullHeight ? { flex: 1 } : { height: 400 }}
      >
        <Background />
        <Controls />
        <MiniMap
          nodeColor={(node) => {
            const def = getDefinition((node.data as Record<string, unknown>).nodeType as string)
            return def?.headerColor ?? '#4b5563'
          }}
          style={{ background: '#1a1a2e' }}
        />
      </ReactFlow>

      {/* Config panel */}
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

      {/* Context menu */}
      {contextMenu && (
        <>
          <div className="fixed inset-0 z-40" onClick={() => setContextMenu(null)} onContextMenu={(e) => { e.preventDefault(); setContextMenu(null) }} />
          <div
            className="fixed z-50 min-w-[160px] rounded-md border border-border bg-zinc-900 p-1 shadow-xl shadow-black/50"
            style={{ left: contextMenu.x, top: contextMenu.y }}
          >
            <button
              className="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-foreground hover:bg-accent transition-colors"
              onClick={() => { cmdK.setOpen(true); setContextMenu(null) }}
            >
              <span className="text-muted-foreground">⌘K</span>
              Add Node
            </button>
            <div className="my-1 h-px bg-border/50" />
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

export function WorkflowTopology(props: WorkflowTopologyProps) {
  return (
    <ReactFlowProvider>
      <WorkflowTopologyInner {...props} />
    </ReactFlowProvider>
  )
}
