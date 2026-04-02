/**
 * Custom ReactFlow edge component that shows port-type labels on edges.
 * Renders a colored bezier path with an inline label showing the port type
 * and optional condition. Click the label to edit the condition.
 */
import { useState, useCallback } from 'react'
import { getBezierPath, EdgeLabelRenderer, useReactFlow, type EdgeProps } from '@xyflow/react'
import { portTypeColor } from '@/lib/workflow-types'

export function LabeledEdge({
  id, sourceX, sourceY, targetX, targetY,
  sourcePosition, targetPosition,
  style, markerEnd, data,
}: EdgeProps) {
  const [edgePath, labelX, labelY] = getBezierPath({
    sourceX, sourceY, sourcePosition,
    targetX, targetY, targetPosition,
  })
  const reactFlow = useReactFlow()
  const [editing, setEditing] = useState(false)
  const [conditionDraft, setConditionDraft] = useState('')

  const portType = (data?.port_type as string) ?? ''
  const condition = (data?.condition as string) ?? ''
  const outputPreview = (data?.output_preview as string) ?? ''
  const color = portType ? portTypeColor(portType) : (style?.stroke as string) ?? '#6b7280'

  const handleLabelClick = useCallback(() => {
    setConditionDraft(condition)
    setEditing(true)
  }, [condition])

  const saveCondition = useCallback((value: string) => {
    setEditing(false)
    reactFlow.setEdges((edges) =>
      edges.map((e) =>
        e.id === id
          ? { ...e, data: { ...e.data, condition: value || undefined } }
          : e,
      ),
    )
  }, [reactFlow, id])

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      saveCondition(conditionDraft)
    } else if (e.key === 'Escape') {
      setEditing(false)
    }
  }, [conditionDraft, saveCondition])

  return (
    <>
      <path id={id} style={{ ...style, stroke: color }} className="react-flow__edge-path" d={edgePath} markerEnd={markerEnd} />
      <EdgeLabelRenderer>
        <div
          style={{
            position: 'absolute',
            transform: `translate(-50%, -50%) translate(${labelX}px,${labelY}px)`,
            fontSize: 9,
            fontFamily: "'JetBrains Mono', monospace",
            pointerEvents: 'all',
          }}
          className="nodrag nopan"
        >
          {editing ? (
            <input
              autoFocus
              type="text"
              value={conditionDraft}
              onChange={(e) => setConditionDraft(e.target.value)}
              onBlur={() => saveCondition(conditionDraft)}
              onKeyDown={handleKeyDown}
              placeholder="condition..."
              style={{
                width: 120,
                fontSize: 9,
                fontFamily: "'JetBrains Mono', monospace",
                color: '#eab308',
                background: '#0F0F11',
                border: `1px solid ${color}60`,
                borderRadius: 4,
                padding: '2px 6px',
                outline: 'none',
              }}
            />
          ) : (
            <div
              onClick={handleLabelClick}
              title="Click to add condition"
              style={{
                color: '#525252',
                background: '#1a1a2e',
                padding: '1px 6px',
                borderRadius: 4,
                border: `1px solid ${color}40`,
                cursor: 'pointer',
                minWidth: portType ? undefined : 30,
              }}
            >
              {portType || '\u00b7'}
              {condition && <span style={{ color: '#eab308', marginLeft: 4 }}>&#x2B26; {condition}</span>}
              {outputPreview && (
                <div
                  style={{
                    color: '#22c55e',
                    fontSize: 8,
                    marginTop: 1,
                    maxWidth: 140,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                  }}
                  title={outputPreview}
                >
                  {outputPreview.length > 40 ? outputPreview.slice(0, 40) + '...' : outputPreview}
                </div>
              )}
            </div>
          )}
        </div>
      </EdgeLabelRenderer>
    </>
  )
}
