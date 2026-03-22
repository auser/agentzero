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
}

interface RunWorkflowButtonProps {
  disabled?: boolean
}

export function RunWorkflowButton({ disabled }: RunWorkflowButtonProps) {
  const reactFlow = useReactFlow()
  const [running, setRunning] = useState(false)
  const [output, setOutput] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [runId, setRunId] = useState<string | null>(null)

  const handleRun = useCallback(async () => {
    setRunning(true)
    setOutput(null)
    setError(null)

    const nodes = reactFlow.getNodes()
    const edges = reactFlow.getEdges()

    // Build a summary message describing the workflow for the agent
    const agentNodes = nodes.filter((n) =>
      (n.data as Record<string, unknown>).nodeType === 'agent',
    )
    const channelNodes = nodes.filter((n) =>
      (n.data as Record<string, unknown>).nodeType === 'channel',
    )

    const summary = [
      `Execute workflow with ${nodes.length} nodes and ${edges.length} connections.`,
      agentNodes.length > 0
        ? `Agents: ${agentNodes.map((n) => (n.data as Record<string, unknown>).name).join(', ')}`
        : null,
      channelNodes.length > 0
        ? `Channels: ${channelNodes.map((n) => (n.data as Record<string, unknown>).name).join(', ')}`
        : null,
    ]
      .filter(Boolean)
      .join(' ')

    try {
      const result = await api.post<RunResponse>('/v1/runs', {
        message: summary,
        mode: 'steer',
      })
      setRunId(result.run_id)

      // Poll for completion
      const poll = async () => {
        for (let i = 0; i < 60; i++) {
          await new Promise((r) => setTimeout(r, 2000))
          try {
            const status = await api.get<RunResponse>(`/v1/runs/${result.run_id}`)
            if (status.status === 'completed' || status.status === 'success') {
              setOutput(status.response_text ?? status.message ?? 'Completed')
              setRunning(false)
              return
            }
            if (status.status === 'failed' || status.status === 'error') {
              setError(status.message ?? 'Run failed')
              setRunning(false)
              return
            }
          } catch {
            // Network error — keep polling
          }
        }
        setError('Run timed out after 2 minutes')
        setRunning(false)
      }
      poll()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to submit run')
      setRunning(false)
    }
  }, [reactFlow])

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
