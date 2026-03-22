/**
 * Workflow topology visualization powered by ReactFlow.
 * Supports drag-drop from palette, port-to-port connections,
 * Cmd+K command palette, and localStorage persistence.
 */
import { useState, useCallback, useEffect } from 'react'
import {
  ReactFlow,
  Background,
  Controls,
  MiniMap,
  useNodesState,
  useEdgesState,
  ReactFlowProvider,
  type Node,
  type Edge,
} from '@xyflow/react'
import '@xyflow/react/dist/style.css'

import { AgentNode } from '@/components/workflows/AgentNode'
import { ProviderNode } from '@/components/workflows/ProviderNode'
import { GroupNode } from '@/components/workflows/GroupNode'
import { LabeledEdge } from '@/components/workflows/LabeledEdge'
import { NodeDetailPanel } from '@/components/workflows/NodeDetailPanel'
import { CommandPalette } from '@/components/workflows/CommandPalette'
import { useCommandPalette } from '@/lib/hooks/useCommandPalette'
import { CreateAgentDialog } from '@/components/workflows/CreateAgentDialog'
import { ConfigPanel } from '@/components/workflows/ConfigPanel'
import { CanvasContextMenu } from '@/components/dashboard/CanvasContextMenu'
import { useWorkflowPersistence } from '@/components/dashboard/useWorkflowPersistence'
import { useNodeActions } from '@/components/dashboard/useNodeActions'
import { useUndoRedo } from '@/components/dashboard/useUndoRedo'
import { RunWorkflowButton } from '@/components/workflows/RunWorkflowButton'
import { getDefinition } from '@/lib/node-definitions'

interface WorkflowTopologyProps {
  fullHeight?: boolean
  readOnly?: boolean
}

const nodeTypes = {
  agent: AgentNode,
  tool: AgentNode,
  channel: AgentNode,
  human_input: AgentNode,
  schedule: AgentNode,
  gate: AgentNode,
  subagent: AgentNode,
  role: AgentNode,
  provider: ProviderNode,
  group: GroupNode,
}

const edgeTypes = {
  default: LabeledEdge,
}

