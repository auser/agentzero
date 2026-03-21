/**
 * Workflow node type theme tokens.
 * Uses Tailwind color classes — no hardcoded hex values.
 * For canvas rendering (which needs hex), resolve from CSS variables at runtime.
 */

/** Resolve a CSS variable to its computed value */
export function cssVar(name: string): string {
  return getComputedStyle(document.documentElement)
    .getPropertyValue(name)
    .trim()
}

/** Convert HSL CSS var value to hex for canvas rendering */
export function hslVarToHex(cssVarName: string): string {
  const hsl = cssVar(cssVarName)
  if (!hsl) return '#6b7280'
  // CSS vars are in "H S% L%" format
  const el = document.createElement('div')
  el.style.color = `hsl(${hsl})`
  document.body.appendChild(el)
  const computed = getComputedStyle(el).color
  document.body.removeChild(el)
  // computed is "rgb(r, g, b)"
  const match = computed.match(/(\d+)/g)
  if (!match || match.length < 3) return '#6b7280'
  return '#' + match.slice(0, 3).map((n) => parseInt(n).toString(16).padStart(2, '0')).join('')
}

/**
 * Node type visual config.
 * Tailwind classes for React components, CSS-resolvable for canvas.
 */
export const NODE_TYPES = {
  agent: {
    label: 'Agent',
    tailwind: {
      dot: 'bg-blue-500',
      border: 'border-blue-500/50',
      text: 'text-blue-500',
      bg: 'bg-blue-500/10',
    },
  },
  tool: {
    label: 'Tool',
    tailwind: {
      dot: 'bg-violet-500',
      border: 'border-violet-500/50',
      text: 'text-violet-500',
      bg: 'bg-violet-500/10',
    },
  },
  channel: {
    label: 'Channel',
    tailwind: {
      dot: 'bg-pink-500',
      border: 'border-pink-500/50',
      text: 'text-pink-500',
      bg: 'bg-pink-500/10',
    },
  },
  subagent: {
    label: 'Sub-Agent',
    tailwind: {
      dot: 'bg-emerald-500',
      border: 'border-emerald-500/50',
      text: 'text-emerald-500',
      bg: 'bg-emerald-500/10',
    },
  },
  schedule: {
    label: 'Schedule',
    tailwind: {
      dot: 'bg-yellow-500',
      border: 'border-yellow-500/50',
      text: 'text-yellow-500',
      bg: 'bg-yellow-500/10',
    },
  },
  gate: {
    label: 'Approval',
    tailwind: {
      dot: 'bg-red-500',
      border: 'border-red-500/50',
      text: 'text-red-500',
      bg: 'bg-red-500/10',
    },
  },
} as const

export type NodeType = keyof typeof NODE_TYPES
