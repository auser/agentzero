import { type ReactNode, useState } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api/client'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Pencil, Save, X } from 'lucide-react'

export const Route = createFileRoute('/config/')({
  component: ConfigPage,
})

interface ConfigSection {
  key: string
  value: unknown
}

interface ConfigResponse {
  sections: ConfigSection[]
}

function renderValue(value: unknown, depth = 0): ReactNode {
  if (value === null || value === undefined) return <span className="text-muted-foreground">null</span>
  if (typeof value === 'boolean') return <span className={value ? 'text-emerald-400' : 'text-red-400'}>{String(value)}</span>
  if (typeof value === 'number') return <span className="text-blue-400">{value}</span>
  if (typeof value === 'string') return <span className="text-amber-300">&quot;{value}&quot;</span>
  if (Array.isArray(value)) {
    if (value.length === 0) return <span className="text-muted-foreground">[]</span>
    return (
      <div className={depth > 0 ? 'ml-4' : ''}>
        {value.map((item, i) => (
          <div key={i} className="flex items-start gap-1">
            <span className="text-muted-foreground">-</span>
            {renderValue(item, depth + 1)}
          </div>
        ))}
      </div>
    )
  }
  if (typeof value === 'object') {
    return (
      <div className={depth > 0 ? 'ml-4' : ''}>
        {Object.entries(value as Record<string, unknown>).map(([k, v]) => (
          <div key={k} className="flex items-start gap-1.5 py-0.5">
            <span className="text-purple-300 shrink-0">{k}:</span>
            {renderValue(v, depth + 1)}
          </div>
        ))}
      </div>
    )
  }
  return <span>{String(value)}</span>
}

function SectionEditor({ section, onSaved }: { section: ConfigSection; onSaved: () => void }) {
  const [editing, setEditing] = useState(false)
  const [json, setJson] = useState('')
  const [error, setError] = useState('')

  const saveMutation = useMutation({
    mutationFn: (value: unknown) =>
      api.put<{ updated: boolean }>('/v1/config', {
        sections: [{ key: section.key, value }],
      }),
    onSuccess: () => {
      setEditing(false)
      setError('')
      onSaved()
    },
    onError: (err) => {
      setError(err instanceof Error ? err.message : 'Save failed')
    },
  })

  function startEdit() {
    setJson(JSON.stringify(section.value, null, 2))
    setError('')
    setEditing(true)
  }

  function handleSave() {
    try {
      const parsed = JSON.parse(json)
      saveMutation.mutate(parsed)
    } catch {
      setError('Invalid JSON')
    }
  }

  return (
    <AccordionItem value={section.key} className="border border-border rounded-lg px-4">
      <AccordionTrigger className="py-3">
        <span className="text-sm font-mono">[{section.key}]</span>
      </AccordionTrigger>
      <AccordionContent>
        {editing ? (
          <div className="space-y-2 pb-3">
            <textarea
              value={json}
              onChange={(e) => setJson(e.target.value)}
              rows={Math.min(20, json.split('\n').length + 2)}
              className="w-full rounded-md border border-input bg-background px-3 py-2 text-xs font-mono resize-y focus:outline-none focus:ring-1 focus:ring-ring"
              spellCheck={false}
            />
            {error && <p className="text-xs text-destructive">{error}</p>}
            <div className="flex gap-2">
              <Button size="sm" onClick={handleSave} disabled={saveMutation.isPending}>
                <Save className="h-3 w-3 mr-1.5" />
                {saveMutation.isPending ? 'Saving…' : 'Save'}
              </Button>
              <Button size="sm" variant="outline" onClick={() => { setEditing(false); setError('') }}>
                <X className="h-3 w-3 mr-1.5" />
                Cancel
              </Button>
            </div>
          </div>
        ) : (
          <div className="pb-3">
            <div className="flex justify-end mb-2">
              <Button size="sm" variant="ghost" className="h-7 text-xs" onClick={startEdit}>
                <Pencil className="h-3 w-3 mr-1.5" />
                Edit
              </Button>
            </div>
            <ScrollArea className="max-h-64">
              <div className="font-mono text-xs">
                {renderValue(section.value)}
              </div>
            </ScrollArea>
          </div>
        )}
      </AccordionContent>
    </AccordionItem>
  )
}

function ConfigPage() {
  const qc = useQueryClient()

  const { data, isPending, error } = useQuery({
    queryKey: ['config'],
    queryFn: () => api.get<ConfigResponse>('/v1/config'),
    retry: false,
  })

  return (
    <div className="space-y-4 max-w-3xl">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Config</h1>
      </div>

      {isPending && <p className="text-sm text-muted-foreground">Loading…</p>}

      {error && (
        <div className="rounded-lg border border-border p-4 space-y-2">
          <p className="text-sm font-medium">Config endpoint not yet available</p>
          <p className="text-xs text-muted-foreground">
            Use the CLI: <code className="bg-muted px-1 rounded">agentzero config show</code>
          </p>
        </div>
      )}

      {data?.sections && (
        <Accordion type="multiple" className="space-y-2">
          {data.sections.map((section) => (
            <SectionEditor
              key={section.key}
              section={section}
              onSaved={() => void qc.invalidateQueries({ queryKey: ['config'] })}
            />
          ))}
        </Accordion>
      )}
    </div>
  )
}
