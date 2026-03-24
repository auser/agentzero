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
    { key: 'tools_count', type: 'badge', label: 'Tools', defaultValue: '0 added' },
  ],
  inputs: [
    { id: 'input', label: 'input', direction: 'input', port_type: 'text' },
    { id: 'context', label: 'context', direction: 'input', port_type: 'json' },
    { id: 'tools', label: 'tools', direction: 'input', port_type: 'tool' },
    { id: 'role', label: 'role', direction: 'input', port_type: 'role' },
    { id: 'provider', label: 'provider', direction: 'input', port_type: 'config' },
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
  fields: [],
  inputs: [
    { id: 'input', label: 'input', direction: 'input', port_type: 'text' },
  ],
  outputs: [
    { id: 'result', label: 'result', direction: 'output', port_type: 'text' },
  ],
}

// ── Channel ──────────────────────────────────────────────────────────────────

const CHANNEL_DEFINITION: NodeDefinition = {
  type: 'channel',
  label: 'Channel',
  icon: '📡',
  headerColor: '#ec4899',
  category: 'integration',
  fields: [],
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
  fields: [],
  inputs: [
    { id: 'input', label: 'input', direction: 'input', port_type: 'text' },
  ],
  outputs: [
    { id: 'response', label: 'response', direction: 'output', port_type: 'text' },
  ],
}

// ── Save to File ────────────────────────────────────────────────────────────

const SAVE_FILE_DEFINITION: NodeDefinition = {
  type: 'save_file',
  label: 'Save to File',
  icon: '💾',
  headerColor: '#0ea5e9',
  category: 'io',
  fields: [
    { key: 'path', type: 'text', label: 'File Path' },
    { key: 'mode', type: 'select', label: 'Mode', options: ['overwrite', 'append'] },
  ],
  inputs: [
    { id: 'content', label: 'content', direction: 'input', port_type: 'text' },
  ],
  outputs: [
    { id: 'path', label: 'path', direction: 'output', port_type: 'text' },
    { id: 'done', label: 'done', direction: 'output', port_type: 'event' },
  ],
}

// ── Read File ───────────────────────────────────────────────────────────────

const READ_FILE_DEFINITION: NodeDefinition = {
  type: 'read_file',
  label: 'Read File',
  icon: '📄',
  headerColor: '#0ea5e9',
  category: 'io',
  fields: [
    { key: 'path', type: 'text', label: 'File Path' },
  ],
  inputs: [
    { id: 'path', label: 'path', direction: 'input', port_type: 'text' },
  ],
  outputs: [
    { id: 'content', label: 'content', direction: 'output', port_type: 'text' },
  ],
}

// ── HTTP Request ────────────────────────────────────────────────────────────

