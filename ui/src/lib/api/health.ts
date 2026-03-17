import { api } from './client'

export interface HealthResponse {
  status: 'ok' | 'degraded' | 'down'
  service: string
  version: string
}

export const healthApi = {
  get: (signal?: AbortSignal) => api.get<HealthResponse>('/health', signal),
  ready: (signal?: AbortSignal) => api.get<HealthResponse>('/health/ready', signal),
  live: (signal?: AbortSignal) => api.get<HealthResponse>('/health/live', signal),
}
