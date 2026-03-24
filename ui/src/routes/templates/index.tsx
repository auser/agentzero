import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useState, useMemo } from 'react'
import { workflowsApi } from '@/lib/api/workflows'
import { loadLocalTemplates, deleteLocalTemplate } from '@/lib/template-store'
import { ALL_TEMPLATES, type WorkflowTemplate } from '@/lib/workflow-templates'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetFooter } from '@/components/ui/sheet'
import { Label } from '@/components/ui/label'
import { Search, Trash2, Play, Pencil } from 'lucide-react'
import { formatDistanceToNow } from 'date-fns'
import { useNavigate } from '@tanstack/react-router'

export const Route = createFileRoute('/templates/')({
  component: TemplatesPage,
})

type TemplateEntry = {
  id: string
  name: string
  description: string
  category: string
  nodeCount: number
  source: 'builtin' | 'api' | 'local'
  workflowId?: string
  localId?: string
  createdAt?: number
  template: WorkflowTemplate
}

const CATEGORY_COLORS: Record<string, string> = {
  research: 'bg-blue-500/10 text-blue-400',
  content: 'bg-violet-500/10 text-violet-400',
  engineering: 'bg-orange-500/10 text-orange-400',
  support: 'bg-pink-500/10 text-pink-400',
  analytics: 'bg-green-500/10 text-green-400',
  custom: 'bg-amber-500/10 text-amber-400',
}