const HTTP_REQUEST_DEFINITION: NodeDefinition = {
  type: 'http_request',
  label: 'HTTP Request',
  icon: '🌐',
  headerColor: '#14b8a6',
  category: 'io',
  fields: [
    { key: 'url', type: 'text', label: 'URL' },
    { key: 'method', type: 'select', label: 'Method', options: ['GET', 'POST', 'PUT', 'DELETE'] },
  ],
  inputs: [
    { id: 'body', label: 'body', direction: 'input', port_type: 'json' },
  ],
  outputs: [
    { id: 'response', label: 'response', direction: 'output', port_type: 'text' },
    { id: 'status', label: 'status', direction: 'output', port_type: 'number' },
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

// ── Constant ────────────────────────────────────────────────────────────────

const CONSTANT_DEFINITION: NodeDefinition = {
  type: 'constant',
  label: 'Constant',
  icon: '📌',
  headerColor: '#737373',
  category: 'core',
  fields: [],
  inputs: [],
  outputs: [
    { id: 'value', label: 'value', direction: 'output', port_type: 'text' },
  ],
}

// ── Registry ─────────────────────────────────────────────────────────────────

// ── Registry ─────────────────────────────────────────────────────────────────

const BUILT_IN_DEFINITIONS: NodeDefinition[] = [
  AGENT_DEFINITION,
  TOOL_DEFINITION,
  CHANNEL_DEFINITION,
  HUMAN_INPUT_DEFINITION,
  SAVE_FILE_DEFINITION,
  READ_FILE_DEFINITION,
  HTTP_REQUEST_DEFINITION,
  SCHEDULE_DEFINITION,
  GATE_DEFINITION,
  SUBAGENT_DEFINITION,
  ROLE_DEFINITION,
  PROVIDER_DEFINITION,
  CONSTANT_DEFINITION,
]

const CUSTOM_STORAGE_KEY = 'agentzero-custom-node-definitions'

/** Load user-created node definitions from localStorage. */
function loadCustomDefinitions(): NodeDefinition[] {
  if (typeof window === 'undefined') return []
  try {
    const raw = localStorage.getItem(CUSTOM_STORAGE_KEY)
    return raw ? JSON.parse(raw) : []
  } catch { return [] }
}

/** Save user-created node definitions to localStorage. */
function saveCustomDefinitions(defs: NodeDefinition[]) {
  try { localStorage.setItem(CUSTOM_STORAGE_KEY, JSON.stringify(defs)) } catch { /* full */ }
}

// Mutable registry — built-ins + user-created
let _customDefinitions = loadCustomDefinitions()
let _allDefinitions = [...BUILT_IN_DEFINITIONS, ..._customDefinitions]
let _definitionsMap = new Map<string, NodeDefinition>(_allDefinitions.map((d) => [d.type, d]))

// Change listeners for React re-renders
type DefinitionListener = () => void
const _listeners = new Set<DefinitionListener>()

function _rebuild() {
  _allDefinitions = [...BUILT_IN_DEFINITIONS, ..._customDefinitions]
  _definitionsMap = new Map(_allDefinitions.map((d) => [d.type, d]))
  _listeners.forEach((fn) => fn())
}

/** All node type definitions (built-in + user-created). */
export function getAllDefinitions(): NodeDefinition[] {
  return _allDefinitions
}

/** Subscribe to definition changes — returns unsubscribe function. */
export function onDefinitionsChange(fn: DefinitionListener): () => void {
  _listeners.add(fn)
  return () => _listeners.delete(fn)
}

/** Register a custom node definition. Persists to localStorage. */
export function registerNodeDefinition(def: NodeDefinition) {
  _customDefinitions = _customDefinitions.filter((d) => d.type !== def.type)
  _customDefinitions.push(def)
  saveCustomDefinitions(_customDefinitions)
  _rebuild()
}

/** Remove a custom node definition. */
export function unregisterNodeDefinition(type: string) {
  _customDefinitions = _customDefinitions.filter((d) => d.type !== type)
  saveCustomDefinitions(_customDefinitions)
  _rebuild()
}

/** Check if a node type is user-created (not built-in). */
export function isCustomDefinition(type: string): boolean {
  return _customDefinitions.some((d) => d.type === type)
}

// Backwards-compatible — static snapshot (use getAllDefinitions() for live data)
export const ALL_NODE_DEFINITIONS = _allDefinitions

/** Look up a node definition by type key. */
export function getDefinition(type: string): NodeDefinition | undefined {
  return _definitionsMap.get(type)
}

/** Get ports for a node type from its definition. */
export function portsForType(type: string): Port[] {
  const def = _definitionsMap.get(type)
  if (!def) return []
  return [...(def.inputs ?? []), ...(def.outputs ?? [])]
}

export { AGENT_DEFINITION, TOOL_DEFINITION, CHANNEL_DEFINITION, HUMAN_INPUT_DEFINITION, SCHEDULE_DEFINITION, GATE_DEFINITION, SUBAGENT_DEFINITION, ROLE_DEFINITION, PROVIDER_DEFINITION, CONSTANT_DEFINITION }
