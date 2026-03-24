/**
 * Key selector popover for JSON → text port connections.
 * When connecting a JSON output to a text input (or vice versa),
 * this popover lets the user specify which key path to extract.
 */
import { useState, useRef, useEffect } from 'react'
import { Button } from '@/components/ui/button'
import { ArrowRight, Check, X } from 'lucide-react'

/** Common key paths suggested for different contexts. */
const COMMON_PATHS: Record<string, string[]> = {
  response: ['content', 'text', 'message', 'result', 'data'],
  result: ['output', 'text', 'content', 'data', 'value', 'items'],
  tool_calls: ['name', 'arguments', 'id'],
  events: ['type', 'data', 'message'],
  default: ['text', 'content', 'data', 'value', 'result', 'message'],
}

export interface PendingConnection {
  fromNodeId: string
  fromPortId: string
  fromPortType: string
  toNodeId: string
  toPortId: string
  toPortType: string
}

interface KeySelectorProps {
  connection: PendingConnection
  onConfirm: (connection: PendingConnection, keyPath: string | null) => void
  onCancel: () => void
}

export function KeySelector({ connection, onConfirm, onCancel }: KeySelectorProps) {
  const [keyPath, setKeyPath] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  // Always show the selector when types differ
  const needsTransform = connection.fromPortType !== connection.toPortType

  const suggestions = COMMON_PATHS[connection.fromPortId] ?? COMMON_PATHS.default

  const handleConfirm = () => {
    onConfirm(connection, keyPath || null)
  }

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') handleConfirm()
    if (e.key === 'Escape') onCancel()
  }

  if (!needsTransform) {
    // Types match — just confirm directly
    onConfirm(connection, null)
    return null
  }

  return (
    <div className="absolute inset-0 z-50 flex items-center justify-center bg-black/30 backdrop-blur-sm rounded-lg">
      <div className="bg-card border border-border rounded-lg shadow-xl p-4 w-80 space-y-3">
        {/* Header */}
        <div className="space-y-1">
          <h4 className="text-sm font-medium">Extract Key</h4>
          <p className="text-xs text-muted-foreground">
            Connect{' '}
            <span className="font-mono text-primary">{connection.fromPortId}</span>
            <ArrowRight className="inline h-3 w-3 mx-1" />
            <span className="font-mono text-primary">{connection.toPortId}</span>
          </p>
          <p className="text-[11px] text-muted-foreground/70">
            {connection.fromPortType === 'json'
              ? 'Select which key from the JSON output to pass as text input.'
              : 'The text value will be wrapped as JSON with this key.'}
          </p>
        </div>

        {/* Key path input */}
        <div className="space-y-1.5">
          <label className="text-[11px] text-muted-foreground font-medium">
            Key path
          </label>
          <input
            ref={inputRef}
            type="text"
            value={keyPath}
            onChange={(e) => setKeyPath(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder="e.g. result.text or leave empty for raw value"
            className="w-full h-8 px-2 text-xs font-mono rounded border border-input bg-background focus:ring-1 focus:ring-ring outline-none"
          />
        </div>

        {/* Suggestions */}
        <div className="flex flex-wrap gap-1">
          {suggestions.map((path) => (
            <button
              key={path}
              onClick={() => setKeyPath(path)}
              className={`text-[10px] px-2 py-0.5 rounded border transition-colors ${
                keyPath === path
                  ? 'bg-primary/20 border-primary/40 text-primary'
                  : 'bg-muted/30 border-border/50 text-muted-foreground hover:bg-muted/50'
              }`}
            >
              .{path}
            </button>
          ))}
        </div>

        {/* Actions */}
        <div className="flex justify-end gap-2 pt-1">
          <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={onCancel}>
            <X className="h-3 w-3 mr-1" />
            Cancel
          </Button>
          <Button size="sm" className="h-7 text-xs" onClick={handleConfirm}>
            <Check className="h-3 w-3 mr-1" />
            Connect
          </Button>
        </div>
      </div>
    </div>
  )
}
