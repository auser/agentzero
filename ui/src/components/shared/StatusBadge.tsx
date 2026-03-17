import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'

type Status = 'active' | 'stopped' | 'running' | 'pending' | 'completed' | 'failed' | 'cancelled' | 'ok' | 'degraded' | 'down' | 'open' | 'closed' | 'error' | 'connecting'

const statusConfig: Record<Status, { label: string; className: string }> = {
  active:     { label: 'Active',      className: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30' },
  stopped:    { label: 'Stopped',     className: 'bg-zinc-500/15 text-zinc-400 border-zinc-500/30' },
  running:    { label: 'Running',     className: 'bg-blue-500/15 text-blue-400 border-blue-500/30' },
  pending:    { label: 'Pending',     className: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30' },
  completed:  { label: 'Completed',   className: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30' },
  failed:     { label: 'Failed',      className: 'bg-red-500/15 text-red-400 border-red-500/30' },
  cancelled:  { label: 'Cancelled',   className: 'bg-zinc-500/15 text-zinc-400 border-zinc-500/30' },
  ok:         { label: 'OK',          className: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30' },
  degraded:   { label: 'Degraded',    className: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30' },
  down:       { label: 'Down',        className: 'bg-red-500/15 text-red-400 border-red-500/30' },
  open:       { label: 'Connected',   className: 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30' },
  closed:     { label: 'Disconnected',className: 'bg-zinc-500/15 text-zinc-400 border-zinc-500/30' },
  error:      { label: 'Error',       className: 'bg-red-500/15 text-red-400 border-red-500/30' },
  connecting: { label: 'Connecting',  className: 'bg-yellow-500/15 text-yellow-400 border-yellow-500/30' },
}

interface StatusBadgeProps {
  status: Status
  className?: string
}

export function StatusBadge({ status, className }: StatusBadgeProps) {
  const cfg = statusConfig[status] ?? { label: status, className: 'bg-zinc-500/15 text-zinc-400' }
  return (
    <Badge variant="outline" className={cn('text-xs font-medium', cfg.className, className)}>
      {cfg.label}
    </Badge>
  )
}
