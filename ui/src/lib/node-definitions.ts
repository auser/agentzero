/**
 * Declarative node type definitions for the workflow builder.
 * Single source of truth for node types, fields, ports, and appearance.
 * Used by: WorkflowTopology (ReactFlow), DraggablePalette, CommandPalette.
 */
import type { NodeDefinition, Port } from '@/lib/workflow-types'

// ── Agent ────────────────────────────────────────────────────────────────────

const AGENT_DEFINITION: NodeDefinition = {
  type: 'agent',
  label: 'Agent',
  icon: '🤖',
  headerColor: '#3b82f6',
  category: 'core',
  fields: [
    { key: 'system_prompt', type: 'textarea', label: 'Prompt' },
    { key: 'provider', type: 'select', label: 'Provider' },
    { key: 'model', type: 'select', label: 'Model' },
    { key: 'tools_count', type: 'badge', label: 'Tools', defaultValue: '0 added' },
  ],
  inputs: [
    { id: 'input', label: 'input', direction: 'input', port_type: 'text' },
    { id: 'context', label: 'context', direction: 'input', port_type: 'json' },
    { id: 'tools', label: 'tools', direction: 'input', port_type: 'tool' },
    { id: 'role', label: 'role', direction: 'input', port_type: 'role' },
  ],
  outputs: [
    { id: 'response', label: 'response', direction: 'output', port_type: 'text' },
    { id: 'tool_calls', label: 'tool calls', direction: 'output', port_type: 'tool' },
    { id: 'events', label: 'events', direction: 'output', port_type: 'event' },
  ],
}

// ── Tool ─────────────────────────────────────────────────────────────────────

const TOOL_DEFINITION: NodeDefinition = {
  type: 'tool',
  label: 'Tool',
  icon: '🔧',
  headerColor: '#8b5cf6',
  category: 'core',
  fields: [
    { key: 'tool_name', type: 'select', label: 'Tool' },
  ],
  inputs: [
    { id: 'input', label: 'input', direction: 'input', port_type: 'json' },
    { id: 'config', label: 'config', direction: 'input', port_type: 'config' },
  ],
  outputs: [
    { id: 'result', label: 'result', direction: 'output', port_type: 'json' },
  ],
}

// ── Channel ──────────────────────────────────────────────────────────────────

const CHANNEL_DEFINITION: NodeDefinition = {
  type: 'channel',
  label: 'Channel',
  icon: '📡',
  headerColor: '#ec4899',
  category: 'integration',
  fields: [
    { key: 'channel_type', type: 'select', label: 'Type', options: ['telegram', 'discord', 'slack', 'email', 'webhook', 'chat'] },
  ],
  inputs: [
    { id: 'send', label: 'send', direction: 'input', port_type: 'text' },
  ],
  outputs: [
    { id: 'trigger', label: 'trigger', direction: 'output', port_type: 'event' },
    { id: 'message', label: 'message', direction: 'output', port_type: 'text' },
  ],
}

// ── Human Input ─────────────────────────────────────────────────────────────

const HUMAN_INPUT_DEFINITION: NodeDefinition = {
  type: 'human_input',
  label: 'Human Input',
  icon: '✋',
  headerColor: '#f59e0b',
  category: 'integration',
  fields: [
    { key: 'prompt', type: 'textarea', label: 'Prompt' },
    { key: 'channel', type: 'select', label: 'Channel', options: ['chat', 'telegram', 'discord', 'slack', 'email'] },
    { key: 'timeout', type: 'text', label: 'Timeout' },
  ],
  inputs: [
    { id: 'request', label: 'request', direction: 'input', port_type: 'text' },
    { id: 'context', label: 'context', direction: 'input', port_type: 'json' },
  ],
  outputs: [
    { id: 'response', label: 'response', direction: 'output', port_type: 'text' },
    { id: 'timed_out', label: 'timed out', direction: 'output', port_type: 'event' },
  ],
}

// ── Schedule ─────────────────────────────────────────────────────────────────

