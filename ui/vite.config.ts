import path from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// Tauri expects a fixed dev-server port it can point the webview at, and it
// surfaces build/debug info through TAURI_ENV_* variables.
const host = process.env.TAURI_DEV_HOST

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  // Prevent Vite from obscuring Rust errors.
  clearScreen: false,
  server: {
    port: 5473,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 5474 } : undefined,
    watch: {
      // Don't watch the Rust backend.
      ignored: ['**/src-tauri/**'],
    },
  },
  // Produce output the webview engines Tauri targets can parse.
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome105' : 'safari13',
    minify: process.env.TAURI_ENV_DEBUG ? false : 'esbuild',
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
})
