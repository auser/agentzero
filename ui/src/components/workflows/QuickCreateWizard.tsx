import { useState, useCallback } from 'react'
import { Button } from '@/components/ui/button'
import { Wand2, X, ChevronRight, ChevronLeft, Check } from 'lucide-react'
import { workflowsApi } from '@/lib/api/workflows'

interface WizardState {
  name: string
  agentName: string
  agentPrompt: string
  tools: string[]
  channelType: string
  schedule: string
}

const TOOL_OPTIONS = [
  { id: 'read_file', label: 'File Reading' },
  { id: 'write_file', label: 'File Writing' },
  { id: 'shell', label: 'Shell Commands' },
  { id: 'web_search', label: 'Web Search' },
  { id: 'web_fetch', label: 'Web Fetch' },
  { id: 'content_search', label: 'Code Search' },
  { id: 'memory_store', label: 'Memory' },
  { id: 'http_request', label: 'HTTP Requests' },
]

const CHANNEL_OPTIONS = [
  { id: '', label: 'None' },
  { id: 'slack', label: 'Slack' },
  { id: 'discord', label: 'Discord' },
  { id: 'telegram', label: 'Telegram' },
  { id: 'email', label: 'Email' },
  { id: 'webhook', label: 'Webhook' },
]

const SCHEDULE_OPTIONS = [
  { id: '', label: 'Manual only' },
  { id: '*/5 * * * *', label: 'Every 5 minutes' },
  { id: '0 * * * *', label: 'Every hour' },
  { id: '0 9 * * *', label: 'Daily at 9am' },
  { id: '0 9 * * 1', label: 'Weekly Monday 9am' },
]

const STEPS = ['Name', 'Agent', 'Tools', 'Channel', 'Schedule', 'Review']

interface Props {
  onComplete: (workflowId: string) => void
  onCancel: () => void
}

