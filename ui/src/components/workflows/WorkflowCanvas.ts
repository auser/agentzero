/**
 * Converts AgentZero topology data to workflow-graph Workflow format.
 * Port definitions are sourced from the shared node-definitions registry.
 */
import type { Workflow, Job, Port } from '@auser/workflow-graph-web'
import type { TopologyNode, TopologyEdge } from '@/lib/api/topology'
import { portsForType } from '@/lib/node-definitions'

/** Get ports for a node type (delegates to node-definitions registry). */
export function portsForNodeType(nodeType: string): Port[] {
  return portsForType(nodeType)
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
    ports: portsForType('agent'),
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
