import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { runsApi, type RunListItem, type RunStatus } from '@/lib/api/runs'
import { Button } from '@/components/ui/button'
import { Label } from '@/components/ui/label'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { Sheet, SheetContent, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { ScrollArea } from '@/components/ui/scroll-area'
import { StatusBadge } from '@/components/shared/StatusBadge'
import { CostDisplay } from '@/components/shared/CostDisplay'
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
import { useRunStream } from '@/hooks/useRunStream'
import { Plus, X, AlertTriangle } from 'lucide-react'
import { formatDistanceToNow } from 'date-fns'

export const Route = createFileRoute('/runs/')({
  component: RunsPage,
})

const STATUS_OPTIONS: Array<{ label: string; value: RunStatus | 'all' }> = [
  { label: 'All', value: 'all' },
  { label: 'Pending', value: 'pending' },
  { label: 'Running', value: 'running' },
  { label: 'Completed', value: 'completed' },
  { label: 'Failed', value: 'failed' },
  { label: 'Cancelled', value: 'cancelled' },
]

function RunDetailPanel({ run, onClose }: { run: RunListItem; onClose: () => void }) {
  const { data: transcript } = useQuery({
    queryKey: ['runs', run.run_id, 'transcript'],
    queryFn: () => runsApi.transcript(run.run_id),
  })

  const { data: events } = useQuery({
    queryKey: ['runs', run.run_id, 'events'],
    queryFn: () => runsApi.events(run.run_id),
  })

  const { chunks, isDone } = useRunStream(
    run.status === 'running' ? run.run_id : null
  )

  return (
    <Sheet open onOpenChange={(v) => { if (!v) onClose() }}>
      <SheetContent className="w-full sm:max-w-2xl flex flex-col">
        <SheetHeader>
          <div className="flex items-center justify-between">
            <div>
              <SheetTitle className="font-mono text-sm">{run.run_id}</SheetTitle>
              <div className="flex items-center gap-2 mt-1">
                <StatusBadge status={run.status} />
                <span className="text-xs text-muted-foreground">{run.agent_id}</span>
                <CostDisplay microdollars={run.cost_microdollars} className="text-xs text-muted-foreground" />
              </div>
            </div>
          </div>
        </SheetHeader>

        <Tabs defaultValue="transcript" className="flex-1 flex flex-col min-h-0">
          <TabsList className="w-full">
            <TabsTrigger value="transcript" className="flex-1">Transcript</TabsTrigger>
            <TabsTrigger value="events" className="flex-1">Tool Events</TabsTrigger>
            {run.status === 'running' && (
              <TabsTrigger value="stream" className="flex-1">Live</TabsTrigger>
            )}
          </TabsList>

          <TabsContent value="transcript" className="flex-1 min-h-0">
            <ScrollArea className="h-full">
              <div className="space-y-3 p-1">
                {(transcript?.entries ?? []).map((entry, i) => (
                  <div key={i} className={`flex ${entry.role === 'user' ? 'justify-end' : 'justify-start'}`}>
                    <div className={`max-w-[85%] rounded-lg px-3 py-2 text-xs ${
                      entry.role === 'user' ? 'bg-primary text-primary-foreground' : 'bg-secondary'
                    }`}>
                      <p className="font-medium mb-1 opacity-60 capitalize">{entry.role}</p>
                      <p className="whitespace-pre-wrap wrap-break-word">{entry.content}</p>
                    </div>
                  </div>
                ))}
                {!transcript?.entries.length && (
                  <p className="text-xs text-muted-foreground text-center py-4">No transcript</p>
                )}
              </div>
            </ScrollArea>
          </TabsContent>

          <TabsContent value="events" className="flex-1 min-h-0">
            <ScrollArea className="h-full">
              <div className="space-y-1 p-1">
                {(events?.events ?? []).map((e, i) => (
                  <div key={i} className="rounded border border-border p-2 text-xs space-y-0.5">
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{e.type}</span>
                      {e.tool && <span className="text-muted-foreground font-mono">{e.tool}</span>}
                    </div>
                    {e.result && <p className="text-muted-foreground truncate">{e.result}</p>}
                    {e.error && <p className="text-destructive">{e.error}</p>}
                  </div>
                ))}
                {!events?.events.length && (
                  <p className="text-xs text-muted-foreground text-center py-4">No events</p>
                )}
              </div>
            </ScrollArea>
          </TabsContent>

          {run.status === 'running' && (
            <TabsContent value="stream" className="flex-1 min-h-0">
              <ScrollArea className="h-full">
                <pre className="p-2 text-xs font-mono whitespace-pre-wrap wrap-break-word">
                  {chunks.join('')}
                  {!isDone && <span className="animate-pulse">▋</span>}
                </pre>
              </ScrollArea>
            </TabsContent>
          )}
        </Tabs>
      </SheetContent>
    </Sheet>
  )
}

function SubmitRunSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const qc = useQueryClient()
  const [message, setMessage] = useState('')
  const [mode, setMode] = useState<'steer' | 'followup' | 'collect' | 'interrupt'>('steer')

  const submitMutation = useMutation({
    mutationFn: () => runsApi.submit({ message, mode }),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ['runs'] })
      onClose()
      setMessage('')
    },
  })

  return (
    <Sheet open={open} onOpenChange={(v) => { if (!v) onClose() }}>
      <SheetContent className="w-full sm:max-w-lg">
        <SheetHeader><SheetTitle>Submit Run</SheetTitle></SheetHeader>
        <form
          onSubmit={(e) => { e.preventDefault(); submitMutation.mutate() }}
          className="space-y-4 py-4"
        >
          <div className="space-y-1">
            <Label>Message *</Label>
            <textarea
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              rows={4}
              required
              className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm resize-none focus:outline-none focus:ring-1 focus:ring-ring"
              placeholder="What should the agent do?"
            />
          </div>
          <div className="space-y-1">
            <Label>Mode</Label>
            <Select value={mode} onValueChange={(v) => setMode(v as typeof mode)}>
              <SelectTrigger><SelectValue /></SelectTrigger>
              <SelectContent>
                <SelectItem value="steer">Steer (default)</SelectItem>
                <SelectItem value="followup">Follow-up</SelectItem>
                <SelectItem value="collect">Collect (fan-out)</SelectItem>
                <SelectItem value="interrupt">Interrupt</SelectItem>
              </SelectContent>
            </Select>
          </div>
          {submitMutation.error && (
            <p className="text-xs text-destructive">{submitMutation.error.message}</p>
          )}
          <div className="flex gap-2 justify-end">
            <Button type="button" variant="outline" onClick={onClose}>Cancel</Button>
            <Button type="submit" disabled={submitMutation.isPending || !message.trim()}>
              {submitMutation.isPending ? 'Submitting…' : 'Submit'}
            </Button>
          </div>
        </form>
      </SheetContent>
    </Sheet>
  )
}

