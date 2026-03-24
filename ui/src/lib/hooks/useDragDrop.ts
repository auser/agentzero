/**
 * Reusable drag-drop hooks for workflow canvas nodes.
 *
 * @example
 *   const { handleDragOver, handleDrop } = useCanvasDrop(reactFlow, (node) => {
 *     setNodes((nds) => [...nds, node])
 *   })
 */
import { useCallback, type DragEvent } from 'react'
import { useReactFlow, type Node } from '@xyflow/react'

const MIME_TYPE = 'application/workflow-node'

export interface DragPayload {
  id: string
  name: string
  nodeType: string
  metadata?: Record<string, unknown>
}

export function dragPayloadToNode(data: DragPayload, position: { x: number; y: number }): Node {
  return {
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
}

/**
 * Encode a drag payload for the palette drag-start event.
 */
export function encodeDragData(e: DragEvent, data: DragPayload) {
  e.dataTransfer.setData(MIME_TYPE, JSON.stringify(data))
  e.dataTransfer.effectAllowed = 'copy'
}

/**
 * Hook providing drop-target handlers for a ReactFlow canvas.
 */
export function useCanvasDrop(onNodeAdded: (node: Node) => void) {
  const reactFlow = useReactFlow()

  const handleDragOver = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault()
    e.dataTransfer.dropEffect = 'copy'
  }, [])

  const handleDrop = useCallback((e: DragEvent<HTMLDivElement>) => {
    e.preventDefault()
    const raw = e.dataTransfer.getData(MIME_TYPE)
    if (!raw) return
    try {
      const payload: DragPayload = JSON.parse(raw)
      const position = reactFlow.screenToFlowPosition({ x: e.clientX, y: e.clientY })
      onNodeAdded(dragPayloadToNode(payload, position))
    } catch (err) {
      console.error('Failed to add dropped node:', err)
    }
  }, [reactFlow, onNodeAdded])

  return { handleDragOver, handleDrop }
}
