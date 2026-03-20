/**
 * Compact schedule overview showing upcoming cron jobs.
 */
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api/client'
import { Link } from '@tanstack/react-router'
import { CalendarClock, ChevronRight } from 'lucide-react'

interface CronJob {
  id: string
  name: string
  schedule: string
  enabled: boolean
}

interface CronListResponse {
  jobs: CronJob[]
}

export function ScheduleOverview() {
  const { data } = useQuery({
    queryKey: ['cron'],
    queryFn: () => api.get<CronListResponse>('/v1/cron'),
    retry: false,
  })

  const jobs = data?.jobs ?? []
  const enabledJobs = jobs.filter((j) => j.enabled)

  if (enabledJobs.length === 0) {
    return null
  }

  return (
    <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm">
      <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
          <CalendarClock className="h-3.5 w-3.5" />
          Schedules
        </h3>
        <Link
          to="/schedule"
          className="text-xs text-primary hover:text-primary/80 flex items-center gap-0.5 transition-colors"
        >
          Manage <ChevronRight className="h-3 w-3" />
        </Link>
      </div>
      <div className="p-2">
        <div className="space-y-0.5">
          {enabledJobs.slice(0, 4).map((job) => (
            <div
              key={job.id}
              className="flex items-center justify-between px-3 py-2 rounded-md hover:bg-muted/30 transition-colors"
            >
              <span className="text-sm truncate">{job.name}</span>
              <span className="text-[10px] font-mono text-muted-foreground/70 bg-muted/50 px-1.5 py-0.5 rounded shrink-0">
                {job.schedule}
              </span>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}
