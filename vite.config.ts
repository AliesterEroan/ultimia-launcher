import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { fileURLToPath, URL } from 'node:url'

// base './' is required: the Tauri custom-protocol shell loads assets by relative path.
export default defineConfig({
  base: './',
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  build: {
    sourcemap: false,
    minify: true,
  },
  server: {
    port: 8443,
    strictPort: true,
  },
  preview: {
    port: 8443,
    strictPort: true,
  },
})