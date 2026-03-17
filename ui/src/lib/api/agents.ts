import { api } from './client'

export interface AgentListItem {
  agent_id: string
  name: string
  description: string
  model: string
  provider: string
  status: 'active' | 'stopped'
  source: 'static' | 'dynamic'
  keywords: string[]
  allowed_tools: string[]
  channels: string[]
  created_at: number
  updated_at: number
}

export interface AgentListResponse {
  object: string
  data: AgentListItem[]
  total: number
}

export interface CreateAgentPayload {
  name: string
  description?: string
  system_prompt?: string
  provider?: string
  model?: string
  keywords?: string[]
  allowed_tools?: string[]
  channels?: Record<string, { enabled: boolean }>
}

export interface UpdateAgentPayload extends Partial<Omit<CreateAgentPayload, 'name'>> {
  name?: string
  status?: 'active' | 'stopped'
}

export const agentsApi = {
  list: (signal?: AbortSignal) =>
    api.get<AgentListResponse>('/v1/agents', signal),

  get: (id: string, signal?: AbortSignal) =>
    api.get<AgentListItem>(`/v1/agents/${id}`, signal),

  create: (payload: CreateAgentPayload) =>
    api.post<AgentListItem>('/v1/agents', payload),

  update: (id: string, payload: UpdateAgentPayload) =>
    api.patch<AgentListItem>(`/v1/agents/${id}`, payload),

  delete: (id: string) =>
    api.delete<void>(`/v1/agents/${id}`),
}
