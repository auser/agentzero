/**
 * Persisted workflow state — survives page refresh.
 * Stores manually added nodes and edges (not topology-sourced ones).
 */
import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import type { Job } from '@auser/workflow-graph-web'

interface WorkflowEdge {
  fromNodeId: string
  fromPortId: string
  toNodeId: string
  toPortId: string
  metadata?: Record<string, unknown>
}

interface WorkflowState {
  /** Manually added nodes (not from topology API). */
  addedNodes: Job[]
  /** Manually created edges (port-to-port connections). */
  edges: WorkflowEdge[]

  addNode: (node: Job) => void
  removeNode: (id: string) => void
  addEdge: (edge: WorkflowEdge) => void
  removeEdge: (fromNodeId: string, toNodeId: string) => void
  clear: () => void
}

export const useWorkflowStore = create<WorkflowState>()(
  persist(
    (set) => ({
      addedNodes: [],
      edges: [],

      addNode: (node) =>
        set((state) => {
          if (state.addedNodes.some((n) => n.id === node.id)) return state
          return { addedNodes: [...state.addedNodes, node] }
        }),

      removeNode: (id) =>
        set((state) => ({
          addedNodes: state.addedNodes.filter((n) => n.id !== id),
          edges: state.edges.filter(
            (e) => e.fromNodeId !== id && e.toNodeId !== id,
          ),
        })),

      addEdge: (edge) =>
        set((state) => {
          const exists = state.edges.some(
            (e) =>
              e.fromNodeId === edge.fromNodeId &&
              e.fromPortId === edge.fromPortId &&
              e.toNodeId === edge.toNodeId &&
              e.toPortId === edge.toPortId,
          )
          if (exists) return state
          return { edges: [...state.edges, edge] }
        }),

      removeEdge: (fromNodeId, toNodeId) =>
        set((state) => ({
          edges: state.edges.filter(
            (e) => !(e.fromNodeId === fromNodeId && e.toNodeId === toNodeId),
          ),
        })),

      clear: () => set({ addedNodes: [], edges: [] }),
    }),
    { name: 'agentzero-workflow' },
  ),
)
