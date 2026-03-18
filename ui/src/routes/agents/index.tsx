import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { agentsApi, type AgentListItem, type CreateAgentPayload, type UpdateAgentPayload } from '@/lib/api/agents'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetFooter } from '@/components/ui/sheet'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { StatusBadge } from '@/components/shared/StatusBadge'
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
import { Plus, Pencil, Trash2, BarChart3 } from 'lucide-react'
import { formatDistanceToNow } from 'date-fns'
import { AgentCostChart } from '@/components/agents/AgentCostChart'

export const Route = createFileRoute('/agents/')({
  component: AgentsPage,
})

type FormData = {
  name: string
  description: string
  system_prompt: string
  provider: string
  model: string
  keywords: string
  allowed_tools: string
}

const emptyForm: FormData = {
  name: '', description: '', system_prompt: '',
  provider: '', model: '', keywords: '', allowed_tools: '',
}

function toPayload(f: FormData): UpdateAgentPayload {
  return {
    name: f.name,
    description: f.description || undefined,
    system_prompt: f.system_prompt || undefined,
    provider: f.provider || undefined,
    model: f.model || undefined,
    keywords: f.keywords ? f.keywords.split(',').map((k) => k.trim()).filter(Boolean) : undefined,
    allowed_tools: f.allowed_tools ? f.allowed_tools.split(',').map((t) => t.trim()).filter(Boolean) : undefined,
  }
}

