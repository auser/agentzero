/**
 * Returns a debounced version of the given async callback.
 * The timer resets on each call; only the last invocation fires.
 *
 * @example
 *   const save = useDebouncedCallback(async () => {
 *     await api.save(data)
 *   }, 800)
 */
import { useCallback, useRef } from 'react'

export function useDebouncedCallback<T extends (...args: never[]) => void | Promise<void>>(
  callback: T,
  delayMs: number,
): (...args: Parameters<T>) => void {
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)

  return useCallback(
    (...args: Parameters<T>) => {
      clearTimeout(timerRef.current)
      timerRef.current = setTimeout(() => callback(...args), delayMs)
    },
    [callback, delayMs],
  )
}
