import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { fileURLToPath } from 'node:url'
import path from 'path'

const __dirname = path.dirname(fileURLToPath(import.meta.url))

export default defineConfig({
  plugins: [react()],
  resolve: {
    dedupe: ['react', 'react-dom'],
    alias: [
      { find: '@liquid-glass/shader-utils', replacement: path.resolve(__dirname, 'liquid-glass-react-master/src/shader-utils.ts') },
      { find: '@liquid-glass/utils', replacement: path.resolve(__dirname, 'liquid-glass-react-master/src/utils.ts') },
      { find: '@liquid-glass', replacement: path.resolve(__dirname, 'liquid-glass-react-master/src/index.tsx') },
    ],
  },
  build: {
    target: 'es2020',
    rollupOptions: {
      output: {
        manualChunks(id: string) {
          if (id.includes('node_modules/mermaid')) return 'mermaid';
          if (id.includes('node_modules/katex')) return 'katex';
          if (id.includes('node_modules/react') || id.includes('node_modules/react-dom')) return 'vendor-react';
        },
      },
    },
    chunkSizeWarningLimit: 500,
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/chat': 'http://127.0.0.1:3000',
      '/health': 'http://127.0.0.1:3000',
      '/tools': 'http://127.0.0.1:3000',
      '/presets': 'http://127.0.0.1:3000',
    },
  },
})
