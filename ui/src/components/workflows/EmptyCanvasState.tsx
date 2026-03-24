/**
 * Empty state shown when the workflow canvas has zero nodes.
 * Offers entry points to the template gallery or Cmd+K palette.
 */
import { Layout, Zap } from 'lucide-react'

interface EmptyCanvasStateProps {
  onOpenGallery: () => void
  onStartScratch: () => void
}

export function EmptyCanvasState({ onOpenGallery, onStartScratch }: EmptyCanvasStateProps) {
  return (
    <div
      className="absolute inset-0 z-10 flex items-center justify-center pointer-events-none"
      style={{ fontFamily: "'JetBrains Mono', monospace" }}
    >
      <div className="flex flex-col items-center gap-6 pointer-events-auto">
        {/* Icon */}
        <div
          className="flex items-center justify-center w-14 h-14 rounded-2xl"
          style={{
            background: 'rgba(34,197,94,0.10)',
            border: '1px solid rgba(34,197,94,0.20)',
          }}
        >
          <Layout className="h-6 w-6 text-green-400" />
        </div>

        {/* Text */}
        <div className="text-center space-y-1.5">
          <h2 className="text-base font-semibold text-white">Build Your Workflow</h2>
          <p className="text-xs text-neutral-500 max-w-[280px] leading-relaxed">
            Start from a template or add nodes manually with{' '}
            <kbd className="text-[10px] text-neutral-400 bg-neutral-800 px-1 py-0.5 rounded border border-neutral-700">
              Cmd+K
            </kbd>
          </p>
        </div>

        {/* Buttons */}
        <div className="flex items-center gap-3">
          <button
            onClick={onOpenGallery}
            className="flex items-center gap-2 px-4 py-2 text-xs font-medium text-white transition-opacity hover:opacity-90"
            style={{
              background: '#22c55e',
              borderRadius: 8,
            }}
          >
            <Layout className="h-3.5 w-3.5" />
            Choose a Template
          </button>
          <button
            onClick={onStartScratch}
            className="flex items-center gap-2 px-4 py-2 text-xs font-medium text-neutral-300 transition-colors hover:text-white"
            style={{
              background: 'transparent',
              border: '1px solid rgba(255,255,255,0.10)',
              borderRadius: 8,
            }}
          >
            <Zap className="h-3.5 w-3.5" />
            Start from Scratch
          </button>
        </div>
      </div>
    </div>
  )
}
