/**
 * In-canvas approval overlay for gate nodes.
 * When a workflow run is paused at a gate node (PausedCheckpoint),
 * shows approve/deny buttons directly on the canvas near the gate node.
 */
import { useCallback, useState } from 'react'
import { Check, X, Loader2 } from 'lucide-react'
import { api } from '@/lib/api/client'

interface PendingGate {
  runId: string
  nodeId: string
  nodeName: string
  position: { x: number; y: number }
}

interface Props {
  gates: PendingGate[]
  onResolved: () => void
}

export function ApprovalOverlay({ gates, onResolved }: Props) {
  const [resolving, setResolving] = useState<string | null>(null)

  const handleDecision = useCallback(
    async (gate: PendingGate, decision: 'approved' | 'denied') => {
      setResolving(gate.nodeId)
      try {
        await api.post(`/v1/workflows/runs/${gate.runId}/resume`, {
          node_id: gate.nodeId,
          decision,
        })
        onResolved()
      } catch (e) {
        console.error('Failed to resume gate', e)
      } finally {
        setResolving(null)
      }
    },
    [onResolved],
  )

  if (gates.length === 0) return null

  return (
    <>
      {gates.map((gate) => (
        <div
          key={gate.nodeId}
          className="absolute z-20 pointer-events-auto"
          style={{
            left: gate.position.x,
            top: gate.position.y - 60,
            transform: 'translateX(-50%)',
          }}
        >
          <div className="bg-card border border-amber-500/50 rounded-lg shadow-lg px-3 py-2 flex items-center gap-2">
            <span className="text-xs font-medium text-amber-500">
              {gate.nodeName || 'Gate'} awaiting approval
            </span>
            {resolving === gate.nodeId ? (
              <Loader2 className="h-3 w-3 animate-spin text-muted-foreground" />
            ) : (
              <>
                <button
                  onClick={() => handleDecision(gate, 'approved')}
                  className="p-1 rounded bg-green-600 hover:bg-green-500 text-white transition-colors"
                  title="Approve"
                >
                  <Check className="h-3 w-3" />
                </button>
                <button
                  onClick={() => handleDecision(gate, 'denied')}
                  className="p-1 rounded bg-red-600 hover:bg-red-500 text-white transition-colors"
                  title="Deny"
                >
                  <X className="h-3 w-3" />
                </button>
              </>
            )}
          </div>
        </div>
      ))}
    </>
  )
}
