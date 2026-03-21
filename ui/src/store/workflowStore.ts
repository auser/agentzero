/**
 * Persisted workflow state — survives page refresh.
 * Stores manually added nodes, edges, and node positions.
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
  /** Persisted node positions keyed by node ID. */
  nodePositions: Record<string, [number, number]>

  addNode: (node: Job) => void
  removeNode: (id: string) => void
  addEdge: (edge: WorkflowEdge) => void
  removeEdge: (fromNodeId: string, toNodeId: string) => void
  savePositions: (positions: Record<string, [number, number]>) => void
  clear: () => void
}

export const useWorkflowStore = create<WorkflowState>()(
  persist(
    (set) => ({
      addedNodes: [],
      edges: [],
      nodePositions: {},

      addNode: (node) =>
        set((state) => {
          if (state.addedNodes.some((n) => n.id === node.id)) return state
          return { addedNodes: [...state.addedNodes, node] }
        }),

      removeNode: (id) =>
        set((state) => {
          const { [id]: _, ...restPositions } = state.nodePositions
          return {
            addedNodes: state.addedNodes.filter((n) => n.id !== id),
            edges: state.edges.filter(
              (e) => e.fromNodeId !== id && e.toNodeId !== id,
            ),
            nodePositions: restPositions,
          }
        }),

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

      savePositions: (positions) =>
        set((state) => ({
          nodePositions: { ...state.nodePositions, ...positions },
        })),

      clear: () => set({ addedNodes: [], edges: [], nodePositions: {} }),
    }),
    { name: 'agentzero-workflow' },
  ),
)
