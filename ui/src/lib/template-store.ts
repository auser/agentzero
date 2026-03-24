/**
 * Local template store — persists saved templates to localStorage
 * as a fallback when the API is unavailable.
 */
import type { WorkflowTemplate } from '@/lib/workflow-templates'

const STORAGE_KEY = 'agentzero-saved-templates'

export type SavedTemplate = WorkflowTemplate & { savedAt: number }

export function loadLocalTemplates(): SavedTemplate[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    return raw ? JSON.parse(raw) : []
  } catch { return [] }
}

export function saveLocalTemplate(template: SavedTemplate) {
  const existing = loadLocalTemplates().filter((t) => t.id !== template.id)
  existing.push(template)
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(existing)) } catch { /* full */ }
}

export function deleteLocalTemplate(id: string) {
  const existing = loadLocalTemplates().filter((t) => t.id !== id)
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(existing)) } catch { /* full */ }
}
