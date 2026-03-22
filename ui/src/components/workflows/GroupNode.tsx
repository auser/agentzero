/**
 * Compound/Group node — a container for child nodes.
 * Children are positioned relative to the group.
 * Click header to collapse/expand.
 */
import { memo, useState, useCallback } from 'react'
import { type NodeProps, NodeResizer } from '@xyflow/react'

export interface GroupNodeData {
  name: string
  nodeType: 'group'
  collapsed: boolean
  [key: string]: unknown
}

function GroupNodeComponent({ id, data, selected }: NodeProps) {
  const nodeData = data as unknown as GroupNodeData
  const [editing, setEditing] = useState(false)
  const [nameValue, setNameValue] = useState(nodeData.name || 'Group')

  const handleDoubleClick = useCallback(() => {
    setEditing(true)
  }, [])

  const handleBlur = useCallback(() => {
    setEditing(false)
  }, [])

  return (
    <>
      <NodeResizer
        isVisible={selected}
        minWidth={200}
        minHeight={100}
        lineStyle={{ borderColor: '#7C3AED40' }}
        handleStyle={{ width: 8, height: 8, background: '#7C3AED', borderRadius: 2 }}
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
        {/* Header label */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            padding: '6px 12px',
          }}
          onDoubleClick={handleDoubleClick}
        >
          <span style={{ fontSize: 12, color: '#7C3AED' }}>⊞</span>
          {editing ? (
            <input
              className="nodrag"
              autoFocus
              value={nameValue}
              onChange={(e) => setNameValue(e.target.value)}
              onBlur={handleBlur}
              onKeyDown={(e) => {
                if (e.key === 'Enter') handleBlur()
              }}
              style={{
                background: 'transparent',
                border: 'none',
                outline: 'none',
                fontSize: 12,
                fontWeight: 600,
                color: '#A78BFA',
                fontFamily: "'JetBrains Mono', monospace",
                width: 120,
              }}
            />
          ) : (
            <span style={{ fontSize: 12, fontWeight: 600, color: '#A78BFA' }}>
              {nameValue}
            </span>
          )}
        </div>
      </div>
    </>
  )
}

export const GroupNode = memo(GroupNodeComponent)
