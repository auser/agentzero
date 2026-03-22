/**
 * Undo/redo history for workflow canvas operations.
 * Maintains a stack of node+edge snapshots with Cmd+Z / Cmd+Shift+Z support.
 */
import { useCallback, useRef, useEffect } from 'react'
import type { Node, Edge } from '@xyflow/react'

interface Snapshot {
  nodes: Node[]
  edges: Edge[]
}

const MAX_HISTORY = 50

export function useUndoRedo(
  setNodes: React.Dispatch<React.SetStateAction<Node[]>>,
  setEdges: React.Dispatch<React.SetStateAction<Edge[]>>,
) {
  const historyRef = useRef<Snapshot[]>([])
  const indexRef = useRef(-1)
  const isRestoringRef = useRef(false)

  const push = useCallback((nodes: Node[], edges: Edge[]) => {
    if (isRestoringRef.current) return

    // Truncate any future states if we're not at the end
    const history = historyRef.current
    history.splice(indexRef.current + 1)

    // Push new snapshot
    history.push({
      nodes: JSON.parse(JSON.stringify(nodes)),
      edges: JSON.parse(JSON.stringify(edges)),
    })

    // Enforce max size
    if (history.length > MAX_HISTORY) {
      history.shift()
    }

    indexRef.current = history.length - 1
  }, [])

  const undo = useCallback(() => {
    if (indexRef.current <= 0) return
    indexRef.current -= 1
    const snapshot = historyRef.current[indexRef.current]
    if (!snapshot) return

    isRestoringRef.current = true
    setNodes(snapshot.nodes)
    setEdges(snapshot.edges)
    // Use microtask to clear flag after React processes the state update
    queueMicrotask(() => { isRestoringRef.current = false })
  }, [setNodes, setEdges])

  const redo = useCallback(() => {
    if (indexRef.current >= historyRef.current.length - 1) return
    indexRef.current += 1
    const snapshot = historyRef.current[indexRef.current]
    if (!snapshot) return

    isRestoringRef.current = true
    setNodes(snapshot.nodes)
    setEdges(snapshot.edges)
    queueMicrotask(() => { isRestoringRef.current = false })
  }, [setNodes, setEdges])

  const canUndo = useCallback(() => indexRef.current > 0, [])
  const canRedo = useCallback(() => indexRef.current < historyRef.current.length - 1, [])

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (!(e.metaKey || e.ctrlKey) || e.key !== 'z') return
      e.preventDefault()
      if (e.shiftKey) {
        redo()
      } else {
        undo()
      }
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [undo, redo])

  return { push, undo, redo, canUndo, canRedo }
}
