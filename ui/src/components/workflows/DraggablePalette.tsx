/**
 * Draggable palette of agents, tools, and channels.
 * Items rendered as small draggable node chips.
 */
import { useQuery } from '@tanstack/react-query'
import { agentsApi } from '@/lib/api/agents'
import { api } from '@/lib/api/client'
import { portsFromSchema, type Port, type ToolInfo } from '@/lib/workflow-types'
import { Bot, Wrench, Radio, CalendarClock, ShieldCheck, Zap, GitFork, UserCircle, Pin, ChevronDown, ChevronRight, Search } from 'lucide-react'
import { type DragEvent, useState, useMemo, useCallback, useRef } from 'react'
import { portsForNodeType } from '@/components/workflows/WorkflowCanvas'
import { getDefinition } from '@/lib/node-definitions'

interface ToolsResponse {
  tools: ToolInfo[]
}

interface ConfigResponse {
  channels?: Record<string, { enabled?: boolean }>
}

export interface DragNodeData {
  nodeType: string
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

  const def = getDefinition(nodeType)
  const color = def?.headerColor ?? '#6b7280'

  return (
    <div
      draggable
      onDragStart={handleDragStart}
      className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md border cursor-grab active:cursor-grabbing hover:brightness-125 transition-all select-none text-[11px] font-medium text-foreground"
      style={{ borderColor: `${color}80`, backgroundColor: `${color}1a` }}
    >
      <span className="h-2 w-2 rounded-full shrink-0" style={{ backgroundColor: color }} />
      {name}
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

  // Common channel types (always available, even if not configured)
  const COMMON_CHANNELS = ['slack', 'discord', 'telegram', 'email', 'webhook', 'chat']
  const allChannelItems: DragNodeData[] = useMemo(() => {
    const items: DragNodeData[] = []
    const seen = new Set<string>()

    // Add configured channels first
    for (const ch of filteredChannels) {
      seen.add(ch.name)
      items.push({
        nodeType: 'channel', id: `channel-${ch.name}`, name: ch.name,
        metadata: { node_type: 'channel', channel_type: ch.name, connected: ch.connected },
        ports: portsForNodeType('channel'),
      })
    }

    // Add common unconfigured channels
    for (const name of COMMON_CHANNELS) {
      if (!seen.has(name) && name.includes(filter)) {
        items.push({
          nodeType: 'channel', id: `channel-${name}`, name,
          metadata: { node_type: 'channel', channel_type: name, connected: false },
          ports: portsForNodeType('channel'),
        })
      }
    }

    return items
  }, [filteredChannels, filter])

  // Schedule nodes
  const filteredSchedules: DragNodeData[] = useMemo(() => {
    if (!('schedule'.includes(filter) || 'cron'.includes(filter))) return []
    return [{
      nodeType: 'schedule', id: `schedule-${Date.now()}`, name: 'cron schedule',
      metadata: { node_type: 'schedule' },
      ports: portsForNodeType('schedule'),
    }]
  }, [filter])

  // Gate nodes
  const filteredGates: DragNodeData[] = useMemo(() => {
    if (!('gate'.includes(filter) || 'approval'.includes(filter))) return []
    return [{
      nodeType: 'gate', id: `gate-${Date.now()}`, name: 'approval gate',
      metadata: { node_type: 'gate' },
      ports: portsForNodeType('gate'),
    }]
  }, [filter])

  // Role nodes
  const filteredRoles: DragNodeData[] = useMemo(() => {
    if (!('role'.includes(filter))) return []
    return [{
      nodeType: 'role', id: `role-${Date.now()}`, name: 'custom role',
      metadata: { node_type: 'role' },
      ports: portsForNodeType('role'),
    }]
  }, [filter])

  // Provider nodes
  const filteredProviders: DragNodeData[] = useMemo(() => {
    if (!('provider'.includes(filter) || 'model'.includes(filter) || 'llm'.includes(filter))) return []
    return [{
      nodeType: 'provider', id: `provider-${Date.now()}`, name: 'LLM provider',
      metadata: { node_type: 'provider' },
      ports: portsForNodeType('provider'),
    }]
  }, [filter])

  // Sub-Agent nodes
  const filteredSubAgents: DragNodeData[] = useMemo(() => {
    if (!('subagent'.includes(filter) || 'sub-agent'.includes(filter) || 'delegate'.includes(filter))) return []
    return [{
      nodeType: 'subagent', id: `subagent-${Date.now()}`, name: 'sub-agent',
      metadata: { node_type: 'subagent' },
      ports: portsForNodeType('subagent'),
    }]
  }, [filter])

  // Constant nodes
  const filteredConstants: DragNodeData[] = useMemo(() => {
    if (!('constant'.includes(filter) || 'value'.includes(filter) || 'string'.includes(filter) || 'text'.includes(filter))) return []
    return [{
      nodeType: 'constant', id: `constant-${Date.now()}`, name: 'constant',
      metadata: { node_type: 'constant' },
      ports: portsForNodeType('constant'),
    }]
  }, [filter])

  // Keyboard navigation: collect all visible items for arrow key traversal.
  const allItems: DragNodeData[] = useMemo(() => [
    ...filteredAgents.map(a => ({
      nodeType: 'agent', id: a.agent_id, name: a.name,
      metadata: { node_type: 'agent', model: a.model, description: a.description, status: a.status },
      ports: portsForNodeType('agent'),
    } as DragNodeData)),
    ...filteredSchedules,
    ...filteredGates,
    ...filteredRoles,
    ...filteredProviders,
    ...filteredSubAgents,
  ], [filteredAgents, filteredSchedules, filteredGates, filteredRoles, filteredProviders, filteredSubAgents])

  const [focusedIdx, setFocusedIdx] = useState(-1)
  const listRef = useRef<HTMLDivElement>(null)

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (allItems.length === 0) return
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setFocusedIdx(prev => Math.min(prev + 1, allItems.length - 1))
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      setFocusedIdx(prev => Math.max(prev - 1, 0))
    } else if (e.key === 'Escape') {
      setFocusedIdx(-1)
    }
  }, [allItems.length])

  return (
    <div
      className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm overflow-hidden h-full flex flex-col"
      onKeyDown={handleKeyDown}
    >
      <div className="px-3 py-2.5 border-b border-border/50">
        <h3 className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground mb-2">
          Components
        </h3>
        <div className="relative">
          <Search className="absolute left-2 top-1/2 -translate-y-1/2 h-3 w-3 text-muted-foreground/40" />
          <input
            type="text"
            value={search}
            onChange={(e) => { setSearch(e.target.value); setFocusedIdx(-1) }}
            placeholder="Filter... (↑↓ to navigate)"
            className="w-full h-6 pl-6 pr-2 text-[11px] rounded border border-border/50 bg-background/50 focus:ring-1 focus:ring-ring outline-none placeholder:text-muted-foreground/30"
          />
        </div>
      </div>

      <div className="overflow-y-auto flex-1">
        {filteredAgents.length > 0 && (
          <CollapsibleSection icon={<Bot className="h-3 w-3" />} label="Agents" count={filteredAgents.length} color="#3b82f6">
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
            color="#8b5cf6"
            defaultOpen={tools.length <= 6}
          >
            {tools.map((t) => (
              <NodeChip
                key={t.name}
                name={t.name}
                nodeType="tool"
                data={{
                  nodeType: 'tool', id: `tool-${t.name}`, name: t.name,
                  metadata: {
                    node_type: 'tool', tool_name: t.name, description: t.description,
                    tool_inputs: portsFromSchema(t.input_schema).length > 0
                      ? portsFromSchema(t.input_schema)
                      : (getDefinition('tool')?.inputs ?? []),
                    tool_outputs: getDefinition('tool')?.outputs ?? [],
                  },
                  ports: [
                    ...(portsFromSchema(t.input_schema).length > 0
                      ? portsFromSchema(t.input_schema)
                      : (getDefinition('tool')?.inputs ?? [])),
                    ...(getDefinition('tool')?.outputs ?? []),
                  ],
                }}
              />
            ))}
          </CollapsibleSection>
        ))}

        {/* Channels — always show common types + any configured ones */}
        <CollapsibleSection icon={<Radio className="h-3 w-3" />} label="Channels" count={allChannelItems.length} color="#ec4899">
          {allChannelItems.map((ch) => (
            <NodeChip
              key={ch.id}
              name={ch.name}
              nodeType="channel"
              data={ch}
            />
          ))}
        </CollapsibleSection>

        {/* Schedules */}
        {filteredSchedules.length > 0 && (
          <CollapsibleSection icon={<CalendarClock className="h-3 w-3" />} label="Schedules" count={filteredSchedules.length} color="#eab308">
            {filteredSchedules.map((s) => (
              <NodeChip key={s.id} name={s.name} nodeType="schedule" data={s} />
            ))}
          </CollapsibleSection>
        )}

        {/* Gates */}
        {filteredGates.length > 0 && (
          <CollapsibleSection icon={<ShieldCheck className="h-3 w-3" />} label="Gates" count={filteredGates.length} color="#ef4444">
            {filteredGates.map((g) => (
              <NodeChip key={g.id} name={g.name} nodeType="gate" data={g} />
            ))}
          </CollapsibleSection>
        )}

        {/* Roles */}
        {filteredRoles.length > 0 && (
          <CollapsibleSection icon={<UserCircle className="h-3 w-3" />} label="Roles" count={filteredRoles.length} color="#a855f7">
            {filteredRoles.map((r) => (
              <NodeChip key={r.id} name={r.name} nodeType="role" data={r} />
            ))}
          </CollapsibleSection>
        )}

        {/* Providers */}
        {filteredProviders.length > 0 && (
          <CollapsibleSection icon={<Zap className="h-3 w-3" />} label="Providers" count={filteredProviders.length} color="#6b7280">
            {filteredProviders.map((p) => (
              <NodeChip key={p.id} name={p.name} nodeType="provider" data={p} />
            ))}
          </CollapsibleSection>
        )}

        {/* Sub-Agents */}
        {filteredSubAgents.length > 0 && (
          <CollapsibleSection icon={<GitFork className="h-3 w-3" />} label="Sub-Agents" count={filteredSubAgents.length} color="#22c55e">
            {filteredSubAgents.map((s) => (
              <NodeChip key={s.id} name={s.name} nodeType="subagent" data={s} />
            ))}
          </CollapsibleSection>
        )}

        {/* Constants */}
        {filteredConstants.length > 0 && (
          <CollapsibleSection icon={<Pin className="h-3 w-3" />} label="Constants" count={filteredConstants.length} color="#737373">
            {filteredConstants.map((c) => (
              <NodeChip key={c.id} name={c.name} nodeType="constant" data={c} />
            ))}
          </CollapsibleSection>
        )}
      </div>
    </div>
  )
}
