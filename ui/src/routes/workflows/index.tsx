import { createFileRoute, Link } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { workflowsApi, type WorkflowRecord } from '@/lib/api/workflows'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
import { Plus, Trash2, Pencil, FileEdit, GitBranch } from 'lucide-react'
import { formatDistanceToNow } from 'date-fns'

export const Route = createFileRoute('/workflows/')({
  component: WorkflowsPage,
})

function WorkflowsPage() {
  const queryClient = useQueryClient()
  const [createName, setCreateName] = useState('')
  const [deleteTarget, setDeleteTarget] = useState<WorkflowRecord | null>(null)

  const { data, isLoading } = useQuery({
    queryKey: ['workflows'],
    queryFn: () => workflowsApi.list('layout'),
    refetchInterval: 5_000,
  })

  const createMutation = useMutation({
    mutationFn: (name: string) =>
      workflowsApi.create({ name, description: '', layout: { nodes: [], edges: [] } }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['workflows'] })
      setCreateName('')
    },
  })

  const deleteMutation = useMutation({
    mutationFn: (id: string) => workflowsApi.delete(id),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['workflows'] })
      setDeleteTarget(null)
    },
  })

  const workflows = data?.data ?? []

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold">Workflows</h1>
          <p className="text-sm text-muted-foreground mt-1">
            Visual agent pipelines — connect agents, tools, and channels into executable workflows.
          </p>
        </div>
      </div>

      {/* Create new */}
      <div className="flex items-center gap-2">
        <Input
          placeholder="New workflow name..."
          value={createName}
          onChange={(e) => setCreateName(e.target.value)}
          className="max-w-xs h-8 text-sm"
          onKeyDown={(e) => {
            if (e.key === 'Enter' && createName.trim()) {
              createMutation.mutate(createName.trim())
            }
          }}
        />
        <Button
          size="sm"
          className="h-8"
          disabled={!createName.trim() || createMutation.isPending}
          onClick={() => createMutation.mutate(createName.trim())}
        >
          <Plus className="h-3.5 w-3.5 mr-1" />
          Create
        </Button>
      </div>

      {/* Workflow list */}
      {isLoading ? (
        <div className="text-sm text-muted-foreground">Loading workflows...</div>
      ) : workflows.length === 0 ? (
        <div className="border border-dashed border-border rounded-lg p-12 text-center">
          <GitBranch className="h-10 w-10 mx-auto text-muted-foreground/40 mb-3" />
          <p className="text-sm text-muted-foreground">No workflows yet.</p>
          <p className="text-xs text-muted-foreground/60 mt-1">
            Create one above or import a template from the editor.
          </p>
        </div>
      ) : (
        <div className="grid gap-3">
          {workflows.map((wf) => (
            <WorkflowCard
              key={wf.workflow_id}
              workflow={wf}
              onDelete={() => setDeleteTarget(wf)}
            />
          ))}
        </div>
      )}

      {/* Delete confirmation */}
      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete Workflow"
        description={`Delete "${deleteTarget?.name}"? This cannot be undone.`}
        onConfirm={() => deleteTarget && deleteMutation.mutate(deleteTarget.workflow_id)}
        onCancel={() => setDeleteTarget(null)}
        confirmLabel="Delete"
        variant="destructive"
      />
    </div>
  )
}

