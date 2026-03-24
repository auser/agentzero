/**
 * Converts AgentZero topology data to ReactFlow nodes and edges.
 * Port definitions are sourced from the shared node-definitions registry.
 */
import type { Node, Edge } from '@xyflow/react'
import type { TopologyNode, TopologyEdge } from '@/lib/api/topology'
import type { Port } from '@/lib/workflow-types'
import { portsForType } from '@/lib/node-definitions'
import { portTypeColor, statusColor } from '@/lib/workflow-types'
import type { AgentNodeData } from '@/components/workflows/AgentNode'

/** Get ports for a node type (delegates to node-definitions registry). */
export function portsForNodeType(nodeType: string): Port[] {
  return portsForType(nodeType)
}

/**
 * Convert topology API data to ReactFlow nodes and edges.
 * Merges saved positions from localStorage when available.
 */
export function topologyToReactFlow(
  nodes: TopologyNode[],
  edges: TopologyEdge[],
  savedPositions?: Record<string, { x: number; y: number }>,
): { nodes: Node[]; edges: Edge[] } {
  const rfNodes: Node[] = nodes.map((node, i) => {
    const saved = savedPositions?.[node.agent_id]
    return {
      id: node.agent_id,
      type: 'agent',
      position: saved ?? { x: 30, y: 30 + i * 180 },
      data: {
        name: node.name,
        nodeType: 'agent',
        status: node.status,
        metadata: {
          node_type: 'agent',
          agent_id: node.agent_id,
          status: node.status,
          run_count: node.active_run_count,
          cost_microdollars: node.total_cost_microdollars,
        },
      } satisfies AgentNodeData,
    }
  })

  const rfEdges: Edge[] = edges.map((edge) => {
    // Find the source node to determine port type for edge coloring
    const sourceNode = nodes.find((n) => n.agent_id === edge.from_agent_id)
    const edgeColor = sourceNode?.status === 'running'
      ? statusColor('running')
      : portTypeColor('text')

    return {
      id: `${edge.from_agent_id}->${edge.to_agent_id}`,
      source: edge.from_agent_id,
      target: edge.to_agent_id,
      sourceHandle: 'response',
      targetHandle: 'message',

      animated: sourceNode?.status === 'running',
      style: { stroke: edgeColor, strokeWidth: 2 },
    }
  })

  return { nodes: rfNodes, edges: rfEdges }
}

/** Map topology status to a normalized status string. */
export function mapTopologyStatus(status: string): string {
  switch (status) {
    case 'running': return 'running'
    case 'active': return 'success'
    case 'stopped': return 'cancelled'
    case 'stale': return 'failure'
    default: return 'queued'
  }
}
