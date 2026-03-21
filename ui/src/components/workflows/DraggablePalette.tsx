/**
 * Draggable palette of agents, tools, and channels.
 * Items can be dragged onto the workflow canvas to add as nodes.
 */
import { useQuery } from '@tanstack/react-query'
import { agentsApi } from '@/lib/api/agents'
import { api } from '@/lib/api/client'
import { topologyApi } from '@/lib/api/topology'
import type { Port } from '@auser/workflow-graph-web'
import { Bot, Wrench, Radio, GripVertical } from 'lucide-react'
import type { DragEvent } from 'react'
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

function DraggableItem({
  data,
  name,
  detail,
  dot,
}: {
  data: DragNodeData
  name: string
  detail?: string
  dot?: string
}) {
  const handleDragStart = (e: DragEvent<HTMLDivElement>) => {
    e.dataTransfer.setData('application/workflow-node', JSON.stringify(data))
    e.dataTransfer.effectAllowed = 'copy'
  }

  return (
    <div
      draggable
      onDragStart={handleDragStart}
      className="flex items-center gap-2 px-3 py-1.5 text-xs cursor-grab active:cursor-grabbing hover:bg-muted/30 rounded-sm transition-colors group select-none"
    >
      <GripVertical className="h-3 w-3 text-muted-foreground/30 group-hover:text-muted-foreground/60 shrink-0 transition-colors" />
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

export function DraggablePalette() {
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

  return (
    <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm overflow-hidden h-full flex flex-col">
      <div className="px-4 py-3 border-b border-border/50">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          Components
        </h3>
        <p className="text-[10px] text-muted-foreground/50 mt-0.5">
          Drag items onto the canvas
        </p>
      </div>
      <div className="overflow-y-auto flex-1 divide-y divide-border/30">
        {/* Agents */}
        <div className="py-1">
          <SectionHeader icon={<Bot className="h-3 w-3" />} label="Agents" count={agentList.length} />
          {agentList.map((a) => (
            <DraggableItem
              key={a.agent_id}
              name={a.name}
              detail={a.model}
              dot={a.status === 'active' ? 'bg-emerald-500' : a.status === 'running' ? 'bg-blue-500' : 'bg-zinc-600'}
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
        </div>

        {/* Tools */}
        <div className="py-1">
          <SectionHeader icon={<Wrench className="h-3 w-3" />} label="Tools" count={tools.length} />
          {tools.map((t) => (
            <DraggableItem
              key={t.name}
              name={t.name}
              detail={t.description?.slice(0, 20)}
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
        </div>

        {/* Channels */}
        {channels.length > 0 && (
          <div className="py-1">
            <SectionHeader icon={<Radio className="h-3 w-3" />} label="Channels" count={channels.length} />
            {channels.map((ch) => (
              <DraggableItem
                key={ch.name}
                name={ch.name}
                dot={ch.connected ? 'bg-emerald-500' : 'bg-zinc-600'}
                detail={ch.connected ? 'on' : 'off'}
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
          </div>
        )}

        {/* Active topology nodes (read-only info) */}
        {nodes.length > 0 && (
          <div className="py-1">
            <SectionHeader icon={<Bot className="h-3 w-3" />} label="Active Nodes" count={nodes.length} />
            {nodes.map((n) => (
              <div
                key={n.agent_id}
                className="flex items-center gap-2 px-3 py-1.5 text-xs text-muted-foreground/70"
              >
                <span
                  className={`h-1.5 w-1.5 rounded-full shrink-0 ${
                    n.status === 'running' ? 'bg-emerald-500' : n.status === 'active' ? 'bg-blue-500' : 'bg-zinc-600'
                  }`}
                />
                <span className="truncate">{n.name}</span>
                <span className="text-[10px] text-muted-foreground/40 ml-auto">
                  {n.active_run_count} run{n.active_run_count !== 1 ? 's' : ''}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