const SCHEDULE_DEFINITION: NodeDefinition = {
  type: 'schedule',
  label: 'Schedule',
  icon: '⏰',
  headerColor: '#eab308',
  category: 'trigger',
  fields: [
    { key: 'cron', type: 'text', label: 'Cron' },
  ],
  inputs: [],
  outputs: [
    { id: 'trigger', label: 'trigger', direction: 'output', port_type: 'event' },
  ],
}

// ── Gate ──────────────────────────────────────────────────────────────────────

const GATE_DEFINITION: NodeDefinition = {
  type: 'gate',
  label: 'Approval',
  icon: '🛡️',
  headerColor: '#ef4444',
  category: 'control',
  fields: [],
  inputs: [
    { id: 'request', label: 'request', direction: 'input', port_type: 'json' },
  ],
  outputs: [
    { id: 'approved', label: 'approved', direction: 'output', port_type: 'event' },
    { id: 'denied', label: 'denied', direction: 'output', port_type: 'event' },
  ],
}

// ── Sub-Agent ────────────────────────────────────────────────────────────────

const SUBAGENT_DEFINITION: NodeDefinition = {
  type: 'subagent',
  label: 'Sub-Agent',
  icon: '🔀',
  headerColor: '#22c55e',
  category: 'core',
  fields: [],
  inputs: [
    { id: 'task', label: 'task', direction: 'input', port_type: 'text' },
    { id: 'context', label: 'context', direction: 'input', port_type: 'json' },
  ],
  outputs: [
    { id: 'result', label: 'result', direction: 'output', port_type: 'text' },
    { id: 'status', label: 'status', direction: 'output', port_type: 'event' },
  ],
}

// ── Provider ─────────────────────────────────────────────────────────────────

const PROVIDER_DEFINITION: NodeDefinition = {
  type: 'provider',
  label: 'Provider',
  icon: '⚡',
  headerColor: '#171717',
  category: 'config',
  fields: [
    { key: 'provider_name', type: 'select', label: 'Provider' },
    { key: 'model_name', type: 'select', label: 'Model' },
  ],
  inputs: [],
  outputs: [
    { id: 'provider_config', label: 'provider', direction: 'output', port_type: 'config' },
  ],
}

// ── Role ─────────────────────────────────────────────────────────────────────

const ROLE_DEFINITION: NodeDefinition = {
  type: 'role',
  label: 'Role',
  icon: '🎭',
  headerColor: '#a855f7',
  category: 'core',
  fields: [
    { key: 'role_name', type: 'text', label: 'Name' },
    { key: 'role_description', type: 'textarea', label: 'Description' },
    { key: 'role_instructions', type: 'textarea', label: 'Instructions' },
  ],
  inputs: [],
  outputs: [
    { id: 'role_config', label: 'role', direction: 'output', port_type: 'role' },
  ],
}

// ── Registry ─────────────────────────────────────────────────────────────────

/** All built-in node type definitions. */
export const ALL_NODE_DEFINITIONS: NodeDefinition[] = [
  AGENT_DEFINITION,
  TOOL_DEFINITION,
  CHANNEL_DEFINITION,
  HUMAN_INPUT_DEFINITION,
  SCHEDULE_DEFINITION,
  GATE_DEFINITION,
  SUBAGENT_DEFINITION,
  ROLE_DEFINITION,
  PROVIDER_DEFINITION,
]

const DEFINITIONS_MAP = new Map<string, NodeDefinition>(
  ALL_NODE_DEFINITIONS.map((d) => [d.type, d]),
)

/** Look up a node definition by type key. */
export function getDefinition(type: string): NodeDefinition | undefined {
  return DEFINITIONS_MAP.get(type)
}

/** Get ports for a node type from its definition. */
export function portsForType(type: string): Port[] {
  const def = DEFINITIONS_MAP.get(type)
  if (!def) return []
  return [...(def.inputs ?? []), ...(def.outputs ?? [])]
}

export { AGENT_DEFINITION, TOOL_DEFINITION, CHANNEL_DEFINITION, HUMAN_INPUT_DEFINITION, SCHEDULE_DEFINITION, GATE_DEFINITION, SUBAGENT_DEFINITION, ROLE_DEFINITION, PROVIDER_DEFINITION }
