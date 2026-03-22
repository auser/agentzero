/**
 * Right-click context menu for the workflow canvas.
 */

interface CanvasContextMenuProps {
  position: { x: number; y: number }
  onAddNode: () => void
  onCreateNodeType?: () => void
  onClearAll: () => void
  onClose: () => void
}

export function CanvasContextMenu({ position, onAddNode, onCreateNodeType, onClearAll, onClose }: CanvasContextMenuProps) {
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
        <button
          className="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-foreground hover:bg-accent transition-colors"
          onClick={onAddNode}
        >
          <span className="text-muted-foreground">⌘K</span>
          Add Node
        </button>
        {onCreateNodeType && (
          <button
            className="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-foreground hover:bg-accent transition-colors"
            onClick={onCreateNodeType}
          >
            <span className="text-muted-foreground">🧩</span>
            Create Node Type
          </button>
        )}
        <div className="my-1 h-px bg-border/50" />
        <button
          className="flex w-full items-center gap-2 rounded-sm px-3 py-1.5 text-xs text-destructive hover:bg-destructive/10 transition-colors"
          onClick={onClearAll}
        >
          Clear All
        </button>
      </div>
    </>
  )
}
