import { createFileRoute, useNavigate } from '@tanstack/react-router'
import { useState } from 'react'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { useAuthStore } from '@/store/authStore'
import { ApiError } from '@/lib/api/client'
import { authApi } from '@/lib/api/auth'

export const Route = createFileRoute('/login')({
  component: LoginPage,
})

function normalizeUrl(url: string): string {
  let trimmed = url.trim().replace(/\/+$/, '')
  if (trimmed && !/^https?:\/\//i.test(trimmed)) {
    trimmed = `http://${trimmed}`
  }
  return trimmed
}

function LoginPage() {
  const [token, setToken] = useState('')
  const [pairingCode, setPairingCode] = useState('')
  const [baseUrl, setBaseUrl] = useState('http://localhost:42617')
  const [tokenError, setTokenError] = useState('')
  const [pairError, setPairError] = useState('')
  const [loading, setLoading] = useState(false)
  const { setToken: saveToken, setBaseUrl: saveBaseUrl } = useAuthStore()
  const navigate = useNavigate()

  async function handleTokenLogin(e: React.FormEvent) {
    e.preventDefault()
    setTokenError('')
    if (!token.trim()) { setTokenError('Token is required'); return }
    saveBaseUrl(normalizeUrl(baseUrl))
    saveToken(token.trim())
    void navigate({ to: '/dashboard' })
  }

  async function handlePair(e: React.FormEvent) {
    e.preventDefault()
    setPairError('')
    setLoading(true)
    try {
      saveBaseUrl(normalizeUrl(baseUrl))
      const res = await authApi.pair(pairingCode.trim())
      saveToken(res.token)
      void navigate({ to: '/dashboard' })
    } catch (err) {
      setPairError(err instanceof ApiError ? err.message : 'Pairing failed')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-dvh flex items-center justify-center bg-background p-4">
      <div className="w-full max-w-md space-y-4">
        <div className="text-center space-y-1">
          <h1 className="text-2xl font-semibold tracking-tight">AgentZero</h1>
          <p className="text-sm text-muted-foreground">Connect to your gateway</p>
        </div>

        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">Gateway URL</CardTitle>
          </CardHeader>
          <CardContent>
            <Input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder="http://localhost:42617"
            />
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">API Token</CardTitle>
            <CardDescription>Paste an existing API key (az_…)</CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={handleTokenLogin} className="space-y-3">
              <div className="space-y-1">
                <Label>Token</Label>
                <Input
                  value={token}
                  onChange={(e) => setToken(e.target.value)}
                  placeholder="az_..."
                  type="password"
                />
              </div>
              {tokenError && <p className="text-xs text-destructive">{tokenError}</p>}
              <Button type="submit" className="w-full">Connect</Button>
            </form>
          </CardContent>
        </Card>

        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-base">Pairing Code</CardTitle>
            <CardDescription>Use a one-time pairing code from the CLI</CardDescription>
          </CardHeader>
          <CardContent>
            <form onSubmit={handlePair} className="space-y-3">
              <div className="space-y-1">
                <Label>Pairing Code</Label>
                <Input
                  value={pairingCode}
                  onChange={(e) => setPairingCode(e.target.value)}
                  placeholder="XXXX-XXXX"
                />
              </div>
              {pairError && <p className="text-xs text-destructive">{pairError}</p>}
              <Button type="submit" className="w-full" disabled={loading}>
                {loading ? 'Pairing…' : 'Pair'}
              </Button>
            </form>
          </CardContent>
        </Card>
      </div>
    </div>
  )
}
