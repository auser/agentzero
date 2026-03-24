/**
 * Reusable hook for registering global keyboard shortcuts.
 *
 * @example
 *   useKeyboardShortcut({ key: 'k', meta: true }, () => setOpen(true))
 *   useKeyboardShortcut({ key: 'Escape' }, onClose)
 */
import { useEffect } from 'react'

interface ShortcutOptions {
  key: string
  meta?: boolean
  ctrl?: boolean
  shift?: boolean
  alt?: boolean
}

export function useKeyboardShortcut(
  shortcut: ShortcutOptions,
  callback: () => void,
  enabled = true,
) {
  useEffect(() => {
    if (!enabled) return

    const handler = (e: KeyboardEvent) => {
      if (shortcut.meta && !(e.metaKey || e.ctrlKey)) return
      if (shortcut.ctrl && !e.ctrlKey) return
      if (shortcut.shift && !e.shiftKey) return
      if (shortcut.alt && !e.altKey) return
      if (e.key !== shortcut.key) return

      e.preventDefault()
      callback()
    }

    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [shortcut.key, shortcut.meta, shortcut.ctrl, shortcut.shift, shortcut.alt, callback, enabled])
}
