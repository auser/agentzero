/**
 * Agent status panel with compact agent cards.
 */
import { useQuery } from '@tanstack/react-query'
import { agentsApi } from '@/lib/api/agents'
import { StatusBadge } from '@/components/shared/StatusBadge'
import { Link } from '@tanstack/react-router'
import { Bot, ChevronRight } from 'lucide-react'

export function AgentStatusPanel() {
  const { data: agents } = useQuery({
    queryKey: ['agents'],
    queryFn: () => agentsApi.list(),
    refetchInterval: 5_000,
  })

  const agentList = agents?.data ?? []

  return (
    <div className="rounded-lg border border-border/50 bg-card/80 backdrop-blur-sm">
      <div className="flex items-center justify-between px-4 py-3 border-b border-border/50">
        <h3 className="text-xs font-medium uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
          <Bot className="h-3.5 w-3.5" />
          Agents
        </h3>
        <Link
          to="/agents"
          className="text-xs text-primary hover:text-primary/80 flex items-center gap-0.5 transition-colors"
        >
          Manage <ChevronRight className="h-3 w-3" />
        </Link>
      </div>
      <div className="p-2">
        {agentList.length === 0 ? (
          <p className="text-sm text-muted-foreground text-center py-6">
            No agents configured
          </p>
        ) : (
          <div className="space-y-0.5">
            {agentList.slice(0, 8).map((agent) => (
              <div
                key={agent.agent_id}
                className="flex items-center justify-between px-3 py-2 rounded-md hover:bg-muted/30 transition-colors group"
              >
                <div className="flex items-center gap-2.5 min-w-0 flex-1">
                  <div
                    className={`h-2 w-2 rounded-full shrink-0 ${
                      agent.status === 'active'
                        ? 'bg-emerald-500 shadow-[0_0_6px_rgba(34,197,94,0.4)]'
                        : agent.status === 'running'
                          ? 'bg-blue-500 shadow-[0_0_6px_rgba(59,130,246,0.4)]'
                          : 'bg-zinc-600'
                    }`}
                  />
                  <span className="text-sm font-medium truncate group-hover:text-foreground transition-colors">
                    {agent.name}
                  </span>
                  {agent.model && (
                    <span className="text-[10px] text-muted-foreground/70 bg-muted/50 px-1.5 py-0.5 rounded shrink-0 hidden sm:inline">
                      {agent.model}
                    </span>
                  )}
                </div>
                <StatusBadge status={agent.status} />
              </div>
            ))}
            {agentList.length > 8 && (
              <Link
                to="/agents"
                className="block text-xs text-muted-foreground text-center py-2 hover:text-primary transition-colors"
              >
                +{agentList.length - 8} more agents
              </Link>
            )}
          </div>
        )}
      </div>
    </div>
  )
}
