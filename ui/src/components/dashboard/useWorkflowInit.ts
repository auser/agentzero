/**
 * Hook handling workflow initialization:
 * API → localStorage → topology fallback chain.
 */
import { useEffect } from 'react'
import type { Node, Edge } from '@xyflow/react'
import { workflowsApi } from '@/lib/api/workflows'
import { topologyToReactFlow } from '@/components/workflows/WorkflowCanvas'

const DEFAULT_WORKFLOW_NAME = 'default'
const STORAGE_KEY = 'agentzero-workflow'
const OLD_STORAGE_KEY = 'agentzero-workflow-reactflow'

// Migrate old localStorage key
if (typeof window !== 'undefined') {
  const old = localStorage.getItem(OLD_STORAGE_KEY)
  if (old && !localStorage.getItem(STORAGE_KEY)) {
    localStorage.setItem(STORAGE_KEY, old)
    localStorage.removeItem(OLD_STORAGE_KEY)
  }
}

export interface WorkflowRefs {
  initializedRef: React.RefObject<boolean>
  workflowIdRef: React.MutableRefObject<string | null>
  lastKnownUpdatedAtRef: React.MutableRefObject<number>
}

export function useWorkflowInit(
  setNodes: React.Dispatch<React.SetStateAction<Node[]>>,
  setEdges: React.Dispatch<React.SetStateAction<Edge[]>>,
  topology: { nodes: { agent_id: string; status: string }[]; edges?: unknown[] } | undefined,
  refs: WorkflowRefs,
) {
  const { initializedRef, workflowIdRef, lastKnownUpdatedAtRef } = refs

  useEffect(() => {
    if (initializedRef.current) return

    const applyTopology = () => {
      if (!topology?.nodes || topology.nodes.length === 0) return false
      const { nodes: rfNodes, edges: rfEdges } = topologyToReactFlow(
        topology.nodes, topology.edges ?? [],
      )
      setNodes(rfNodes)
      setEdges(rfEdges)
      return { rfNodes, rfEdges }
    }

    const init = async () => {
      try {
        const list = await workflowsApi.list('layout')
        const existing = list.data.find((w) => w.name === DEFAULT_WORKFLOW_NAME) ?? list.data[0]

        if (existing?.layout?.nodes && (existing.layout.nodes as unknown[]).length > 0) {
          setNodes(existing.layout.nodes as Node[])
          setEdges((existing.layout.edges ?? []) as Edge[])
          workflowIdRef.current = existing.workflow_id
          lastKnownUpdatedAtRef.current = existing.updated_at
          initializedRef.current = true
          return
        }

        const result = applyTopology()
        if (result) {
          try {
            const created = await workflowsApi.create({
              name: DEFAULT_WORKFLOW_NAME,
              description: 'Default workflow layout',
              layout: { nodes: result.rfNodes, edges: result.rfEdges },
            })
            workflowIdRef.current = created.workflow_id
            lastKnownUpdatedAtRef.current = created.updated_at
          } catch { /* server may not be running */ }
          initializedRef.current = true
        }
      } catch {
        // API not available — try localStorage
        try {
          const raw = localStorage.getItem(STORAGE_KEY)
          if (raw) {
            const saved = JSON.parse(raw)
            if (saved?.nodes?.length > 0) {
              setNodes(saved.nodes)
              setEdges(saved.edges ?? [])
              initializedRef.current = true
              return
            }
          }
        } catch { /* ignore */ }

        if (applyTopology()) {
          initializedRef.current = true
        }
      }
    }

    init()
  }, [topology, setNodes, setEdges, initializedRef, workflowIdRef, lastKnownUpdatedAtRef])
}
