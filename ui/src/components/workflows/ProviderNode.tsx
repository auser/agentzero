/**
 * Provider node — collapsible card showing provider + model selection.
 * Outputs a config port that wires into Agent nodes.
 *
 * Design tokens from Pencil node-designs.pen:
 * - Background: #171717
 * - Border: #ffffff10, 1px
 * - Corner radius: 14px
 * - Font: JetBrains Mono
 */
import { memo, useState, useMemo, useCallback } from 'react'
import { Handle, Position, NodeResizer, useReactFlow, type NodeProps } from '@xyflow/react'
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

/** Known providers — always shown even when the API is unavailable. */
const KNOWN_PROVIDERS = Object.keys(PROVIDER_COLORS)

/** Well-known models per provider for offline/fallback use. */
const KNOWN_MODELS: Record<string, string[]> = {
  anthropic: ['claude-sonnet-4-20250514', 'claude-opus-4-20250514', 'claude-haiku-4-20250506'],
  openai: ['gpt-4o', 'gpt-4o-mini', 'o3', 'o4-mini'],
  ollama: ['llama3', 'mistral', 'codellama', 'deepseek-coder', 'phi3', 'gemma2'],
  google: ['gemini-2.5-pro', 'gemini-2.5-flash'],
  mistral: ['mistral-large-latest', 'mistral-small-latest', 'codestral-latest'],
  groq: ['llama-3.3-70b-versatile', 'mixtral-8x7b-32768'],
  together: ['meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo'],
  fireworks: ['accounts/fireworks/models/llama-v3p1-70b-instruct'],
  nvidia: ['meta/llama-3.1-405b-instruct'],
}

function ProviderNodeComponent({ id, data, selected }: NodeProps) {
  const nodeData = data as unknown as AgentNodeData
  const providerName = (nodeData.metadata?.provider_name as string) ?? ''
  const modelName = (nodeData.metadata?.model_name as string) ?? ''
  const reactFlow = useReactFlow()
  const [collapsed, setCollapsed] = useState(true)

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
    retry: false,
  })
  const allModels = modelsData?.data ?? []

  // Merge API providers with known providers
  const providers = useMemo(() => {
    const apiProviders = allModels.map((m) => m.owned_by).filter(Boolean)
    return [...new Set([...KNOWN_PROVIDERS, ...apiProviders])].sort()
  }, [allModels])

  // Merge API models with known fallback models for the selected provider
  const filteredModels = useMemo(() => {
    const apiModels = providerName
      ? allModels.filter((m) => m.owned_by === providerName).map((m) => m.id)
      : allModels.map((m) => m.id)
    const fallback = providerName ? (KNOWN_MODELS[providerName.toLowerCase()] ?? []) : []
    return [...new Set([...apiModels, ...fallback])]
  }, [allModels, providerName])

  const provColor = PROVIDER_COLORS[providerName.toLowerCase()] ?? '#6b7280'

  const selectStyle = {
    width: '100%',
    background: '#0F0F11',
    borderRadius: 8,
    padding: '8px 12px',
    fontSize: 12,
    border: 'none',
    outline: 'none',
    fontFamily: "'JetBrains Mono', monospace",
    appearance: 'none' as const,
    boxSizing: 'border-box' as const,
  }

  return (
    <>
    <NodeResizer
      isVisible={selected}
      minWidth={160}
      minHeight={60}
      lineStyle={{ borderColor: `${provColor}40` }}
      handleStyle={{ width: 6, height: 6, background: provColor, borderRadius: 2 }}
    />
    <div
      style={{
        minWidth: 160,
        width: '100%',
        height: '100%',
        background: '#171717',
        borderRadius: 14,
        border: selected ? `2px solid ${provColor}` : '1px solid rgba(255,255,255,0.06)',
        fontFamily: "'JetBrains Mono', monospace",
        overflow: 'visible',
        boxShadow: '0 2px 8px rgba(0,0,0,0.3)',
      }}
    >
      {/* Header — click to toggle collapse */}
      <div
        style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '10px 14px', cursor: 'pointer' }}
        onClick={() => setCollapsed((c) => !c)}
      >
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
        <span style={{ fontSize: 10, color: '#525252', transition: 'transform 0.15s', transform: collapsed ? 'rotate(-90deg)' : 'rotate(0deg)' }}>
          &#9660;
        </span>
      </div>

      {/* Summary line when collapsed */}
      {collapsed && modelName && (
        <div style={{ padding: '0 14px 8px', fontSize: 11, color: '#737373' }}>
          {modelName}
        </div>
      )}

      {/* Fields — hidden when collapsed */}
      {!collapsed && (
        <>
          {/* Provider selector */}
          <div style={{ padding: '0 14px 6px' }}>
            <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
              Provider
            </div>
            <select
              className="nodrag nowheel"
              value={providerName}
              onChange={(e) => {
                updateField('provider_name', e.target.value)
                updateField('model_name', '')
              }}
              style={{ ...selectStyle, color: providerName ? '#E5E5E5' : '#525252' }}
            >
              <option value="">-- select --</option>
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
            {filteredModels.length > 0 ? (
              <select
                className="nodrag nowheel"
                value={modelName}
                onChange={(e) => updateField('model_name', e.target.value)}
                style={{ ...selectStyle, color: modelName ? '#E5E5E5' : '#525252' }}
              >
                <option value="">-- select --</option>
                {filteredModels.map((m) => (
                  <option key={m} value={m}>{m}</option>
                ))}
              </select>
            ) : (
              <input
                className="nodrag nowheel"
                type="text"
                placeholder="Enter model name..."
                value={modelName}
                onChange={(e) => updateField('model_name', e.target.value)}
                style={{ ...selectStyle, color: modelName ? '#E5E5E5' : '#525252' }}
              />
            )}
          </div>
        </>
      )}

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
    </>
  )
}

export const ProviderNode = memo(ProviderNodeComponent)
