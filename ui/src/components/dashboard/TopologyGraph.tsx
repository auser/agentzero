import { useRef, useEffect, useCallback } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useNavigate } from '@tanstack/react-router'
import { topologyApi, type TopologyNode, type TopologyEdge } from '@/lib/api/topology'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'

const STATUS_COLORS: Record<string, string> = {
  running: '#22c55e',
  active: '#3b82f6',
  stale: '#eab308',
  stopped: '#6b7280',
  idle: '#6b7280',
}

const NODE_W = 160
const NODE_H = 56
const H_GAP = 80
const V_GAP = 40

interface PositionedNode extends TopologyNode {
  x: number
  y: number
}

function layoutNodes(nodes: TopologyNode[], edges: TopologyEdge[]): PositionedNode[] {
  if (nodes.length === 0) return []

  // Simple left-to-right layout: roots on the left, children to the right.
  const children = new Set(edges.map((e) => e.to_agent_id))
  const roots = nodes.filter((n) => !children.has(n.agent_id))
  const nonRoots = nodes.filter((n) => children.has(n.agent_id))

  const positioned: PositionedNode[] = []
  const padding = 20

  // Place roots in a column on the left.
  roots.forEach((node, i) => {
    positioned.push({ ...node, x: padding, y: padding + i * (NODE_H + V_GAP) })
  })

  // Place children in a column to the right.
  nonRoots.forEach((node, i) => {
    positioned.push({
      ...node,
      x: padding + NODE_W + H_GAP,
      y: padding + i * (NODE_H + V_GAP),
    })
  })

  return positioned
}

function findNodeCenter(nodes: PositionedNode[], agentId: string): { cx: number; cy: number } | null {
  const node = nodes.find((n) => n.agent_id === agentId)
  if (!node) return null
  return { cx: node.x + NODE_W / 2, cy: node.y + NODE_H / 2 }
}

export function TopologyGraph() {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const navigate = useNavigate()

  const { data: topology } = useQuery({
    queryKey: ['topology'],
    queryFn: () => topologyApi.get(),
    refetchInterval: 3_000,
  })

  const nodes = topology?.nodes ?? []
  const edges = topology?.edges ?? []
  const positioned = layoutNodes(nodes, edges)

  const draw = useCallback(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const rect = canvas.getBoundingClientRect()
    canvas.width = rect.width * dpr
    canvas.height = rect.height * dpr
    ctx.scale(dpr, dpr)

    ctx.clearRect(0, 0, rect.width, rect.height)

    // Draw edges.
    for (const edge of edges) {
      const from = findNodeCenter(positioned, edge.from_agent_id)
      const to = findNodeCenter(positioned, edge.to_agent_id)
      if (!from || !to) continue

      ctx.beginPath()
      ctx.moveTo(from.cx + NODE_W / 2, from.cy)
      ctx.lineTo(to.cx - NODE_W / 2, to.cy)
      ctx.strokeStyle = '#4b5563'
      ctx.lineWidth = 2
      ctx.stroke()

      // Arrow head.
      const angle = Math.atan2(to.cy - from.cy, to.cx - NODE_W / 2 - (from.cx + NODE_W / 2))
      const arrowX = to.cx - NODE_W / 2
      const arrowY = to.cy
      ctx.beginPath()
      ctx.moveTo(arrowX, arrowY)
      ctx.lineTo(arrowX - 8 * Math.cos(angle - 0.4), arrowY - 8 * Math.sin(angle - 0.4))
      ctx.lineTo(arrowX - 8 * Math.cos(angle + 0.4), arrowY - 8 * Math.sin(angle + 0.4))
      ctx.closePath()
      ctx.fillStyle = '#4b5563'
      ctx.fill()
    }

    // Draw nodes.
    for (const node of positioned) {
      const color = STATUS_COLORS[node.status] ?? '#6b7280'

      // Node background.
      ctx.fillStyle = '#1f2937'
      ctx.strokeStyle = color
      ctx.lineWidth = 2
      ctx.beginPath()
      ctx.roundRect(node.x, node.y, NODE_W, NODE_H, 8)
      ctx.fill()
      ctx.stroke()

      // Status dot.
      ctx.beginPath()
      ctx.arc(node.x + 16, node.y + NODE_H / 2, 5, 0, Math.PI * 2)
      ctx.fillStyle = color
      ctx.fill()

      // Agent name.
      ctx.fillStyle = '#e5e7eb'
      ctx.font = '13px ui-monospace, monospace'
      ctx.fillText(
        node.name.length > 14 ? node.name.slice(0, 14) + '...' : node.name,
        node.x + 28,
        node.y + 22,
      )

      // Stats line.
      ctx.fillStyle = '#9ca3af'
      ctx.font = '11px ui-sans-serif, system-ui'
      const runs = node.active_run_count
      const cost = (node.total_cost_microdollars / 1_000_000).toFixed(2)
      ctx.fillText(`${runs} run${runs !== 1 ? 's' : ''} · $${cost}`, node.x + 28, node.y + 40)
    }
  }, [positioned, edges])

  useEffect(() => {
    draw()
  }, [draw])

  // Handle click on nodes.
  const handleClick = useCallback(
    (e: React.MouseEvent<HTMLCanvasElement>) => {
      const canvas = canvasRef.current
      if (!canvas) return
      const rect = canvas.getBoundingClientRect()
      const x = e.clientX - rect.left
      const y = e.clientY - rect.top

      for (const node of positioned) {
        if (x >= node.x && x <= node.x + NODE_W && y >= node.y && y <= node.y + NODE_H) {
          void navigate({ to: '/agents' })
          break
        }
      }
    },
    [positioned, navigate],
  )

  if (nodes.length === 0) {
    return null
  }

  const maxY = Math.max(...positioned.map((n) => n.y + NODE_H)) + 20

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">Agent Topology</CardTitle>
      </CardHeader>
      <CardContent>
        <canvas
          ref={canvasRef}
          onClick={handleClick}
          className="w-full cursor-pointer rounded"
          style={{ height: Math.max(maxY, 120), background: '#111827' }}
        />
      </CardContent>
    </Card>
  )
}
