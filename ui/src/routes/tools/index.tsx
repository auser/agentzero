import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { api } from '@/lib/api/client'
import { Accordion, AccordionContent, AccordionItem, AccordionTrigger } from '@/components/ui/accordion'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'

export const Route = createFileRoute('/tools/')({
  component: ToolsPage,
})

interface ToolSummary {
  name: string
  description: string
  category: string
  gate_flag?: string
  input_schema?: Record<string, unknown>
}

interface ToolsResponse {
  tools: ToolSummary[]
}

const CATEGORY_ORDER = ['file', 'web', 'execution', 'memory', 'scheduling', 'delegation', 'media', 'hardware', 'other']

function ToolsPage() {
  const { data, isPending } = useQuery({
    queryKey: ['tools'],
    queryFn: () => api.get<ToolsResponse>('/v1/tools'),
  })

  const byCategory: Record<string, ToolSummary[]> = {}
  for (const t of data?.tools ?? []) {
    const cat = t.category || 'other'
    ;(byCategory[cat] ??= []).push(t)
  }

  const categories = [
    ...CATEGORY_ORDER.filter((c) => byCategory[c]),
    ...Object.keys(byCategory).filter((c) => !CATEGORY_ORDER.includes(c)),
  ]

  return (
    <div className="space-y-4 max-w-3xl">
      <h1 className="text-xl font-semibold">Tools</h1>

      {isPending && <p className="text-sm text-muted-foreground">Loading…</p>}

      {!isPending && categories.length === 0 && (
        <p className="text-sm text-muted-foreground">
          No tools endpoint yet — add <code className="text-xs bg-muted px-1 rounded">GET /v1/tools</code> to the gateway.
        </p>
      )}

      <Accordion type="multiple" className="space-y-2">
        {categories.map((cat) => (
          <AccordionItem key={cat} value={cat} className="border border-border rounded-lg px-4">
            <AccordionTrigger className="py-3">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium capitalize">{cat}</span>
                <Badge variant="secondary" className="text-xs">{byCategory[cat].length}</Badge>
              </div>
            </AccordionTrigger>
            <AccordionContent>
              <div className="space-y-3 pb-2">
                {byCategory[cat].map((tool) => (
                  <div key={tool.name} className="rounded-md border border-border p-3 space-y-2">
                    <div className="flex items-start justify-between gap-2">
                      <div>
                        <p className="text-sm font-mono font-medium">{tool.name}</p>
                        <p className="text-xs text-muted-foreground mt-0.5">{tool.description}</p>
                      </div>
                      {tool.gate_flag && (
                        <Badge variant="outline" className="text-xs shrink-0">{tool.gate_flag}</Badge>
                      )}
                    </div>
                    {tool.input_schema && (
                      <details className="text-xs">
                        <summary className="cursor-pointer text-muted-foreground hover:text-foreground select-none">
                          View schema
                        </summary>
                        <ScrollArea className="mt-1 max-h-48">
                          <pre className="bg-muted rounded p-2 font-mono text-xs overflow-x-auto">
                            {JSON.stringify(tool.input_schema, null, 2)}
                          </pre>
                        </ScrollArea>
                      </details>
                    )}
                  </div>
                ))}
              </div>
            </AccordionContent>
          </AccordionItem>
        ))}
      </Accordion>
    </div>
  )
}
