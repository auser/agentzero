/**
 * Compound/Group node — a true container for child nodes.
 * - Click chevron to collapse/expand
 * - Collapsed: renders as a single node with aggregated ports
 *   (inputs from entry nodes, outputs from exit nodes)
 * - Resize scales children proportionally
 * - Double-click label to rename
 */
import { memo, useState, useCallback, useRef, useMemo } from 'react'
import { Handle, Position, type NodeProps, NodeResizer, useReactFlow } from '@xyflow/react'
import { getDefinition } from '@/lib/node-definitions'
import { portTypeColor, type Port } from '@/lib/workflow-types'

export interface GroupNodeData {
  name: string
  nodeType: 'group'
  collapsed: boolean
  expandedSize?: { width: number; height: number }
  [key: string]: unknown
}

const COLLAPSED_WIDTH = 260
const COLLAPSED_HEADER = 40

function GroupNodeComponent({ id, data, selected }: NodeProps) {
  const nodeData = data as unknown as GroupNodeData
  const collapsed = nodeData.collapsed ?? false
  const reactFlow = useReactFlow()
  const [editing, setEditing] = useState(false)
  const [nameValue, setNameValue] = useState(nodeData.name || 'Group')
  const prevSizeRef = useRef<{ w: number; h: number } | null>(null)

  const children = reactFlow.getNodes().filter((n) => n.parentId === id)
  const edges = reactFlow.getEdges()
  const childIds = new Set(children.map((c) => c.id))

  // Aggregate ports for collapsed view:
  // Entry inputs = child input ports with no incoming edge from inside the group
  // Exit outputs = child output ports with no outgoing edge to inside the group
  const { entryInputs, exitOutputs } = useMemo(() => {
    const inputs: (Port & { childId: string })[] = []
    const outputs: (Port & { childId: string })[] = []

    for (const child of children) {
      const childData = child.data as Record<string, unknown>
      const def = getDefinition((childData.nodeType as string) ?? '')
      const childInputs = (childData.tool_inputs as Port[]) ?? def?.inputs ?? []
      const childOutputs = (childData.tool_outputs as Port[]) ?? def?.outputs ?? []

      for (const port of childInputs) {
        // Is there an internal edge connecting to this port?
        const hasInternalSource = edges.some(
          (e) => e.target === child.id && e.targetHandle === port.id && childIds.has(e.source),
        )
        if (!hasInternalSource) {
          inputs.push({ ...port, childId: child.id, id: `${child.id}__${port.id}` })
        }
      }

      for (const port of childOutputs) {
        const hasInternalTarget = edges.some(
          (e) => e.source === child.id && e.sourceHandle === port.id && childIds.has(e.target),
        )
        if (!hasInternalTarget) {
          outputs.push({ ...port, childId: child.id, id: `${child.id}__${port.id}` })
        }
      }
    }
    return { entryInputs: inputs, exitOutputs: outputs }
  }, [children, edges, childIds])

  const collapsedHeight = COLLAPSED_HEADER + Math.max(entryInputs.length, exitOutputs.length) * 22 + 12

  const toggleCollapse = useCallback((e: React.MouseEvent) => {
    e.stopPropagation()
    const nextCollapsed = !collapsed

    reactFlow.setNodes((nodes) =>
      nodes.map((n) => {
        if (n.id === id) {
          if (nextCollapsed) {
            const w = (n.measured?.width ?? n.style?.width ?? 300) as number
            const h = (n.measured?.height ?? n.style?.height ?? 200) as number
            return {
              ...n,
              data: { ...n.data, collapsed: true, expandedSize: { width: w, height: h } },
              style: { ...n.style, width: COLLAPSED_WIDTH, height: collapsedHeight },
            }
          }
          const saved = (n.data as GroupNodeData).expandedSize
          return {
            ...n,
            data: { ...n.data, collapsed: false },
            style: { ...n.style, width: saved?.width ?? 300, height: saved?.height ?? 200 },
          }
        }
        if (n.parentId === id) {
          return { ...n, hidden: nextCollapsed }
        }
        return n
      }),
    )
  }, [id, collapsed, collapsedHeight, reactFlow])

  // Proportional resize: scale children when group is resized
  const handleResize = useCallback((_: unknown, params: { width: number; height: number }) => {
    const prev = prevSizeRef.current
    if (!prev) {
      prevSizeRef.current = { w: params.width, h: params.height }
      return
    }
    const scaleX = params.width / prev.w
    const scaleY = params.height / prev.h
    if (Math.abs(scaleX - 1) < 0.01 && Math.abs(scaleY - 1) < 0.01) {
      prevSizeRef.current = { w: params.width, h: params.height }
      return
    }

    reactFlow.setNodes((nodes) =>
      nodes.map((n) => {
        if (n.parentId === id) {
          return {
            ...n,
            position: { x: n.position.x * scaleX, y: n.position.y * scaleY },
          }
        }
        return n
      }),
    )
    prevSizeRef.current = { w: params.width, h: params.height }
  }, [id, reactFlow])

  const startEditing = useCallback((e: React.MouseEvent) => {
    e.stopPropagation()
    setEditing(true)
  }, [])

  const finishEditing = useCallback(() => {
    setEditing(false)
    reactFlow.setNodes((nodes) =>
      nodes.map((n) =>
        n.id === id ? { ...n, data: { ...n.data, name: nameValue } } : n,
      ),
    )
  }, [id, nameValue, reactFlow])

  // ── Collapsed view: single node with aggregated ports ──
  if (collapsed) {
    return (
      <>
      <NodeResizer
        isVisible={selected}
        minWidth={160}
        minHeight={COLLAPSED_HEADER + 12}
        lineStyle={{ borderColor: '#7C3AED40' }}
        handleStyle={{ width: 6, height: 6, background: '#7C3AED', borderRadius: 2 }}
      />
      <div
        style={{
          width: '100%',
          height: '100%',
          borderRadius: 14,
          border: selected ? '2px solid #7C3AED' : '1px solid rgba(255,255,255,0.08)',
          background: '#1C1C1E',
          fontFamily: "'JetBrains Mono', monospace",
          overflow: 'visible',
          userSelect: 'none',
          boxShadow: '0 2px 8px rgba(0,0,0,0.4)',
        }}
      >
        {/* Header */}
        <div
          style={{
            display: 'flex', alignItems: 'center', gap: 6,
            padding: '8px 12px',
            borderBottom: (entryInputs.length > 0 || exitOutputs.length > 0)
              ? '1px solid rgba(255,255,255,0.04)' : 'none',
          }}
        >
          <span style={{ fontSize: 12, color: '#7C3AED' }}>⊞</span>
          <span
            style={{ fontSize: 13, fontWeight: 600, color: '#A78BFA', flex: 1 }}
            onDoubleClick={startEditing}
          >
            {editing ? (
              <input
                className="nodrag nopan" autoFocus value={nameValue}
                onChange={(e) => setNameValue(e.target.value)}
                onBlur={finishEditing}
                onKeyDown={(e) => { if (e.key === 'Enter') finishEditing() }}
                style={{
                  background: 'transparent', border: 'none', outline: 'none',
                  fontSize: 13, fontWeight: 600, color: '#A78BFA',
                  fontFamily: "'JetBrains Mono', monospace", width: '100%', textAlign: 'center',
                }}
              />
            ) : nameValue}
          </span>
          <span style={{
            fontSize: 9, color: '#7C3AED', background: 'rgba(124,58,237,0.15)',
            borderRadius: 4, padding: '1px 5px', fontWeight: 500,
          }}>
            {children.length}
          </span>
          <button
            className="nodrag nopan"
            onClick={toggleCollapse}
            onDoubleClick={(e) => e.stopPropagation()}
            style={{
              background: 'none', border: 'none', cursor: 'pointer',
              fontSize: 10, color: '#525252', padding: '2px 4px',
              lineHeight: 1, transform: 'rotate(-90deg)',
              transition: 'transform 0.15s',
            }}
          >
            ▼
          </button>
        </div>

        {/* Aggregated ports */}
        {(entryInputs.length > 0 || exitOutputs.length > 0) && (
          <div style={{ padding: '4px 0 6px' }}>
            {Array.from({ length: Math.max(entryInputs.length, exitOutputs.length) }).map((_, i) => {
              const inp = entryInputs[i]
              const out = exitOutputs[i]
              return (
                <div key={i} style={{
                  display: 'flex', alignItems: 'center', justifyContent: 'space-between',
                  padding: '2px 12px', position: 'relative', minHeight: 18,
                }}>
                  {inp ? (
                    <>
                      <Handle type="target" position={Position.Left} id={inp.id}
                        style={{
                          width: 12, height: 12,
                          background: portTypeColor(inp.port_type ?? ''),
                          border: '2px solid #1C1C1E', left: -6,
                          top: '50%', transform: 'translateY(-50%)', position: 'absolute',
                        }}
                      />
                      <span style={{ fontSize: 11, color: portTypeColor(inp.port_type ?? '') }}>
                        {inp.label}
                      </span>
                    </>
                  ) : <div />}
                  {out ? (
                    <>
                      <Handle type="source" position={Position.Right} id={out.id}
                        style={{
                          width: 12, height: 12,
                          background: portTypeColor(out.port_type ?? ''),
                          border: '2px solid #1C1C1E', right: -6,
                          top: '50%', transform: 'translateY(-50%)', position: 'absolute',
                        }}
                      />
                      <span style={{
                        fontSize: 11, color: portTypeColor(out.port_type ?? ''),
                        marginLeft: 'auto',
                      }}>
                        {out.label}
                      </span>
                    </>
                  ) : <div />}
                </div>
              )
            })}
          </div>
        )}
      </div>
      </>
    )
  }

  // ── Expanded view: transparent container ──
  return (
    <>
      <NodeResizer
        isVisible={selected}
        minWidth={200}
        minHeight={100}
        lineStyle={{ borderColor: '#7C3AED40' }}
        handleStyle={{ width: 8, height: 8, background: '#7C3AED', borderRadius: 2 }}
        onResize={handleResize}
      />
      <div
        style={{
          width: '100%',
          height: '100%',
          borderRadius: 16,
          border: selected ? '2px dashed #7C3AED' : '1px dashed rgba(255,255,255,0.1)',
          background: 'rgba(124, 58, 237, 0.03)',
          fontFamily: "'JetBrains Mono', monospace",
          overflow: 'visible',
          userSelect: 'none',
        }}
      >
        <div
          style={{
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            gap: 6, padding: '6px 12px',
          }}
        >
          <span style={{ fontSize: 12, color: '#7C3AED' }}>⊞</span>
          {editing ? (
            <input
              className="nodrag nopan" autoFocus value={nameValue}
              onChange={(e) => setNameValue(e.target.value)}
              onBlur={finishEditing}
              onKeyDown={(e) => { if (e.key === 'Enter') finishEditing() }}
              style={{
                background: 'rgba(124,58,237,0.1)', border: '1px solid #7C3AED40',
                borderRadius: 4, outline: 'none', fontSize: 12, fontWeight: 600,
                color: '#A78BFA', fontFamily: "'JetBrains Mono', monospace",
                textAlign: 'center', width: 100, padding: '2px 4px',
              }}
            />
          ) : (
            <span
              style={{ fontSize: 12, fontWeight: 600, color: '#A78BFA', flex: 1 }}
              onDoubleClick={startEditing}
            >
              {nameValue}
            </span>
          )}
          <button
            className="nodrag nopan"
            onClick={toggleCollapse}
            onDoubleClick={(e) => e.stopPropagation()}
            style={{
              background: 'none', border: 'none', cursor: 'pointer',
              fontSize: 10, color: '#525252', padding: '2px 4px',
              lineHeight: 1, transition: 'transform 0.15s',
            }}
          >
            ▼
          </button>
        </div>
      </div>
    </>
  )
}

export const GroupNode = memo(GroupNodeComponent)
