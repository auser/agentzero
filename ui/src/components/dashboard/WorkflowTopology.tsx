/**
 * Workflow topology visualization powered by ReactFlow.
 * Supports drag-drop from palette, port-to-port connections,
 * Cmd+K command palette, and localStorage persistence.
 */
import { useState, useCallback } from 'react'
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
  schedule: AgentNode,
  gate: AgentNode,
  subagent: AgentNode,
  role: AgentNode,
  provider: ProviderNode,
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
  } = useNodeActions(setNodes, setEdges, onNodesChange, onEdgesChange, persistWithHistory)

  const handleNodeClick = useCallback((_: React.MouseEvent, node: Node) => {
    setSelectedNodeId(node.id)
  }, [])

  const handlePaneClick = useCallback(() => {
    setSelectedNodeId(null)
  }, [])

  const handleContextMenu = useCallback((e: React.MouseEvent) => {
    e.preventDefault()
    setContextMenu({ x: e.clientX, y: e.clientY })
  }, [])

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
        onNodeClick={readOnly ? undefined : handleNodeClick}
        onPaneClick={readOnly ? undefined : handlePaneClick}
        onDrop={readOnly ? undefined : handleDrop}
        onDragOver={readOnly ? undefined : handleDragOver}
        fitView
        deleteKeyCode={readOnly ? null : ['Backspace', 'Delete']}
        edgesReconnectable={!readOnly}
        elementsSelectable={!readOnly}
        nodesDraggable={!readOnly}
        nodesConnectable={!readOnly}
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
