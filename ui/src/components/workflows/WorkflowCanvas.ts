/**
 * Converts AgentZero topology data to workflow-graph Workflow format.
 * Defines port schemas for agent, tool, and channel node types.
 */
import type { Workflow, Job, Port } from '@auser/workflow-graph-web'
import type { TopologyNode, TopologyEdge } from '@/lib/api/topology'

/** Standard port definitions for each node type. */
export const AGENT_PORTS: Port[] = [
  { id: 'message', label: 'message', direction: 'input', port_type: 'text' },
  { id: 'context', label: 'context', direction: 'input', port_type: 'json' },
  { id: 'tools', label: 'tools', direction: 'input', port_type: 'tool_call' },
  { id: 'response', label: 'response', direction: 'output', port_type: 'text' },
  { id: 'tool_calls', label: 'tool calls', direction: 'output', port_type: 'tool_call' },
  { id: 'events', label: 'events', direction: 'output', port_type: 'event' },
]

export const TOOL_PORTS: Port[] = [
  { id: 'input', label: 'input', direction: 'input', port_type: 'json' },
  { id: 'config', label: 'config', direction: 'input', port_type: 'config' },
  { id: 'result', label: 'result', direction: 'output', port_type: 'json' },
]

export const CHANNEL_PORTS: Port[] = [
  { id: 'send', label: 'send', direction: 'input', port_type: 'text' },
  { id: 'trigger', label: 'trigger', direction: 'output', port_type: 'event' },
  { id: 'message', label: 'message', direction: 'output', port_type: 'text' },
]

export const SCHEDULE_PORTS: Port[] = [
  { id: 'trigger', label: 'trigger', direction: 'output', port_type: 'event' },
]

export const GATE_PORTS: Port[] = [
  { id: 'request', label: 'request', direction: 'input', port_type: 'json' },
  { id: 'approved', label: 'approved', direction: 'output', port_type: 'event' },
  { id: 'denied', label: 'denied', direction: 'output', port_type: 'event' },
]

export const SUBAGENT_PORTS: Port[] = [
  { id: 'task', label: 'task', direction: 'input', port_type: 'text' },
  { id: 'context', label: 'context', direction: 'input', port_type: 'json' },
  { id: 'result', label: 'result', direction: 'output', port_type: 'text' },
  { id: 'status', label: 'status', direction: 'output', port_type: 'event' },
]

/** Get ports for a node type. */
export function portsForNodeType(nodeType: string): Port[] {
  switch (nodeType) {
    case 'agent':
      return AGENT_PORTS
    case 'tool':
      return TOOL_PORTS
    case 'channel':
      return CHANNEL_PORTS
    case 'schedule':
      return SCHEDULE_PORTS
    case 'gate':
      return GATE_PORTS
    case 'subagent':
      return SUBAGENT_PORTS
    default:
      return []
  }
}

export function topologyToWorkflow(
  nodes: TopologyNode[],
  edges: TopologyEdge[],
): Workflow {
  const jobs: Job[] = nodes.map((node) => ({
    id: node.agent_id,
    name: node.name,
    status: mapTopologyStatus(node.status),
    command: '',
    depends_on: edges
      .filter((e) => e.to_agent_id === node.agent_id)
      .map((e) => e.from_agent_id),
    metadata: {
      node_type: 'agent',
      status: node.status,
      run_count: node.active_run_count,
      cost_microdollars: node.total_cost_microdollars,
    },
    ports: AGENT_PORTS,
  }))

  return {
    id: 'topology',
    name: 'Agent Topology',
    trigger: 'live',
    jobs,
  }
}

function mapTopologyStatus(status: string): Job['status'] {
  switch (status) {
    case 'running':
      return 'running'
    case 'active':
      return 'success'
    case 'stopped':
      return 'cancelled'
    case 'stale':
      return 'failure'
    default:
      return 'queued'
  }
}
