import { Link } from '@tanstack/react-router'
import {
  LayoutDashboard, MessageSquare, Bot, PlayCircle,
  Wrench, Radio, Cpu, Settings, Brain,
  CalendarClock, CheckSquare, Activity, AlertTriangle,
  ChevronLeft, ChevronRight,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { useSettingsStore } from '@/store/settingsStore'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { ConfirmDialog } from '@/components/shared/ConfirmDialog'
import { useState } from 'react'
import { runsApi } from '@/lib/api/runs'
import { useQueryClient } from '@tanstack/react-query'

const navItems = [
  { to: '/dashboard',  label: 'Dashboard',  icon: LayoutDashboard },
  { to: '/chat',       label: 'Chat',        icon: MessageSquare },
  { to: '/agents',     label: 'Agents',      icon: Bot },
  { to: '/runs',       label: 'Runs',        icon: PlayCircle },
  { to: '/tools',      label: 'Tools',       icon: Wrench },
  { to: '/channels',   label: 'Channels',    icon: Radio },
  { to: '/models',     label: 'Models',      icon: Cpu },
  { to: '/config',     label: 'Config',      icon: Settings },
  { to: '/memory',     label: 'Memory',      icon: Brain },
  { to: '/schedule',   label: 'Schedule',    icon: CalendarClock },
  { to: '/approvals',  label: 'Approvals',   icon: CheckSquare },
  { to: '/events',     label: 'Events',      icon: Activity },
] as const

export function Sidebar() {
  const { sidebarCollapsed, toggleSidebar } = useSettingsStore()
  const [estopOpen, setEstopOpen] = useState(false)
  const queryClient = useQueryClient()

  async function handleEstop() {
    await runsApi.estop()
    void queryClient.invalidateQueries({ queryKey: ['runs'] })
  }

  return (
    <aside
      className={cn(
        'flex flex-col h-full border-r border-border bg-[hsl(var(--sidebar-background))] transition-all duration-200',
        sidebarCollapsed ? 'w-14' : 'w-52'
      )}
    >
      {/* Logo / toggle */}
      <div className="flex items-center justify-between px-3 py-4 border-b border-border">
        {!sidebarCollapsed && (
          <span className="text-sm font-semibold text-foreground tracking-tight">AgentZero</span>
        )}
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 ml-auto text-muted-foreground hover:text-foreground"
          onClick={toggleSidebar}
        >
          {sidebarCollapsed ? <ChevronRight className="h-4 w-4" /> : <ChevronLeft className="h-4 w-4" />}
        </Button>
      </div>

      {/* Nav links */}
      <nav className="flex-1 py-2 space-y-0.5 px-2 overflow-y-auto">
        {navItems.map(({ to, label, icon: Icon }) => (
          <Tooltip key={to} delayDuration={0}>
            <TooltipTrigger asChild>
              <Link
                to={to}
                className={cn(
                  'flex items-center gap-3 rounded-md px-2 py-2 text-sm transition-colors',
                  'text-[hsl(var(--sidebar-foreground))] hover:bg-[hsl(var(--sidebar-accent))] hover:text-[hsl(var(--sidebar-accent-foreground))]',
                  '[&.active]:bg-[hsl(var(--sidebar-accent))] [&.active]:text-[hsl(var(--sidebar-accent-foreground))]'
                )}
              >
                <Icon className="h-4 w-4 shrink-0" />
                {!sidebarCollapsed && <span>{label}</span>}
              </Link>
            </TooltipTrigger>
            {sidebarCollapsed && (
              <TooltipContent side="right">{label}</TooltipContent>
            )}
          </Tooltip>
        ))}
      </nav>

      {/* Emergency stop */}
      <div className="p-2 border-t border-border">
        <Tooltip delayDuration={0}>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size={sidebarCollapsed ? 'icon' : 'sm'}
              className="w-full border-red-800/50 text-red-400 hover:bg-red-950/50 hover:text-red-300"
              onClick={() => setEstopOpen(true)}
            >
              <AlertTriangle className="h-4 w-4 shrink-0" />
              {!sidebarCollapsed && <span className="ml-2">Emergency Stop</span>}
            </Button>
          </TooltipTrigger>
          {sidebarCollapsed && (
            <TooltipContent side="right" className="bg-red-950 text-red-300 border-red-800">
              Emergency Stop
            </TooltipContent>
          )}
        </Tooltip>
      </div>

      <ConfirmDialog
        open={estopOpen}
        onOpenChange={setEstopOpen}
        title="Emergency Stop"
        description="This will cancel all currently running and pending jobs. This cannot be undone."
        confirmLabel="Stop All"
        destructive
        onConfirm={handleEstop}
      />
    </aside>
  )
}
