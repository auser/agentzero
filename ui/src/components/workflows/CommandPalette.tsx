/**
 * Cmd+K command palette for quickly adding nodes to the workflow canvas.
 * Fuzzy searches across agents, tools, and channels.
 */
import { useState, useEffect, useMemo, useRef } from 'react'
import { useQuery } from '@tanstack/react-query'
import { agentsApi } from '@/lib/api/agents'
import { Plus } from 'lucide-react'
import { api } from '@/lib/api/client'
import { Bot, Wrench, Radio, Search, Hand, Clock, Shield, GitBranch, Sparkles, Zap, Theater } from 'lucide-react'
import { portsForNodeType } from '@/components/workflows/WorkflowCanvas'
import { getDefinition, ALL_NODE_DEFINITIONS } from '@/lib/node-definitions'
import type { DragNodeData } from '@/components/workflows/DraggablePalette'

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

interface PaletteItem {
  id: string
  name: string
  category: string
  icon: React.ReactNode
  detail?: string
  color: string
  data: DragNodeData
}

interface CommandPaletteProps {
  open: boolean
  onClose: () => void
  onSelect: (data: DragNodeData) => void
  onCreateAgent?: () => void
}

export function CommandPalette({ open, onClose, onSelect, onCreateAgent }: CommandPaletteProps) {
  const [query, setQuery] = useState('')
  const [selectedIndex, setSelectedIndex] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

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

  // Build flat list of all items
  const allItems: PaletteItem[] = useMemo(() => {
    const items: PaletteItem[] = []

    // "Create new agent" action at the top
    if (onCreateAgent) {
      items.push({
        id: '__create_agent__',
        name: 'Create new agent...',
        category: 'Action',
        icon: <Plus className="h-3.5 w-3.5" />,
        detail: 'Add a new agent to your workspace',
        color: '#22c55e',
        data: {
          nodeType: 'agent', id: '', name: '',
          metadata: {}, ports: portsForNodeType('agent'),
        },
      })
    }

    for (const a of agents?.data ?? []) {
      items.push({
        id: a.agent_id,
        name: a.name,
        category: 'Agent',
        icon: <Bot className="h-3.5 w-3.5" />,
        detail: a.model,
        color: getDefinition('agent')?.headerColor ?? '#3b82f6',
        data: {
          nodeType: 'agent', id: a.agent_id, name: a.name,
          metadata: { node_type: 'agent', model: a.model, description: a.description, status: a.status },
          ports: portsForNodeType('agent'),
        },
      })
    }

    for (const t of toolsData?.tools ?? []) {
      items.push({
        id: `tool-${t.name}`,
        name: t.name,
        category: 'Tool',
        icon: <Wrench className="h-3.5 w-3.5" />,
        detail: t.description?.slice(0, 40),
        color: getDefinition('tool')?.headerColor ?? '#8b5cf6',
        data: {
          nodeType: 'tool', id: `tool-${t.name}`, name: t.name,
          metadata: { node_type: 'tool', tool_name: t.name, description: t.description },
          ports: portsForNodeType('tool'),
        },
      })
    }

    for (const [name, cfg] of Object.entries(configData?.channels ?? {})) {
      items.push({
        id: `channel-${name}`,
        name,
        category: 'Channel',
        icon: <Radio className="h-3.5 w-3.5" />,
        detail: cfg.enabled !== false ? 'connected' : 'offline',
        color: getDefinition('channel')?.headerColor ?? '#ec4899',
        data: {
          nodeType: 'channel', id: `channel-${name}`, name,
          metadata: { node_type: 'channel', channel_type: name, connected: cfg.enabled !== false },
          ports: portsForNodeType('channel'),
        },
      })
    }

    // Static node types — always available regardless of API data
    const STATIC_ICON_MAP: Record<string, React.ReactNode> = {
      role: <Theater className="h-3.5 w-3.5" />,
      schedule: <Clock className="h-3.5 w-3.5" />,
      gate: <Shield className="h-3.5 w-3.5" />,
      subagent: <GitBranch className="h-3.5 w-3.5" />,
      human_input: <Hand className="h-3.5 w-3.5" />,
      provider: <Zap className="h-3.5 w-3.5" />,
    }

    for (const def of ALL_NODE_DEFINITIONS) {
      // Skip types already covered by dynamic data above
      if (['agent', 'tool', 'channel'].includes(def.type)) continue
      const icon = STATIC_ICON_MAP[def.type] ?? <Sparkles className="h-3.5 w-3.5" />
      items.push({
        id: `${def.type}_static`,
        name: def.label,
        category: def.category === 'core' ? 'Node' : def.category === 'integration' ? 'Channel' : def.category,
        icon,
        detail: def.fields?.[0]?.label ?? def.type,
        color: def.headerColor,
        data: {
          nodeType: def.type,
          id: `${def.type}_new`,
          name: def.label,
          metadata: { node_type: def.type },
          ports: portsForNodeType(def.type),
        },
      })
    }

    return items
  }, [agents, toolsData, configData, onCreateAgent])

  // Fuzzy filter
  const filtered = useMemo(() => {
    if (!query) return allItems
    const q = query.toLowerCase()
    return allItems.filter(
      (item) =>
        item.name.toLowerCase().includes(q) ||
        item.category.toLowerCase().includes(q) ||
        item.detail?.toLowerCase().includes(q),
    )
  }, [allItems, query])

  // Reset selection when query changes
  useEffect(() => {
    setSelectedIndex(0) // eslint-disable-line react-hooks/set-state-in-effect -- reset on query change
  }, [query])

  // Focus input when opened
  useEffect(() => {
    if (open) {
      setQuery('') // eslint-disable-line react-hooks/set-state-in-effect -- reset on open
      setSelectedIndex(0)
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [open])

  // Global keyboard handler — works regardless of focus
  useEffect(() => {
    if (!open) return

    const handler = (e: KeyboardEvent) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        setSelectedIndex((i) => Math.min(i + 1, filtered.length - 1))
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        setSelectedIndex((i) => Math.max(i - 1, 0))
      } else if (e.key === 'Enter') {
        e.preventDefault()
        if (filtered[selectedIndex]) {
          if (filtered[selectedIndex].id === '__create_agent__') {
            onCreateAgent?.()
          } else {
            onSelect(filtered[selectedIndex].data)
          }
          onClose()
        }
      } else if (e.key === 'Escape') {
        onClose()
      }
    }

    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [open, filtered, selectedIndex, onSelect, onClose, onCreateAgent])

  if (!open) return null

  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center pt-[20vh] bg-black/60 backdrop-blur-sm" onClick={onClose}>
      <div
        className="w-[480px] max-h-[400px] bg-card border border-border rounded-xl shadow-2xl overflow-hidden flex flex-col"
        onClick={(e) => e.stopPropagation()}
      >
        {/* Search input */}
        <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
          <Search className="h-4 w-4 text-muted-foreground/50 shrink-0" />
          <input
            ref={inputRef}
            autoFocus
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Add a node... (agent, tool, or channel)"
            className="flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground/40"
          />
          <kbd className="text-[9px] text-muted-foreground/40 bg-muted/30 px-1.5 py-0.5 rounded border border-border/50">
            esc
          </kbd>
        </div>

        {/* Results */}
        <div className="overflow-y-auto flex-1 py-1">
          {filtered.length === 0 ? (
            <p className="text-sm text-muted-foreground/50 text-center py-8">No matches</p>
          ) : (
            filtered.slice(0, 20).map((item, i) => (
              <button
                key={item.id}
                ref={(el) => { if (i === selectedIndex && el) el.scrollIntoView({ block: 'nearest' }) }}
                className={`flex items-center gap-3 w-full px-4 py-2 text-left transition-colors ${
                  i === selectedIndex ? 'bg-primary/15 border-l-2 border-primary' : 'hover:bg-muted/30 border-l-2 border-transparent'
                }`}
                onClick={() => {
                  if (item.id === '__create_agent__') {
                    onCreateAgent?.()
                  } else {
                    onSelect(item.data)
                  }
                  onClose()
                }}
                onMouseEnter={() => setSelectedIndex(i)}
              >
                <span style={{ color: item.color }}>{item.icon}</span>
                <div className="flex-1 min-w-0">
                  <span className="text-sm font-medium">{item.name}</span>
                  {item.detail && (
                    <span className="text-xs text-muted-foreground/50 ml-2 truncate">{item.detail}</span>
                  )}
                </div>
                <span
                  className="text-[9px] font-medium uppercase tracking-wider px-1.5 py-0.5 rounded"
                  style={{ color: item.color, backgroundColor: `${item.color}26` }}
                >
                  {item.category}
                </span>
              </button>
            ))
          )}
        </div>
      </div>
    </div>
  )
}

