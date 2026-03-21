/**
 * Quick config toggles panel for enabling/disabling tools and settings
 * from the workflow view. Changes are saved via PUT /v1/config and
 * hot-reloaded by the gateway's ConfigWatcher.
 */
import { useState } from 'react'
import { useQuery, useMutation, useQueryClient } from '@tanstack/react-query'
import { api } from '@/lib/api/client'
import { Button } from '@/components/ui/button'
import { Settings, X, Save } from 'lucide-react'

interface SecurityConfig {
  write_file?: { enabled: boolean; max_write_bytes?: number }
  enable_web_search?: boolean
  enable_web_fetch?: boolean
  enable_http_request?: boolean
  enable_browser?: boolean
  enable_git?: boolean
  enable_cron?: boolean
  enable_code_interpreter?: boolean
  [key: string]: unknown
}

interface ConfigResponse {
  security?: SecurityConfig
  provider?: { kind?: string; model?: string }
  [key: string]: unknown
}

interface ConfigPanelProps {
  open: boolean
  onClose: () => void
}

const TOOL_TOGGLES = [
  { key: 'write_file', label: 'Write File', nested: true },
  { key: 'enable_web_search', label: 'Web Search' },
  { key: 'enable_web_fetch', label: 'Web Fetch' },
  { key: 'enable_http_request', label: 'HTTP Requests' },
  { key: 'enable_browser', label: 'Browser' },
  { key: 'enable_git', label: 'Git Operations' },
  { key: 'enable_cron', label: 'Cron / Scheduling' },
  { key: 'enable_code_interpreter', label: 'Code Interpreter' },
] as const

export function ConfigPanel({ open, onClose }: ConfigPanelProps) {
  const queryClient = useQueryClient()
  const [dirty, setDirty] = useState(false)

  const { data: config, isLoading } = useQuery({
    queryKey: ['config'],
    queryFn: () => api.get<ConfigResponse>('/v1/config'),
    enabled: open,
  })

  const [localSecurity, setLocalSecurity] = useState<SecurityConfig>({})

  // Sync local state when config loads
  const security = { ...config?.security, ...localSecurity }

  const saveMutation = useMutation({
    mutationFn: async (securityUpdate: SecurityConfig) => {
      const currentConfig = await api.get<ConfigResponse>('/v1/config')
      return api.put('/v1/config', {
        ...currentConfig,
        security: {
          ...currentConfig.security,
          ...securityUpdate,
        },
      })
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['config'] })
      void queryClient.invalidateQueries({ queryKey: ['tools'] })
      setDirty(false)
      setLocalSecurity({})
    },
  })

  const toggleTool = (key: string, nested: boolean = false) => {
    setDirty(true)
    if (nested) {
      const current = (security[key] as { enabled: boolean } | undefined)?.enabled ?? false
      setLocalSecurity((prev) => ({
        ...prev,
        [key]: { ...((security[key] as Record<string, unknown>) ?? {}), enabled: !current },
      }))
    } else {
      const current = (security[key] as boolean) ?? false
      setLocalSecurity((prev) => ({ ...prev, [key]: !current }))
    }
  }

  const isEnabled = (key: string, nested: boolean = false): boolean => {
    if (nested) {
      return (security[key] as { enabled: boolean } | undefined)?.enabled ?? false
    }
    return (security[key] as boolean) ?? false
  }

  const handleSave = () => {
    saveMutation.mutate(localSecurity)
  }

  if (!open) return null

  return (
    <div className="absolute top-12 right-0 z-40 w-72 bg-card border border-border rounded-lg shadow-xl overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-2.5 border-b border-border">
        <div className="flex items-center gap-1.5">
          <Settings className="h-3.5 w-3.5 text-muted-foreground" />
          <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
            Quick Config
          </span>
        </div>
        <button onClick={onClose} className="text-muted-foreground hover:text-foreground transition-colors">
          <X className="h-3.5 w-3.5" />
        </button>
      </div>

      {isLoading ? (
        <div className="p-4 text-xs text-muted-foreground text-center">Loading config...</div>
      ) : (
        <div className="p-3 space-y-3">
          {/* Tool toggles */}
          <div className="space-y-1">
            <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground px-1">
              Tools
            </p>
            {TOOL_TOGGLES.map(({ key, label, nested }) => (
              <label
                key={key}
                className="flex items-center justify-between px-2 py-1.5 rounded hover:bg-muted/30 cursor-pointer transition-colors"
              >
                <span className="text-xs">{label}</span>
                <button
                  type="button"
                  role="switch"
                  aria-checked={isEnabled(key, nested)}
                  onClick={() => toggleTool(key, nested)}
                  className={`relative w-8 h-4.5 rounded-full transition-colors ${
                    isEnabled(key, nested) ? 'bg-primary' : 'bg-muted'
                  }`}
                >
                  <span
                    className={`absolute top-0.5 left-0.5 h-3.5 w-3.5 rounded-full bg-white transition-transform ${
                      isEnabled(key, nested) ? 'translate-x-3.5' : 'translate-x-0'
                    }`}
                  />
                </button>
              </label>
            ))}
          </div>

          {/* Provider info */}
          {config?.provider && (
            <div className="space-y-1">
              <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground px-1">
                Provider
              </p>
              <div className="px-2 py-1.5 text-xs text-muted-foreground">
                <span className="font-medium text-foreground">{config.provider.model}</span>
                {config.provider.kind && (
                  <span className="text-muted-foreground/50 ml-1">({config.provider.kind})</span>
                )}
              </div>
            </div>
          )}

          {/* Save */}
          {dirty && (
            <Button
              size="sm"
              className="w-full h-8 text-xs"
              onClick={handleSave}
              disabled={saveMutation.isPending}
            >
              <Save className="h-3 w-3 mr-1.5" />
              {saveMutation.isPending ? 'Saving...' : 'Save & Apply'}
            </Button>
          )}

          {saveMutation.isError && (
            <p className="text-[10px] text-destructive px-1">
              Failed: {(saveMutation.error as Error).message}
            </p>
          )}

          {saveMutation.isSuccess && !dirty && (
            <p className="text-[10px] text-emerald-500 px-1">Config saved and hot-reloaded</p>
          )}
        </div>
      )}
    </div>
  )
}
