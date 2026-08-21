import path from 'path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  // Tauri-specific: prevent Vite from obscuring Rust errors
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    hmr: false,
    watch: {
      ignored: ['**/src-tauri/**', '**/backend/**', '**/target/**', '**/assets/Fonts/**'],
    },
  },
  build: {
    target: 'es2022',
    minify: 'esbuild',
    rollupOptions: {
      output: {
        manualChunks(id) {
          // Exact package dirs — a bare 'react' prefix would also capture
          // react-remove-scroll / react-style-singleton (Radix transitive
          // deps), invalidating the vendor chunk on Radix updates.
          if (id.includes('node_modules/react/') || id.includes('node_modules/react-dom/')) {
            return 'vendor';
          }
        },
      },
    },
  },
})
