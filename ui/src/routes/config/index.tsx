import { type ReactNode } from 'react'
import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api/client'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'

export const Route = createFileRoute('/config/')({
  component: ConfigPage,
})

interface ConfigSection {
  key: string
  value: unknown
}

interface ConfigResponse {
  sections: ConfigSection[]
  raw?: string
}

function renderValue(value: unknown, depth = 0): ReactNode {
  if (value === null || value === undefined) return <span className="text-muted-foreground">null</span>
  if (typeof value === 'boolean') return <span className={value ? 'text-emerald-400' : 'text-red-400'}>{String(value)}</span>
  if (typeof value === 'number') return <span className="text-blue-400">{value}</span>
  if (typeof value === 'string') return <span className="text-amber-300">"{value}"</span>
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

function ConfigPage() {
  const { data, isPending, error } = useQuery({
    queryKey: ['config'],
    queryFn: () => api.get<ConfigResponse>('/v1/config'),
    retry: false,
  })

  return (
    <div className="space-y-4 max-w-3xl">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold">Config</h1>
        <Badge variant="outline" className="text-xs">Read-only view</Badge>
      </div>

      {isPending && <p className="text-sm text-muted-foreground">Loading…</p>}

      {error && (
        <div className="rounded-lg border border-border p-4 space-y-2">
          <p className="text-sm font-medium">Config endpoint not yet available</p>
          <p className="text-xs text-muted-foreground">
            Add <code className="bg-muted px-1 rounded">GET /v1/config</code> to the gateway to enable this page.
          </p>
          <p className="text-xs text-muted-foreground">
            Until then, use the CLI: <code className="bg-muted px-1 rounded">agentzero config show</code>
          </p>
        </div>
      )}

      {data?.raw && (
        <ScrollArea className="h-[70vh] rounded-lg border border-border">
          <pre className="p-4 text-xs font-mono whitespace-pre-wrap">{data.raw}</pre>
        </ScrollArea>
      )}

      {data?.sections && (
        <Accordion type="multiple" className="space-y-2">
          {data.sections.map((section) => (
            <AccordionItem key={section.key} value={section.key} className="border border-border rounded-lg px-4">
              <AccordionTrigger className="py-3">
                <span className="text-sm font-mono">[{section.key}]</span>
              </AccordionTrigger>
              <AccordionContent>
                <ScrollArea className="max-h-64 pb-3">
                  <div className="font-mono text-xs pb-2">
                    {renderValue(section.value)}
                  </div>
                </ScrollArea>
              </AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>
      )}
    </div>
  )
}
