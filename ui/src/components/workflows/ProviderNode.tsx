/**
 * Provider node — compact chip-style node matching Pencil Provider Chip design.
 * Shows provider name, model selector, and a config output port.
 *
 * Design tokens from Pencil node-designs.pen:
 * - Background: #171717
 * - Border: #ffffff10, 1px
 * - Corner radius: 8px (chip) / 14px (expanded)
 * - Status dot: 16px green ellipse
 * - Font: JetBrains Mono
 * - Output dot: #3B82F6 (blue), 8px
 */
import { memo, useMemo, useCallback } from 'react'
import { Handle, Position, useReactFlow, type NodeProps } from '@xyflow/react'
import { useQuery } from '@tanstack/react-query'
import { modelsApi } from '@/lib/api/models'
import type { AgentNodeData } from '@/components/workflows/AgentNode'

/** Provider brand colors */
const PROVIDER_COLORS: Record<string, string> = {
  anthropic: '#D97706',
  openai: '#10A37F',
  nvidia: '#76B900',
  ollama: '#FFFFFF',
  google: '#4285F4',
  mistral: '#F97316',
  groq: '#F97316',
  together: '#6366F1',
  fireworks: '#EF4444',
}

function ProviderNodeComponent({ id, data, selected }: NodeProps) {
  const nodeData = data as unknown as AgentNodeData
  const providerName = (nodeData.metadata?.provider_name as string) ?? ''
  const modelName = (nodeData.metadata?.model_name as string) ?? ''
  const reactFlow = useReactFlow()

  const updateField = useCallback((key: string, value: string) => {
    reactFlow.setNodes((nodes) =>
      nodes.map((n) =>
        n.id === id
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
  }, [reactFlow, id])

  const { data: modelsData } = useQuery({
    queryKey: ['models'],
    queryFn: () => modelsApi.list(),
    staleTime: 60_000,
  })
  const allModels = modelsData?.data ?? []

  const providers = useMemo(() =>
    [...new Set(allModels.map((m) => m.owned_by).filter(Boolean))].sort(),
    [allModels],
  )

  const filteredModels = useMemo(() => {
    const models = providerName
      ? allModels.filter((m) => m.owned_by === providerName)
      : allModels
    return [...new Set(models.map((m) => m.id))]
  }, [allModels, providerName])

  const provColor = PROVIDER_COLORS[providerName.toLowerCase()] ?? '#6b7280'

  return (
    <div
      style={{
        width: 220,
        background: '#171717',
        borderRadius: 14,
        border: selected ? `2px solid ${provColor}` : '1px solid rgba(255,255,255,0.06)',
        fontFamily: "'JetBrains Mono', monospace",
        overflow: 'hidden',
        boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
      }}
    >
      {/* Header */}
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '10px 14px' }}>
        {/* Provider status dot */}
        <div
          style={{
            width: 12,
            height: 12,
            borderRadius: '50%',
            background: provColor,
            flexShrink: 0,
          }}
        />
        <span style={{ fontSize: 13, fontWeight: 500, color: '#E5E5E5', flex: 1 }}>
          {providerName || 'Provider'}
        </span>
        <span style={{ fontSize: 13, color: '#525252' }}>⋮</span>
      </div>

      {/* Provider selector */}
      <div style={{ padding: '0 14px 6px' }}>
        <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
          Provider
        </div>
        <select
          className="nodrag nowheel"
          defaultValue={providerName}
          onChange={(e) => {
            updateField('provider_name', e.target.value)
            updateField('model_name', '') // reset model when provider changes
          }}
          style={{
            width: '100%',
            background: '#0F0F11',
            borderRadius: 8,
            padding: '8px 12px',
            fontSize: 12,
            color: providerName ? '#E5E5E5' : '#525252',
            border: 'none',
            outline: 'none',
            fontFamily: "'JetBrains Mono', monospace",
            appearance: 'none',
            boxSizing: 'border-box',
          }}
        >
          <option value="">— select —</option>
          {providers.map((p) => (
            <option key={p} value={p}>{p}</option>
          ))}
        </select>
      </div>

      {/* Model selector */}
      <div style={{ padding: '0 14px 10px' }}>
        <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
          Model
        </div>
        <select
          className="nodrag nowheel"
          defaultValue={modelName}
          onChange={(e) => updateField('model_name', e.target.value)}
          style={{
            width: '100%',
            background: '#0F0F11',
            borderRadius: 8,
            padding: '8px 12px',
            fontSize: 12,
            color: modelName ? '#E5E5E5' : '#525252',
            border: 'none',
            outline: 'none',
            fontFamily: "'JetBrains Mono', monospace",
            appearance: 'none',
            boxSizing: 'border-box',
          }}
        >
          <option value="">— select —</option>
          {filteredModels.map((m) => (
            <option key={m} value={m}>{m}</option>
          ))}
        </select>
      </div>

      {/* Output port */}
      <Handle
        type="source"
        position={Position.Right}
        id="provider_config"
        style={{
          width: 14,
          height: 14,
          background: provColor,
          border: '2px solid #171717',
          top: 24,
        }}
      />
    </div>
  )
}

export const ProviderNode = memo(ProviderNodeComponent)
