/**
 * "Run Workflow" button + execution log panel.
 * Starts workflow via POST, then polls GET /v1/workflows/runs/:id for
 * real-time node status updates until completion.
 */
import { useState, useCallback, useRef, useEffect } from 'react'
import { useReactFlow } from '@xyflow/react'
import { api } from '@/lib/api/client'
import { Play, Square, X, Loader2 } from 'lucide-react'

interface StartResponse {
  run_id: string
  workflow_id: string
  status: string
}

interface RunStatus {
  run_id: string
  workflow_id: string
  status: string
  node_statuses: Record<string, string>
  node_outputs: Record<string, string>
  outputs: Record<string, unknown>
  started_at: number
  finished_at: number | null
  error: string | null
}

/** Map workflow node status to canvas display status */
function mapNodeStatus(wfStatus: string): string {
  switch (wfStatus) {
    case 'completed': return 'success'
    case 'running': return 'running'
    case 'failed': return 'failure'
    case 'skipped': return 'stale'
    case 'suspended': return 'queued'
    case 'pending': return 'queued'
    default: return 'queued'
  }
}

/** Single line in the execution log */
interface LogEntry {
  timestamp: number
  node_name: string
  node_id: string
  status: string
  output?: string
}

interface RunWorkflowButtonProps {
  workflowId?: string | null
  disabled?: boolean
}

