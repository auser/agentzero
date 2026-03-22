/**
 * Slide-out detail panel for editing a selected node's fields.
 * Opens from the right side of the canvas when a node is clicked.
 *
 * Design tokens (matching AgentNode):
 * - Background: #1C1C1E, border: rgba(255,255,255,0.06)
 * - Font: JetBrains Mono
 * - Input bg: #0F0F11, border radius: 8px
 * - Labels: #737373, text: #E5E5E5
 * - Width: 320px
 */
import { useState, useEffect, useCallback, useMemo, useRef } from 'react'
import { useReactFlow } from '@xyflow/react'
import { useQuery } from '@tanstack/react-query'
import { getDefinition } from '@/lib/node-definitions'
import { portTypeColor, type Port } from '@/lib/workflow-types'
import { modelsApi } from '@/lib/api/models'
import type { AgentNodeData } from '@/components/workflows/AgentNode'

interface NodeDetailPanelProps {
  nodeId: string | null
  onClose: () => void
}

export function NodeDetailPanel({ nodeId, onClose }: NodeDetailPanelProps) {
  const reactFlow = useReactFlow()

  const panelRef = useRef<HTMLDivElement>(null)

  // Close on Escape
  useEffect(() => {
    if (!nodeId) return
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [nodeId, onClose])

  // Close on click outside panel
  useEffect(() => {
    if (!nodeId) return
    const handler = (e: MouseEvent) => {
      if (panelRef.current && !panelRef.current.contains(e.target as HTMLElement)) {
        onClose()
      }
    }
    // Use setTimeout so the double-click that opened the panel doesn't immediately close it
    const timer = setTimeout(() => {
      window.addEventListener('mousedown', handler)
    }, 100)
    return () => {
      clearTimeout(timer)
      window.removeEventListener('mousedown', handler)
    }
  }, [nodeId, onClose])

  const node = nodeId ? reactFlow.getNode(nodeId) : null
  const nodeData = node?.data as AgentNodeData | undefined
  const def = nodeData ? getDefinition(nodeData.nodeType) : undefined

  // Fetch models for provider/model selects
  const { data: modelsData } = useQuery({
    queryKey: ['models'],
    queryFn: () => modelsApi.list(),
    staleTime: 60_000,
    enabled: !!nodeId,
  })
  const allModels = modelsData?.data ?? []

  const providers = useMemo(() =>
    [...new Set(allModels.map((m) => m.owned_by).filter(Boolean))].sort(),
    [allModels],
  )

  const selectedProvider = (nodeData?.metadata?.provider as string) ?? ''
  const filteredModels = useMemo(() => {
    const models = selectedProvider
      ? allModels.filter((m) => m.owned_by === selectedProvider)
      : allModels
    return [...new Set(models.map((m) => m.id))]
  }, [allModels, selectedProvider])

  const updateField = useCallback((key: string, value: string) => {
    if (!nodeId) return
    reactFlow.setNodes((nodes) =>
      nodes.map((n) =>
        n.id === nodeId
          ? {
              ...n,
              data: {
                ...n.data,
                metadata: { ...(n.data as AgentNodeData).metadata, [key]: value },
              },
            }
          : n,
      ),
    )
  }, [reactFlow, nodeId])

  // Get edges connected to this node
  const connectedEdges = useMemo(() => {
    if (!nodeId) return []
    return reactFlow.getEdges().filter(
      (e) => e.source === nodeId || e.target === nodeId,
    )
  }, [reactFlow, nodeId])

  // Use custom ports from metadata if available, otherwise fall back to definition
  const inputs: Port[] = (nodeData?.metadata?.tool_inputs as Port[]) ?? def?.inputs ?? []
  const outputs: Port[] = (nodeData?.metadata?.tool_outputs as Port[]) ?? def?.outputs ?? []
  const fields = def?.fields ?? []
  const hasAgentId = !!(nodeData?.metadata?.agent_id)
  const isToolNode = nodeData?.nodeType === 'tool'
  const isCustomNode = nodeData?.nodeType === 'human_input'
  const canEditPorts = isToolNode || isCustomNode
  const [editingPorts, setEditingPorts] = useState(false)

  const PORT_TYPES = ['text', 'json', 'number', 'boolean', 'array', 'event', 'config']

  const updatePorts = useCallback((direction: 'input' | 'output', newPorts: Port[]) => {
    if (!nodeId) return
    const metaKey = direction === 'input' ? 'tool_inputs' : 'tool_outputs'
    reactFlow.setNodes((nodes) =>
      nodes.map((n) =>
        n.id === nodeId
          ? {
              ...n,
              data: {
                ...n.data,
                metadata: { ...(n.data as AgentNodeData).metadata, [metaKey]: newPorts },
              },
            }
          : n,
      ),
    )
  }, [reactFlow, nodeId])

  const addPort = useCallback((direction: 'input' | 'output') => {
    const current = direction === 'input' ? inputs : outputs
    const newPort: Port = {
      id: `${direction}_${Date.now()}`,
      label: `new_${direction}`,
      direction,
      port_type: 'text',
    }
    updatePorts(direction, [...current, newPort])
  }, [inputs, outputs, updatePorts])

  const removePort = useCallback((direction: 'input' | 'output', portId: string) => {
    const current = direction === 'input' ? inputs : outputs
    updatePorts(direction, current.filter((p) => p.id !== portId))
  }, [inputs, outputs, updatePorts])

  const updatePort = useCallback((direction: 'input' | 'output', portId: string, updates: Partial<Port>) => {
    const current = direction === 'input' ? inputs : outputs
    updatePorts(direction, current.map((p) =>
      p.id === portId ? { ...p, ...updates } : p,
    ))
  }, [inputs, outputs, updatePorts])

  return (
      <div
        ref={panelRef}
        style={{
          position: 'absolute',
          top: 0,
          right: 0,
          bottom: 0,
          width: 320,
          background: '#1C1C1E',
          borderLeft: '1px solid rgba(255,255,255,0.06)',
          fontFamily: "'JetBrains Mono', monospace",
          zIndex: 50,
          display: 'flex',
          flexDirection: 'column',
          transform: nodeId ? 'translateX(0)' : 'translateX(100%)',
          transition: 'transform 200ms ease-in-out',
          pointerEvents: nodeId ? 'auto' : 'none',
          boxShadow: nodeId ? '-4px 0 16px rgba(0,0,0,0.4)' : 'none',
        }}
      >
      {/* Header */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '14px 16px',
          borderBottom: '1px solid rgba(255,255,255,0.06)',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          {def?.icon && <span style={{ fontSize: 16 }}>{def.icon}</span>}
          <span style={{ fontSize: 14, fontWeight: 600, color: '#E5E5E5' }}>
            {def?.label ?? 'Node'}
          </span>
          {hasAgentId && (
            <span
              style={{
                fontSize: 9,
                color: '#22c55e',
                background: 'rgba(34,197,94,0.1)',
                border: '1px solid rgba(34,197,94,0.2)',
                borderRadius: 4,
                padding: '2px 6px',
                fontWeight: 500,
              }}
            >
              Connected to API
            </span>
          )}
        </div>
        <button
          onClick={onClose}
          style={{
            background: 'none',
            border: 'none',
            color: '#737373',
            cursor: 'pointer',
            fontSize: 18,
            lineHeight: 1,
            padding: '0 2px',
            fontFamily: "'JetBrains Mono', monospace",
          }}
          aria-label="Close panel"
        >
          x
        </button>
      </div>

      {/* Scrollable body */}
      <div style={{ flex: 1, overflowY: 'auto', padding: 16 }}>
        {nodeData && (
          <>
            {/* Node name (read-only display) */}
            <div style={{ marginBottom: 16 }}>
              <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
                Name
              </div>
              <div
                style={{
                  fontSize: 13,
                  fontWeight: 500,
                  color: '#E5E5E5',
                  background: '#0F0F11',
                  borderRadius: 8,
                  padding: '10px 12px',
                }}
              >
                {nodeData.name}
              </div>
            </div>

            {/* Editable fields */}
            {fields.map((field) => {
              const value = (nodeData.metadata?.[field.key] as string) ?? ''

              if (field.type === 'textarea') {
                return (
                  <div key={field.key} style={{ marginBottom: 12 }}>
                    <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
                      {field.label}
                    </div>
                    <textarea
                      placeholder={`Enter ${field.label.toLowerCase()}...`}
                      defaultValue={value}
                      onBlur={(e) => updateField(field.key, e.target.value)}
                      rows={4}
                      style={{
                        width: '100%',
                        background: '#0F0F11',
                        borderRadius: 8,
                        padding: '10px 12px',
                        fontSize: 12,
                        color: '#E5E5E5',
                        border: 'none',
                        outline: 'none',
                        fontFamily: "'JetBrains Mono', monospace",
                        boxSizing: 'border-box',
                        resize: 'vertical',
                        lineHeight: 1.6,
                      }}
                    />
                  </div>
                )
              }

              if (field.type === 'text') {
                return (
                  <div key={field.key} style={{ marginBottom: 12 }}>
                    <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
                      {field.label}
                    </div>
                    <input
                      type="text"
                      placeholder={`Enter ${field.label.toLowerCase()}...`}
                      defaultValue={value}
                      onBlur={(e) => updateField(field.key, e.target.value)}
                      style={{
                        width: '100%',
                        background: '#0F0F11',
                        borderRadius: 8,
                        padding: '10px 12px',
                        fontSize: 12,
                        color: '#E5E5E5',
                        border: 'none',
                        outline: 'none',
                        fontFamily: "'JetBrains Mono', monospace",
                        boxSizing: 'border-box',
                      }}
                    />
                  </div>
                )
              }

              if (field.type === 'select') {
                return (
                  <div key={field.key} style={{ marginBottom: 12 }}>
                    <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
                      {field.label}
                    </div>
                    <select
                      defaultValue={value}
                      onChange={(e) => updateField(field.key, e.target.value)}
                      style={{
                        width: '100%',
                        background: '#0F0F11',
                        borderRadius: 8,
                        padding: '8px 12px',
                        fontSize: 12,
                        color: value ? '#E5E5E5' : '#525252',
                        border: 'none',
                        outline: 'none',
                        fontFamily: "'JetBrains Mono', monospace",
                        appearance: 'none',
                        boxSizing: 'border-box',
                      }}
                    >
                      <option value="">--</option>
                      {(field.key === 'provider'
                        ? providers
                        : field.key === 'model' || field.key === 'model_name'
                          ? filteredModels
                          : (field.options ?? [])
                      ).map((opt) => (
                        <option key={opt} value={opt}>{opt}</option>
                      ))}
                    </select>
                  </div>
                )
              }

              if (field.type === 'badge') {
                return (
                  <div
                    key={field.key}
                    style={{
                      display: 'flex',
                      alignItems: 'center',
                      justifyContent: 'space-between',
                      marginBottom: 12,
                      padding: '8px 0',
                    }}
                  >
                    <span style={{ fontSize: 12, fontWeight: 500, color: '#E5E5E5' }}>
                      {field.label}
                    </span>
                    <span style={{ fontSize: 12, color: '#737373' }}>
                      {value || (field.defaultValue as string) || '0 added'}
                    </span>
                  </div>
                )
              }

              return null
            })}

            {/* Port connections summary + editor */}
            {(inputs.length > 0 || outputs.length > 0 || canEditPorts) && (
              <div style={{ marginTop: 16 }}>
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    marginBottom: 8,
                  }}
                >
                  <div
                    style={{
                      fontSize: 11,
                      fontWeight: 600,
                      color: '#737373',
                      textTransform: 'uppercase',
                      letterSpacing: '0.05em',
                    }}
                  >
                    Ports
                  </div>
                  {canEditPorts && (
                    <button
                      onClick={() => setEditingPorts((e) => !e)}
                      style={{
                        fontSize: 10,
                        color: editingPorts ? '#f59e0b' : '#525252',
                        background: 'none',
                        border: 'none',
                        cursor: 'pointer',
                        fontFamily: "'JetBrains Mono', monospace",
                      }}
                    >
                      {editingPorts ? 'Done' : 'Edit'}
                    </button>
                  )}
                </div>

                {/* Inputs */}
                {inputs.length > 0 && (
                  <div style={{ fontSize: 10, color: '#525252', marginBottom: 4 }}>Inputs</div>
                )}
                {inputs.map((port) => {
                  const connected = connectedEdges.filter(
                    (e) => e.target === nodeId && e.targetHandle === port.id,
                  )
                  return (
                    <div
                      key={`in-${port.id}`}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 6,
                        padding: '4px 0',
                      }}
                    >
                      <div
                        style={{
                          width: 8,
                          height: 8,
                          borderRadius: '50%',
                          background: portTypeColor(port.port_type ?? ''),
                          flexShrink: 0,
                        }}
                      />
                      {editingPorts ? (
                        <>
                          <input
                            type="text"
                            value={port.label}
                            onChange={(e) => updatePort('input', port.id, { id: e.target.value, label: e.target.value })}
                            style={{
                              flex: 1,
                              background: '#0F0F11',
                              borderRadius: 4,
                              padding: '4px 6px',
                              fontSize: 11,
                              color: '#E5E5E5',
                              border: 'none',
                              outline: 'none',
                              fontFamily: "'JetBrains Mono', monospace",
                              minWidth: 0,
                            }}
                          />
                          <select
                            value={port.port_type ?? 'text'}
                            onChange={(e) => updatePort('input', port.id, { port_type: e.target.value })}
                            style={{
                              background: '#0F0F11',
                              borderRadius: 4,
                              padding: '4px 4px',
                              fontSize: 10,
                              color: portTypeColor(port.port_type ?? ''),
                              border: 'none',
                              outline: 'none',
                              fontFamily: "'JetBrains Mono', monospace",
                              appearance: 'none',
                              width: 55,
                            }}
                          >
                            {PORT_TYPES.map((t) => (
                              <option key={t} value={t}>{t}</option>
                            ))}
                          </select>
                          <button
                            onClick={() => removePort('input', port.id)}
                            style={{
                              background: 'none',
                              border: 'none',
                              color: '#ef4444',
                              cursor: 'pointer',
                              fontSize: 14,
                              lineHeight: 1,
                              padding: 0,
                            }}
                          >
                            ×
                          </button>
                        </>
                      ) : (
                        <>
                          <span style={{ fontSize: 11, color: '#A3A3A3', flex: 1 }}>{port.label}</span>
                          <span style={{ fontSize: 9, color: portTypeColor(port.port_type ?? ''), opacity: 0.7 }}>
                            {port.port_type}
                          </span>
                          <span style={{ fontSize: 10, color: connected.length > 0 ? '#22c55e' : '#525252' }}>
                            {connected.length > 0 ? `${connected.length}` : '·'}
                          </span>
                        </>
                      )}
                    </div>
                  )
                })}
                {editingPorts && (
                  <button
                    onClick={() => addPort('input')}
                    style={{
                      fontSize: 10,
                      color: '#3b82f6',
                      background: 'none',
                      border: 'none',
                      cursor: 'pointer',
                      padding: '4px 0',
                      fontFamily: "'JetBrains Mono', monospace",
                    }}
                  >
                    + Add input
                  </button>
                )}

                {/* Outputs */}
                {outputs.length > 0 && (
                  <div style={{ fontSize: 10, color: '#525252', marginTop: 8, marginBottom: 4 }}>Outputs</div>
                )}
                {outputs.map((port) => {
                  const connected = connectedEdges.filter(
                    (e) => e.source === nodeId && e.sourceHandle === port.id,
                  )
                  return (
                    <div
                      key={`out-${port.id}`}
                      style={{
                        display: 'flex',
                        alignItems: 'center',
                        gap: 6,
                        padding: '4px 0',
                      }}
                    >
                      <div
                        style={{
                          width: 8,
                          height: 8,
                          borderRadius: '50%',
                          background: portTypeColor(port.port_type ?? ''),
                          flexShrink: 0,
                        }}
                      />
                      {editingPorts ? (
                        <>
                          <input
                            type="text"
                            value={port.label}
                            onChange={(e) => updatePort('output', port.id, { id: e.target.value, label: e.target.value })}
                            style={{
                              flex: 1,
                              background: '#0F0F11',
                              borderRadius: 4,
                              padding: '4px 6px',
                              fontSize: 11,
                              color: '#E5E5E5',
                              border: 'none',
                              outline: 'none',
                              fontFamily: "'JetBrains Mono', monospace",
                              minWidth: 0,
                            }}
                          />
                          <select
                            value={port.port_type ?? 'text'}
                            onChange={(e) => updatePort('output', port.id, { port_type: e.target.value })}
                            style={{
                              background: '#0F0F11',
                              borderRadius: 4,
                              padding: '4px 4px',
                              fontSize: 10,
                              color: portTypeColor(port.port_type ?? ''),
                              border: 'none',
                              outline: 'none',
                              fontFamily: "'JetBrains Mono', monospace",
                              appearance: 'none',
                              width: 55,
                            }}
                          >
                            {PORT_TYPES.map((t) => (
                              <option key={t} value={t}>{t}</option>
                            ))}
                          </select>
                          <button
                            onClick={() => removePort('output', port.id)}
                            style={{
                              background: 'none',
                              border: 'none',
                              color: '#ef4444',
                              cursor: 'pointer',
                              fontSize: 14,
                              lineHeight: 1,
                              padding: 0,
                            }}
                          >
                            ×
                          </button>
                        </>
                      ) : (
                        <>
                          <span style={{ fontSize: 11, color: '#A3A3A3', flex: 1 }}>{port.label}</span>
                          <span style={{ fontSize: 9, color: portTypeColor(port.port_type ?? ''), opacity: 0.7 }}>
                            {port.port_type}
                          </span>
                          <span style={{ fontSize: 10, color: connected.length > 0 ? '#22c55e' : '#525252' }}>
                            {connected.length > 0 ? `${connected.length}` : '·'}
                          </span>
                        </>
                      )}
                    </div>
                  )
                })}
                {editingPorts && (
                  <button
                    onClick={() => addPort('output')}
                    style={{
                      fontSize: 10,
                      color: '#22c55e',
                      background: 'none',
                      border: 'none',
                      cursor: 'pointer',
                      padding: '4px 0',
                      fontFamily: "'JetBrains Mono', monospace",
                    }}
                  >
                    + Add output
                  </button>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  )
}
