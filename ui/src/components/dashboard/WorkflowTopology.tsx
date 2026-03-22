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
  SelectionMode,
  useReactFlow,
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
import { TemplateGallery } from '@/components/workflows/TemplateGallery'
import { EmptyCanvasState } from '@/components/workflows/EmptyCanvasState'
import { getDefinition } from '@/lib/node-definitions'
import type { WorkflowTemplate } from '@/lib/workflow-templates'
import { workflowsApi } from '@/lib/api/workflows'

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
  const reactFlowInstance = useReactFlow()
  const [nodes, setNodes, onNodesChange] = useNodesState<Node>([])
  const [edges, setEdges, onEdgesChange] = useEdgesState<Edge>([])
  const [createAgentOpen, setCreateAgentOpen] = useState(false)
  const [configPanelOpen, setConfigPanelOpen] = useState(false)
  const [templateGalleryOpen, setTemplateGalleryOpen] = useState(false)
  const [saveTemplateOpen, setSaveTemplateOpen] = useState(false)
  const [templateName, setTemplateName] = useState('')
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null)
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null)
  const cmdK = useCommandPalette()

  const { persistState, handleClear, workflowId } = useWorkflowPersistence(setNodes, setEdges)
  const { push: pushHistory, undo, redo } = useUndoRedo(setNodes, setEdges)

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
    // Groups handle their own double-click (rename)
    if (node.type === 'group') return
    setSelectedNodeId(node.id)
  }, [])

  const handlePaneClick = useCallback(() => {
    setSelectedNodeId(null)
  }, [])

  // Detach from group when dragged outside, attach when dragged into a group
  const handleNodeDragStop = useCallback((_: React.MouseEvent, draggedNode: Node) => {
    if (draggedNode.type === 'group') return
    const allNodes = reactFlowInstance.getNodes()

    // ── Detach: child dragged outside parent ──
    if (draggedNode.parentId) {
      const parent = reactFlowInstance.getNode(draggedNode.parentId)
      if (parent) {
        const pw = (parent.measured?.width ?? parent.style?.width ?? 300) as number
        const ph = (parent.measured?.height ?? parent.style?.height ?? 200) as number
        const outside =
          draggedNode.position.x < -20 || draggedNode.position.y < -20 ||
          draggedNode.position.x > pw + 20 || draggedNode.position.y > ph + 20
        if (outside) {
          setNodes((nds) =>
            nds.map((n) =>
              n.id === draggedNode.id
                ? {
                    ...n,
                    parentId: undefined,
                    expandParent: undefined,
                    position: {
                      x: draggedNode.position.x + parent.position.x,
                      y: draggedNode.position.y + parent.position.y,
                    },
                  }
                : n,
            ),
          )
          return
        }
      }
    }

    // ── Attach: free node dragged into a group ──
    if (!draggedNode.parentId) {
      const absX = draggedNode.position.x
      const absY = draggedNode.position.y
      for (const group of allNodes) {
        if (group.type !== 'group' || group.id === draggedNode.id) continue
        if ((group.data as Record<string, unknown>).collapsed) continue
        const gw = (group.measured?.width ?? group.style?.width ?? 300) as number
        const gh = (group.measured?.height ?? group.style?.height ?? 200) as number
        if (
          absX > group.position.x && absX < group.position.x + gw &&
          absY > group.position.y && absY < group.position.y + gh
        ) {
          setNodes((nds) =>
            nds.map((n) =>
              n.id === draggedNode.id
                ? {
                    ...n,
                    parentId: group.id,
                    expandParent: true,
                    position: {
                      x: absX - group.position.x,
                      y: absY - group.position.y,
                    },
                  }
                : n,
            ),
          )
          return
        }
      }
    }
  }, [reactFlowInstance, setNodes])

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
            expandParent: true,
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
              expandParent: undefined,
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

  // ── Load a workflow template onto the canvas ──
  const handleTemplateSelect = useCallback((template: WorkflowTemplate) => {
    const templateNodes = template.nodes.map((n) => ({
      id: n.id,
      type: n.data.nodeType,
      position: n.position,
      data: n.data,
    }))
    const templateEdges = template.edges.map((e) => ({
      id: e.id,
      source: e.source,
      target: e.target,
      sourceHandle: e.sourceHandle,
      targetHandle: e.targetHandle,
      data: e.data,
    }))
    setNodes(templateNodes)
    setEdges(templateEdges)
    setTemplateGalleryOpen(false)
    persistWithHistory()
  }, [setNodes, setEdges, persistWithHistory])

  // Save current canvas as a template to the server
  const handleSaveAsTemplate = useCallback(async (name: string) => {
    if (!name.trim()) return
    const currentNodes = reactFlowInstance.getNodes()
    const currentEdges = reactFlowInstance.getEdges()

    try {
      await workflowsApi.create({
        name: name.trim(),
        description: `Custom template with ${currentNodes.length} nodes`,
        layout: {
          nodes: currentNodes.map((n) => ({
            id: n.id, type: n.type, position: n.position, data: n.data,
          })),
          edges: currentEdges.map((e) => ({
            id: e.id, source: e.source, target: e.target,
            sourceHandle: e.sourceHandle ?? 'output',
            targetHandle: e.targetHandle ?? 'input',
            data: e.data,
          })),
        },
      })
    } catch {
      // Server may not be running — that's OK
    }
    setSaveTemplateOpen(false)
    setTemplateName('')
  }, [reactFlowInstance])

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
    } else if (mod && e.shiftKey && e.key === 'f') {
      e.preventDefault()
      reactFlowInstance.fitView({ padding: 0.2, duration: 300 })
    }
  }, [readOnly, handleGroupSelected, handleUngroupSelected, reactFlowInstance])

  // Keyboard listener for group/ungroup
  useEffect(() => {
    if (readOnly) return
    const handler = (e: KeyboardEvent) => handleKeyDown(e)
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [readOnly, handleKeyDown])

  return (
    <div
      className={fullHeight ? 'h-full flex flex-col relative' : readOnly ? 'h-full relative' : 'relative'}
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
        onNodeDragStop={readOnly ? undefined : handleNodeDragStop}
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
        selectionMode={SelectionMode.Partial}
        panOnDrag={!readOnly}
        zoomOnScroll={!readOnly}
        className="bg-background"
        colorMode="dark"
        defaultEdgeOptions={{ style: { strokeWidth: 2 }, selectable: !readOnly, focusable: !readOnly }}
        edgesFocusable={!readOnly}
        isValidConnection={isValidConnection}
        proOptions={{ hideAttribution: true }}
        style={fullHeight ? { flex: 1 } : { height: readOnly ? '100%' : 400 }}
      >
        <Background />
        {!readOnly && (
          <Controls>
            <button
              onClick={undo}
              title="Undo (Cmd+Z)"
              style={{
                width: 26, height: 26, display: 'flex', alignItems: 'center',
                justifyContent: 'center', background: 'transparent', border: 'none',
                color: '#737373', cursor: 'pointer', fontSize: 14,
              }}
            >
              ↩
            </button>
            <button
              onClick={redo}
              title="Redo (Cmd+Shift+Z)"
              style={{
                width: 26, height: 26, display: 'flex', alignItems: 'center',
                justifyContent: 'center', background: 'transparent', border: 'none',
                color: '#737373', cursor: 'pointer', fontSize: 14,
              }}
            >
              ↪
            </button>
          </Controls>
        )}
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

      {!readOnly && nodes.length === 0 && (
        <EmptyCanvasState
          onOpenGallery={() => setTemplateGalleryOpen(true)}
          onStartScratch={() => cmdK.setOpen(true)}
        />
      )}

      {!readOnly && (
        <TemplateGallery
          open={templateGalleryOpen}
          onClose={() => setTemplateGalleryOpen(false)}
          onSelect={handleTemplateSelect}
        />
      )}

      {!readOnly && <RunWorkflowButton workflowId={workflowId} />}

      {/* Top-left toolbar — templates, zoom-to-fit, save template */}
      {!readOnly && (
        <div style={{ position: 'absolute', top: 12, left: 12, zIndex: 20, display: 'flex', gap: 6 }}>
          <button
            onClick={() => setTemplateGalleryOpen(true)}
            title="Browse workflow templates"
            style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '8px 14px', background: '#1C1C1E', color: '#A3A3A3',
              border: '1px solid rgba(255,255,255,0.06)', borderRadius: 8,
              fontSize: 12, fontWeight: 500, fontFamily: "'JetBrains Mono', monospace",
              cursor: 'pointer', boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
            }}
          >
            <span style={{ fontSize: 14 }}>&#x2B1A;</span>
            Templates
          </button>
          <button
            onClick={() => reactFlowInstance.fitView({ padding: 0.2, duration: 300 })}
            title="Zoom to fit all nodes"
            style={{
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              width: 36, height: 36, background: '#1C1C1E', color: '#A3A3A3',
              border: '1px solid rgba(255,255,255,0.06)', borderRadius: 8,
              fontSize: 16, cursor: 'pointer', boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
            }}
          >
            &#x2922;
          </button>
          {nodes.length > 0 && !saveTemplateOpen && (
            <button
              onClick={() => setSaveTemplateOpen(true)}
              title="Save current workflow as a template"
              style={{
                display: 'flex', alignItems: 'center', gap: 6,
                padding: '8px 14px', background: '#1C1C1E', color: '#A3A3A3',
                border: '1px solid rgba(255,255,255,0.06)', borderRadius: 8,
                fontSize: 12, fontWeight: 500, fontFamily: "'JetBrains Mono', monospace",
                cursor: 'pointer', boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
              }}
            >
              Save Template
            </button>
          )}
          {saveTemplateOpen && (
            <div style={{
              display: 'flex', alignItems: 'center', gap: 4,
              background: '#1C1C1E', border: '1px solid rgba(255,255,255,0.06)',
              borderRadius: 8, padding: '4px 6px',
              boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
            }}>
              <input
                autoFocus
                type="text"
                value={templateName}
                onChange={(e) => setTemplateName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && templateName.trim()) handleSaveAsTemplate(templateName)
                  if (e.key === 'Escape') { setSaveTemplateOpen(false); setTemplateName('') }
                }}
                placeholder="Template name..."
                style={{
                  width: 160, background: '#0F0F11', border: 'none', borderRadius: 6,
                  padding: '6px 10px', fontSize: 12, color: '#E5E5E5', outline: 'none',
                  fontFamily: "'JetBrains Mono', monospace",
                }}
              />
              <button
                onClick={() => { if (templateName.trim()) handleSaveAsTemplate(templateName) }}
                disabled={!templateName.trim()}
                style={{
                  padding: '6px 10px', background: templateName.trim() ? '#22c55e' : '#374151',
                  color: '#fff', border: 'none', borderRadius: 6, fontSize: 11, fontWeight: 600,
                  fontFamily: "'JetBrains Mono', monospace", cursor: templateName.trim() ? 'pointer' : 'not-allowed',
                }}
              >
                Save
              </button>
              <button
                onClick={() => { setSaveTemplateOpen(false); setTemplateName('') }}
                style={{
                  padding: '6px 8px', background: 'transparent', color: '#737373',
                  border: 'none', cursor: 'pointer', fontSize: 14,
                }}
              >
                &#x2715;
              </button>
            </div>
          )}
        </div>
      )}

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
        onClose={handlePaneClick}
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
