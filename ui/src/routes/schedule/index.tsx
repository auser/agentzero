import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { api } from '@/lib/api/client'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetFooter } from '@/components/ui/sheet'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
import { Plus, Trash2 } from 'lucide-react'

export const Route = createFileRoute('/schedule/')({
  component: SchedulePage,
})

interface CronJob {
  id: string
  schedule: string
  message: string
  agent_id?: string
  enabled: boolean
  last_run?: string
  next_run?: string
}

interface CronListResponse {
  jobs: CronJob[]
}

function SchedulePage() {
  const qc = useQueryClient()
  const [sheetOpen, setSheetOpen] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<CronJob | null>(null)
  const [form, setForm] = useState({ schedule: '', message: '', agent_id: '' })

  const { data, isPending, error } = useQuery({
    queryKey: ['cron'],
    queryFn: () => api.get<CronListResponse>('/v1/cron'),
    retry: false,
  })

  const createMutation = useMutation({
    mutationFn: () => api.post<CronJob>('/v1/cron', { ...form, agent_id: form.agent_id || undefined }),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ['cron'] }); setSheetOpen(false); setForm({ schedule: '', message: '', agent_id: '' }) },
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => api.delete(`/v1/cron/${id}`),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['cron'] }),
  })

  const toggleMutation = useMutation({
    mutationFn: ({ id, enabled }: { id: string; enabled: boolean }) =>
      api.patch(`/v1/cron/${id}`, { enabled }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['cron'] }),
  })

  return (
    <div className="space-y-4 max-w-4xl">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Schedule</h1>
        <Button size="sm" onClick={() => setSheetOpen(true)}>
          <Plus className="h-4 w-4 mr-2" />
          New Job
        </Button>
      </div>

      {error && (
        <div className="rounded-lg border border-border p-4">
          <p className="text-sm font-medium">Schedule endpoint not yet available</p>
          <p className="text-xs text-muted-foreground mt-1">
            Add <code className="bg-muted px-1 rounded">GET /v1/cron</code> to the gateway.
          </p>
        </div>
      )}

      {!error && (
        <div className="rounded-lg border border-border overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>Enabled</TableHead>
                <TableHead>Schedule</TableHead>
                <TableHead>Message</TableHead>
                <TableHead>Agent</TableHead>
                <TableHead className="w-16" />
              </TableRow>
            </TableHeader>
            <TableBody>
              {isPending && (
                <TableRow><TableCell colSpan={5} className="text-center text-muted-foreground py-8">Loading…</TableCell></TableRow>
              )}
              {!isPending && data?.jobs.length === 0 && (
                <TableRow><TableCell colSpan={5} className="text-center text-muted-foreground py-8">No scheduled jobs</TableCell></TableRow>
              )}
              {data?.jobs.map((job) => (
                <TableRow key={job.id}>
                  <TableCell>
                    <Switch
                      checked={job.enabled}
                      onCheckedChange={(v) => toggleMutation.mutate({ id: job.id, enabled: v })}
                    />
                  </TableCell>
                  <TableCell className="font-mono text-xs">{job.schedule}</TableCell>
                  <TableCell className="text-sm max-w-64 truncate">{job.message}</TableCell>
                  <TableCell className="text-xs text-muted-foreground">{job.agent_id ?? '—'}</TableCell>
                  <TableCell>
                    <Button
                      variant="ghost" size="icon" className="h-7 w-7 text-destructive hover:text-destructive"
                      onClick={() => setDeleteTarget(job)}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      )}

      <Sheet open={sheetOpen} onOpenChange={setSheetOpen}>
        <SheetContent className="w-full sm:max-w-md">
          <SheetHeader><SheetTitle>New Scheduled Job</SheetTitle></SheetHeader>
          <form
            onSubmit={(e) => { e.preventDefault(); createMutation.mutate() }}
            className="space-y-4 py-4"
          >
            <div className="space-y-1">
              <Label>Cron Schedule *</Label>
              <Input
                value={form.schedule}
                onChange={(e) => setForm((f) => ({ ...f, schedule: e.target.value }))}
                placeholder="0 9 * * 1-5"
                required
              />
              <p className="text-xs text-muted-foreground">Standard 5-field cron expression</p>
            </div>
            <div className="space-y-1">
              <Label>Message *</Label>
              <textarea
                value={form.message}
                onChange={(e) => setForm((f) => ({ ...f, message: e.target.value }))}
                rows={3}
                required
                className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm resize-none focus:outline-none focus:ring-1 focus:ring-ring"
                placeholder="What should the agent do at this time?"
              />
            </div>
            <div className="space-y-1">
              <Label>Agent ID <span className="text-muted-foreground text-xs">(optional)</span></Label>
              <Input
                value={form.agent_id}
                onChange={(e) => setForm((f) => ({ ...f, agent_id: e.target.value }))}
                placeholder="Leave blank for default agent"
              />
            </div>
            <SheetFooter>
              <Button type="button" variant="outline" onClick={() => setSheetOpen(false)}>Cancel</Button>
              <Button type="submit" disabled={createMutation.isPending}>
                {createMutation.isPending ? 'Creating…' : 'Create'}
              </Button>
            </SheetFooter>
          </form>
        </SheetContent>
      </Sheet>

      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(v) => { if (!v) setDeleteTarget(null) }}
        title="Delete Job"
        description={`Delete the job with schedule "${deleteTarget?.schedule}"?`}
        confirmLabel="Delete"
        destructive
        onConfirm={() => { if (deleteTarget) deleteMutation.mutate(deleteTarget.id) }}
      />
    </div>
  )
}