function WorkflowCard({ workflow, onDelete }: { workflow: WorkflowRecord; onDelete: () => void }) {
  const queryClient = useQueryClient()
  const [editing, setEditing] = useState(false)
  const [editName, setEditName] = useState(workflow.name)
  const [editDesc, setEditDesc] = useState(workflow.description ?? '')

  const updateMutation = useMutation({
    mutationFn: (payload: { name?: string; description?: string }) =>
      workflowsApi.update(workflow.workflow_id, payload),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['workflows'] })
      setEditing(false)
    },
  })

  const nodeCount = (workflow.layout?.nodes as unknown[] | undefined)?.length ?? 0
  const edgeCount = (workflow.layout?.edges as unknown[] | undefined)?.length ?? 0
  const updatedAgo = workflow.updated_at
    ? formatDistanceToNow(new Date(workflow.updated_at * 1000), { addSuffix: true })
    : 'unknown'

  const commitEdit = () => {
    const trimmedName = editName.trim()
    const trimmedDesc = editDesc.trim()
    const nameChanged = trimmedName && trimmedName !== workflow.name
    const descChanged = trimmedDesc !== (workflow.description ?? '')
    if (nameChanged || descChanged) {
      const payload: { name?: string; description?: string } = {}
      if (nameChanged) payload.name = trimmedName
      if (descChanged) payload.description = trimmedDesc
      updateMutation.mutate(payload)
    } else {
      setEditName(workflow.name)
      setEditDesc(workflow.description ?? '')
      setEditing(false)
    }
  }

  const cancelEdit = () => {
    setEditName(workflow.name)
    setEditDesc(workflow.description ?? '')
    setEditing(false)
  }

  return (
    <div className="flex items-center justify-between border border-border/60 rounded-lg px-4 py-3 bg-card/50 hover:bg-card/80 transition-colors">
      <div className="flex items-center gap-3 min-w-0 flex-1">
        <div className="h-9 w-9 rounded-md bg-primary/10 flex items-center justify-center shrink-0">
          <GitBranch className="h-4 w-4 text-primary" />
        </div>
        <div className="min-w-0 flex-1">
          {editing ? (
            <div className="space-y-1">
              <Input
                value={editName}
                onChange={(e) => setEditName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commitEdit()
                  if (e.key === 'Escape') cancelEdit()
                }}
                placeholder="Workflow name"
                className="h-6 text-sm font-medium px-1 py-0 max-w-xs"
                autoFocus
              />
              <Input
                value={editDesc}
                onChange={(e) => setEditDesc(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter') commitEdit()
                  if (e.key === 'Escape') cancelEdit()
                }}
                placeholder="Add a description..."
                className="h-5 text-xs px-1 py-0 max-w-md text-muted-foreground"
              />
              <div className="flex gap-1 pt-0.5">
                <Button size="sm" className="h-5 text-[10px] px-2" onClick={commitEdit}>Save</Button>
                <Button size="sm" variant="ghost" className="h-5 text-[10px] px-2" onClick={cancelEdit}>Cancel</Button>
              </div>
            </div>
          ) : (
            <>
              <Link
                to="/workflows/$workflowId"
                params={{ workflowId: workflow.workflow_id }}
                className="text-sm font-medium hover:underline block truncate"
              >
                {workflow.name || 'Untitled'}
              </Link>
              {workflow.description && (
                <p className="text-xs text-muted-foreground/70 truncate">{workflow.description}</p>
              )}
            </>
          )}
          <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
            <span>{nodeCount} nodes</span>
            <span className="text-muted-foreground/30">|</span>
            <span>{edgeCount} edges</span>
            <span className="text-muted-foreground/30">|</span>
            <span>updated {updatedAgo}</span>
          </div>
        </div>
      </div>

      <div className="flex items-center gap-1 shrink-0">
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7"
          onClick={() => { setEditName(workflow.name); setEditDesc(workflow.description ?? ''); setEditing(true) }}
          title="Edit name & description"
        >
          <Pencil className="h-3.5 w-3.5" />
        </Button>
        <Button variant="ghost" size="icon" className="h-7 w-7" asChild>
          <Link to="/workflows/$workflowId" params={{ workflowId: workflow.workflow_id }}>
            <FileEdit className="h-3.5 w-3.5" />
          </Link>
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-destructive hover:text-destructive"
          onClick={onDelete}
        >
          <Trash2 className="h-3.5 w-3.5" />
        </Button>
      </div>
    </div>
  )
}