function RunsPage() {
  const qc = useQueryClient()
  const [statusFilter, setStatusFilter] = useState<RunStatus | 'all'>('all')
  const [selectedRun, setSelectedRun] = useState<RunListItem | null>(null)
  const [submitOpen, setSubmitOpen] = useState(false)
  const [cancelTarget, setCancelTarget] = useState<RunListItem | null>(null)
  const [estopOpen, setEstopOpen] = useState(false)

  const { data, isPending } = useQuery({
    queryKey: ['runs', { status: statusFilter }],
    queryFn: () => runsApi.list(statusFilter === 'all' ? undefined : statusFilter),
    refetchInterval: 5_000,
  })

  const cancelMutation = useMutation({
    mutationFn: (id: string) => runsApi.cancel(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['runs'] }),
  })

  const estopMutation = useMutation({
    mutationFn: runsApi.estop,
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['runs'] }),
  })

  return (
    <div className="space-y-4 max-w-6xl">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Runs</h1>
        <div className="flex gap-2">
          <Button size="sm" onClick={() => setSubmitOpen(true)}>
            <Plus className="h-4 w-4 mr-2" />
            New Run
          </Button>
          <Button
            size="sm"
            variant="outline"
            className="border-red-800/50 text-red-400 hover:bg-red-950/50"
            onClick={() => setEstopOpen(true)}
          >
            <AlertTriangle className="h-4 w-4 mr-2" />
            E-Stop
          </Button>
        </div>
      </div>

      {/* Status filter */}
      <div className="flex gap-1.5 flex-wrap">
        {STATUS_OPTIONS.map(({ label, value }) => (
          <Button
            key={value}
            size="sm"
            variant={statusFilter === value ? 'secondary' : 'outline'}
            className="h-7 text-xs"
            onClick={() => setStatusFilter(value)}
          >
            {label}
          </Button>
        ))}
      </div>

      <div className="rounded-lg border border-border overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Run ID</TableHead>
              <TableHead>Status</TableHead>
              <TableHead>Agent</TableHead>
              <TableHead>Cost</TableHead>
              <TableHead>Submitted</TableHead>
              <TableHead className="w-20" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {isPending && (
              <TableRow>
                <TableCell colSpan={6} className="text-center text-muted-foreground py-8">Loading…</TableCell>
              </TableRow>
            )}
            {!isPending && data?.data.length === 0 && (
              <TableRow>
                <TableCell colSpan={6} className="text-center text-muted-foreground py-8">No runs</TableCell>
              </TableRow>
            )}
            {data?.data.map((run) => (
              <TableRow
                key={run.run_id}
                className="cursor-pointer hover:bg-muted/30"
                onClick={() => setSelectedRun(run)}
              >
                <TableCell className="font-mono text-xs text-muted-foreground">{run.run_id.slice(0, 16)}…</TableCell>
                <TableCell><StatusBadge status={run.status} /></TableCell>
                <TableCell className="text-xs">{run.agent_id}</TableCell>
                <TableCell><CostDisplay microdollars={run.cost_microdollars} className="text-xs" /></TableCell>
                <TableCell className="text-xs text-muted-foreground">
                  {run.accepted_at ? formatDistanceToNow(new Date(run.accepted_at), { addSuffix: true }) : '—'}
                </TableCell>
                <TableCell onClick={(e) => e.stopPropagation()}>
                  {(run.status === 'pending' || run.status === 'running') && (
                    <Button
                      variant="ghost" size="icon" className="h-7 w-7 text-destructive hover:text-destructive"
                      onClick={() => setCancelTarget(run)}
                    >
                      <X className="h-3.5 w-3.5" />
                    </Button>
                  )}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      {selectedRun && <RunDetailPanel run={selectedRun} onClose={() => setSelectedRun(null)} />}
      <SubmitRunSheet open={submitOpen} onClose={() => setSubmitOpen(false)} />

      <ConfirmDialog
        open={cancelTarget !== null}
        onOpenChange={(v) => { if (!v) setCancelTarget(null) }}
        title="Cancel Run"
        description={`Cancel run ${cancelTarget?.run_id.slice(0, 16)}…?`}
        confirmLabel="Cancel Run"
        destructive
        onConfirm={() => { if (cancelTarget) cancelMutation.mutate(cancelTarget.run_id) }}
      />
      <ConfirmDialog
        open={estopOpen}
        onOpenChange={setEstopOpen}
        title="Emergency Stop"
        description="Cancel all running and pending jobs."
        confirmLabel="Stop All"
        destructive
        onConfirm={() => estopMutation.mutate()}
      />
    </div>
  )
}
