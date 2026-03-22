/**
 * Compound/Group node — a container for child nodes.
 * Children are positioned relative to the group.
 * Click chevron to collapse/expand. Double-click label to rename.
 */
import { memo, useState, useCallback } from 'react'
import { type NodeProps, NodeResizer, useReactFlow } from '@xyflow/react'

export interface GroupNodeData {
  name: string
  nodeType: 'group'
  collapsed: boolean
  expandedSize?: { width: number; height: number }
  [key: string]: unknown
}

const COLLAPSED_WIDTH = 180
const COLLAPSED_HEIGHT = 40

function GroupNodeComponent({ id, data, selected }: NodeProps) {
  const nodeData = data as unknown as GroupNodeData
  const collapsed = nodeData.collapsed ?? false
  const reactFlow = useReactFlow()
  const [editing, setEditing] = useState(false)
  const [nameValue, setNameValue] = useState(nodeData.name || 'Group')

  const childCount = reactFlow.getNodes().filter((n) => n.parentId === id).length

  const toggleCollapse = useCallback((e: React.MouseEvent) => {
    e.stopPropagation()
    const nextCollapsed = !collapsed

    reactFlow.setNodes((nodes) =>
      nodes.map((n) => {
        if (n.id === id) {
          if (nextCollapsed) {
            const w = (n.measured?.width ?? n.width ?? n.style?.width ?? 300) as number
            const h = (n.measured?.height ?? n.height ?? n.style?.height ?? 200) as number
            return {
              ...n,
              data: { ...n.data, collapsed: true, expandedSize: { width: w, height: h } },
              style: { ...n.style, width: COLLAPSED_WIDTH, height: COLLAPSED_HEIGHT },
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
  }, [id, collapsed, reactFlow])

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

  return (
    <>
      {!collapsed && (
        <NodeResizer
          isVisible={selected}
          minWidth={200}
          minHeight={100}
          lineStyle={{ borderColor: '#7C3AED40' }}
          handleStyle={{ width: 8, height: 8, background: '#7C3AED', borderRadius: 2 }}
        />
      )}
      <div
        style={{
          width: '100%',
          height: '100%',
          borderRadius: collapsed ? 10 : 16,
          border: selected ? '2px dashed #7C3AED' : '1px dashed rgba(255,255,255,0.1)',
          background: collapsed ? 'rgba(124, 58, 237, 0.08)' : 'rgba(124, 58, 237, 0.03)',
          fontFamily: "'JetBrains Mono', monospace",
          overflow: 'visible',
          userSelect: 'none',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            gap: 6,
            padding: '6px 12px',
            height: collapsed ? '100%' : 'auto',
          }}
        >
          {/* Collapse toggle */}
          <button
            className="nodrag nopan"
            onClick={toggleCollapse}
            onDoubleClick={(e) => e.stopPropagation()}
            style={{
              background: 'none',
              border: 'none',
              cursor: 'pointer',
              fontSize: 10,
              color: '#7C3AED',
              padding: '2px 4px',
              lineHeight: 1,
              transform: collapsed ? 'rotate(-90deg)' : 'rotate(0deg)',
              transition: 'transform 0.15s',
            }}
          >
            ▼
          </button>

          <span style={{ fontSize: 12, color: '#7C3AED' }}>⊞</span>

          {editing ? (
            <input
              className="nodrag nopan"
              autoFocus
              value={nameValue}
              onChange={(e) => setNameValue(e.target.value)}
              onBlur={finishEditing}
              onKeyDown={(e) => { if (e.key === 'Enter') finishEditing() }}
              style={{
                background: 'rgba(124, 58, 237, 0.1)',
                border: '1px solid #7C3AED40',
                borderRadius: 4,
                outline: 'none',
                fontSize: 12,
                fontWeight: 600,
                color: '#A78BFA',
                fontFamily: "'JetBrains Mono', monospace",
                textAlign: 'center',
                width: 100,
                padding: '2px 4px',
              }}
            />
          ) : (
            <span
              style={{ fontSize: 12, fontWeight: 600, color: '#A78BFA' }}
              onDoubleClick={startEditing}
            >
              {nameValue}
            </span>
          )}

          {collapsed && childCount > 0 && (
            <span
              style={{
                fontSize: 9,
                color: '#7C3AED',
                background: 'rgba(124, 58, 237, 0.15)',
                borderRadius: 4,
                padding: '1px 5px',
                fontWeight: 500,
              }}
            >
              {childCount}
            </span>
          )}
        </div>
      </div>
    </>
  )
}

export const GroupNode = memo(GroupNodeComponent)
