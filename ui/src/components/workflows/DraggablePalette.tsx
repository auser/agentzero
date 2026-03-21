/**
 * Draggable palette of agents, tools, and channels.
 * Items rendered as small draggable node chips.
 */
import { useQuery } from '@tanstack/react-query'
import { agentsApi } from '@/lib/api/agents'
import { api } from '@/lib/api/client'
import type { Port } from '@auser/workflow-graph-web'
import { Bot, Wrench, Radio, ChevronDown, ChevronRight, Search } from 'lucide-react'
import { type DragEvent, useState, useMemo } from 'react'
import { portsForNodeType } from '@/components/workflows/WorkflowCanvas'
import { NODE_TYPES, type NodeType } from '@/lib/workflow-theme'

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

export interface DragNodeData {
  nodeType: 'agent' | 'tool' | 'channel'
  id: string
  name: string
  metadata: Record<string, unknown>
  ports: Port[]
}

function NodeChip({
  data,
  name,
  nodeType,
}: {
  data: DragNodeData
  name: string
  nodeType: string
}) {
  const handleDragStart = (e: DragEvent<HTMLDivElement>) => {
    e.dataTransfer.setData('application/workflow-node', JSON.stringify(data))
    e.dataTransfer.effectAllowed = 'copy'
  }

  const theme = NODE_TYPES[nodeType as NodeType] ?? NODE_TYPES.tool

  return (
    <div
      draggable
      onDragStart={handleDragStart}
      className={`inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md border cursor-grab active:cursor-grabbing hover:brightness-125 transition-all select-none text-[11px] font-medium text-foreground ${theme.tailwind.bg} ${theme.tailwind.border}`}
    >
      <span className={`h-2 w-2 rounded-full shrink-0 ${theme.tailwind.dot}`} />
      {name}
    </div>
  )
}

function CollapsibleSection({
  icon,
  label,
  count,
  textClass,
  defaultOpen = true,
  children,
}: {
  icon: React.ReactNode
  label: string
  count: number
  textClass: string
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
        <span className={`flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider ${textClass}`}>
          {open ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
          {icon}
          {label}
        </span>
        <span className="text-[9px] text-muted-foreground/50 bg-muted/30 px-1.5 py-0.5 rounded-full">
          {count}
        </span>
      </button>
      {open && <div className="px-2 pb-2 flex flex-wrap gap-1.5">{children}</div>}
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

  const toolCategories = useMemo(() => {
    const cats: Record<string, ToolInfo[]> = {
      'File & Search': [],
      'Memory': [],
      'Agents': [],
      'System': [],
      'Other': [],
    }
    for (const tool of filteredTools) {
      const n = tool.name
      if (n.startsWith('memory_')) cats['Memory'].push(tool)
      else if (['read_file', 'glob_search', 'content_search', 'pdf_read', 'image_info', 'docx_read'].includes(n)) cats['File & Search'].push(tool)
      else if (['subagent_spawn', 'subagent_list', 'subagent_manage', 'delegate_coordination_status', 'agents_ipc'].includes(n)) cats['Agents'].push(tool)
      else if (['shell', 'process', 'screenshot', 'task_plan', 'cli_discovery', 'proxy_config'].includes(n)) cats['System'].push(tool)
      else cats['Other'].push(tool)
    }
    return Object.entries(cats).filter(([, tools]) => tools.length > 0)
  }, [filteredTools])

  return (
    <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm overflow-hidden h-full flex flex-col">
      <div className="px-3 py-2.5 border-b border-border/50">
        <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground mb-2">
          Components
        </h3>
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

      <div className="overflow-y-auto flex-1">
        {filteredAgents.length > 0 && (
          <CollapsibleSection icon={<Bot className="h-3 w-3" />} label="Agents" count={filteredAgents.length} textClass="text-blue-500">
            {filteredAgents.map((a) => (
              <NodeChip
                key={a.agent_id}
                name={a.name}
                nodeType="agent"
                data={{
                  nodeType: 'agent', id: a.agent_id, name: a.name,
                  metadata: { node_type: 'agent', model: a.model, description: a.description, status: a.status },
                  ports: portsForNodeType('agent'),
                }}
              />
            ))}
          </CollapsibleSection>
        )}

        {toolCategories.map(([category, tools]) => (
          <CollapsibleSection
            key={category}
            icon={<Wrench className="h-3 w-3" />}
            label={category}
            count={tools.length}
            textClass="text-violet-500"
            defaultOpen={tools.length <= 6}
          >
            {tools.map((t) => (
              <NodeChip
                key={t.name}
                name={t.name}
                nodeType="tool"
                data={{
                  nodeType: 'tool', id: `tool-${t.name}`, name: t.name,
                  metadata: { node_type: 'tool', tool_name: t.name, description: t.description },
                  ports: portsForNodeType('tool'),
                }}
              />
            ))}
          </CollapsibleSection>
        ))}

        {filteredChannels.length > 0 && (
          <CollapsibleSection icon={<Radio className="h-3 w-3" />} label="Channels" count={filteredChannels.length} textClass="text-pink-500">
            {filteredChannels.map((ch) => (
              <NodeChip
                key={ch.name}
                name={ch.name}
                nodeType="channel"
                data={{
                  nodeType: 'channel', id: `channel-${ch.name}`, name: ch.name,
                  metadata: { node_type: 'channel', channel_type: ch.name, connected: ch.connected },
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
