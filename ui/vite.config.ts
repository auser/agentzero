import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { TanStackRouterVite } from '@tanstack/router-plugin/vite'
import path from 'path'

export default defineConfig({
  plugins: [
    TanStackRouterVite({ routesDirectory: './src/routes', generatedRouteTree: './src/routeTree.gen.ts' }),
    react(),
    tailwindcss(),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    outDir: 'dist',
    emptyOutDir: true,
  },
  server: {
    port: 5173,
    proxy: {
      '/v1': { target: 'http://localhost:42617', changeOrigin: true },
      '/ws': { target: 'ws://localhost:42617', ws: true, changeOrigin: true },
      '/health': { target: 'http://localhost:42617', changeOrigin: true },
      '/metrics': { target: 'http://localhost:42617', changeOrigin: true },
      '/pair': { target: 'http://localhost:42617', changeOrigin: true },
      '/api': { target: 'http://localhost:42617', changeOrigin: true },
    },
  },
})
