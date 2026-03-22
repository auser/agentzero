/**
 * Hook wiring node/edge change handlers with auto-persist,
 * plus connection coloring and Cmd+K / drop insertion.
 */
import { useCallback } from 'react'
import {
  useReactFlow,
  addEdge,
  type Connection,
  type Node,
  type Edge,
  type NodeChange,
  type EdgeChange,
} from '@xyflow/react'
import { useCanvasDrop, dragPayloadToNode, type DragPayload } from '@/lib/hooks/useDragDrop'
import { useConnectionValidation } from '@/lib/hooks/useConnectionValidation'
import { getDefinition } from '@/lib/node-definitions'
import { portTypeColor } from '@/lib/workflow-types'

export function useNodeActions(
  setNodes: React.Dispatch<React.SetStateAction<Node[]>>,
  setEdges: React.Dispatch<React.SetStateAction<Edge[]>>,
  onNodesChange: (changes: NodeChange[]) => void,
  onEdgesChange: (changes: EdgeChange[]) => void,
  persistState: () => void,
) {
  const reactFlow = useReactFlow()

  const handleNodesChange = useCallback((changes: NodeChange[]) => {
    onNodesChange(changes)
    persistState()
  }, [onNodesChange, persistState])

  const handleEdgesChange = useCallback((changes: EdgeChange[]) => {
    onEdgesChange(changes)
    persistState()
  }, [onEdgesChange, persistState])

  const handleConnect = useCallback((connection: Connection) => {
    const sourceNode = reactFlow.getNode(connection.source)
    if (!sourceNode) return

    const sourceData = sourceNode.data as Record<string, unknown>
    const sourceDef = getDefinition((sourceData.nodeType as string) ?? '')
    const sourcePort = sourceDef?.outputs?.find((p) => p.id === connection.sourceHandle)
    const color = portTypeColor(sourcePort?.port_type ?? '')

    setEdges((eds) => addEdge({
      ...connection,
      style: { stroke: color, strokeWidth: 2 },
      data: { port_type: sourcePort?.port_type ?? '' },
    }, eds))
    persistState()
  }, [reactFlow, setEdges, persistState])

  const addNode = useCallback((node: Node) => {
    setNodes((nds) => [...nds, node])
    persistState()
  }, [setNodes, persistState])

  const { handleDragOver, handleDrop } = useCanvasDrop(addNode)

  const handleCmdKSelect = useCallback((data: DragPayload) => {
    const position = reactFlow.screenToFlowPosition({
      x: window.innerWidth / 2,
      y: window.innerHeight / 2,
    })
    addNode(dragPayloadToNode(data, position))
  }, [reactFlow, addNode])

  const isValidConnection = useConnectionValidation()

  return {
    handleNodesChange,
    handleEdgesChange,
    handleConnect,
    handleDragOver,
    handleDrop,
    handleCmdKSelect,
    isValidConnection,
  }
}
