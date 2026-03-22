/**
 * Hook to manage command palette open/close state with Cmd+K toggle.
 */
import { useState } from 'react'
import { useKeyboardShortcut } from '@/lib/hooks/useKeyboardShortcut'

export function useCommandPalette() {
  const [open, setOpen] = useState(false)

  useKeyboardShortcut({ key: 'k', meta: true }, () => setOpen((o) => !o))

  return { open, setOpen, onClose: () => setOpen(false) }
}
