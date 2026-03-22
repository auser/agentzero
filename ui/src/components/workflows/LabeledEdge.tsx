/**
 * Custom ReactFlow edge component that shows port-type labels on edges.
 * Renders a colored bezier path with an inline label showing the port type
 * and optional condition.
 */
import { getBezierPath, EdgeLabelRenderer, type EdgeProps } from '@xyflow/react'
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

  const portType = (data?.port_type as string) ?? ''
  const condition = (data?.condition as string) ?? ''
  const color = portType ? portTypeColor(portType) : (style?.stroke as string) ?? '#6b7280'

  return (
    <>
      <path id={id} style={{ ...style, stroke: color }} className="react-flow__edge-path" d={edgePath} markerEnd={markerEnd} />
      <EdgeLabelRenderer>
        {portType && (
          <div
            style={{
              position: 'absolute',
              transform: `translate(-50%, -50%) translate(${labelX}px,${labelY}px)`,
              fontSize: 9,
              fontFamily: "'JetBrains Mono', monospace",
              color: '#525252',
              background: '#1a1a2e',
              padding: '1px 6px',
              borderRadius: 4,
              border: `1px solid ${color}40`,
              pointerEvents: 'all',
            }}
            className="nodrag nopan"
          >
            {portType}
            {condition && <span style={{ color: '#eab308', marginLeft: 4 }}>&#x2B26; {condition}</span>}
          </div>
        )}
      </EdgeLabelRenderer>
    </>
  )
}
