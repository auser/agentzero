import { api } from './client'

export interface TopologyNode {
  agent_id: string
  name: string
  status: string
  active_run_count: number
  total_cost_microdollars: number
}

export interface TopologyEdge {
  from_agent_id: string
  to_agent_id: string
  run_id: string
  edge_type: string
}

export interface TopologyResponse {
  nodes: TopologyNode[]
  edges: TopologyEdge[]
}

export const topologyApi = {
  get: (signal?: AbortSignal) =>
    api.get<TopologyResponse>('/v1/topology', signal),
}
