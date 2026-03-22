/**
 * Orchestrator hook composing workflow init, save, and sync.
 */
import { useCallback, useRef } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useReactFlow, type Node, type Edge } from '@xyflow/react'
import { topologyApi } from '@/lib/api/topology'
import { workflowsApi } from '@/lib/api/workflows'
import { useDebouncedCallback } from '@/lib/hooks/useDebouncedCallback'
import { useWorkflowInit } from '@/components/dashboard/useWorkflowInit'
import { useWorkflowSync } from '@/components/dashboard/useWorkflowSync'

const STORAGE_KEY = 'agentzero-workflow'
const SAVE_DEBOUNCE_MS = 800

export function useWorkflowPersistence(
  setNodes: React.Dispatch<React.SetStateAction<Node[]>>,
  setEdges: React.Dispatch<React.SetStateAction<Edge[]>>,
) {
  const reactFlow = useReactFlow()
  const isDirtyRef = useRef(false)
  const initializedRef = useRef(false)
  const workflowIdRef = useRef<string | null>(null)
  const lastKnownUpdatedAtRef = useRef<number>(0)

  const refs = { initializedRef, workflowIdRef, lastKnownUpdatedAtRef }

  const { data: topology } = useQuery({
    queryKey: ['topology'],
    queryFn: () => topologyApi.get(),
    refetchInterval: 3_000,
  })

  useWorkflowInit(setNodes, setEdges, topology, refs)
  useWorkflowSync(setNodes, setEdges, topology, refs, isDirtyRef)

  const saveLayout = useCallback(async () => {
    const layout = { nodes: reactFlow.getNodes(), edges: reactFlow.getEdges() }

    if (workflowIdRef.current) {
      try {
        const updated = await workflowsApi.update(workflowIdRef.current, { layout })
        lastKnownUpdatedAtRef.current = updated.updated_at
        isDirtyRef.current = false
      } catch {
        try { localStorage.setItem(STORAGE_KEY, JSON.stringify(layout)) } catch { /* full */ }
      }
    } else {
      try { localStorage.setItem(STORAGE_KEY, JSON.stringify(layout)) } catch { /* full */ }
    }
  }, [reactFlow])

  const debouncedSave = useDebouncedCallback(saveLayout, SAVE_DEBOUNCE_MS)

  const persistState = useCallback(() => {
    isDirtyRef.current = true
    debouncedSave()
  }, [debouncedSave])

  const handleClear = useCallback(() => {
    setNodes([])
    setEdges([])
    localStorage.removeItem(STORAGE_KEY)
    if (workflowIdRef.current) {
      workflowsApi.update(workflowIdRef.current, { layout: { nodes: [], edges: [] } }).catch(() => {})
    }
  }, [setNodes, setEdges])

  return { persistState, handleClear, workflowId: workflowIdRef.current }
}