export function RunWorkflowButton({ workflowId, disabled }: RunWorkflowButtonProps) {
  const reactFlow = useReactFlow()
  const [running, setRunning] = useState(false)
  const [log, setLog] = useState<LogEntry[]>([])
  const [finalOutput, setFinalOutput] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [runId, setRunId] = useState<string | null>(null)
  const [showPanel, setShowPanel] = useState(false)
  const [showInputPrompt, setShowInputPrompt] = useState(false)
  const [userInput, setUserInput] = useState('')
  const logEndRef = useRef<HTMLDivElement>(null)
  // Track which node statuses we've already logged to avoid duplicates
  const seenStatusesRef = useRef<Map<string, string>>(new Map())
  const abortRef = useRef<AbortController | null>(null)

  // Cleanup abort controller on unmount
  useEffect(() => () => { abortRef.current?.abort() }, [])

  const cancelRun = useCallback(async () => {
    abortRef.current?.abort()
    if (runId) {
      try {
        await api.delete(`/v1/workflows/runs/${runId}`)
      } catch { /* best-effort */ }
    }
    setRunning(false)
    appendLog({ time: new Date().toLocaleTimeString(), icon: '⏹', node: 'system', text: 'Run cancelled' })
  }, [runId])

  const appendLog = useCallback((entry: LogEntry) => {
    setLog((prev) => [...prev, entry])
    setTimeout(() => logEndRef.current?.scrollIntoView({ behavior: 'smooth' }), 50)
  }, [])

  // Apply node statuses to the canvas and log new changes
  const applyNodeStatuses = useCallback((
    nodeStatuses: Record<string, string>,
    nodeOutputs: Record<string, string>,
  ) => {
    const seen = seenStatusesRef.current

    // Update canvas node appearances
    reactFlow.setNodes((nodes) =>
      nodes.map((n) => {
        const wfStatus = nodeStatuses[n.id]
        if (!wfStatus) return n
        const displayStatus = mapNodeStatus(wfStatus)
        if ((n.data as Record<string, unknown>).status === displayStatus) return n
        return { ...n, data: { ...n.data, status: displayStatus } }
      }),
    )

    // Log new status changes
    for (const [nodeId, status] of Object.entries(nodeStatuses)) {
      const prevStatus = seen.get(nodeId)
      if (prevStatus === status) continue
      seen.set(nodeId, status)

      // Find the node name from the canvas
      const node = reactFlow.getNode(nodeId)
      const nodeName = (node?.data as Record<string, unknown>)?.name as string ?? nodeId

      appendLog({
        timestamp: Date.now(),
        node_name: nodeName,
        node_id: nodeId,
        status,
        output: status === 'completed' || status === 'failed'
          ? nodeOutputs[nodeId] ?? undefined
          : undefined,
      })
    }
  }, [reactFlow, appendLog])

  const resetNodeStatuses = useCallback(() => {
    reactFlow.setNodes((nodes) =>
      nodes.map((n) => ({
        ...n,
        data: { ...n.data, status: 'queued' },
      })),
    )
  }, [reactFlow])

  const hasHumanInput = useCallback(() => {
    return reactFlow.getNodes().some(
      (n) => (n.data as Record<string, unknown>).nodeType === 'human_input',
    )
  }, [reactFlow])

  const getHumanInputLabel = useCallback(() => {
    const hiNode = reactFlow.getNodes().find(
      (n) => (n.data as Record<string, unknown>).nodeType === 'human_input',
    )
    return (hiNode?.data as Record<string, unknown>)?.name as string ?? 'Input'
  }, [reactFlow])

  const handleRunClick = useCallback(() => {
    if (hasHumanInput()) {
      setShowInputPrompt(true)
      setUserInput('')
    } else {
      void executeWorkflow('Execute workflow')
    }
  }, [hasHumanInput])

  const handleInputSubmit = useCallback(() => {
    if (!userInput.trim()) return
    void executeWorkflow(userInput.trim())
  }, [userInput])

  /** Start execution and poll for updates */
  const executeWorkflow = useCallback(async (message: string) => {
    if (!workflowId) return
    abortRef.current?.abort()
    const abort = new AbortController()
    abortRef.current = abort
    setShowInputPrompt(false)
    setRunning(true)
    setLog([])
    setFinalOutput(null)
    setError(null)
    setRunId(null)
    setShowPanel(true)
    resetNodeStatuses()
    seenStatusesRef.current = new Map()

    try {
      // Start the workflow — returns immediately with a run_id
      const start = await api.post<StartResponse>(
        `/v1/workflows/${workflowId}/execute`,
        { input: { message } },
      )
      setRunId(start.run_id)

      // Poll for status updates every 500ms
      for (let i = 0; i < 600; i++) {
        if (abort.signal.aborted) return
        await new Promise((r) => setTimeout(r, 500))
        if (abort.signal.aborted) return

        try {
          const status = await api.get<RunStatus>(
            `/v1/workflows/runs/${start.run_id}`,
          )

          // Apply node statuses to canvas + log
          if (status.node_statuses) {
            applyNodeStatuses(status.node_statuses, status.node_outputs ?? {})
          }

          if (status.status === 'completed') {
            // Collect final output text
            if (status.outputs) {
              const outputTexts = Object.entries(status.outputs)
                .filter(([key]) =>
                  key.endsWith(':response') ||
                  key.endsWith(':result') ||
                  key.endsWith(':content'),
                )
                .map(([key, val]) => {
                  const nodePart = key.split(':')[0]
                  // Resolve node name from canvas
                  const node = reactFlow.getNode(nodePart)
                  const name = (node?.data as Record<string, unknown>)?.name as string ?? nodePart
                  return `[${name}]\n${typeof val === 'string' ? val : JSON.stringify(val, null, 2)}`
                })
              setFinalOutput(outputTexts.join('\n\n') || 'Workflow completed')
            } else {
              setFinalOutput('Workflow completed')
            }
            setRunning(false)
            return
          }

          if (status.status === 'failed') {
            setError(status.error ?? 'Workflow failed')
            setRunning(false)
            return
          }
        } catch {
          // Network blip — keep polling
        }
      }

      setError('Workflow timed out after 5 minutes')
      setRunning(false)
    } catch (e) {
      setError(e instanceof Error ? e.message : 'Failed to start workflow')
      setRunning(false)
    }
  }, [workflowId, reactFlow, resetNodeStatuses, applyNodeStatuses])

  const closePanel = useCallback(() => {
    setShowPanel(false)
    setLog([])
    setFinalOutput(null)
    setError(null)
    setRunId(null)
  }, [])

  return (
    <>
      {/* Run / Stop button */}
      <div style={{ position: 'absolute', top: 12, right: 12, zIndex: 20, display: 'flex', gap: 6 }}>
        {running && (
          <button
            onClick={() => void cancelRun()}
            style={{
              display: 'flex', alignItems: 'center', gap: 6,
              padding: '8px 16px',
              background: '#ef4444',
              color: '#fff', border: 'none', borderRadius: 8,
              fontSize: 13, fontWeight: 600,
              fontFamily: "'JetBrains Mono', monospace",
              cursor: 'pointer',
              boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
            }}
          >
            <Square className="h-3.5 w-3.5" />
            Stop
          </button>
        )}
        <button
          onClick={handleRunClick}
          disabled={disabled || running}
          data-run-workflow
          style={{
            display: 'flex', alignItems: 'center', gap: 6,
            padding: '8px 16px',
            background: running ? '#1f2937' : '#22c55e',
            color: '#fff', border: 'none', borderRadius: 8,
            fontSize: 13, fontWeight: 600,
            fontFamily: "'JetBrains Mono', monospace",
            cursor: disabled || running ? 'not-allowed' : 'pointer',
            opacity: disabled ? 0.5 : 1,
            boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
          }}
        >
          {running ? <Loader2 className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
          {running ? 'Running...' : 'Run'}
        </button>
      </div>

      {/* Human input prompt */}
      {showInputPrompt && (
        <div
          style={{
            position: 'absolute', inset: 0, zIndex: 50,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            background: 'rgba(0,0,0,0.6)', backdropFilter: 'blur(4px)',
          }}
          onClick={() => setShowInputPrompt(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            style={{
              width: 480, background: '#1C1C1E', borderRadius: 14,
              border: '1px solid rgba(255,255,255,0.06)',
              fontFamily: "'JetBrains Mono', monospace",
              boxShadow: '0 8px 32px rgba(0,0,0,0.5)',
              overflow: 'hidden',
            }}
          >
            <div style={{
              display: 'flex', alignItems: 'center', gap: 8,
              padding: '14px 20px', borderBottom: '1px solid rgba(255,255,255,0.04)',
            }}>
              <span style={{ fontSize: 16 }}>✋</span>
              <span style={{ fontSize: 13, fontWeight: 600, color: '#E5E5E5' }}>
                {getHumanInputLabel()}
              </span>
            </div>
            <div style={{ padding: '16px 20px' }}>
              <textarea
                autoFocus
                placeholder="Enter your input for the workflow..."
                value={userInput}
                onChange={(e) => setUserInput(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                    e.preventDefault()
                    handleInputSubmit()
                  }
                }}
                rows={4}
                style={{
                  width: '100%', background: '#0F0F11', borderRadius: 8,
                  padding: '12px 14px', fontSize: 13, color: '#E5E5E5',
                  border: '1px solid rgba(255,255,255,0.06)', outline: 'none',
                  fontFamily: "'JetBrains Mono', monospace",
                  boxSizing: 'border-box', resize: 'vertical', lineHeight: 1.6,
                }}
              />
              <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 12 }}>
                <button
                  onClick={() => setShowInputPrompt(false)}
                  style={{
                    padding: '8px 16px', background: 'transparent', color: '#737373',
                    border: '1px solid rgba(255,255,255,0.06)', borderRadius: 8,
                    fontSize: 12, fontWeight: 500, cursor: 'pointer',
                    fontFamily: "'JetBrains Mono', monospace",
                  }}
                >
                  Cancel
                </button>
                <button
                  onClick={handleInputSubmit}
                  disabled={!userInput.trim()}
                  style={{
                    padding: '8px 16px', background: '#22c55e', color: '#fff',
                    border: 'none', borderRadius: 8,
                    fontSize: 12, fontWeight: 600,
                    cursor: userInput.trim() ? 'pointer' : 'not-allowed',
                    opacity: userInput.trim() ? 1 : 0.5,
                    fontFamily: "'JetBrains Mono', monospace",
                    display: 'flex', alignItems: 'center', gap: 6,
                  }}
                >
                  <Play className="h-3.5 w-3.5" />
                  Run
                </button>
              </div>
              <div style={{ fontSize: 10, color: '#525252', marginTop: 8, textAlign: 'right' }}>
                Cmd+Enter to run
              </div>
            </div>
          </div>
        </div>
      )}

      {/* Execution log panel */}
      {showPanel && (
        <div
          style={{
            position: 'absolute', bottom: 0, left: 0, right: 0,
            maxHeight: '45%', background: '#1C1C1E',
            borderTop: '1px solid rgba(255,255,255,0.06)',
            fontFamily: "'JetBrains Mono', monospace",
            zIndex: 20, display: 'flex', flexDirection: 'column',
          }}
        >
          {/* Header */}
          <div style={{
            display: 'flex', alignItems: 'center', justifyContent: 'space-between',
            padding: '8px 16px', borderBottom: '1px solid rgba(255,255,255,0.04)',
          }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              {running && <Loader2 className="h-3 w-3 animate-spin" style={{ color: '#22c55e' }} />}
              <span style={{
                fontSize: 11, fontWeight: 600, color: '#737373',
                textTransform: 'uppercase', letterSpacing: '0.05em',
              }}>
                {error ? 'Error' : running ? 'Executing' : 'Output'}
              </span>
              {runId && <span style={{ fontSize: 10, color: '#525252' }}>{runId}</span>}
            </div>
            <button
              onClick={closePanel}
              style={{ background: 'none', border: 'none', cursor: 'pointer', color: '#737373' }}
            >
              <X className="h-3.5 w-3.5" />
            </button>
          </div>

          {/* Log entries */}
          <div style={{
            padding: '8px 16px', fontSize: 12,
            overflowY: 'auto', flex: 1, lineHeight: 1.8,
          }}>
            {log.map((entry, i) => {
              const statusIcon = entry.status === 'running' ? '▶'
                : entry.status === 'completed' ? '✓'
                : entry.status === 'failed' ? '✗'
                : '·'
              const statusColor = entry.status === 'running' ? '#3b82f6'
                : entry.status === 'completed' ? '#22c55e'
                : entry.status === 'failed' ? '#ef4444'
                : '#525252'
              return (
                <div key={i} style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                  <span style={{ color: '#525252', fontSize: 10, minWidth: 52, paddingTop: 2 }}>
                    {new Date(entry.timestamp).toLocaleTimeString([], {
                      hour: '2-digit', minute: '2-digit', second: '2-digit',
                    })}
                  </span>
                  <span style={{ color: statusColor, minWidth: 14 }}>{statusIcon}</span>
                  <span style={{ color: '#A3A3A3' }}>{entry.node_name}</span>
                  {entry.output && (
                    <span style={{
                      color: entry.status === 'failed' ? '#ef4444' : '#525252',
                      overflow: 'hidden', textOverflow: 'ellipsis',
                      whiteSpace: 'nowrap', flex: 1,
                    }}>
                      — {entry.output.slice(0, 120)}{entry.output.length > 120 ? '...' : ''}
                    </span>
                  )}
                </div>
              )
            })}

            {/* Final output */}
            {finalOutput && (
              <div style={{
                marginTop: 12, padding: '10px 12px',
                background: '#0F0F11', borderRadius: 8,
                border: '1px solid rgba(255,255,255,0.04)',
                color: '#E5E5E5', whiteSpace: 'pre-wrap', lineHeight: 1.6,
              }}>
                {finalOutput}
              </div>
            )}

            {/* Error */}
            {error && (
              <div style={{
                marginTop: 12, padding: '10px 12px',
                background: 'rgba(239,68,68,0.08)', borderRadius: 8,
                border: '1px solid rgba(239,68,68,0.2)',
                color: '#ef4444', whiteSpace: 'pre-wrap',
              }}>
                {error}
              </div>
            )}

            <div ref={logEndRef} />
          </div>
        </div>
      )}
    </>
  )
}
