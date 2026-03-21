/**
 * Draggable palette of agents, tools, and channels.
 * Collapsible sections with search filter and compact items.
 */
import { useQuery } from '@tanstack/react-query'
import { agentsApi } from '@/lib/api/agents'
import { api } from '@/lib/api/client'
import type { Port } from '@auser/workflow-graph-web'
import { Bot, Wrench, Radio, GripVertical, ChevronDown, ChevronRight, Search } from 'lucide-react'
import { type DragEvent, useState, useMemo } from 'react'
import { portsForNodeType } from '@/components/workflows/WorkflowCanvas'

interface ToolInfo {
  name: string
  description?: string
}

interface ToolsResponse {
  tools: ToolInfo[]
}

interface ConfigResponse {
  channels?: Record<string, { enabled?: boolean }>
}

/** Data transferred during drag. Encoded as JSON in dataTransfer. */
export interface DragNodeData {
  nodeType: 'agent' | 'tool' | 'channel'
  id: string
  name: string
  metadata: Record<string, unknown>
  ports: Port[]
}

function DraggableItem({
  data,
  name,
  detail,
  color,
}: {
  data: DragNodeData
  name: string
  detail?: string
  color: string
}) {
  const handleDragStart = (e: DragEvent<HTMLDivElement>) => {
    e.dataTransfer.setData('application/workflow-node', JSON.stringify(data))
    e.dataTransfer.effectAllowed = 'copy'
  }

  return (
    <div
      draggable
      onDragStart={handleDragStart}
      className="flex items-center gap-1.5 px-2 py-1 text-[11px] cursor-grab active:cursor-grabbing hover:bg-muted/40 rounded transition-colors group"
    >
      <GripVertical className="h-3 w-3 text-muted-foreground/20 group-hover:text-muted-foreground/50 shrink-0" />
      <span
        className="h-2 w-2 rounded-full shrink-0"
        style={{ backgroundColor: color }}
      />
      <span className="truncate font-medium">{name}</span>
      {detail && (
        <span className="text-[9px] text-muted-foreground/40 ml-auto truncate max-w-[80px]">
          {detail}
        </span>
      )}
    </div>
  )
}

