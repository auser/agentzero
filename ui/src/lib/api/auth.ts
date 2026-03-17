import { ApiError, getBaseUrl } from './client'

export const authApi = {
  pair: async (pairingCode: string): Promise<{ token: string }> => {
    const res = await fetch(`${getBaseUrl()}/pair`, {
      method: 'POST',
      headers: { 'X-Pairing-Code': pairingCode },
    })
    if (!res.ok) {
      const err = await res.json().catch(() => ({ error: { message: res.statusText } }))
      throw new ApiError(res.status, (err as { error?: { message?: string } }).error?.message ?? res.statusText)
    }
    return res.json() as Promise<{ token: string }>
  },
}
