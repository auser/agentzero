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

// Migrate from old format (addedNodes/edges/nodePositions) to new (graphState)
function migrateOldFormat() {
  try {
    const raw = localStorage.getItem('agentzero-workflow')
    if (!raw) return
    const parsed = JSON.parse(raw)
    // Old format had addedNodes/edges/nodePositions, new format has graphState
    if (parsed?.state?.addedNodes || parsed?.state?.edges || parsed?.state?.nodePositions) {
      localStorage.removeItem('agentzero-workflow')
    }
  } catch {
    // If parsing fails, clear it
    localStorage.removeItem('agentzero-workflow')
  }
}

// Run migration on load
migrateOldFormat()

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
