import { create } from 'zustand'
import { persist } from 'zustand/middleware'

interface AuthState {
  token: string | null
  baseUrl: string
  setToken: (token: string | null) => void
  setBaseUrl: (url: string) => void
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      token: null,
      baseUrl: '',
      setToken: (token) => set({ token }),
      setBaseUrl: (baseUrl) => set({ baseUrl }),
    }),
    { name: 'agentzero-auth' }
  )
)
