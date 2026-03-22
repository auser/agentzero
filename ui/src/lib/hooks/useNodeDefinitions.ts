/**
 * React hook for the live node definitions registry.
 * Re-renders when custom definitions are added/removed.
 */
import { useSyncExternalStore } from 'react'
import {
  getAllDefinitions,
  onDefinitionsChange,
  type NodeDefinition,
} from '@/lib/node-definitions'

// Need to import the type from workflow-types since node-definitions re-exports it
export type { NodeDefinition }

export function useNodeDefinitions(): NodeDefinition[] {
  return useSyncExternalStore(
    onDefinitionsChange,
    getAllDefinitions,
    getAllDefinitions,
  )
}
