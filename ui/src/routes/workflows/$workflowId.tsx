import { createFileRoute, Link } from '@tanstack/react-router'
import { WorkflowTopology } from '@/components/dashboard/WorkflowTopology'
import { DraggablePalette } from '@/components/workflows/DraggablePalette'
import { useWorkflowStore } from '@/store/workflowStore'
import { Button } from '@/components/ui/button'
import { Trash2, ArrowLeft } from 'lucide-react'

export const Route = createFileRoute('/workflows/$workflowId')({
  component: WorkflowEditorPage,
})

function WorkflowEditorPage() {
  const { workflowId } = Route.useParams()
  const { graphState, clear } = useWorkflowStore()

  return (
    <div className="h-full flex flex-col -m-6">
      {/* Toolbar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-border/50 bg-card/50 shrink-0">
        <div className="flex items-center gap-2">
          <Button variant="ghost" size="icon" className="h-7 w-7" asChild>
            <Link to="/workflows">
              <ArrowLeft className="h-3.5 w-3.5" />
            </Link>
          </Button>
          <h1 className="text-sm font-semibold">Workflow Editor</h1>
          {graphState && (
            <span className="text-[10px] text-muted-foreground/50">
              {graphState.workflow?.jobs?.length ?? 0} nodes saved
            </span>
          )}
        </div>
        <div className="flex items-center gap-2">
          {graphState && (
            <Button
              variant="ghost"
              size="sm"
              className="h-7 text-xs text-muted-foreground"
              onClick={() => { clear(); window.location.reload() }}
            >
              <Trash2 className="h-3 w-3 mr-1" />
              Clear
            </Button>
          )}
          <span className="text-[9px] text-muted-foreground/40">
            Press <kbd className="bg-muted/30 px-1 py-0.5 rounded">⌘K</kbd> to add nodes
          </span>
        </div>
      </div>

      {/* Main area: graph + palette */}
      <div className="flex-1 flex min-h-0">
        <div className="flex-1 min-w-0 h-full">
          <WorkflowTopology fullHeight workflowId={workflowId} />
        </div>
        <div className="w-64 border-l border-border/50 shrink-0 overflow-hidden">
          <DraggablePalette />
        </div>
      </div>
    </div>
  )
}