function WorkflowTopologyInner({ fullHeight = false, readOnly = false }: WorkflowTopologyProps) {
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([])
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([])
  const [createAgentOpen, setCreateAgentOpen] = useState(false)
  const [configPanelOpen, setConfigPanelOpen] = useState(false)
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null)
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null)
  const cmdK = useCommandPalette()

  const { persistState, handleClear } = useWorkflowPersistence(setNodes, setEdges)
  const { push: pushHistory } = useUndoRedo(setNodes, setEdges)

  // Push history snapshot on every persist (debounced saves also capture undo state)
  const persistWithHistory = useCallback(() => {
    const currentNodes = nodes
    const currentEdges = edges
    pushHistory(currentNodes, currentEdges)
    persistState()
  }, [nodes, edges, pushHistory, persistState])

  const {
    handleNodesChange,
    handleEdgesChange,
    handleConnect,
    handleDragOver,
    handleDrop,
    handleCmdKSelect,
    isValidConnection,
    onConnectStart,
    onConnectEnd,
  } = useNodeActions(setNodes, setEdges, onNodesChange, onEdgesChange, persistWithHistory)

  const handleNodeDoubleClick = useCallback((_: React.MouseEvent, node: Node) => {
    setSelectedNodeId(node.id)
  }, [])

  const handlePaneClick = useCallback(() => {
    setSelectedNodeId(null)
  }, [])

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setContextMenu({ x: e.clientX, y: e.clientY })
  }, [])

  // ── Group selected nodes (Ctrl/Cmd+G) ──
  const handleGroupSelected = useCallback(() => {
    const selected = nodes.filter((n) => n.selected && n.type !== 'group')
    if (selected.length < 2) return

    // Compute bounding box of selected nodes
    const PADDING = 40
    let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity
    for (const n of selected) {
      const x = n.position.x
      const y = n.position.y
      const w = (n.measured?.width ?? n.width ?? 260) as number
      const h = (n.measured?.height ?? n.height ?? 200) as number
      if (x < minX) minX = x
      if (y < minY) minY = y
      if (x + w > maxX) maxX = x + w
      if (y + h > maxY) maxY = y + h
    }

    const groupId = `group_${Date.now()}`
    const groupNode: Node = {
      id: groupId,
      type: 'group',
      position: { x: minX - PADDING, y: minY - PADDING },
      style: {
        width: maxX - minX + PADDING * 2,
        height: maxY - minY + PADDING * 2,
      },
      data: { name: 'Group', nodeType: 'group', collapsed: false },
    }

    // Re-parent selected nodes: position becomes relative to group
    setNodes((nds) => {
      const updated = nds.map((n) => {
        if (selected.find((s) => s.id === n.id)) {
          return {
            ...n,
            parentId: groupId,
            position: {
              x: n.position.x - (minX - PADDING),
              y: n.position.y - (minY - PADDING),
            },
            selected: false,
          }
        }
        return n
      })
      // Insert group node BEFORE its children (ReactFlow requires parent first)
      return [groupNode, ...updated]
    })
    persistWithHistory()
  }, [nodes, setNodes, persistWithHistory])

  // ── Ungroup (Ctrl/Cmd+Shift+G) ──
  const handleUngroupSelected = useCallback(() => {
    const selectedGroups = nodes.filter((n) => n.selected && n.type === 'group')
    if (selectedGroups.length === 0) return

    const groupIds = new Set(selectedGroups.map((g) => g.id))

    setNodes((nds) => {
      const updated = nds
        .filter((n) => !groupIds.has(n.id)) // Remove group nodes
        .map((n) => {
          if (n.parentId && groupIds.has(n.parentId)) {
            // Find the group to convert relative position back to absolute
            const group = selectedGroups.find((g) => g.id === n.parentId)
            const gx = group?.position.x ?? 0
            const gy = group?.position.y ?? 0
            return {
              ...n,
              parentId: undefined,
              position: {
                x: n.position.x + gx,
                y: n.position.y + gy,
              },
            }
          }
          return n
        })
      return updated
    })
    persistWithHistory()
  }, [nodes, setNodes, persistWithHistory])

  // Keyboard shortcuts for group/ungroup
  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (readOnly) return
    const mod = e.metaKey || e.ctrlKey
    if (mod && e.key === 'g' && !e.shiftKey) {
      e.preventDefault()
      handleGroupSelected()
    } else if (mod && e.key === 'g' && e.shiftKey) {
      e.preventDefault()
      handleUngroupSelected()
    }
  }, [readOnly, handleGroupSelected, handleUngroupSelected])

  // Keyboard listener for group/ungroup
  useEffect(() => {
    if (readOnly) return
    const handler = (e: KeyboardEvent) => handleKeyDown(e)
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [readOnly, handleKeyDown])

  return (
    <div
      className={fullHeight ? 'h-full flex flex-col' : readOnly ? 'h-full' : ''}
      onContextMenu={readOnly ? undefined : handleContextMenu}
    >
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodesChange={readOnly ? undefined : handleNodesChange}
        onEdgesChange={readOnly ? undefined : handleEdgesChange}
        onConnect={readOnly ? undefined : handleConnect}
        onConnectStart={readOnly ? undefined : onConnectStart}
        onConnectEnd={readOnly ? undefined : onConnectEnd}
        onNodeDoubleClick={readOnly ? undefined : handleNodeDoubleClick}
        onPaneClick={readOnly ? undefined : handlePaneClick}
        onDrop={readOnly ? undefined : handleDrop}
        onDragOver={readOnly ? undefined : handleDragOver}
        fitView
        deleteKeyCode={readOnly ? null : ['Backspace', 'Delete']}
        edgesReconnectable={!readOnly}
        elementsSelectable={!readOnly}
        nodesDraggable={!readOnly}
        nodesConnectable={!readOnly}
        nodesFocusable
        selectNodesOnDrag={false}
        panOnDrag={!readOnly}
        zoomOnScroll={!readOnly}
        className="bg-background"
        colorMode="dark"
        defaultEdgeOptions={{ style: { strokeWidth: 2 }, selectable: !readOnly, focusable: !readOnly }}
        edgesFocusable={!readOnly}
        isValidConnection={isValidConnection}
        style={fullHeight ? { flex: 1 } : { height: readOnly ? '100%' : 400 }}
      >
        <Background />
        {!readOnly && <Controls />}
        {!readOnly && (
          <MiniMap
            nodeColor={(node) => {
              const def = getDefinition((node.data as Record<string, unknown>).nodeType as string)
              return def?.headerColor ?? '#4b5563'
            }}
            style={{ background: '#1a1a2e' }}
          />
        )}
      </ReactFlow>

      {!readOnly && <RunWorkflowButton />}

      {!readOnly && <ConfigPanel open={configPanelOpen} onClose={() => setConfigPanelOpen(false)} />}

      {!readOnly && (
        <CommandPalette
          open={cmdK.open}
          onClose={cmdK.onClose}
          onSelect={handleCmdKSelect}
          onCreateAgent={() => setCreateAgentOpen(true)}
        />
      )}

      {!readOnly && (
        <CreateAgentDialog
          open={createAgentOpen}
          onClose={() => setCreateAgentOpen(false)}
        />
      )}

      {contextMenu && (
        <CanvasContextMenu
          position={contextMenu}
          onAddNode={() => { cmdK.setOpen(true); setContextMenu(null) }}
          onClearAll={() => { handleClear(); setContextMenu(null) }}
          onClose={() => setContextMenu(null)}
        />
      )}

      <NodeDetailPanel
        nodeId={selectedNodeId}
        onClose={() => setSelectedNodeId(null)}
      />
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
