import { createFileRoute } from '@tanstack/react-router'
import { useQuery } from '@tanstack/react-query'
import { useRef, useState, useEffect } from 'react'
import { agentsApi } from '@/lib/api/agents'
import { modelsApi } from '@/lib/api/models'
import { useChat } from '@/hooks/useChat'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { StatusBadge } from '@/components/shared/StatusBadge'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Send, Trash2 } from 'lucide-react'
import { cn } from '@/lib/utils'

export const Route = createFileRoute('/chat/')({
  component: ChatPage,
})

function ChatPage() {
  const [input, setInput] = useState('')
  const [selectedAgent, setSelectedAgent] = useState<string | undefined>()
  const [selectedModel, setSelectedModel] = useState<string | undefined>()
  const bottomRef = useRef<HTMLDivElement>(null)

  const { messages, status, send, clear } = useChat()

  const { data: agents } = useQuery({
    queryKey: ['agents'],
    queryFn: () => agentsApi.list(),
  })

  const { data: models } = useQuery({
    queryKey: ['models'],
    queryFn: () => modelsApi.list(),
  })

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages])

  function handleSend(e: React.FormEvent) {
    e.preventDefault()
    if (!input.trim()) return
    send(input.trim(), selectedAgent, selectedModel)
    setInput('')
  }

  return (
    <div className="flex flex-col h-full max-w-3xl mx-auto gap-3">
      {/* Toolbar */}
      <div className="flex items-center gap-2 shrink-0">
        <Select value={selectedAgent} onValueChange={setSelectedAgent}>
          <SelectTrigger className="w-44 h-8 text-xs">
            <SelectValue placeholder="Any agent" />
          </SelectTrigger>
          <SelectContent>
            {agents?.data.map((a) => (
              <SelectItem key={a.agent_id} value={a.agent_id} className="text-xs">
                {a.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <Select value={selectedModel} onValueChange={setSelectedModel}>
          <SelectTrigger className="w-44 h-8 text-xs">
            <SelectValue placeholder="Default model" />
          </SelectTrigger>
          <SelectContent>
            {models?.data
              .filter((m, i, arr) => arr.findIndex((x) => x.id === m.id && x.owned_by === m.owned_by) === i)
              .map((m) => (
              <SelectItem key={`${m.owned_by}/${m.id}`} value={m.id} className="text-xs">
                {m.id}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>

        <div className="ml-auto flex items-center gap-2">
          <StatusBadge status={status} />
          <Button variant="ghost" size="icon" className="h-7 w-7" onClick={clear} title="Clear conversation">
            <Trash2 className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>

      {/* Messages */}
      <ScrollArea className="flex-1 rounded-lg border border-border bg-card">
        <div className="p-4 space-y-4">
          {messages.length === 0 && (
            <p className="text-sm text-muted-foreground text-center py-8">
              Send a message to start chatting
            </p>
          )}
          {messages.map((msg) => (
            <div
              key={msg.id}
              className={cn(
                'flex',
                msg.role === 'user' ? 'justify-end' : 'justify-start'
              )}
            >
              <div
                className={cn(
                  'max-w-[80%] rounded-lg px-4 py-2.5 text-sm',
                  msg.role === 'user'
                    ? 'bg-primary text-primary-foreground'
                    : 'bg-secondary text-secondary-foreground'
                )}
              >
                <p className="whitespace-pre-wrap wrap-break-word">{msg.content}</p>
                {msg.streaming && (
                  <span className="inline-flex gap-0.5 ml-1">
                    <span className="animate-bounce w-1 h-1 rounded-full bg-current" style={{ animationDelay: '0ms' }} />
                    <span className="animate-bounce w-1 h-1 rounded-full bg-current" style={{ animationDelay: '150ms' }} />
                    <span className="animate-bounce w-1 h-1 rounded-full bg-current" style={{ animationDelay: '300ms' }} />
                  </span>
                )}
              </div>
            </div>
          ))}
          <div ref={bottomRef} />
        </div>
      </ScrollArea>

      {/* Input */}
      <form onSubmit={handleSend} className="flex gap-2 shrink-0">
        <Input
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="Message…"
          className="flex-1"
          onKeyDown={(e) => {
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault()
              handleSend(e)
            }
          }}
        />
        <Button type="submit" size="icon" disabled={!input.trim()}>
          <Send className="h-4 w-4" />
        </Button>
      </form>
    </div>
  )
}
