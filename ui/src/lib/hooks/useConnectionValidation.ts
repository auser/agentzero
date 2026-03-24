/**
 * Reusable connection-validation hook for ReactFlow port-type matching.
 * Ensures only ports of the same type can be connected.
 */
import { useCallback } from 'react'
import { useReactFlow, type IsValidConnection } from '@xyflow/react'
import { getDefinition } from '@/lib/node-definitions'

export function useConnectionValidation(): IsValidConnection {
  const reactFlow = useReactFlow()

  return useCallback((connection) => {
    const src = reactFlow.getNode(connection.source)
    const tgt = reactFlow.getNode(connection.target)
    if (!src || !tgt) return false

    const srcDef = getDefinition((src.data as Record<string, unknown>).nodeType as string)
    const tgtDef = getDefinition((tgt.data as Record<string, unknown>).nodeType as string)
    const srcPort = srcDef?.outputs?.find((p) => p.id === connection.sourceHandle)
    const tgtPort = tgtDef?.inputs?.find((p) => p.id === connection.targetHandle)

    if (!srcPort || !tgtPort) return true // allow unknown ports
    return srcPort.port_type === tgtPort.port_type
  }, [reactFlow])
}
