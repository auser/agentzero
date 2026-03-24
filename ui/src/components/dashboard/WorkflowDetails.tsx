/**
 * Sidebar panel listing agents, tools, channels, and nodes in the current workflow.
 */
import { useQuery } from '@tanstack/react-query'
import { agentsApi } from '@/lib/api/agents'
import { api } from '@/lib/api/client'
import { topologyApi } from '@/lib/api/topology'
import { Bot, Wrench, Radio, Network } from 'lucide-react'

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

function SectionHeader({ icon, label, count }: { icon: React.ReactNode; label: string; count: number }) {
  return (
    <div className="flex items-center justify-between px-3 py-2">
      <span className="text-[10px] font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
        {icon}
        {label}
      </span>
      <span className="text-[10px] text-muted-foreground/60 bg-muted/50 px-1.5 py-0.5 rounded">
        {count}
      </span>
    </div>
  )
}

function ListItem({ name, detail, dot }: { name: string; detail?: string; dot?: string }) {
  return (
    <div className="flex items-center gap-2 px-3 py-1.5 text-xs hover:bg-muted/20 rounded-sm transition-colors">
      {dot && (
        <span className={`h-1.5 w-1.5 rounded-full shrink-0 ${dot}`} />
      )}
      <span className="truncate">{name}</span>
      {detail && (
        <span className="text-[10px] text-muted-foreground/50 ml-auto shrink-0">{detail}</span>
      )}
    </div>
  )
}

export function WorkflowDetails() {
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

  const { data: topology } = useQuery({
    queryKey: ['topology'],
    queryFn: () => topologyApi.get(),
    refetchInterval: 3_000,
  })

  const agentList = agents?.data ?? []
  const tools = toolsData?.tools ?? []
  const channels = Object.entries(configData?.channels ?? {}).map(([name, cfg]) => ({
    name,
    connected: cfg.enabled !== false,
  }))
  const nodes = topology?.nodes ?? []
  const edges = topology?.edges ?? []

  return (
    <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm overflow-hidden h-full">
      <div className="px-4 py-3 border-b border-border/50">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Workflow Components
        </h3>
      </div>
      <div className="overflow-y-auto max-h-[280px] divide-y divide-border/30">
        {/* Agents */}
        <div className="py-1">
          <SectionHeader icon={<Bot className="h-3 w-3" />} label="Agents" count={agentList.length} />
          {agentList.slice(0, 6).map((a) => (
            <ListItem
              key={a.agent_id}
              name={a.name}
              detail={a.model}
              dot={a.status === 'active' ? 'bg-emerald-500' : a.status === 'running' ? 'bg-blue-500' : 'bg-zinc-600'}
            />
          ))}
        </div>

        {/* Tools */}
        <div className="py-1">
          <SectionHeader icon={<Wrench className="h-3 w-3" />} label="Tools" count={tools.length} />
          {tools.slice(0, 8).map((t) => (
            <ListItem key={t.name} name={t.name} />
          ))}
          {tools.length > 8 && (
            <div className="text-[10px] text-muted-foreground/50 px-3 py-1">
              +{tools.length - 8} more
            </div>
          )}
        </div>

        {/* Channels */}
        {channels.length > 0 && (
          <div className="py-1">
            <SectionHeader icon={<Radio className="h-3 w-3" />} label="Channels" count={channels.length} />
            {channels.map((ch) => (
              <ListItem
                key={ch.name}
                name={ch.name}
                dot={ch.connected ? 'bg-emerald-500' : 'bg-zinc-600'}
                detail={ch.connected ? 'on' : 'off'}
              />
            ))}
          </div>
        )}

        {/* Topology nodes */}
        <div className="py-1">
          <SectionHeader icon={<Network className="h-3 w-3" />} label="Nodes" count={nodes.length} />
          {nodes.map((n) => (
            <ListItem
              key={n.agent_id}
              name={n.name}
              detail={`${n.active_run_count} run${n.active_run_count !== 1 ? 's' : ''}`}
              dot={n.status === 'running' ? 'bg-emerald-500' : n.status === 'active' ? 'bg-blue-500' : 'bg-zinc-600'}
            />
          ))}
          {edges.length > 0 && (
            <div className="text-[10px] text-muted-foreground/50 px-3 py-1">
              {edges.length} connection{edges.length !== 1 ? 's' : ''}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