function TemplatesPage() {
  const queryClient = useQueryClient()
  const navigate = useNavigate()
  const [search, setSearch] = useState('')
  const [deleteTarget, setDeleteTarget] = useState<TemplateEntry | null>(null)
  const [editTarget, setEditTarget] = useState<TemplateEntry | null>(null)
  const [editName, setEditName] = useState('')
  const [editDescription, setEditDescription] = useState('')
  const [localVersion, setLocalVersion] = useState(0)

  const { data: apiWorkflows } = useQuery({
    queryKey: ['workflows', 'templates'],
    queryFn: () => workflowsApi.list('layout'),
    staleTime: 0,
  })

  const templates = useMemo(() => {
    void localVersion
    const entries: TemplateEntry[] = []

    for (const t of ALL_TEMPLATES) {
      entries.push({
        id: t.id, name: t.name, description: t.description,
        category: t.category, nodeCount: t.nodeCount,
        source: 'builtin', template: t,
      })
    }

    for (const w of apiWorkflows?.data ?? []) {
      if (w.name === 'default') continue
      const nodes = (w.layout?.nodes ?? []) as WorkflowTemplate['nodes']
      const edges = (w.layout?.edges ?? []) as WorkflowTemplate['edges']
      if (nodes.length === 0) continue
      const tpl: WorkflowTemplate = {
        id: w.workflow_id, name: w.name,
        description: w.description || `${nodes.length} nodes`,
        category: 'custom', nodeCount: nodes.length, nodes, edges,
      }
      entries.push({
        id: w.workflow_id, name: w.name,
        description: w.description || `${nodes.length} nodes`,
        category: 'custom', nodeCount: nodes.length,
        source: 'api', workflowId: w.workflow_id,
        createdAt: w.created_at, template: tpl,
      })
    }

    const apiNames = new Set(entries.map((e) => e.name))
    for (const lt of loadLocalTemplates()) {
      if (apiNames.has(lt.name)) continue
      entries.push({
        id: lt.id, name: lt.name, description: lt.description,
        category: lt.category, nodeCount: lt.nodeCount,
        source: 'local', localId: lt.id,
        createdAt: lt.savedAt, template: lt,
      })
    }

    return entries
  }, [apiWorkflows, localVersion])

  const filtered = useMemo(() => {
    if (!search) return templates
    const q = search.toLowerCase()
    return templates.filter(
      (t) =>
        t.name.toLowerCase().includes(q) ||
        t.description.toLowerCase().includes(q) ||
        t.category.toLowerCase().includes(q),
    )
  }, [templates, search])

  const handleDelete = async () => {
    if (!deleteTarget) return
    if (deleteTarget.localId) {
      deleteLocalTemplate(deleteTarget.localId)
      setLocalVersion((v) => v + 1)
    }
    if (deleteTarget.workflowId) {
      try {
        await workflowsApi.delete(deleteTarget.workflowId)
        void queryClient.invalidateQueries({ queryKey: ['workflows', 'templates'] })
      } catch { /* API unavailable */ }
    }
    setDeleteTarget(null)
  }

  const handleUseTemplate = (entry: TemplateEntry) => {
    sessionStorage.setItem('agentzero-load-template', JSON.stringify(entry.template))
    void navigate({ to: '/dashboard' })
  }

  const handleOpenOnCanvas = (entry: TemplateEntry) => {
    sessionStorage.setItem('agentzero-load-template', JSON.stringify(entry.template))
    void navigate({ to: '/dashboard' })
  }

  const openEdit = (entry: TemplateEntry) => {
    setEditTarget(entry)
    setEditName(entry.name)
    setEditDescription(entry.description)
  }

  const handleEditSave = async () => {
    if (!editTarget || !editName.trim()) return
    if (editTarget.workflowId) {
      try {
        await workflowsApi.update(editTarget.workflowId, {
          name: editName.trim(),
          description: editDescription.trim(),
        })
        void queryClient.invalidateQueries({ queryKey: ['workflows', 'templates'] })
      } catch { /* API unavailable */ }
    }
    setEditTarget(null)
  }

  const customCount = templates.filter((t) => t.source !== 'builtin').length

  return (
    <div className="p-6 space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">Templates</h1>
          <p className="text-sm text-muted-foreground mt-1">
            {templates.length} templates ({ALL_TEMPLATES.length} built-in, {customCount} saved)
          </p>
        </div>
      </div>

      <div className="relative max-w-sm">
        <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
        <Input
          placeholder="Search templates..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="pl-9"
        />
      </div>

      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead className="w-[40%]">Template</TableHead>
              <TableHead>Category</TableHead>
              <TableHead className="text-right">Nodes</TableHead>
              <TableHead>Created</TableHead>
              <TableHead className="text-right">Actions</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {filtered.length === 0 ? (
              <TableRow>
                <TableCell colSpan={5} className="text-center text-muted-foreground py-12">
                  No templates found
                </TableCell>
              </TableRow>
            ) : (
              filtered.map((entry) => (
                <TableRow key={entry.id}>
                  <TableCell>
                    <div>
                      <div className="font-medium">{entry.name}</div>
                      <div className="text-xs text-muted-foreground line-clamp-1 mt-0.5">{entry.description}</div>
                    </div>
                  </TableCell>
                  <TableCell>
                    <span className={`text-[10px] font-medium uppercase tracking-wider px-2 py-1 rounded ${CATEGORY_COLORS[entry.category] ?? 'bg-muted text-muted-foreground'}`}>
                      {entry.category}
                    </span>
                  </TableCell>
                  <TableCell className="text-right tabular-nums">{entry.nodeCount}</TableCell>
                  <TableCell className="text-sm text-muted-foreground">
                    {entry.createdAt
                      ? formatDistanceToNow(entry.createdAt * 1000, { addSuffix: true })
                      : entry.source === 'builtin' ? 'Built-in' : '—'}
                  </TableCell>
                  <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-2">
                      <Button
                        variant="outline"
                        size="sm"
                        className="h-7 text-xs"
                        onClick={() => handleUseTemplate(entry)}
                      >
                        <Play className="h-3 w-3 mr-1" />
                        Use
                      </Button>
                      {entry.source !== 'builtin' && (
                        <>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-7 text-xs"
                            onClick={() => openEdit(entry)}
                          >
                            <Pencil className="h-3 w-3 mr-1" />
                            Edit
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-7 text-xs text-muted-foreground hover:text-destructive"
                            onClick={() => setDeleteTarget(entry)}
                          >
                            <Trash2 className="h-3 w-3 mr-1" />
                            Delete
                          </Button>
                        </>
                      )}
                    </div>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
        </Table>
      </div>

      <ConfirmDialog
        open={deleteTarget !== null}
        onOpenChange={(open) => { if (!open) setDeleteTarget(null) }}
        title="Delete Template"
        description={`Delete "${deleteTarget?.name}"? This cannot be undone.`}
        confirmLabel="Delete"
        destructive
        onConfirm={handleDelete}
      />

      <Sheet open={editTarget !== null} onOpenChange={(open) => { if (!open) setEditTarget(null) }}>
        <SheetContent>
          <SheetHeader>
            <SheetTitle>Edit Template</SheetTitle>
          </SheetHeader>
          <div className="space-y-4 py-4">
            <div className="space-y-2">
              <Label>Name</Label>
              <Input value={editName} onChange={(e) => setEditName(e.target.value)} />
            </div>
            <div className="space-y-2">
              <Label>Description</Label>
              <Input value={editDescription} onChange={(e) => setEditDescription(e.target.value)} />
            </div>
            <p className="text-xs text-muted-foreground">
              {editTarget?.nodeCount} nodes
            </p>
          </div>
          <SheetFooter className="gap-2">
            <Button
              variant="outline"
              onClick={() => {
                if (editTarget) handleOpenOnCanvas(editTarget)
              }}
            >
              Open on Canvas
            </Button>
            <Button variant="outline" onClick={() => setEditTarget(null)}>Cancel</Button>
            <Button onClick={handleEditSave} disabled={!editName.trim()}>Save</Button>
          </SheetFooter>
        </SheetContent>
      </Sheet>
    </div>
  )
}
