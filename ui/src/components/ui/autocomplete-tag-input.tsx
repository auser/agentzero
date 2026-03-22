import { useState, useRef, useCallback, useMemo, useEffect, type KeyboardEvent } from 'react'
import { X } from 'lucide-react'
import { cn } from '@/lib/utils'

export interface Suggestion {
  name: string
  description?: string
  category?: string
}

interface AutocompleteTagInputProps {
  value: string[]
  onChange: (tags: string[]) => void
  suggestions: Suggestion[]
  placeholder?: string
  isLoading?: boolean
  className?: string
}

export function AutocompleteTagInput({
  value,
  onChange,
  suggestions,
  placeholder = 'Search...',
  isLoading,
  className,
}: AutocompleteTagInputProps) {
  const [input, setInput] = useState('')
  const [open, setOpen] = useState(false)
  const [highlighted, setHighlighted] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)

  const filtered = useMemo(() => {
    const q = input.toLowerCase()
    return suggestions
      .filter((s) => !value.includes(s.name))
      .filter(
        (s) =>
          !q ||
          s.name.toLowerCase().includes(q) ||
          s.description?.toLowerCase().includes(q) ||
          s.category?.toLowerCase().includes(q),
      )
      .slice(0, 10)
  }, [input, suggestions, value])

  useEffect(() => {
    setHighlighted(0)
  }, [filtered.length])

  // scroll highlighted item into view
  useEffect(() => {
    const el = listRef.current?.children[highlighted] as HTMLElement | undefined
    el?.scrollIntoView({ block: 'nearest' })
  }, [highlighted])

  const addTag = useCallback(
    (tag: string) => {
      if (tag && !value.includes(tag)) {
        onChange([...value, tag])
      }
      setInput('')
      setOpen(false)
    },
    [value, onChange],
  )

  const removeTag = useCallback(
    (idx: number) => {
      onChange(value.filter((_, i) => i !== idx))
    },
    [value, onChange],
  )

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown' && open) {
      e.preventDefault()
      setHighlighted((h) => Math.min(h + 1, filtered.length - 1))
    } else if (e.key === 'ArrowUp' && open) {
      e.preventDefault()
      setHighlighted((h) => Math.max(h - 1, 0))
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (open && filtered[highlighted]) {
        addTag(filtered[highlighted].name)
      } else if (input.trim()) {
        addTag(input.trim())
      }
    } else if (e.key === 'Escape') {
      setOpen(false)
    } else if (e.key === 'Backspace' && !input && value.length > 0) {
      removeTag(value.length - 1)
    }
  }

  return (
    <div className="relative">
      <div
        className={cn(
          'flex flex-wrap items-center gap-1 min-h-9 px-2 py-1 rounded-md border border-input bg-background focus-within:ring-1 focus-within:ring-ring cursor-text',
          className,
        )}
        onClick={() => inputRef.current?.focus()}
      >
        {value.map((tag, i) => (
          <span
            key={tag}
            className="inline-flex items-center gap-0.5 rounded-full bg-secondary text-secondary-foreground px-2 py-0.5 text-xs font-medium"
          >
            {tag}
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation()
                removeTag(i)
              }}
              className="ml-0.5 rounded-full hover:bg-secondary-foreground/20 p-0.5"
            >
              <X className="h-3 w-3" />
            </button>
          </span>
        ))}
        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={(e) => {
            setInput(e.target.value)
            setOpen(true)
          }}
          onKeyDown={handleKeyDown}
          onFocus={() => setOpen(true)}
          onBlur={() => {
            // delay to allow click on suggestion
            setTimeout(() => setOpen(false), 150)
          }}
          placeholder={value.length === 0 ? placeholder : ''}
          className="flex-1 min-w-[100px] bg-transparent text-sm outline-none placeholder:text-muted-foreground"
        />
      </div>

      {open && filtered.length > 0 && (
        <div
          ref={listRef}
          className="absolute top-full left-0 right-0 mt-1 z-50 max-h-48 overflow-y-auto rounded-md border border-border bg-popover shadow-md"
        >
          {filtered.map((s, i) => (
            <button
              key={s.name}
              type="button"
              className={cn(
                'w-full text-left px-3 py-1.5 text-sm flex items-center justify-between gap-2 hover:bg-accent',
                i === highlighted && 'bg-accent',
              )}
              onMouseDown={(e) => {
                e.preventDefault()
                addTag(s.name)
              }}
              onMouseEnter={() => setHighlighted(i)}
            >
              <span className="font-mono truncate">{s.name}</span>
              {s.category && (
                <span className="text-xs text-muted-foreground shrink-0">{s.category}</span>
              )}
            </button>
          ))}
        </div>
      )}

      {open && isLoading && filtered.length === 0 && (
        <div className="absolute top-full left-0 right-0 mt-1 z-50 rounded-md border border-border bg-popover shadow-md px-3 py-2 text-sm text-muted-foreground">
          Loading tools...
        </div>
      )}
    </div>
  )
}
