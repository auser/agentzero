/**
 * Custom ReactFlow node component — renders LangFlow-style cards
 * matching the Pencil node-designs.pen design system.
 *
 * Design tokens from Pencil:
 * - Background: #1C1C1E
 * - Border: #ffffff0a, 1px
 * - Corner radius: 14px
 * - Font: JetBrains Mono
 * - Port dots: #7C3AED (purple), 10px
 * - Label color: #737373
 * - Text color: #E5E5E5
 * - Input bg: #0F0F11
 */
import { memo, useState, useCallback, useMemo } from 'react'
import { Handle, Position, NodeResizer, useReactFlow, type NodeProps } from '@xyflow/react'
import { useQuery } from '@tanstack/react-query'
import { getDefinition } from '@/lib/node-definitions'
import { portTypeColor, statusColor } from '@/lib/workflow-types'
import { modelsApi } from '@/lib/api/models'
import { agentsApi } from '@/lib/api/agents'


export interface AgentNodeData {
  name: string
  nodeType: string
  status: string
  metadata: Record<string, unknown>
  [key: string]: unknown
}

/** Node type → icon mapping (matching Pencil designs) */
const NODE_ICONS: Record<string, string> = {
  agent: '🤖',
  tool: '🔧',
  role: '🎭',
  channel: '📡',
  human_input: '✋',
  save_file: '💾',
  read_file: '📄',
  http_request: '🌐',
  schedule: '⏰',
  gate: '🛡️',
  subagent: '🔀',
}

