import { createRootRoute, Outlet, redirect } from '@tanstack/react-router'
import { TooltipProvider } from '@/components/ui/tooltip'
import { Sidebar } from '@/components/layout/Sidebar'
import { Topbar } from '@/components/layout/Topbar'
import { FloatingChat } from '@/components/shared/FloatingChat'
import { useGlobalEvents } from '@/hooks/useGlobalEvents'
import { useAuthStore } from '@/store/authStore'

function Shell() {
  useGlobalEvents()
  return (
    <TooltipProvider>
      <div className="flex h-dvh overflow-hidden">
        <Sidebar />
        <div className="flex flex-col flex-1 min-w-0 overflow-hidden">
          <Topbar />
          <main className="flex-1 overflow-auto p-6">
            <Outlet />
          </main>
        </div>
      </div>
      <FloatingChat />
    </TooltipProvider>
  )
}

export const Route = createRootRoute({
  beforeLoad: ({ location }) => {
    const token = useAuthStore.getState().token
    if (!token && location.pathname !== '/login') {
      throw redirect({ to: '/login' })
    }
  },
  component: Shell,
})
