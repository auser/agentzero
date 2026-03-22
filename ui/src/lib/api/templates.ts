import { api } from './client'

export interface TemplateRecord {
  template_id: string
  name: string
  description: string
  category: string
  tags: string[]
  version: number
  node_count: number
  edge_count: number
  layout?: {
    nodes: unknown[]
    edges: unknown[]
  }
  created_at: number
  updated_at: number
}

export interface TemplateListResponse {
  object: string
  data: TemplateRecord[]
  total: number
}

export interface CreateTemplatePayload {
  name: string
  description?: string
  category?: string
  tags?: string[]
  layout?: { nodes: unknown[]; edges: unknown[] }
}

export const templatesApi = {
  list: (include?: string, signal?: AbortSignal) =>
    api.get<TemplateListResponse>(
      `/v1/templates${include ? `?include=${include}` : ''}`,
      signal,
    ),

  get: (id: string, include?: string, signal?: AbortSignal) =>
    api.get<TemplateRecord>(
      `/v1/templates/${id}${include ? `?include=${include}` : ''}`,
      signal,
    ),

  create: (payload: CreateTemplatePayload) =>
    api.post<TemplateRecord>('/v1/templates', payload),

  update: (id: string, payload: Partial<CreateTemplatePayload>) =>
    api.patch<TemplateRecord>(`/v1/templates/${id}`, payload),

  delete: (id: string) =>
    api.delete<{ template_id: string; deleted: boolean }>(`/v1/templates/${id}`),
}