function AgentNodeComponent({ id, data, selected }: NodeProps) {
  const nodeData = data as unknown as AgentNodeData
  const def = getDefinition(nodeData.nodeType)

  const reactFlow = useReactFlow()

  // Fetch models for the provider/model dropdowns
  const { data: modelsData } = useQuery({
    queryKey: ['models'],
    queryFn: () => modelsApi.list(),
    staleTime: 60_000,
  })
  const allModels = modelsData?.data ?? []

  // Derive providers from models
  const providers = useMemo(() =>
    [...new Set(allModels.map((m) => m.owned_by).filter(Boolean))].sort(),
    [allModels],
  )

  // Filter models by selected provider
  const selectedProvider = (nodeData.metadata?.provider as string) ?? ''
  const filteredModels = useMemo(() => {
    const models = selectedProvider
      ? allModels.filter((m) => m.owned_by === selectedProvider)
      : allModels
    return [...new Set(models.map((m) => m.id))]
  }, [allModels, selectedProvider])

  // Update node data and optionally save to API
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
    // Save to agent API if this is an agent node with an agent_id
    const agentId = nodeData.metadata?.agent_id as string
    if (agentId && (key === 'system_prompt' || key === 'model' || key === 'provider')) {
      agentsApi.update(agentId, { [key]: value }).catch(() => {})
    }
  }, [reactFlow, id, nodeData.metadata])
  const icon = NODE_ICONS[nodeData.nodeType] ?? '⚙️'
  const label = def?.label ?? 'Node'
  // Use tool-specific ports from metadata if available (derived from input_schema)
  const inputs = (nodeData.metadata?.tool_inputs as typeof def.inputs) ?? def?.inputs ?? []
  const outputs = (nodeData.metadata?.tool_outputs as typeof def.outputs) ?? def?.outputs ?? []
  const fields = def?.fields ?? []
  const status = nodeData.status ?? 'queued'
  const sColor = statusColor(status)

  // Collapse state — double-click header to toggle
  const [collapsed, setCollapsed] = useState(true)

  // Role-specific state (must be declared unconditionally per Rules of Hooks)
  const roleName = (nodeData.metadata?.role_name as string) ?? ''
  const [roleSearch, setRoleSearch] = useState(roleName)
  const [showDropdown, setShowDropdown] = useState(false)

  // ── Compact card layout for Role nodes ──
  if (nodeData.nodeType === 'role') {
    const PRESET_ROLES: Record<string, string> = {
      Researcher: 'Research topics thoroughly, find reliable sources, summarize findings with citations.',
      Writer: 'Write clear, engaging content. Follow the given style guide and tone.',
      Reviewer: 'Review content for accuracy, clarity, and completeness. Provide constructive feedback.',
      Coder: 'Write clean, tested code. Follow best practices and project conventions.',
      Analyst: 'Analyze data, identify patterns, and provide actionable insights.',
      Planner: 'Break down complex tasks into actionable steps with clear dependencies.',
    }
    const roleDesc = (nodeData.metadata?.role_description as string) ?? ''

    const matchingRoles = Object.keys(PRESET_ROLES).filter((r) =>
      r.toLowerCase().includes(roleSearch.toLowerCase()),
    )

    const selectRole = (name: string) => {
      setRoleSearch(name)
      setShowDropdown(false)
      updateField('role_name', name)
      if (PRESET_ROLES[name]) {
        updateField('role_description', PRESET_ROLES[name])
      }
    }

    return (
      <>
      <NodeResizer
        isVisible={selected}
        minWidth={180}
        minHeight={80}
        lineStyle={{ borderColor: '#a855f740' }}
        handleStyle={{ width: 6, height: 6, background: '#a855f7', borderRadius: 2 }}
      />
      <div
        style={{
          minWidth: 180,
          width: '100%',
          height: '100%',
          background: '#1C1C1E',
          borderRadius: 16,
          border: selected ? '2px solid #a855f7' : '1px solid rgba(255,255,255,0.04)',
          fontFamily: "'JetBrains Mono', monospace",
          padding: 20,
          boxShadow: '0 2px 8px rgba(0,0,0,0.4)',
        }}
      >
        <div style={{ fontSize: 15, fontWeight: 600, color: '#E5E5E5', marginBottom: 12 }}>
          🎭 Role
        </div>

        {/* Searchable role input */}
        <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
          Role
        </div>
        <div style={{ position: 'relative' }}>
          <input
            className="nodrag nowheel"
            type="text"
            placeholder="Search or type a role..."
            value={roleSearch}
            onChange={(e) => {
              setRoleSearch(e.target.value)
              setShowDropdown(true)
            }}
            onFocus={() => setShowDropdown(true)}
            onBlur={() => {
              // Delay to allow click on dropdown item
              setTimeout(() => {
                setShowDropdown(false)
                if (roleSearch) updateField('role_name', roleSearch)
              }, 200)
            }}
            style={{
              width: '100%',
              background: '#0F0F11',
              borderRadius: 8,
              padding: '8px 12px',
              fontSize: 13,
              fontWeight: 500,
              color: '#E5E5E5',
              border: 'none',
              outline: 'none',
              fontFamily: "'JetBrains Mono', monospace",
              boxSizing: 'border-box',
            }}
          />
          {showDropdown && matchingRoles.length > 0 && (
            <div
              className="nowheel"
              style={{
                position: 'absolute',
                top: '100%',
                left: 0,
                right: 0,
                background: '#0F0F11',
                borderRadius: '0 0 8px 8px',
                border: '1px solid rgba(255,255,255,0.06)',
                borderTop: 'none',
                maxHeight: 150,
                overflowY: 'auto',
                zIndex: 10,
              }}
            >
              {matchingRoles.map((r) => (
                <div
                  key={r}
                  onMouseDown={() => selectRole(r)}
                  style={{
                    padding: '6px 12px',
                    fontSize: 12,
                    color: '#A3A3A3',
                    cursor: 'pointer',
                  }}
                  onMouseEnter={(e) => { (e.target as HTMLDivElement).style.background = 'rgba(255,255,255,0.05)' }}
                  onMouseLeave={(e) => { (e.target as HTMLDivElement).style.background = 'transparent' }}
                >
                  {r}
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Instructions (auto-filled from preset or user-editable) */}
        <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4, marginTop: 10 }}>
          Instructions
        </div>
        <textarea
          className="nodrag nowheel"
          placeholder="Describe this role's behavior..."
          value={roleDesc}
          onChange={(e) => updateField('role_description', (e.target as HTMLTextAreaElement).value)}
          rows={3}
          style={{
            width: '100%',
            background: '#0F0F11',
            borderRadius: 8,
            padding: '10px 12px',
            fontSize: 11,
            color: '#A3A3A3',
            border: 'none',
            outline: 'none',
            fontFamily: "'JetBrains Mono', monospace",
            boxSizing: 'border-box',
            resize: 'vertical',
            lineHeight: 1.6,
          }}
        />

        {/* Output port */}
        <Handle
          type="source"
          position={Position.Right}
          id="role_config"
          style={{
            width: 14,
            height: 14,
            background: '#a855f7',
            border: '2px solid #1C1C1E',
            right: 4,
            top: 30,
          }}
        />
      </div>
      </>
    )
  }

  // ── Full agent/tool/channel node layout ──
  return (
    <>
    <NodeResizer
      isVisible={selected}
      minWidth={180}
      minHeight={80}
      lineStyle={{ borderColor: `${sColor}40` }}
      handleStyle={{ width: 6, height: 6, background: sColor, borderRadius: 2 }}
    />
    <div
      style={{
        minWidth: 180,
        width: '100%',
        height: '100%',
        background: '#1C1C1E',
        borderRadius: 14,
        border: status === 'running'
          ? `2px solid ${sColor}`
          : status === 'success'
            ? `2px solid ${sColor}80`
            : selected
              ? `2px solid ${sColor}`
              : '1px solid rgba(255,255,255,0.04)',
        fontFamily: "'JetBrains Mono', monospace",
        overflow: 'visible',
        userSelect: 'none',
        boxShadow: status === 'running'
          ? `0 0 20px ${sColor}90, 0 0 40px ${sColor}50, 0 0 60px ${sColor}30`
          : status === 'success'
            ? `0 0 12px ${sColor}30`
            : status === 'failure'
              ? `0 0 12px ${sColor}30`
              : '0 2px 8px rgba(0,0,0,0.4)',
        animation: status === 'running' ? 'nodeRunningPulse 1.5s ease-in-out infinite' : 'none',
        transition: 'border 0.3s ease, box-shadow 0.3s ease',
      }}
    >
      {/* Header — click to toggle collapse */}
      <div
        style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '12px 16px', cursor: 'pointer' }}
        onClick={() => setCollapsed((c) => !c)}
      >
        <span style={{ fontSize: 16 }}>{icon}</span>
        <span style={{ fontSize: 14, fontWeight: 500, color: '#E5E5E5', flex: 1 }}>
          {label}
        </span>
        {/* Status dot */}
        <div
          style={{
            width: 10,
            height: 10,
            borderRadius: '50%',
            background: sColor,
            boxShadow: status === 'running'
              ? `0 0 8px ${sColor}, 0 0 16px ${sColor}80`
              : status === 'success' || status === 'failure'
                ? `0 0 6px ${sColor}80`
                : 'none',
          }}
        />
        {/* Chevron */}
        <span style={{ fontSize: 10, color: '#525252', transition: 'transform 0.15s', transform: collapsed ? 'rotate(-90deg)' : 'rotate(0deg)' }}>
          ▼
        </span>
      </div>

      {/* Node name */}
      <div style={{ padding: '0 16px 8px', fontSize: 13, fontWeight: 500, color: '#A3A3A3' }}>
        {nodeData.name}
      </div>

      {/* Fields — hidden when collapsed */}
      {!collapsed && fields.map((field) => {
        const value = (nodeData.metadata?.[field.key] as string) ?? ''

        if (field.type === 'textarea' || field.type === 'text') {
          return (
            <div key={field.key} style={{ padding: '0 16px 6px' }}>
              <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
                {field.label}
              </div>
              <input
                className="nodrag nowheel"
                type="text"
                placeholder={`Enter ${field.label.toLowerCase()}...`}
                value={value}
                onChange={(e) => updateField(field.key, e.target.value)}
                style={{
                  width: '100%',
                  background: '#0F0F11',
                  borderRadius: 8,
                  padding: '10px 12px',
                  fontSize: 10,
                  color: '#A3A3A3',
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
            <div key={field.key} style={{ padding: '4px 16px 6px' }}>
              <div style={{ fontSize: 11, fontWeight: 500, color: '#737373', marginBottom: 4 }}>
                {field.label}
              </div>
              <select
                className="nodrag nowheel"
                value={value}
                onChange={(e) => {
                  updateField(field.key, e.target.value)
                  // When provider changes, clear the model selection
                  if (field.key === 'provider') {
                    updateField('model', '')
                  }
                }}
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
                <option value="">—</option>
                {(field.key === 'provider'
                  ? providers
                  : field.key === 'model'
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
                padding: '8px 16px',
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

      {/* Ports — each row has inline handles so they stay aligned */}
      {(inputs.length > 0 || outputs.length > 0) && (
        <div style={{
          padding: collapsed ? '4px 0' : '6px 0 8px',
          borderTop: '1px solid rgba(255,255,255,0.04)',
        }}>
          {Array.from({ length: Math.max(inputs.length, outputs.length) }).map((_, i) => {
            const inp = inputs[i]
            const out = outputs[i]
            return (
              <div
                key={i}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  padding: collapsed ? '2px 6px' : '3px 16px',
                  position: 'relative',
                  minHeight: collapsed ? 16 : 22,
                }}
              >
                {/* Input handle + label */}
                {inp ? (
                  <>
                    <Handle
                      type="target"
                      position={Position.Left}
                      id={inp.id}
                      style={{
                        width: 12,
                        height: 12,
                        background: portTypeColor(inp.port_type ?? ''),
                        border: '2px solid #1C1C1E',
                        left: -6,
                        top: '50%',
                        transform: 'translateY(-50%)',
                        position: 'absolute',
                      }}
                    />
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4, paddingLeft: 4 }}>
                      <span style={{ fontSize: collapsed ? 9 : 11, color: portTypeColor(inp.port_type ?? '') }}>
                        {inp.label}
                      </span>
                      {!collapsed && (
                        <span style={{
                          fontSize: 8,
                          color: '#525252',
                          background: 'rgba(255,255,255,0.04)',
                          borderRadius: 3,
                          padding: '1px 4px',
                        }}>
                          {inp.port_type}
                        </span>
                      )}
                    </div>
                  </>
                ) : <div />}

                {/* Output handle + label */}
                {out ? (
                  <>
                    <Handle
                      type="source"
                      position={Position.Right}
                      id={out.id}
                      style={{
                        width: 12,
                        height: 12,
                        background: portTypeColor(out.port_type ?? ''),
                        border: '2px solid #1C1C1E',
                        right: -6,
                        top: '50%',
                        transform: 'translateY(-50%)',
                        position: 'absolute',
                      }}
                    />
                    <div style={{ display: 'flex', alignItems: 'center', gap: 4, flexDirection: 'row-reverse', marginLeft: 'auto', paddingRight: 4 }}>
                      <span style={{ fontSize: collapsed ? 9 : 11, color: portTypeColor(out.port_type ?? '') }}>
                        {out.label}
                      </span>
                      {!collapsed && (
                        <span style={{
                          fontSize: 8,
                          color: '#525252',
                          background: 'rgba(255,255,255,0.04)',
                          borderRadius: 3,
                          padding: '1px 4px',
                        }}>
                          {out.port_type}
                        </span>
                      )}
                    </div>
                  </>
                ) : <div />}
              </div>
            )
          })}
        </div>
      )}
    </div>
    </>
  )
}

export const AgentNode = memo(AgentNodeComponent)
