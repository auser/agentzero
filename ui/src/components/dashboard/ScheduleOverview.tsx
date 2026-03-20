/**
 * Compact schedule overview showing upcoming cron jobs.
 */
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api/client'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Link } from '@tanstack/react-router'
import { CalendarClock } from 'lucide-react'

interface CronJob {
  id: string
  name: string
  schedule: string
  enabled: boolean
  last_run?: string
  next_run?: string
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
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm flex items-center gap-1.5">
            <CalendarClock className="h-3.5 w-3.5" />
            Schedules
          </CardTitle>
          <Link to="/schedule" className="text-xs text-primary hover:underline">
            Manage
          </Link>
        </div>
      </CardHeader>
      <CardContent>
        <div className="space-y-2">
          {enabledJobs.slice(0, 4).map((job) => (
            <div
              key={job.id}
              className="flex items-center justify-between py-1 border-b border-border last:border-0"
            >
              <span className="text-sm truncate">{job.name}</span>
              <span className="text-[10px] font-mono text-muted-foreground bg-muted px-1.5 py-0.5 rounded shrink-0">
                {job.schedule}
              </span>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}
