/**
 * Draggable palette of agents, tools, and channels.
 * Items rendered as miniature node previews matching the canvas style.
 */
import { useQuery } from '@tanstack/react-query'
import { agentsApi } from '@/lib/api/agents'
import { api } from '@/lib/api/client'
import type { Port } from '@auser/workflow-graph-web'
import { Bot, Wrench, Radio, ChevronDown, ChevronRight, Search } from 'lucide-react'
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

export interface DragNodeData {
  nodeType: 'agent' | 'tool' | 'channel'
  id: string
  name: string
  metadata: Record<string, unknown>
  ports: Port[]
}

const TYPE_COLORS: Record<string, { header: string; border: string; dot: string }> = {
  agent: { header: '#3b82f6', border: '#2563eb', dot: '#22c55e' },
  tool: { header: '#8b5cf6', border: '#7c3aed', dot: '#8b5cf6' },
  channel: { header: '#ec4899', border: '#db2777', dot: '#ec4899' },
}

const PORT_TYPE_COLORS: Record<string, string> = {
  text: '#3b82f6',
  json: '#8b5cf6',
  tool_call: '#f97316',
  event: '#22c55e',
  config: '#6b7280',
}

function MiniNode({
  data,
  name,
  nodeType,
  detail,
  ports,
}: {
  data: DragNodeData
  name: string
  nodeType: string
  detail?: string
  ports: Port[]
}) {
  const handleDragStart = (e: DragEvent<HTMLDivElement>) => {
    e.dataTransfer.setData('application/workflow-node', JSON.stringify(data))
    e.dataTransfer.effectAllowed = 'copy'
  }

  const colors = TYPE_COLORS[nodeType] ?? TYPE_COLORS.tool
  const inputPorts = ports.filter((p) => p.direction === 'input')
  const outputPorts = ports.filter((p) => p.direction === 'output')
  const maxPorts = Math.max(inputPorts.length, outputPorts.length)

  return (
    <div
      draggable
      onDragStart={handleDragStart}
      className="rounded-md border overflow-hidden cursor-grab active:cursor-grabbing hover:scale-[1.02] hover:shadow-lg transition-all select-none"
      style={{ borderColor: colors.border + '60', background: '#1f2937' }}
    >
      {/* Header bar */}
      <div
        className="px-2 py-1 flex items-center gap-1.5"
        style={{ background: colors.header + '30', borderBottom: `1px solid ${colors.border}40` }}
      >
        <span
          className="h-1.5 w-1.5 rounded-full"
          style={{ background: colors.dot }}
        />
        <span className="text-[10px] font-semibold truncate" style={{ color: colors.header }}>
          {name}
        </span>
      </div>

      {/* Ports */}
      {maxPorts > 0 && (
        <div className="px-1.5 py-1 space-y-0.5">
          {Array.from({ length: maxPorts }).map((_, i) => {
            const inp = inputPorts[i]
            const out = outputPorts[i]
            return (
              <div key={i} className="flex items-center justify-between text-[8px] leading-tight">
                {/* Input port */}
                <div className="flex items-center gap-1 min-w-0 flex-1">
                  {inp ? (
                    <>
                      <span
                        className="h-1.5 w-1.5 rounded-full shrink-0"
                        style={{ background: PORT_TYPE_COLORS[inp.port_type ?? ''] ?? '#6b7280' }}
                      />
                      <span className="text-muted-foreground/70 truncate">{inp.label}</span>
                    </>
                  ) : (
                    <span />
                  )}
                </div>
                {/* Output port */}
                <div className="flex items-center gap-1 min-w-0 flex-1 justify-end">
                  {out ? (
                    <>
                      <span className="text-muted-foreground/70 truncate">{out.label}</span>
                      <span
                        className="h-1.5 w-1.5 rounded-full shrink-0"
                        style={{ background: PORT_TYPE_COLORS[out.port_type ?? ''] ?? '#6b7280' }}
                      />
                    </>
                  ) : (
                    <span />
                  )}
                </div>
              </div>
            )
          })}
        </div>
      )}

      {/* Detail */}
      {detail && (
        <div className="px-2 pb-1">
          <span className="text-[7px] text-muted-foreground/30 truncate block">{detail}</span>
        </div>
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
      {open && <div className="px-2 pb-2 grid gap-1.5">{children}</div>}
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
        {/* Agents */}
        {filteredAgents.length > 0 && (
          <CollapsibleSection
            icon={<Bot className="h-3 w-3" />}
            label="Agents"
            count={filteredAgents.length}
            color="#3b82f6"
          >
            {filteredAgents.map((a) => (
              <MiniNode
                key={a.agent_id}
                name={a.name}
                nodeType="agent"
                detail={a.model}
                ports={portsForNodeType('agent')}
                data={{
                  nodeType: 'agent',
                  id: a.agent_id,
                  name: a.name,
                  metadata: { node_type: 'agent', model: a.model, description: a.description, status: a.status },
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
            defaultOpen={tools.length <= 6}
          >
            {tools.map((t) => (
              <MiniNode
                key={t.name}
                name={t.name}
                nodeType="tool"
                detail={t.description?.slice(0, 30)}
                ports={portsForNodeType('tool')}
                data={{
                  nodeType: 'tool',
                  id: `tool-${t.name}`,
                  name: t.name,
                  metadata: { node_type: 'tool', tool_name: t.name, description: t.description },
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
              <MiniNode
                key={ch.name}
                name={ch.name}
                nodeType="channel"
                detail={ch.connected ? 'connected' : 'offline'}
                ports={portsForNodeType('channel')}
                data={{
                  nodeType: 'channel',
                  id: `channel-${ch.name}`,
                  name: ch.name,
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
