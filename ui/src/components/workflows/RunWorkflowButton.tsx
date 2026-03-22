/**
 * "Run Workflow" button + output panel for executing workflows from the canvas.
 * Submits to POST /v1/runs and shows streamed output.
 */
import { useState, useCallback } from 'react'
import { useReactFlow } from '@xyflow/react'
import { api } from '@/lib/api/client'
import { Play, X, Loader2 } from 'lucide-react'

interface RunResponse {
  run_id: string
  status: string
  message?: string
  response_text?: string
  node_statuses?: Record<string, string>
  outputs?: Record<string, unknown>
}

/** Map workflow node status to canvas display status */
function mapNodeStatus(wfStatus: string): string {
  switch (wfStatus) {
    case 'completed': return 'success'
    case 'running': return 'running'
    case 'failed': return 'failure'
    case 'skipped': return 'stale'
    case 'suspended': return 'queued'
    default: return 'queued'
  }
}

interface RunWorkflowButtonProps {
  workflowId?: string | null
  disabled?: boolean
}

export function RunWorkflowButton({ workflowId, disabled }: RunWorkflowButtonProps) {
  const reactFlow = useReactFlow()
  const [running, setRunning] = useState(false)
  const [output, setOutput] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [runId, setRunId] = useState<string | null>(null)

  // Update node statuses on the canvas from workflow run response
  const updateNodeStatuses = useCallback((nodeStatuses: Record<string, string>) => {
    reactFlow.setNodes((nodes) =>
      nodes.map((n) => {
        const wfStatus = nodeStatuses[n.id]
        if (!wfStatus) return n
        const displayStatus = mapNodeStatus(wfStatus)
        if ((n.data as Record<string, unknown>).status === displayStatus) return n
        return {
          ...n,
          data: { ...n.data, status: displayStatus },
        }
      }),
    )
  }, [reactFlow])

  // Reset all nodes to queued status
  const resetNodeStatuses = useCallback(() => {
    reactFlow.setNodes((nodes) =>
      nodes.map((n) => ({
        ...n,
        data: { ...n.data, status: 'queued' },
      })),
    )
  }, [reactFlow])

  const handleRun = useCallback(async () => {
    setRunning(true)
    setOutput(null)
    setError(null)
    resetNodeStatuses()

    try {
      if (workflowId) {
        // Use the workflow execution engine
        const result = await api.post<RunResponse>(
          `/v1/workflows/${workflowId}/execute`,
          { input: { message: 'Execute workflow' } },
        )
        setRunId(result.run_id)

        // Apply node statuses from the response
        if (result.node_statuses) {
          updateNodeStatuses(result.node_statuses)
        }

        // Check if execution completed synchronously
        if (result.status === 'completed') {
          setOutput(result.response_text ?? result.message ?? 'Workflow completed')
          setRunning(false)
          return
        }

        // Poll for async completion with live node status updates
        for (let i = 0; i < 120; i++) {
          await new Promise((r) => setTimeout(r, 1000))
          try {
            const status = await api.get<RunResponse>(`/v1/workflows/runs/${result.run_id}`)

            // Update node statuses on every poll
            if (status.node_statuses) {
              updateNodeStatuses(status.node_statuses)
            }

            if (status.status === 'completed' || status.status === 'success') {
              setOutput(status.response_text ?? status.message ?? 'Workflow completed')
              setRunning(false)
              return
            }
            if (status.status === 'failed' || status.status === 'error') {
              setError(status.message ?? 'Workflow failed')
              setRunning(false)
              return
            }
          } catch {
            // Network error — keep polling
          }
        }
        setError('Workflow timed out after 2 minutes')
        setRunning(false)
      } else {
        // Fallback: submit as a plain run with workflow summary
        const nodes = reactFlow.getNodes()
        const agentNames = nodes
          .filter((n) => (n.data as Record<string, unknown>).nodeType === 'agent')
          .map((n) => (n.data as Record<string, unknown>).name)
          .join(', ')

        const result = await api.post<RunResponse>('/v1/runs', {
          message: `Execute workflow with agents: ${agentNames || 'none'}`,
          mode: 'steer',
        })
        setRunId(result.run_id)
        setOutput(result.response_text ?? result.message ?? 'Run submitted')
        setRunning(false)
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to execute workflow')
      setRunning(false)
    }
  }, [reactFlow, workflowId, updateNodeStatuses, resetNodeStatuses])

  return (
    <>
      {/* Run button — positioned top-right of canvas */}
      <div style={{ position: 'absolute', top: 12, right: 12, zIndex: 20 }}>
        <button
          onClick={handleRun}
          disabled={disabled || running}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            padding: '8px 16px',
            background: running ? '#1f2937' : '#22c55e',
            color: '#fff',
            border: 'none',
            borderRadius: 8,
            fontSize: 13,
            fontWeight: 600,
            fontFamily: "'JetBrains Mono', monospace",
            cursor: disabled || running ? 'not-allowed' : 'pointer',
            opacity: disabled ? 0.5 : 1,
            boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
          }}
        >
          {running ? (
            <Loader2 className="h-4 w-4 animate-spin" />
          ) : (
            <Play className="h-4 w-4" />
          )}
          {running ? 'Running...' : 'Run'}
        </button>
      </div>

      {/* Output panel — slides up from bottom */}
      {(output || error) && (
        <div
          style={{
            position: 'absolute',
            bottom: 0,
            left: 0,
            right: 0,
            maxHeight: '40%',
            background: '#1C1C1E',
            borderTop: '1px solid rgba(255,255,255,0.06)',
            fontFamily: "'JetBrains Mono', monospace",
            zIndex: 20,
            display: 'flex',
            flexDirection: 'column',
          }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'space-between',
              padding: '8px 16px',
              borderBottom: '1px solid rgba(255,255,255,0.04)',
            }}
          >
            <span style={{ fontSize: 11, fontWeight: 600, color: '#737373', textTransform: 'uppercase', letterSpacing: '0.05em' }}>
              {error ? 'Error' : 'Output'}
              {runId && <span style={{ color: '#525252', marginLeft: 8 }}>{runId}</span>}
            </span>
            <button
              onClick={() => { setOutput(null); setError(null); setRunId(null) }}
              style={{ background: 'none', border: 'none', cursor: 'pointer', color: '#737373' }}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>
          <div
            style={{
              padding: '12px 16px',
              fontSize: 12,
              color: error ? '#ef4444' : '#E5E5E5',
              overflowY: 'auto',
              flex: 1,
              whiteSpace: 'pre-wrap',
              lineHeight: 1.6,
            }}
          >
            {error ?? output}
          </div>
        </div>
      )}
    </>
  )
}
