/**
 * Dialog for creating a custom node type.
 * Lets users define name, icon, color, category, and input/output ports.
 * Persists to the dynamic node definitions registry.
 */
import { useState, useEffect, useRef } from 'react'
import { X, Plus, Trash2 } from 'lucide-react'
import { registerNodeDefinition } from '@/lib/node-definitions'
import { portTypeColor, type Port } from '@/lib/workflow-types'

interface CreateNodeTypeDialogProps {
  open: boolean
  onClose: () => void
}

const PORT_TYPES = ['text', 'json', 'number', 'boolean', 'array', 'event', 'config']
const CATEGORIES = ['core', 'integration', 'io', 'trigger', 'control', 'config', 'custom']
const ICONS = ['⚙️', '🔧', '📡', '🤖', '💾', '📄', '🌐', '✋', '⏰', '🛡️', '🔀', '🎭', '⚡', '🔌', '📊', '🧩', '🎯', '📦']

export function CreateNodeTypeDialog({ open, onClose }: CreateNodeTypeDialogProps) {
  const [typeName, setTypeName] = useState('')
  const [label, setLabel] = useState('')
  const [icon, setIcon] = useState('⚙️')
  const [category, setCategory] = useState('custom')
  const [color, setColor] = useState('#6b7280')
  const [inputs, setInputs] = useState<Port[]>([])
  const [outputs, setOutputs] = useState<Port[]>([])
  const inputRef = useRef<HTMLInputElement>(null)

  useEffect(() => {
    if (open) {
      setTypeName('')
      setLabel('')
      setIcon('⚙️')
      setCategory('custom')
      setColor('#6b7280')
      setInputs([])
      setOutputs([])
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [open])

  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => { if (e.key === 'Escape') onClose() }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [open, onClose])

  // Auto-generate type key from label
  useEffect(() => {
    if (label && !typeName) {
      setTypeName(label.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, ''))
    }
  }, [label, typeName])

  const addPort = (direction: 'input' | 'output') => {
    const port: Port = {
      id: `${direction}_${Date.now()}`,
      label: `new_${direction}`,
      direction,
      port_type: 'text',
    }
    if (direction === 'input') setInputs((p) => [...p, port])
    else setOutputs((p) => [...p, port])
  }

  const updatePort = (direction: 'input' | 'output', portId: string, updates: Partial<Port>) => {
    const setter = direction === 'input' ? setInputs : setOutputs
    setter((ports) => ports.map((p) => p.id === portId ? { ...p, ...updates } : p))
  }

  const removePort = (direction: 'input' | 'output', portId: string) => {
    const setter = direction === 'input' ? setInputs : setOutputs
    setter((ports) => ports.filter((p) => p.id !== portId))
  }

  const canSave = label.trim().length > 0 && typeName.trim().length > 0

  const handleSave = () => {
    if (!canSave) return
    const typeKey = typeName.trim().toLowerCase().replace(/[^a-z0-9_]/g, '_')
    registerNodeDefinition({
      type: typeKey,
      label: label.trim(),
      icon,
      headerColor: color,
      category,
      fields: [],
      inputs: inputs.map((p) => ({ ...p, id: p.label.replace(/\s+/g, '_').toLowerCase() })),
      outputs: outputs.map((p) => ({ ...p, id: p.label.replace(/\s+/g, '_').toLowerCase() })),
    })
    onClose()
  }

  if (!open) return null

  const inputStyle = {
    width: '100%', background: '#0F0F11', borderRadius: 6,
    padding: '8px 10px', fontSize: 12, color: '#E5E5E5',
    border: 'none', outline: 'none',
    fontFamily: "'JetBrains Mono', monospace", boxSizing: 'border-box' as const,
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          width: 480, maxHeight: '85vh', overflowY: 'auto',
          background: '#1C1C1E',
          border: '1px solid rgba(255,255,255,0.06)',
          borderRadius: 14,
          fontFamily: "'JetBrains Mono', monospace",
          padding: 24,
        }}
      >
        {/* Header */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 20 }}>
          <h2 style={{ fontSize: 15, fontWeight: 600, color: '#E5E5E5' }}>Create Custom Node</h2>
          <button onClick={onClose} style={{ background: 'none', border: 'none', cursor: 'pointer', color: '#737373' }}>
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Icon + Label row */}
        <div style={{ display: 'flex', gap: 10, marginBottom: 14 }}>
          <div>
            <label style={{ fontSize: 11, fontWeight: 500, color: '#737373', display: 'block', marginBottom: 4 }}>Icon</label>
            <select
              value={icon}
              onChange={(e) => setIcon(e.target.value)}
              style={{ ...inputStyle, width: 60, fontSize: 18, textAlign: 'center', appearance: 'none' }}
            >
              {ICONS.map((i) => <option key={i} value={i}>{i}</option>)}
            </select>
          </div>
          <div style={{ flex: 1 }}>
            <label style={{ fontSize: 11, fontWeight: 500, color: '#737373', display: 'block', marginBottom: 4 }}>Label</label>
            <input ref={inputRef} type="text" value={label} onChange={(e) => setLabel(e.target.value)} placeholder="My Node" style={inputStyle} />
          </div>
        </div>

        {/* Type key + Category + Color */}
        <div style={{ display: 'flex', gap: 10, marginBottom: 14 }}>
          <div style={{ flex: 1 }}>
            <label style={{ fontSize: 11, fontWeight: 500, color: '#737373', display: 'block', marginBottom: 4 }}>Type Key</label>
            <input type="text" value={typeName} onChange={(e) => setTypeName(e.target.value)} placeholder="my_node" style={inputStyle} />
          </div>
          <div>
            <label style={{ fontSize: 11, fontWeight: 500, color: '#737373', display: 'block', marginBottom: 4 }}>Category</label>
            <select value={category} onChange={(e) => setCategory(e.target.value)} style={{ ...inputStyle, width: 110, appearance: 'none' }}>
              {CATEGORIES.map((c) => <option key={c} value={c}>{c}</option>)}
            </select>
          </div>
          <div>
            <label style={{ fontSize: 11, fontWeight: 500, color: '#737373', display: 'block', marginBottom: 4 }}>Color</label>
            <input type="color" value={color} onChange={(e) => setColor(e.target.value)} style={{ width: 40, height: 34, border: 'none', borderRadius: 6, cursor: 'pointer', background: '#0F0F11' }} />
          </div>
        </div>

        {/* Inputs */}
        <div style={{ marginBottom: 14 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
            <label style={{ fontSize: 11, fontWeight: 600, color: '#737373', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Inputs</label>
            <button onClick={() => addPort('input')} style={{ background: 'none', border: 'none', cursor: 'pointer', color: '#3b82f6', fontSize: 11, fontFamily: "'JetBrains Mono', monospace", display: 'flex', alignItems: 'center', gap: 2 }}>
              <Plus className="h-3 w-3" /> Add
            </button>
          </div>
          {inputs.map((port) => (
            <div key={port.id} style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
              <div style={{ width: 8, height: 8, borderRadius: '50%', background: portTypeColor(port.port_type ?? ''), flexShrink: 0 }} />
              <input
                type="text" value={port.label}
                onChange={(e) => updatePort('input', port.id, { label: e.target.value })}
                style={{ ...inputStyle, flex: 1 }}
              />
              <select
                value={port.port_type ?? 'text'}
                onChange={(e) => updatePort('input', port.id, { port_type: e.target.value })}
                style={{ ...inputStyle, width: 75, appearance: 'none', color: portTypeColor(port.port_type ?? ''), fontSize: 11 }}
              >
                {PORT_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
              </select>
              <button onClick={() => removePort('input', port.id)} style={{ background: 'none', border: 'none', color: '#ef4444', cursor: 'pointer', padding: 0 }}>
                <Trash2 className="h-3 w-3" />
              </button>
            </div>
          ))}
          {inputs.length === 0 && <p style={{ fontSize: 11, color: '#525252' }}>No inputs</p>}
        </div>

        {/* Outputs */}
        <div style={{ marginBottom: 20 }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 6 }}>
            <label style={{ fontSize: 11, fontWeight: 600, color: '#737373', textTransform: 'uppercase', letterSpacing: '0.05em' }}>Outputs</label>
            <button onClick={() => addPort('output')} style={{ background: 'none', border: 'none', cursor: 'pointer', color: '#22c55e', fontSize: 11, fontFamily: "'JetBrains Mono', monospace", display: 'flex', alignItems: 'center', gap: 2 }}>
              <Plus className="h-3 w-3" /> Add
            </button>
          </div>
          {outputs.map((port) => (
            <div key={port.id} style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
              <div style={{ width: 8, height: 8, borderRadius: '50%', background: portTypeColor(port.port_type ?? ''), flexShrink: 0 }} />
              <input
                type="text" value={port.label}
                onChange={(e) => updatePort('output', port.id, { label: e.target.value })}
                style={{ ...inputStyle, flex: 1 }}
              />
              <select
                value={port.port_type ?? 'text'}
                onChange={(e) => updatePort('output', port.id, { port_type: e.target.value })}
                style={{ ...inputStyle, width: 75, appearance: 'none', color: portTypeColor(port.port_type ?? ''), fontSize: 11 }}
              >
                {PORT_TYPES.map((t) => <option key={t} value={t}>{t}</option>)}
              </select>
              <button onClick={() => removePort('output', port.id)} style={{ background: 'none', border: 'none', color: '#ef4444', cursor: 'pointer', padding: 0 }}>
                <Trash2 className="h-3 w-3" />
              </button>
            </div>
          ))}
          {outputs.length === 0 && <p style={{ fontSize: 11, color: '#525252' }}>No outputs</p>}
        </div>

        {/* Preview + Save */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {canSave && (
            <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <span style={{ fontSize: 16 }}>{icon}</span>
              <span style={{ fontSize: 12, fontWeight: 600, color }}>
                {label}
              </span>
              <span style={{ fontSize: 10, color: '#525252' }}>
                {inputs.length} in / {outputs.length} out
              </span>
            </div>
          )}
          {!canSave && <div />}
          <div style={{ display: 'flex', gap: 8 }}>
            <button
              onClick={onClose}
              style={{
                padding: '8px 16px', background: 'transparent', color: '#737373',
                border: '1px solid rgba(255,255,255,0.06)', borderRadius: 8,
                fontSize: 12, cursor: 'pointer', fontFamily: "'JetBrains Mono', monospace",
              }}
            >
              Cancel
            </button>
            <button
              onClick={handleSave}
              disabled={!canSave}
              style={{
                padding: '8px 16px', background: canSave ? '#7C3AED' : '#374151',
                color: '#fff', border: 'none', borderRadius: 8,
                fontSize: 12, fontWeight: 600, cursor: canSave ? 'pointer' : 'not-allowed',
                fontFamily: "'JetBrains Mono', monospace",
              }}
            >
              Create Node Type
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
