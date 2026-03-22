/**
 * Data-driven right-click context menu for the workflow canvas.
 * Items are defined in canvas-actions.ts — add new entries there.
 */
import { getContextMenuActions } from '@/lib/canvas-actions'

interface CanvasContextMenuProps {
  position: { x: number; y: number }
  handlers: Record<string, () => void>
  onClose: () => void
}

const ITEM_CLASS =
  'flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-foreground hover:bg-accent transition-colors'
const DANGER_CLASS =
  'flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-destructive hover:bg-destructive/10 transition-colors'

export function CanvasContextMenu({ position, handlers, onClose }: CanvasContextMenuProps) {
  const { primary, secondary, danger } = getContextMenuActions()

  const renderItem = (action: (typeof primary)[0], isDanger = false) => {
    const handler = handlers[action.id]
    if (!handler) return null
    return (
      <button
        key={action.id}
        className={isDanger ? DANGER_CLASS : ITEM_CLASS}
        onClick={() => { handler(); onClose() }}
      >
        {action.menuIcon && <span className="text-muted-foreground">{action.menuIcon}</span>}
        {action.label}
      </button>
    )
  }

  const primaryItems = primary.map((a) => renderItem(a)).filter(Boolean)
  const secondaryItems = secondary.map((a) => renderItem(a)).filter(Boolean)
  const dangerItems = danger.map((a) => renderItem(a, true)).filter(Boolean)

  return (
    <>
      <div
        className="fixed inset-0 z-40"
        onClick={onClose}
        onContextMenu={(e) => { e.preventDefault(); onClose() }}
      />
      <div
        className="fixed z-50 min-w-[180px] rounded-md border border-border bg-zinc-900 p-1 shadow-xl shadow-black/50"
        style={{ left: position.x, top: position.y }}
      >
        {primaryItems}
        {secondaryItems.length > 0 && primaryItems.length > 0 && (
          <div className="my-1 h-px bg-border/50" />
        )}
        {secondaryItems}
        {dangerItems.length > 0 && (
          <div className="my-1 h-px bg-border/50" />
        )}
        {dangerItems}
      </div>
    </>
  )
}
