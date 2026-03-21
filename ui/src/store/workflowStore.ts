/**
 * Persisted workflow state — survives page refresh.
 * Stores the full graph state from workflow-graph's getState() API.
 */
import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { GraphState } from '@auser/workflow-graph-web'

interface WorkflowStoreState {
  /** Full graph state from workflow-graph (nodes, positions, edges, zoom, pan). */
  graphState: GraphState | null
  /** Save the full graph state. */
  saveGraphState: (state: GraphState) => void
  /** Clear all persisted state. */
  clear: () => void
}

export const useWorkflowStore = create<WorkflowStoreState>()(
  persist(
    (set) => ({
      graphState: null,
      saveGraphState: (graphState) => set({ graphState }),
      clear: () => set({ graphState: null }),
    }),
    { name: 'agentzero-workflow' },
  ),
)
