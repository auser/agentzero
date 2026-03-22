/**
 * Custom node renderer for workflow-graph.
 * Draws distinct visuals per node_type metadata field.
 */
import type { Job } from '@/lib/workflow-types'

const NODE_TYPE_STYLES: Record<string, { bg: string; border: string; icon: string; label: string }> = {
  agent: { bg: '#1e293b', border: '#3b82f6', icon: '🤖', label: 'AGENT' },
  tool: { bg: '#1a2332', border: '#8b5cf6', icon: '🔧', label: 'TOOL' },
  subagent: { bg: '#1e2a1e', border: '#22c55e', icon: '🔀', label: 'SUB-AGENT' },
  channel: { bg: '#2a1e2e', border: '#ec4899', icon: '📡', label: 'CHANNEL' },
  schedule: { bg: '#2a2a1e', border: '#eab308', icon: '⏰', label: 'SCHEDULE' },
  gate: { bg: '#2a1e1e', border: '#ef4444', icon: '🛡️', label: 'APPROVAL' },
}

const DEFAULT_STYLE = { bg: '#1f2937', border: '#6b7280', icon: '⚙️', label: 'JOB' }

const STATUS_DOT_COLORS: Record<string, string> = {
  running: '#22c55e',
  success: '#22c55e',
  failure: '#ef4444',
  queued: '#6b7280',
  skipped: '#6b7280',
  cancelled: '#6b7280',
}

/**
 * onRenderNode callback for workflow-graph.
 * Returns true to skip default rendering.
 */
export function renderNode(
  x: number,
  y: number,
  w: number,
  h: number,
  job: Job,
): boolean {
  const canvas = document.querySelector('canvas')
  if (!canvas) return false
  const ctx = canvas.getContext('2d')
  if (!ctx) return false

  const nodeType = (job.metadata?.node_type as string) ?? 'job'
  const style = NODE_TYPE_STYLES[nodeType] ?? DEFAULT_STYLE

  // Background
  ctx.fillStyle = style.bg
  ctx.strokeStyle = style.border
  ctx.lineWidth = 2
  ctx.beginPath()
  ctx.roundRect(x, y, w, h, 8)
  ctx.fill()
  ctx.stroke()

  // Type badge (top-left)
  ctx.fillStyle = style.border
  ctx.font = '9px ui-sans-serif, system-ui, sans-serif'
  ctx.globalAlpha = 0.8
  ctx.fillText(style.label, x + 8, y + 12)
  ctx.globalAlpha = 1.0

  // Status dot
  const dotColor = STATUS_DOT_COLORS[job.status] ?? '#6b7280'
  ctx.beginPath()
  ctx.arc(x + w - 12, y + 12, 4, 0, Math.PI * 2)
  ctx.fillStyle = dotColor
  ctx.fill()

  // Node name
  ctx.fillStyle = '#e5e7eb'
  ctx.font = '13px ui-monospace, monospace'
  const maxChars = Math.floor((w - 20) / 7.5)
  const displayName = job.name.length > maxChars ? job.name.slice(0, maxChars - 1) + '…' : job.name
  ctx.fillText(displayName, x + 8, y + 28)

  // Description from metadata (if present)
  const description = job.metadata?.description as string | undefined
  if (description) {
    ctx.fillStyle = '#9ca3af'
    ctx.font = '11px ui-sans-serif, system-ui, sans-serif'
    const maxDescChars = Math.floor((w - 16) / 6)
    const desc = description.length > maxDescChars ? description.slice(0, maxDescChars - 1) + '…' : description
    ctx.fillText(desc, x + 8, y + 42)
  }

  // Command/model info (bottom line)
  const model = job.metadata?.model as string | undefined
  const toolName = job.metadata?.tool_name as string | undefined
  const infoText = model ?? toolName ?? ''
  if (infoText) {
    ctx.fillStyle = '#6b7280'
    ctx.font = '10px ui-sans-serif, system-ui, sans-serif'
    ctx.fillText(infoText, x + 8, y + h - 8)
  }

  return true // skip default rendering
}
