/**
 * Template gallery modal for the workflow canvas.
 * Displays pre-built workflow templates in a searchable grid.
 * Users can preview and select a template to populate the canvas.
 */
import { useState, useEffect, useRef, useMemo } from 'react'
import { ALL_TEMPLATES, type WorkflowTemplate } from '@/lib/workflow-templates'
import { X, Layout, Search } from 'lucide-react'

interface TemplateGalleryProps {
  open: boolean
  onClose: () => void
  onSelect: (template: WorkflowTemplate) => void
}

const CATEGORY_COLORS: Record<string, string> = {
  research: '#3b82f6',
  content: '#8b5cf6',
  engineering: '#f97316',
  support: '#ec4899',
  analytics: '#22c55e',
}

function categoryColor(category: string): string {
  return CATEGORY_COLORS[category] ?? '#6b7280'
}

export function TemplateGallery({ open, onClose, onSelect }: TemplateGalleryProps) {
  const [query, setQuery] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)

  // Reset search and focus input when opened
  useEffect(() => {
    if (open) {
      setQuery('') // eslint-disable-line react-hooks/set-state-in-effect -- reset on open
      setTimeout(() => inputRef.current?.focus(), 50)
    }
  }, [open])

  // Close on Escape
  useEffect(() => {
    if (!open) return
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [open, onClose])

  const filtered = useMemo(() => {
    if (!query) return ALL_TEMPLATES
    const q = query.toLowerCase()
    return ALL_TEMPLATES.filter(
      (t) =>
        t.name.toLowerCase().includes(q) ||
        t.description.toLowerCase().includes(q) ||
        t.category.toLowerCase().includes(q),
    )
  }, [query])

  if (!open) return null

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="w-[640px] max-h-[80vh] flex flex-col overflow-hidden"
        style={{
          background: '#1C1C1E',
          border: '1px solid rgba(255,255,255,0.06)',
          borderRadius: 14,
          fontFamily: "'JetBrains Mono', monospace",
        }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* Header */}
        <div
          className="flex items-center justify-between px-5 py-4"
          style={{ borderBottom: '1px solid rgba(255,255,255,0.06)' }}
        >
          <div className="flex items-center gap-2">
            <Layout className="h-4 w-4 text-green-400" />
            <h2 className="text-sm font-semibold text-white">Workflow Templates</h2>
          </div>
          <button
            onClick={onClose}
            className="text-neutral-500 hover:text-white transition-colors"
          >
            <X className="h-4 w-4" />
          </button>
        </div>

        {/* Search */}
        <div
          className="flex items-center gap-2 px-5 py-3"
          style={{ borderBottom: '1px solid rgba(255,255,255,0.06)' }}
        >
          <Search className="h-4 w-4 text-neutral-500 shrink-0" />
          <input
            ref={inputRef}
            type="text"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search templates..."
            className="flex-1 bg-transparent text-sm text-white outline-none placeholder:text-neutral-600"
          />
        </div>

        {/* Template grid */}
        <div className="overflow-y-auto flex-1 p-4">
          {filtered.length === 0 ? (
            <p className="text-sm text-neutral-500 text-center py-12">
              No templates match your search
            </p>
          ) : (
            <div className="grid grid-cols-2 gap-3">
              {filtered.map((template) => {
                const color = categoryColor(template.category)
                return (
                  <div
                    key={template.id}
                    className="flex flex-col p-4 transition-colors hover:brightness-110"
                    style={{
                      background: '#1C1C1E',
                      border: '1px solid rgba(255,255,255,0.06)',
                      borderRadius: 14,
                    }}
                  >
                    {/* Category badge + node count */}
                    <div className="flex items-center justify-between mb-2">
                      <span
                        className="text-[9px] font-medium uppercase tracking-wider px-1.5 py-0.5 rounded"
                        style={{ color, backgroundColor: `${color}26` }}
                      >
                        {template.category}
                      </span>
                      <span className="text-[10px] text-neutral-500">
                        {template.nodeCount} nodes
                      </span>
                    </div>

                    {/* Name */}
                    <h3 className="text-sm font-semibold text-white mb-1 leading-tight">
                      {template.name}
                    </h3>

                    {/* Description */}
                    <p className="text-xs text-neutral-400 leading-relaxed mb-4 flex-1">
                      {template.description}
                    </p>

                    {/* Use Template button */}
                    <button
                      onClick={() => onSelect(template)}
                      className="w-full py-1.5 text-xs font-medium text-white transition-opacity hover:opacity-90"
                      style={{
                        background: '#22c55e',
                        borderRadius: 8,
                      }}
                    >
                      Use Template
                    </button>
                  </div>
                )
              })}
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
