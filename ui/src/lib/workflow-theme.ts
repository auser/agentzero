/**
 * Workflow theme utilities.
 * CSS variable resolution for canvas rendering.
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