function AgentsPage() {
  const qc = useQueryClient()
  const [sheetOpen, setSheetOpen] = useState(false)
  const [editingAgent, setEditingAgent] = useState<AgentListItem | null>(null)
  const [deleteTarget, setDeleteTarget] = useState<AgentListItem | null>(null)
  const [statsAgent, setStatsAgent] = useState<AgentListItem | null>(null)
  const [form, setForm] = useState<FormData>(emptyForm)

  const { data, isPending } = useQuery({
    queryKey: ['agents'],
    queryFn: () => agentsApi.list(),
  })

  const createMutation = useMutation({
    mutationFn: (payload: CreateAgentPayload) => agentsApi.create(payload),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ['agents'] }); setSheetOpen(false) },
  })

  const updateMutation = useMutation({
    mutationFn: ({ id, payload }: { id: string; payload: UpdateAgentPayload }) =>
      agentsApi.update(id, payload),
    onSuccess: () => { void qc.invalidateQueries({ queryKey: ['agents'] }); setSheetOpen(false) },
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => agentsApi.delete(id),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['agents'] }),
  })

  const toggleMutation = useMutation({
    mutationFn: ({ id, status }: { id: string; status: 'active' | 'stopped' }) =>
      agentsApi.update(id, { status }),
    onSuccess: () => void qc.invalidateQueries({ queryKey: ['agents'] }),
  })

  function openCreate() {
    setEditingAgent(null)
    setForm(emptyForm)
    setSheetOpen(true)
  }

  function openEdit(agent: AgentListItem) {
    setEditingAgent(agent)
    setForm({
      name: agent.name,
      description: agent.description,
      system_prompt: '',
      provider: agent.provider,
      model: agent.model,
      keywords: agent.keywords.join(', '),
      allowed_tools: agent.allowed_tools.join(', '),
    })
    setSheetOpen(true)
  }

  function handleSubmit(e: React.FormEvent) {
    e.preventDefault()
    const payload = toPayload(form)
    if (editingAgent) {
      updateMutation.mutate({ id: editingAgent.agent_id, payload })
    } else {
      createMutation.mutate(payload as CreateAgentPayload)
    }
  }

  const isSubmitting = createMutation.isPending || updateMutation.isPending

  return (
    <div className="space-y-4 max-w-5xl">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Agents</h1>
        <Button size="sm" onClick={openCreate}>
          <Plus className="h-4 w-4 mr-2" />
          New Agent
        </Button>
      </div>

      <div className="rounded-lg border border-border overflow-hidden">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Name</TableHead>
              <TableHead>Status</TableHead>
              <TableHead>Model</TableHead>
              <TableHead>Keywords</TableHead>
              <TableHead>Source</TableHead>
              <TableHead>Created</TableHead>
              <TableHead className="w-24" />
            </TableRow>
          </TableHeader>
          <TableBody>
            {isPending && (
              <TableRow>
                <TableCell colSpan={7} className="text-center text-muted-foreground py-8">
                  Loading…
                </TableCell>
              </TableRow>
            )}
            {!isPending && data?.data.length === 0 && (
              <TableRow>
                <TableCell colSpan={7} className="text-center text-muted-foreground py-8">
                  No agents yet
                </TableCell>
              </TableRow>
            )}
            {data?.data.map((agent) => (
              <TableRow key={agent.agent_id}>
                <TableCell>
                  <div>
                    <p className="font-medium text-sm">{agent.name}</p>
                    {agent.description && (
                      <p className="text-xs text-muted-foreground truncate max-w-48">{agent.description}</p>
                    )}
                  </div>
                </TableCell>
                <TableCell>
                  <div className="flex items-center gap-2">
                    <Switch
                      checked={agent.status === 'active'}
                      onCheckedChange={(checked) =>
                        toggleMutation.mutate({ id: agent.agent_id, status: checked ? 'active' : 'stopped' })
                      }
                      disabled={agent.source === 'static'}
                    />
                    <StatusBadge status={agent.status} />
                  </div>
                </TableCell>
                <TableCell className="text-xs text-muted-foreground font-mono">{agent.model}</TableCell>
                <TableCell>
                  <div className="flex flex-wrap gap-1">
                    {agent.keywords.slice(0, 3).map((k) => (
                      <span key={k} className="text-xs bg-secondary rounded px-1.5 py-0.5">{k}</span>
                    ))}
                    {agent.keywords.length > 3 && (
                      <span className="text-xs text-muted-foreground">+{agent.keywords.length - 3}</span>
                    )}
                  </div>
                </TableCell>
                <TableCell>
                  <span className="text-xs text-muted-foreground capitalize">{agent.source}</span>
                </TableCell>
                <TableCell className="text-xs text-muted-foreground">
                  {formatDistanceToNow(new Date(agent.created_at * 1000), { addSuffix: true })}
                </TableCell>
                <TableCell>
                  <div className="flex gap-1">
                    <Button
                      variant="ghost" size="icon" className="h-7 w-7"
                      onClick={() => setStatsAgent(agent)}
                      title="View stats"
                    >
                      <BarChart3 className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost" size="icon" className="h-7 w-7"
                      onClick={() => openEdit(agent)}
                      disabled={agent.source === 'static'}
                    >
                      <Pencil className="h-3.5 w-3.5" />
                    </Button>
                    <Button
                      variant="ghost" size="icon" className="h-7 w-7 text-destructive hover:text-destructive"
                      onClick={() => setDeleteTarget(agent)}
                      disabled={agent.source === 'static'}
                    >
                      <Trash2 className="h-3.5 w-3.5" />
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>

      {/* Create / Edit Sheet */}
      <Sheet open={sheetOpen} onOpenChange={setSheetOpen}>
        <SheetContent className="w-full sm:max-w-lg overflow-y-auto">
          <SheetHeader>
            <SheetTitle>{editingAgent ? 'Edit Agent' : 'New Agent'}</SheetTitle>
          </SheetHeader>
          <form onSubmit={handleSubmit} className="space-y-4 py-4">
            <div className="space-y-1">
              <Label>Name *</Label>
              <Input value={form.name} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} required />
            </div>
            <div className="space-y-1">
              <Label>Description</Label>
              <Input value={form.description} onChange={(e) => setForm((f) => ({ ...f, description: e.target.value }))} />
            </div>
            <div className="space-y-1">
              <Label>Provider</Label>
              <Input value={form.provider} onChange={(e) => setForm((f) => ({ ...f, provider: e.target.value }))} placeholder="anthropic" />
            </div>
            <div className="space-y-1">
              <Label>Model</Label>
              <Input value={form.model} onChange={(e) => setForm((f) => ({ ...f, model: e.target.value }))} placeholder="claude-sonnet-4-6" />
            </div>
            <div className="space-y-1">
              <Label>System Prompt</Label>
              <textarea
                value={form.system_prompt}
                onChange={(e) => setForm((f) => ({ ...f, system_prompt: e.target.value }))}
                rows={4}
                className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm resize-none focus:outline-none focus:ring-1 focus:ring-ring"
                placeholder="You are a helpful assistant…"
              />
            </div>
            <div className="space-y-1">
              <Label>Keywords <span className="text-muted-foreground text-xs">(comma-separated)</span></Label>
              <Input value={form.keywords} onChange={(e) => setForm((f) => ({ ...f, keywords: e.target.value }))} placeholder="travel, booking, flights" />
            </div>
            <div className="space-y-1">
              <Label>Allowed Tools <span className="text-muted-foreground text-xs">(comma-separated)</span></Label>
              <Input value={form.allowed_tools} onChange={(e) => setForm((f) => ({ ...f, allowed_tools: e.target.value }))} placeholder="web_search, read_file" />
            </div>
            {(createMutation.error || updateMutation.error) && (
              <p className="text-xs text-destructive">
                {(createMutation.error ?? updateMutation.error)?.message}
              </p>
            )}
            <SheetFooter>
              <Button type="button" variant="outline" onClick={() => setSheetOpen(false)}>Cancel</Button>
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting ? 'Saving…' : editingAgent ? 'Update' : 'Create'}
              </Button>
            </SheetFooter>
          </form>
        </SheetContent>
      </Sheet>

      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(v) => { if (!v) setDeleteTarget(null) }}
        title="Delete Agent"
        description={`Delete "${deleteTarget?.name}"? This cannot be undone.`}
        confirmLabel="Delete"
        destructive
        onConfirm={() => {
          if (deleteTarget) deleteMutation.mutate(deleteTarget.agent_id)
          setDeleteTarget(null)
        }}
      />

      {/* Agent stats panel */}
      <Sheet open={statsAgent !== null} onOpenChange={(v) => { if (!v) setStatsAgent(null) }}>
        <SheetContent className="w-full sm:max-w-2xl overflow-y-auto">
          <SheetHeader>
            <SheetTitle>{statsAgent?.name} — Stats</SheetTitle>
          </SheetHeader>
          <div className="py-4">
            {statsAgent && <AgentCostChart agentId={statsAgent.agent_id} />}
          </div>
        </SheetContent>
      </Sheet>
    </div>
  )
}
