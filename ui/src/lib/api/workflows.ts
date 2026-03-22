import { api } from './client'

export interface WorkflowRecord {
  workflow_id: string
  name: string
  description: string
  definition?: Record<string, unknown>
  layout?: {
    nodes: unknown[]
    edges: unknown[]
    viewport?: { x: number; y: number; zoom: number }
  }
  status: string
  created_at: number
  updated_at: number
}

export interface WorkflowListResponse {
  object: string
  data: WorkflowRecord[]
  total: number
}

export interface CreateWorkflowPayload {
  name: string
  description?: string
  definition?: Record<string, unknown>
  layout?: Record<string, unknown>
}

export interface UpdateWorkflowPayload {
  name?: string
  description?: string
  definition?: Record<string, unknown>
  layout?: Record<string, unknown>
  status?: string
}

export const workflowsApi = {
  list: (include?: string, signal?: AbortSignal) =>
    api.get<WorkflowListResponse>(
      `/v1/workflows${include ? `?include=${include}` : ''}`,
      signal,
    ),

  get: (id: string, include?: string, signal?: AbortSignal) =>
    api.get<WorkflowRecord>(
      `/v1/workflows/${id}${include ? `?include=${include}` : ''}`,
      signal,
    ),

  create: (payload: CreateWorkflowPayload) =>
    api.post<WorkflowRecord>('/v1/workflows', payload),

  update: (id: string, payload: UpdateWorkflowPayload) =>
    api.patch<WorkflowRecord>(`/v1/workflows/${id}`, payload),

  delete: (id: string) =>
    api.delete<{ workflow_id: string; deleted: boolean }>(`/v1/workflows/${id}`),
}
