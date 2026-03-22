/**
 * Data-driven registry for canvas keyboard shortcuts and context menu items.
 *
 * Add new actions here — they automatically appear in the keyboard shortcuts
 * panel, get wired into the keydown handler, and optionally show in the
 * right-click context menu.
 *
 * Usage:
 *   1. Add an entry to CANVAS_ACTIONS
 *   2. Provide the action handler in the `handlers` map passed to
 *      useCanvasKeyboard() and <CanvasContextMenu>
 *   3. Done — shortcut panel and context menu update automatically
 */

export interface CanvasAction {
  /** Unique action identifier. */
  id: string
  /** Human-readable label shown in shortcuts panel and context menu. */
  label: string
  /** Keyboard shortcut display string (e.g. "Cmd + K"). */
  shortcut?: string
  /** Key matching config for the keydown handler. */
  key?: {
    /** The `e.key` value to match (e.g. 'k', 'z', '?', '/') */
    key: string
    /** Requires Cmd/Ctrl modifier. */
    mod?: boolean
    /** Requires Shift modifier. */
    shift?: boolean
    /** Alternative key (e.g. '/' as alternative to '?' with shift). */
    altKey?: string
  }
  /** Show in the right-click context menu. */
  contextMenu?: boolean
  /** Context menu section: 'primary' (top), 'secondary' (below divider), 'danger' (red, bottom). */
  menuSection?: 'primary' | 'secondary' | 'danger'
  /** Icon for context menu (emoji or string). */
  menuIcon?: string
  /** Category for grouping in shortcuts panel. */
  category?: 'editing' | 'navigation' | 'workflow' | 'help'
}

/**
 * All canvas actions. Add new ones here.
 */
export const CANVAS_ACTIONS: CanvasAction[] = [
  // ── Editing ──
  {
    id: 'command-palette',
    label: 'Command palette',
    shortcut: 'Cmd + K',
    key: { key: 'k', mod: true },
    contextMenu: true,
    menuSection: 'primary',
    menuIcon: '⌘K',
    category: 'editing',
  },
  {
    id: 'undo',
    label: 'Undo',
    shortcut: 'Cmd + Z',
    // Handled by useUndoRedo hook directly
    category: 'editing',
  },
  {
    id: 'redo',
    label: 'Redo',
    shortcut: 'Cmd + Shift + Z',
    category: 'editing',
  },
  {
    id: 'delete',
    label: 'Delete selected',
    shortcut: 'Backspace / Delete',
    // Handled by ReactFlow natively
    category: 'editing',
  },
  {
    id: 'group',
    label: 'Group selected nodes',
    shortcut: 'Cmd + G',
    key: { key: 'g', mod: true },
    category: 'editing',
  },
  {
    id: 'ungroup',
    label: 'Ungroup',
    shortcut: 'Cmd + Shift + G',
    key: { key: 'g', mod: true, shift: true },
    category: 'editing',
  },
  {
    id: 'create-node-type',
    label: 'Create node type',
    contextMenu: true,
    menuSection: 'primary',
    menuIcon: '🧩',
    category: 'editing',
  },

  // ── Navigation ──
  {
    id: 'zoom-to-fit',
    label: 'Zoom to fit',
    shortcut: 'Cmd + Shift + F',
    key: { key: 'f', mod: true, shift: true },
    category: 'navigation',
  },

  // ── Workflow ──
  {
    id: 'save-template',
    label: 'Save as template',
    contextMenu: true,
    menuSection: 'secondary',
    menuIcon: '💾',
    category: 'workflow',
  },
  {
    id: 'templates',
    label: 'Browse templates',
    contextMenu: true,
    menuSection: 'secondary',
    menuIcon: '📋',
    category: 'workflow',
  },
  {
    id: 'run-workflow',
    label: 'Run workflow',
    shortcut: 'Cmd + Enter',
    key: { key: 'Enter', mod: true },
    contextMenu: true,
    menuSection: 'secondary',
    menuIcon: '▶',
    category: 'workflow',
  },

  // ── Help ──
  {
    id: 'shortcuts-panel',
    label: 'Keyboard shortcuts',
    shortcut: 'Cmd + ?',
    key: { key: '?', mod: true, altKey: '/' },
    category: 'help',
  },
  {
    id: 'context-menu',
    label: 'Context menu',
    shortcut: 'Right-click',
    category: 'help',
  },

  // ── Danger ──
  {
    id: 'clear-all',
    label: 'Clear all',
    contextMenu: true,
    menuSection: 'danger',
    category: 'editing',
  },
]

/** Get actions that have keyboard shortcuts (for the shortcuts panel). */
export function getShortcutActions(): CanvasAction[] {
  return CANVAS_ACTIONS.filter((a) => a.shortcut)
}

/** Get actions for the context menu, grouped by section. */
export function getContextMenuActions(): {
  primary: CanvasAction[]
  secondary: CanvasAction[]
  danger: CanvasAction[]
} {
  const actions = CANVAS_ACTIONS.filter((a) => a.contextMenu)
  return {
    primary: actions.filter((a) => a.menuSection === 'primary'),
    secondary: actions.filter((a) => a.menuSection === 'secondary'),
    danger: actions.filter((a) => a.menuSection === 'danger'),
  }
}

/** Get actions that have key bindings (for the keydown handler). */
export function getKeyBindingActions(): CanvasAction[] {
  return CANVAS_ACTIONS.filter((a) => a.key)
}

/**
 * Check if a keyboard event matches a canvas action's key binding.
 */
export function matchesKey(action: CanvasAction, e: KeyboardEvent): boolean {
  const k = action.key
  if (!k) return false
  const mod = e.metaKey || e.ctrlKey
  if (k.mod && !mod) return false
  if (k.shift && !e.shiftKey) return false
  // For non-shift bindings, reject if shift is pressed (avoids conflicts)
  if (!k.shift && e.shiftKey && k.key !== '?') return false
  if (e.key === k.key) return true
  if (k.altKey && e.key === k.altKey && e.shiftKey) return true
  return false
}
