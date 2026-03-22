/**
 * Hook handling cross-browser workflow sync via polling,
 * and live status updates from topology data.
 */
import { useRef, useEffect } from 'react'
import type { Node, Edge } from '@xyflow/react'
import { workflowsApi } from '@/lib/api/workflows'
import type { WorkflowRefs } from '@/components/dashboard/useWorkflowInit'

const SYNC_INTERVAL_MS = 5_000

export function useWorkflowSync(
  setNodes: React.Dispatch<React.SetStateAction<Node[]>>,
  setEdges: React.Dispatch<React.SetStateAction<Edge[]>>,
  topology: { nodes: { agent_id: string; status: string }[] } | undefined,
  refs: WorkflowRefs,
  isDirtyRef: React.RefObject<boolean>,
) {
  const { initializedRef, workflowIdRef, lastKnownUpdatedAtRef } = refs
  const isSyncingRef = useRef(false)

  // Live status updates from topology poll
  useEffect(() => {
    if (!initializedRef.current || !topology?.nodes) return
    setNodes((prev) => {
      const statusMap = new Map(topology.nodes.map((n) => [n.agent_id, n.status]))
      return prev.map((node) => {
        const newStatus = statusMap.get(node.id)
        if (newStatus && (node.data as Record<string, unknown>).status !== newStatus) {
          return { ...node, data: { ...node.data, status: newStatus } }
        }
        return node
      })
    })
  }, [topology, setNodes, initializedRef])

  // Cross-browser sync: pull remote changes when not dirty
  useEffect(() => {
    const intervalId = setInterval(async () => {
      if (!workflowIdRef.current || !initializedRef.current || isDirtyRef.current || isSyncingRef.current) return

      try {
        const list = await workflowsApi.list()
        const remote = list.data.find((w) => w.workflow_id === workflowIdRef.current)
        if (!remote || remote.updated_at <= lastKnownUpdatedAtRef.current) return
        if (isDirtyRef.current) return

        isSyncingRef.current = true
        try {
          const full = await workflowsApi.get(workflowIdRef.current!, 'layout')
          if (!isDirtyRef.current && full.layout?.nodes) {
            setNodes(full.layout.nodes as Node[])
            setEdges((full.layout.edges ?? []) as Edge[])
            lastKnownUpdatedAtRef.current = full.updated_at
          }
        } finally {
          isSyncingRef.current = false
        }
      } catch { /* network error — skip */ }
    }, SYNC_INTERVAL_MS)

    return () => clearInterval(intervalId)
  }, [setNodes, setEdges, workflowIdRef, initializedRef, lastKnownUpdatedAtRef, isDirtyRef])
}