export function QuickCreateWizard({ onComplete, onCancel }: Props) {
  const [step, setStep] = useState(0)
  const [creating, setCreating] = useState(false)
  const [state, setState] = useState<WizardState>({
    name: '',
    agentName: '',
    agentPrompt: '',
    tools: ['read_file', 'shell', 'content_search'],
    channelType: '',
    schedule: '',
  })

  const update = useCallback(
    (patch: Partial<WizardState>) => setState((prev) => ({ ...prev, ...patch })),
    [],
  )

  const canProceed =
    step === 0
      ? state.name.trim().length > 0
      : step === 1
        ? state.agentName.trim().length > 0
        : true

  const handleCreate = useCallback(async () => {
    setCreating(true)
    try {
      const nodes: Record<string, unknown>[] = [
        {
          id: 'agent-1',
          type: 'agent',
          position: { x: 250, y: 100 },
          data: {
            label: state.agentName,
            type: 'agent',
            system_prompt: state.agentPrompt,
            allowed_tools: state.tools,
          },
        },
      ]
      const edges: Record<string, unknown>[] = []

      if (state.channelType) {
        nodes.push({
          id: 'channel-1',
          type: 'channel',
          position: { x: 50, y: 100 },
          data: { label: state.channelType, type: 'channel', channel_type: state.channelType },
        })
        edges.push({
          id: 'e-channel-agent',
          source: 'channel-1',
          target: 'agent-1',
          data: { portType: 'text' },
        })
      }

      if (state.schedule) {
        nodes.push({
          id: 'schedule-1',
          type: 'schedule',
          position: { x: 50, y: 250 },
          data: { label: state.schedule, type: 'schedule', cron: state.schedule },
        })
        edges.push({
          id: 'e-schedule-agent',
          source: 'schedule-1',
          target: 'agent-1',
          data: { portType: 'event' },
        })
      }

      const result = await workflowsApi.create({
        name: state.name,
        description: `Agent: ${state.agentName}`,
        layout: { nodes, edges },
      })
      onComplete(result.workflow_id)
    } catch (e) {
      console.error('Failed to create workflow', e)
    } finally {
      setCreating(false)
    }
  }, [state, onComplete])

  return (
    <div className="fixed inset-0 z-50 bg-black/50 flex items-center justify-center">
      <div className="bg-card border border-border rounded-xl shadow-2xl w-full max-w-lg mx-4">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-border/50">
          <div className="flex items-center gap-2">
            <Wand2 className="h-4 w-4 text-primary" />
            <span className="font-semibold">Quick Create</span>
          </div>
          <button onClick={onCancel} className="text-muted-foreground hover:text-foreground">
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Step indicator */}
        <div className="flex gap-1 px-6 py-3 border-b border-border/30">
          {STEPS.map((label, i) => (
            <div
              key={label}
              className={`flex-1 text-center text-[10px] py-1 rounded ${
                i === step
                  ? 'bg-primary text-primary-foreground'
                  : i < step
                    ? 'bg-primary/20 text-primary'
                    : 'bg-muted/30 text-muted-foreground'
              }`}
            >
              {label}
            </div>
          ))}
        </div>

        {/* Content */}
        <div className="px-6 py-6 min-h-[200px]">
          {step === 0 && (
            <div className="space-y-3">
              <label className="text-sm font-medium">Workflow Name</label>
              <input
                autoFocus
                className="w-full bg-muted/30 rounded-lg px-3 py-2 text-sm outline-none focus:ring-1 focus:ring-primary/50"
                placeholder="e.g., Morning Briefing, Code Reviewer, Email Watcher"
                value={state.name}
                onChange={(e) => update({ name: e.target.value })}
              />
            </div>
          )}

          {step === 1 && (
            <div className="space-y-3">
              <label className="text-sm font-medium">Agent Name</label>
              <input
                autoFocus
                className="w-full bg-muted/30 rounded-lg px-3 py-2 text-sm outline-none focus:ring-1 focus:ring-primary/50"
                placeholder="e.g., researcher, writer, analyst"
                value={state.agentName}
                onChange={(e) => update({ agentName: e.target.value })}
              />
              <label className="text-sm font-medium">System Prompt</label>
              <textarea
                className="w-full bg-muted/30 rounded-lg px-3 py-2 text-sm outline-none focus:ring-1 focus:ring-primary/50 resize-none h-20"
                placeholder="What should this agent do? (optional)"
                value={state.agentPrompt}
                onChange={(e) => update({ agentPrompt: e.target.value })}
              />
            </div>
          )}

          {step === 2 && (
            <div className="space-y-2">
              <label className="text-sm font-medium">Tools</label>
              <div className="grid grid-cols-2 gap-2">
                {TOOL_OPTIONS.map((tool) => (
                  <label
                    key={tool.id}
                    className={`flex items-center gap-2 p-2 rounded-lg cursor-pointer text-sm ${
                      state.tools.includes(tool.id)
                        ? 'bg-primary/10 text-primary border border-primary/30'
                        : 'bg-muted/20 text-muted-foreground border border-transparent hover:border-border/50'
                    }`}
                  >
                    <input
                      type="checkbox"
                      className="sr-only"
                      checked={state.tools.includes(tool.id)}
                      onChange={() =>
                        update({
                          tools: state.tools.includes(tool.id)
                            ? state.tools.filter((t) => t !== tool.id)
                            : [...state.tools, tool.id],
                        })
                      }
                    />
                    {state.tools.includes(tool.id) ? (
                      <Check className="h-3 w-3" />
                    ) : (
                      <div className="h-3 w-3" />
                    )}
                    {tool.label}
                  </label>
                ))}
              </div>
            </div>
          )}

          {step === 3 && (
            <div className="space-y-2">
              <label className="text-sm font-medium">Channel</label>
              <div className="space-y-1.5">
                {CHANNEL_OPTIONS.map((ch) => (
                  <label
                    key={ch.id}
                    className={`flex items-center gap-2 p-2.5 rounded-lg cursor-pointer text-sm ${
                      state.channelType === ch.id
                        ? 'bg-primary/10 text-primary border border-primary/30'
                        : 'bg-muted/20 text-muted-foreground border border-transparent hover:border-border/50'
                    }`}
                  >
                    <input
                      type="radio"
                      className="sr-only"
                      name="channel"
                      checked={state.channelType === ch.id}
                      onChange={() => update({ channelType: ch.id })}
                    />
                    {ch.label}
                  </label>
                ))}
              </div>
            </div>
          )}

          {step === 4 && (
            <div className="space-y-2">
              <label className="text-sm font-medium">Schedule</label>
              <div className="space-y-1.5">
                {SCHEDULE_OPTIONS.map((s) => (
                  <label
                    key={s.id}
                    className={`flex items-center gap-2 p-2.5 rounded-lg cursor-pointer text-sm ${
                      state.schedule === s.id
                        ? 'bg-primary/10 text-primary border border-primary/30'
                        : 'bg-muted/20 text-muted-foreground border border-transparent hover:border-border/50'
                    }`}
                  >
                    <input
                      type="radio"
                      className="sr-only"
                      name="schedule"
                      checked={state.schedule === s.id}
                      onChange={() => update({ schedule: s.id })}
                    />
                    {s.label}
                  </label>
                ))}
              </div>
            </div>
          )}

          {step === 5 && (
            <div className="space-y-3 text-sm">
              <h3 className="font-medium">Review</h3>
              <div className="space-y-1.5 text-muted-foreground">
                <p><span className="text-foreground font-medium">Workflow:</span> {state.name}</p>
                <p><span className="text-foreground font-medium">Agent:</span> {state.agentName}</p>
                {state.agentPrompt && <p><span className="text-foreground font-medium">Prompt:</span> {state.agentPrompt.slice(0, 80)}{state.agentPrompt.length > 80 ? '...' : ''}</p>}
                <p><span className="text-foreground font-medium">Tools:</span> {state.tools.join(', ') || 'none'}</p>
                <p><span className="text-foreground font-medium">Channel:</span> {state.channelType || 'none'}</p>
                <p><span className="text-foreground font-medium">Schedule:</span> {state.schedule || 'manual'}</p>
              </div>
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-between px-6 py-4 border-t border-border/50">
          <Button variant="ghost" size="sm" onClick={() => (step > 0 ? setStep(step - 1) : onCancel())}>
            <ChevronLeft className="h-3 w-3 mr-1" />
            {step > 0 ? 'Back' : 'Cancel'}
          </Button>
          {step < 5 ? (
            <Button size="sm" disabled={!canProceed} onClick={() => setStep(step + 1)}>
              Next
              <ChevronRight className="h-3 w-3 ml-1" />
            </Button>
          ) : (
            <Button size="sm" disabled={creating} onClick={handleCreate}>
              {creating ? 'Creating...' : 'Create Workflow'}
              <Wand2 className="h-3 w-3 ml-1" />
            </Button>
          )}
        </div>
      </div>
    </div>
  )
}
