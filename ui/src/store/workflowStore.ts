/**
 * Workflow store — minimal state for workflow builder.
 * ReactFlow state persistence is handled directly via localStorage
 * in WorkflowTopology (key: 'agentzero-workflow-reactflow').
 */
import { create } from 'zustand'

interface WorkflowStoreState {
  /** Clear all persisted workflow state. */
  clear: () => void
}

export const useWorkflowStore = create<WorkflowStoreState>()(() => ({
  clear: () => {
    localStorage.removeItem('agentzero-workflow-reactflow')
    localStorage.removeItem('agentzero-workflow-graph')
    localStorage.removeItem('agentzero-workflow')
  },
}))
