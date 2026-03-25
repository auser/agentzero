import { createFileRoute, Link } from '@tanstack/react-router'
import { WorkflowTopology } from '@/components/dashboard/WorkflowTopology'
import { DraggablePalette } from '@/components/workflows/DraggablePalette'
import { useWorkflowStore } from '@/store/workflowStore'
import { workflowsApi } from '@/lib/api/workflows'
import { Button } from '@/components/ui/button'
import { Trash2, ArrowLeft, Download, Upload, LayoutGrid } from 'lucide-react'
import { useCallback, useRef } from 'react'
import { useReactFlow } from '@xyflow/react'

export const Route = createFileRoute('/workflows/$workflowId')({
  component: WorkflowEditorPage,
})

function WorkflowEditorPage() {
  const { workflowId } = Route.useParams()
  const { graphState, clear } = useWorkflowStore()
  const fileInputRef = useRef<HTMLInputElement>(null)

  const handleExport = useCallback(async () => {
    try {
      const data = await workflowsApi.exportWorkflow(workflowId)
      const blob = new Blob([JSON.stringify(data, null, 2)], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')
      a.href = url
      a.download = `workflow-${workflowId}.agentzero-workflow.json`
      a.click()
      URL.revokeObjectURL(url)
    } catch {
      console.error('Export failed')
    }
  }, [workflowId])

  const handleImport = useCallback(() => {
    fileInputRef.current?.click()
  }, [])

  const handleFileSelected = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    try {
      const text = await file.text()
      const data = JSON.parse(text)
      const result = await workflowsApi.importWorkflow(data)
      window.location.href = `/workflows/${result.workflow_id}`
    } catch {
      console.error('Import failed')
    }
    if (fileInputRef.current) fileInputRef.current.value = ''
  }, [])

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
        <div className="flex items-center gap-1.5">
          <Button
            variant="ghost"
            size="sm"
            className="h-7 text-xs text-muted-foreground"
            onClick={handleExport}
            title="Export workflow as JSON"
          >
            <Download className="h-3 w-3 mr-1" />
            Export
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="h-7 text-xs text-muted-foreground"
            onClick={handleImport}
            title="Import workflow from JSON file"
          >
            <Upload className="h-3 w-3 mr-1" />
            Import
          </Button>
          <AutoLayoutButton />
          <input
            ref={fileInputRef}
            type="file"
            accept=".json,.agentzero-workflow.json"
            className="hidden"
            onChange={handleFileSelected}
          />
          <div className="w-px h-4 bg-border/50 mx-0.5" />
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

function AutoLayoutButton() {
  const { getNodes, setNodes, fitView } = useReactFlow()

  const handleAutoLayout = useCallback(() => {
    const nodes = getNodes()
    if (nodes.length === 0) return

    // Simple top-down grid layout: sort by connections, place in columns
    const COLUMN_WIDTH = 300
    const ROW_HEIGHT = 180
    const COLS = Math.max(1, Math.ceil(Math.sqrt(nodes.length)))

    const laid = nodes.map((node, i) => ({
      ...node,
      position: {
        x: (i % COLS) * COLUMN_WIDTH + 50,
        y: Math.floor(i / COLS) * ROW_HEIGHT + 50,
      },
    }))
    setNodes(laid)
    setTimeout(() => fitView({ padding: 0.2, duration: 300 }), 50)
  }, [getNodes, setNodes, fitView])

  return (
    <Button
      variant="ghost"
      size="sm"
      className="h-7 text-xs text-muted-foreground"
      onClick={handleAutoLayout}
      title="Auto-layout nodes in a grid"
    >
      <LayoutGrid className="h-3 w-3 mr-1" />
      Layout
    </Button>
  )
}
