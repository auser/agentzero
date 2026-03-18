import type { RunEventItem } from '@/lib/api/runs'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

interface ToolTimelineProps {
  events: RunEventItem[]
}

const TOOL_COLORS: Record<string, string> = {}
const PALETTE = [
  '#3b82f6', '#22c55e', '#eab308', '#ef4444', '#8b5cf6',
  '#ec4899', '#14b8a6', '#f97316', '#6366f1', '#84cc16',
]
let colorIdx = 0

function getToolColor(toolName: string): string {
  if (!TOOL_COLORS[toolName]) {
    TOOL_COLORS[toolName] = PALETTE[colorIdx % PALETTE.length]
    colorIdx++
  }
  return TOOL_COLORS[toolName]
}

export function ToolTimeline({ events }: ToolTimelineProps) {
  const toolCalls = events.filter((e) => e.type === 'tool_call' && e.tool)

  if (toolCalls.length === 0) {
    return (
      <div className="text-xs text-muted-foreground text-center py-4">
        No tool calls recorded
      </div>
    )
  }

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-xs">Tool Call Timeline</CardTitle>
      </CardHeader>
      <CardContent>
        <div className="flex flex-col gap-1">
          {toolCalls.map((event, i) => {
            const toolName = event.tool ?? 'unknown'
            const color = getToolColor(toolName)
            return (
              <div key={i} className="flex items-center gap-2 group">
                <span className="text-xs text-muted-foreground w-6 text-right shrink-0">
                  {i + 1}
                </span>
                <div
                  className="h-5 rounded flex items-center px-2 text-xs font-mono text-white truncate min-w-0"
                  style={{ backgroundColor: color, flex: 1, maxWidth: '100%' }}
                  title={`${toolName}${event.result ? `: ${event.result}` : ''}`}
                >
                  {toolName}
                </div>
                {event.error && (
                  <span className="text-xs text-destructive shrink-0">err</span>
                )}
              </div>
            )
          })}
        </div>
        <div className="flex flex-wrap gap-2 mt-3 pt-2 border-t border-border">
          {Object.entries(TOOL_COLORS).map(([name, color]) => (
            <div key={name} className="flex items-center gap-1">
              <div className="w-2.5 h-2.5 rounded-sm" style={{ backgroundColor: color }} />
              <span className="text-xs text-muted-foreground">{name}</span>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}
