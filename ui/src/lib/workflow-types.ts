/**
 * Workflow graph type definitions.
 * Previously imported from @auser/workflow-graph-web, now defined locally
 * for use with ReactFlow.
 */

/** Direction of a port on a node. */
export type PortDirection = 'input' | 'output'

/** A typed input or output port on a node. */
export interface Port {
  id: string
  label: string
  direction: PortDirection
  port_type?: string
  color?: string
}

/** Type of inline field rendered inside a node body. */
export type FieldType = 'text' | 'textarea' | 'select' | 'toggle' | 'badge' | 'slider'

/** Definition of an inline field rendered inside a node. */
export interface FieldDef {
  key: string
  type: FieldType
  label: string
  options?: string[]
  defaultValue?: unknown
  min?: number
  max?: number
}

/**
 * Declarative definition of a node type.
 * Used by AgentNode to render colored headers, inline fields, and typed ports.
 */
export interface NodeDefinition {
  type: string
  label: string
  icon?: string
  headerColor?: string
  category?: string
  fields?: FieldDef[]
  inputs?: Port[]
  outputs?: Port[]
}

/** Port type → color mapping for handles and edges. */
export function portTypeColor(portType: string): string {
  switch (portType) {
    case 'text':
    case 'message':
      return '#3b82f6' // blue
    case 'json':
    case 'data':
      return '#8b5cf6' // violet
    case 'tool':
    case 'tool_call':
      return '#f97316' // orange
    case 'event':
    case 'trigger':
      return '#22c55e' // green
    case 'role':
      return '#a855f7' // purple
    case 'agent':
      return '#3b82f6' // blue
    case 'config':
      return '#6b7280' // gray
    default:
      return '#9ca3af' // default gray
  }
}

/** Status → color mapping for node borders and dots. */
export function statusColor(status: string): string {
  switch (status) {
    case 'running':
      return '#3b82f6' // blue
    case 'success':
    case 'active':
      return '#22c55e' // green
    case 'failure':
    case 'stale':
      return '#ef4444' // red
    case 'queued':
    default:
      return '#6b7280' // gray
  }
}
