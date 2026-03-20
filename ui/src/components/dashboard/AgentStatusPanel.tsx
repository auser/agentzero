/**
 * Live agent status cards showing each agent's state, model, and activity.
 */
import { useQuery } from '@tanstack/react-query'
import { agentsApi } from '@/lib/api/agents'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { StatusBadge } from '@/components/shared/StatusBadge'
import { Link } from '@tanstack/react-router'
import { Bot } from 'lucide-react'

export function AgentStatusPanel() {
  const { data: agents } = useQuery({
    queryKey: ['agents'],
    queryFn: () => agentsApi.list(),
    refetchInterval: 5_000,
  })

  const agentList = agents?.data ?? []

  if (agentList.length === 0) {
    return null
  }

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm flex items-center gap-1.5">
            <Bot className="h-3.5 w-3.5" />
            Agents
          </CardTitle>
          <Link to="/agents" className="text-xs text-primary hover:underline">
            Manage
          </Link>
        </div>
      </CardHeader>
      <CardContent>
        <div className="space-y-2">
          {agentList.slice(0, 6).map((agent) => (
            <div
              key={agent.agent_id}
              className="flex items-center justify-between py-1.5 border-b border-border last:border-0"
            >
              <div className="flex items-center gap-2 min-w-0 flex-1">
                <span className="text-sm font-medium truncate">{agent.name}</span>
                {agent.model && (
                  <span className="text-[10px] text-muted-foreground bg-muted px-1.5 py-0.5 rounded shrink-0">
                    {agent.model}
                  </span>
                )}
              </div>
              <StatusBadge status={agent.status} />
            </div>
          ))}
          {agentList.length > 6 && (
            <p className="text-xs text-muted-foreground text-center pt-1">
              +{agentList.length - 6} more
            </p>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
