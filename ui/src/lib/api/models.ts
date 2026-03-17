import { api } from './client'

export interface ModelInfo {
  id: string
  object: string
  owned_by: string
  created?: number
}

export interface ModelsResponse {
  object: string
  data: ModelInfo[]
}

export const modelsApi = {
  list: (signal?: AbortSignal) =>
    api.get<ModelsResponse>('/v1/models', signal),
}