function CollapsibleSection({
  icon,
  label,
  count,
  color,
  defaultOpen = true,
  children,
}: {
  icon: React.ReactNode
  label: string
  count: number
  color: string
  defaultOpen?: boolean
  children: React.ReactNode
}) {
  const [open, setOpen] = useState(defaultOpen)

  return (
    <div>
      <button
        onClick={() => setOpen(!open)}
        className="flex items-center justify-between w-full px-3 py-1.5 hover:bg-muted/20 transition-colors"
      >
        <span className="flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider" style={{ color }}>
          {open ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          {icon}
          {label}
        </span>
        <span className="text-[9px] text-muted-foreground/50 bg-muted/30 px-1.5 py-0.5 rounded-full">
          {count}
        </span>
      </button>
      {open && <div className="pb-1">{children}</div>}
    </div>
  )
}

export function DraggablePalette() {
  const [search, setSearch] = useState('')

  const { data: agents } = useQuery({
    queryKey: ['agents'],
    queryFn: () => agentsApi.list(),
  })

  const { data: toolsData } = useQuery({
    queryKey: ['tools'],
    queryFn: () => api.get<ToolsResponse>('/v1/tools'),
    retry: false,
  })

  const { data: configData } = useQuery({
    queryKey: ['config'],
    queryFn: () => api.get<ConfigResponse>('/v1/config'),
    retry: false,
  })

  const agentList = agents?.data ?? []
  const allTools = toolsData?.tools ?? []
  const channels = Object.entries(configData?.channels ?? {}).map(([name, cfg]) => ({
    name,
    connected: cfg.enabled !== false,
  }))

  // Filter by search
  const filter = search.toLowerCase()
  const filteredAgents = useMemo(
    () => agentList.filter((a) => a.name.toLowerCase().includes(filter)),
    [agentList, filter],
  )
  const filteredTools = useMemo(
    () => allTools.filter((t) => t.name.toLowerCase().includes(filter) || t.description?.toLowerCase().includes(filter)),
    [allTools, filter],
  )
  const filteredChannels = useMemo(
    () => channels.filter((c) => c.name.toLowerCase().includes(filter)),
    [channels, filter],
  )

  // Categorize tools
  const toolCategories = useMemo(() => {
    const cats: Record<string, ToolInfo[]> = {
      'File & Search': [],
      'Memory': [],
      'Agents': [],
      'System': [],
      'Other': [],
    }
    for (const tool of filteredTools) {
      const name = tool.name
      if (name.startsWith('memory_')) cats['Memory'].push(tool)
      else if (['read_file', 'glob_search', 'content_search', 'pdf_read', 'image_info', 'docx_read'].includes(name)) cats['File & Search'].push(tool)
      else if (['subagent_spawn', 'subagent_list', 'subagent_manage', 'delegate_coordination_status', 'agents_ipc'].includes(name)) cats['Agents'].push(tool)
      else if (['shell', 'process', 'screenshot', 'task_plan', 'cli_discovery', 'proxy_config'].includes(name)) cats['System'].push(tool)
      else cats['Other'].push(tool)
    }
    // Remove empty categories
    return Object.entries(cats).filter(([, tools]) => tools.length > 0)
  }, [filteredTools])

  return (
    <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm overflow-hidden h-full flex flex-col">
      {/* Header */}
      <div className="px-3 py-2.5 border-b border-border/50">
        <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground mb-2">
          Components
        </h3>
        {/* Search */}
        <div className="relative">
          <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground/40" />
          <input
            type="text"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder="Filter..."
            className="w-full h-6 pl-6 pr-2 text-[11px] rounded border border-border/50 bg-background/50 focus:ring-1 focus:ring-ring outline-none placeholder:text-muted-foreground/30"
          />
        </div>
      </div>

      {/* Scrollable content */}
      <div className="overflow-y-auto flex-1">
        {/* Agents */}
        {filteredAgents.length > 0 && (
          <CollapsibleSection
            icon={<Bot className="h-3 w-3" />}
            label="Agents"
            count={filteredAgents.length}
            color="#3b82f6"
          >
            {filteredAgents.map((a) => (
              <DraggableItem
                key={a.agent_id}
                name={a.name}
                detail={a.model}
                color={a.status === 'active' ? '#22c55e' : '#6b7280'}
                data={{
                  nodeType: 'agent',
                  id: a.agent_id,
                  name: a.name,
                  metadata: {
                    node_type: 'agent',
                    model: a.model,
                    description: a.description,
                    status: a.status,
                  },
                  ports: portsForNodeType('agent'),
                }}
              />
            ))}
          </CollapsibleSection>
        )}

        {/* Tool categories */}
        {toolCategories.map(([category, tools]) => (
          <CollapsibleSection
            key={category}
            icon={<Wrench className="h-3 w-3" />}
            label={category}
            count={tools.length}
            color="#8b5cf6"
            defaultOpen={category === 'File & Search' || category === 'System'}
          >
            {tools.map((t) => (
              <DraggableItem
                key={t.name}
                name={t.name}
                detail={t.description?.slice(0, 25)}
                color="#8b5cf6"
                data={{
                  nodeType: 'tool',
                  id: `tool-${t.name}`,
                  name: t.name,
                  metadata: {
                    node_type: 'tool',
                    tool_name: t.name,
                    description: t.description,
                  },
                  ports: portsForNodeType('tool'),
                }}
              />
            ))}
          </CollapsibleSection>
        ))}

        {/* Channels */}
        {filteredChannels.length > 0 && (
          <CollapsibleSection
            icon={<Radio className="h-3 w-3" />}
            label="Channels"
            count={filteredChannels.length}
            color="#ec4899"
          >
            {filteredChannels.map((ch) => (
              <DraggableItem
                key={ch.name}
                name={ch.name}
                detail={ch.connected ? 'connected' : 'off'}
                color={ch.connected ? '#22c55e' : '#6b7280'}
                data={{
                  nodeType: 'channel',
                  id: `channel-${ch.name}`,
                  name: ch.name,
                  metadata: {
                    node_type: 'channel',
                    channel_type: ch.name,
                    connected: ch.connected,
                  },
                  ports: portsForNodeType('channel'),
                }}
              />
            ))}
          </CollapsibleSection>
        )}
      </div>
    </div>
  )
}
