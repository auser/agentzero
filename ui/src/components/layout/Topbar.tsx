import { useQuery } from '@tanstack/react-query'
import { healthApi } from '@/lib/api/health'
import { StatusBadge } from '@/components/shared/StatusBadge'
import { useAuthStore } from '@/store/authStore'
import { Button } from '@/components/ui/button'
import { LogOut } from 'lucide-react'
import { useNavigate } from '@tanstack/react-router'

export function Topbar() {
  const { data: health } = useQuery({
    queryKey: ['health'],
    queryFn: () => healthApi.get(),
    refetchInterval: 30_000,
    retry: false,
  })

  const { token, setToken } = useAuthStore()
  const navigate = useNavigate()

  function logout() {
    setToken(null)
    void navigate({ to: '/login' })
  }

  return (
    <header className="h-12 flex items-center justify-between px-4 border-b border-border bg-card shrink-0">
      <div className="flex items-center gap-2">
        {health ? (
          <StatusBadge status={health.status} />
        ) : (
          <StatusBadge status="closed" />
        )}
        {health && (
          <span className="text-xs text-muted-foreground">v{health.version}</span>
        )}
      </div>

      <div className="flex items-center gap-2">
        {token && (
          <span className="text-xs text-muted-foreground font-mono">
            {token.slice(0, 8)}…
          </span>
        )}
        <Button variant="ghost" size="icon" className="h-7 w-7 text-muted-foreground" onClick={logout}>
          <LogOut className="h-4 w-4" />
        </Button>
      </div>
    </header>
  )
}
