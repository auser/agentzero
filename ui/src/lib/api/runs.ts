import { api } from './client'

export type RunStatus = 'pending' | 'running' | 'completed' | 'failed' | 'cancelled'

export interface RunListItem {
  run_id: string
  status: RunStatus
  agent_id: string
  accepted_at: string
  result?: string
  error?: string
  cost_microdollars?: number
}

export interface RunListResponse {
  object: string
  data: RunListItem[]
  total: number
}

export interface RunEventItem {
  event_type: string
  run_id: string
  tool?: string
  result?: string
  error?: string
}

export interface TranscriptEntry {
  role: 'user' | 'assistant' | 'tool'
  content: string
  created_at?: string
}

export interface TranscriptResponse {
  object: string
  run_id: string
  entries: TranscriptEntry[]
  total: number
}

export interface SubmitRunPayload {
  message: string
  mode?: 'steer' | 'followup' | 'collect' | 'interrupt'
  model?: string
  run_id?: string
}

export const runsApi = {
  list: (status?: RunStatus, signal?: AbortSignal) => {
    const qs = status ? `?status=${status}` : ''
    return api.get<RunListResponse>(`/v1/runs${qs}`, signal)
  },

  get: (id: string, signal?: AbortSignal) =>
    api.get<RunListItem>(`/v1/runs/${id}`, signal),

  submit: (payload: SubmitRunPayload) =>
    api.post<{ run_id: string; accepted_at: string }>('/v1/runs', payload),

  cancel: (id: string) =>
    api.delete<void>(`/v1/runs/${id}`),

  result: (id: string, signal?: AbortSignal) =>
    api.get<{ run_id: string; result: string }>(`/v1/runs/${id}/result`, signal),

  transcript: (id: string, signal?: AbortSignal) =>
    api.get<TranscriptResponse>(`/v1/runs/${id}/transcript`, signal),

  events: (id: string, signal?: AbortSignal) =>
    api.get<{ object: string; run_id: string; events: RunEventItem[]; total: number }>(
      `/v1/runs/${id}/events`,
      signal
    ),

  estop: () =>
    api.post<{ emergency_stop: true; cancelled_count: number; cancelled_ids: string[] }>('/v1/estop'),
}
