/**
 * Modal dialog for saving the current workflow as a template.
 */
import { useState, useEffect, useRef } from 'react'
import { X } from 'lucide-react'

interface SaveTemplateDialogProps {
  open: boolean
  nodeCount: number
  onSave: (name: string, description: string) => void
  onClose: () => void
}

export function SaveTemplateDialog({ open, nodeCount, onSave, onClose }: SaveTemplateDialogProps) {
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open) {
      setName('')
      setDescription('')
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [open])

  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [open, onClose])

  if (!open) return null

  const canSave = name.trim().length > 0

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 420,
          background: '#1C1C1E',
          border: '1px solid rgba(255,255,255,0.06)',
          borderRadius: 14,
          fontFamily: "'JetBrains Mono', monospace",
          padding: 24,
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 20 }}>
          <h2 style={{ fontSize: 15, fontWeight: 600, color: '#E5E5E5' }}>Save as Template</h2>
          <button onClick={onClose} style={{ background: 'none', border: 'none', cursor: 'pointer', color: '#737373' }}>
            <X className="h-4 w-4" />
          </button>
        </div>

        <div style={{ marginBottom: 14 }}>
          <label style={{ fontSize: 11, fontWeight: 500, color: '#737373', display: 'block', marginBottom: 4 }}>
            Name
          </label>
          <input
            ref={inputRef}
            type="text"
            value={name}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => { if (e.key === 'Enter' && canSave) onSave(name.trim(), description.trim()) }}
            placeholder="My workflow template..."
            style={{
              width: '100%', background: '#0F0F11', borderRadius: 8,
              padding: '10px 12px', fontSize: 13, color: '#E5E5E5',
              border: 'none', outline: 'none',
              fontFamily: "'JetBrains Mono', monospace", boxSizing: 'border-box',
            }}
          />
        </div>

        <div style={{ marginBottom: 20 }}>
          <label style={{ fontSize: 11, fontWeight: 500, color: '#737373', display: 'block', marginBottom: 4 }}>
            Description
          </label>
          <textarea
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="What does this workflow do?"
            rows={3}
            style={{
              width: '100%', background: '#0F0F11', borderRadius: 8,
              padding: '10px 12px', fontSize: 12, color: '#A3A3A3',
              border: 'none', outline: 'none', resize: 'vertical',
              fontFamily: "'JetBrains Mono', monospace", boxSizing: 'border-box',
              lineHeight: 1.6,
            }}
          />
        </div>

        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <span style={{ fontSize: 11, color: '#525252' }}>{nodeCount} nodes</span>
          <div style={{ display: 'flex', gap: 8 }}>
            <button
              onClick={onClose}
              style={{
                padding: '8px 16px', background: 'transparent', color: '#737373',
                border: '1px solid rgba(255,255,255,0.06)', borderRadius: 8,
                fontSize: 12, fontWeight: 500, cursor: 'pointer',
                fontFamily: "'JetBrains Mono', monospace",
              }}
            >
              Cancel
            </button>
            <button
              onClick={() => { if (canSave) onSave(name.trim(), description.trim()) }}
              disabled={!canSave}
              style={{
                padding: '8px 16px', background: canSave ? '#22c55e' : '#374151',
                color: '#fff', border: 'none', borderRadius: 8,
                fontSize: 12, fontWeight: 600, cursor: canSave ? 'pointer' : 'not-allowed',
                fontFamily: "'JetBrains Mono', monospace",
              }}
            >
              Save Template
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
