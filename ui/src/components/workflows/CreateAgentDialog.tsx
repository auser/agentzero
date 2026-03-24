/**
 * Inline dialog for creating a new agent from the workflow view.
 * Creates via POST /v1/agents, then invalidates the agents query
 * so the palette updates immediately.
 */
import { useState, useRef, useEffect } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { agentsApi, type CreateAgentPayload } from '@/lib/api/agents'
import { modelsApi } from '@/lib/api/models'
import { Button } from '@/components/ui/button'
import { Bot, X } from 'lucide-react'

interface CreateAgentDialogProps {
  open: boolean
  onClose: () => void
  onCreated?: (agentId: string) => void
}

export function CreateAgentDialog({ open, onClose, onCreated }: CreateAgentDialogProps) {
  const [name, setName] = useState('')
  const [model, setModel] = useState('')
  const [prompt, setPrompt] = useState('')
  const [description, setDescription] = useState('')
  const nameRef = useRef<HTMLInputElement>(null)
  const queryClient = useQueryClient()

  const { data: modelsData } = useQuery({
    queryKey: ['models'],
    queryFn: () => modelsApi.list(),
    enabled: open,
  })

  const createMutation = useMutation({
    mutationFn: (payload: CreateAgentPayload) => agentsApi.create(payload),
    onSuccess: (agent) => {
      void queryClient.invalidateQueries({ queryKey: ['agents'] })
      void queryClient.invalidateQueries({ queryKey: ['topology'] })
      onCreated?.(agent.agent_id)
      resetAndClose()
    },
  })

  useEffect(() => {
    if (open) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- intentional reset on dialog open
      setName('')
      // eslint-disable-next-line react-hooks/set-state-in-effect -- intentional reset on dialog open
      setModel('')
      // eslint-disable-next-line react-hooks/set-state-in-effect -- intentional reset on dialog open
      setPrompt('')
      // eslint-disable-next-line react-hooks/set-state-in-effect -- intentional reset on dialog open
      setDescription('')
      setTimeout(() => nameRef.current?.focus(), 50)
    }
  }, [open])

  const resetAndClose = () => {
    setName('')
    setModel('')
    setPrompt('')
    setDescription('')
    onClose()
  }

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault()
    if (!name.trim()) return
    createMutation.mutate({
      name: name.trim(),
      model: model || undefined,
      system_prompt: prompt || undefined,
      description: description || undefined,
    })
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') resetAndClose()
  }

  if (!open) return null

  const models = modelsData?.data ?? []

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[15vh] bg-black/60 backdrop-blur-sm"
      onClick={resetAndClose}
    >
      <div
        className="w-[520px] border border-border rounded-xl shadow-2xl overflow-hidden"
        style={{ background: '#1C1C1E' }}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-3 border-b border-border">
          <div className="flex items-center gap-2">
            <Bot className="h-4 w-4 text-blue-500" />
            <h2 className="text-sm font-semibold">Create Agent</h2>
          </div>
          <button
            onClick={resetAndClose}
            className="text-muted-foreground hover:text-foreground transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="p-5 space-y-4">
          {/* Name */}
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-muted-foreground">Name</label>
            <input
              ref={nameRef}
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g., morning-brief, code-reviewer"
              className="w-full h-9 px-3 text-sm rounded-md border border-input bg-background focus:ring-1 focus:ring-ring outline-none"
              required
            />
          </div>

          {/* Model */}
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-muted-foreground">Model</label>
            <select
              value={model}
              onChange={(e) => setModel(e.target.value)}
              className="w-full h-9 px-3 text-sm rounded-md border border-input bg-background focus:ring-1 focus:ring-ring outline-none"
            >
              <option value="">Default (from config)</option>
              {models.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.id} ({m.owned_by})
                </option>
              ))}
            </select>
          </div>

          {/* Description */}
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-muted-foreground">
              Description <span className="text-muted-foreground/50">(optional)</span>
            </label>
            <input
              type="text"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              placeholder="Brief description of what this agent does"
              className="w-full h-9 px-3 text-sm rounded-md border border-input bg-background focus:ring-1 focus:ring-ring outline-none"
            />
          </div>

          {/* System Prompt */}
          <div className="space-y-1.5">
            <label className="text-xs font-medium text-muted-foreground">
              System Prompt <span className="text-muted-foreground/50">(optional)</span>
            </label>
            <textarea
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Instructions for the agent..."
              rows={4}
              className="w-full px-3 py-2 text-sm rounded-md border border-input bg-background focus:ring-1 focus:ring-ring outline-none resize-none"
            />
          </div>

          {/* Error */}
          {createMutation.isError && (
            <p className="text-xs text-destructive">
              Failed to create agent: {(createMutation.error as Error).message}
            </p>
          )}

          {/* Actions */}
          <div className="flex justify-end gap-2 pt-1">
            <Button type="button" variant="ghost" size="sm" onClick={resetAndClose}>
              Cancel
            </Button>
            <Button
              type="submit"
              size="sm"
              disabled={!name.trim() || createMutation.isPending}
            >
              {createMutation.isPending ? 'Creating...' : 'Create Agent'}
            </Button>
          </div>
        </form>
      </div>
    </div>
  )
}
