// `vitest/config` re-exports Vite's defineConfig with the `test` block typed.
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { fileURLToPath, URL } from 'node:url';

// Tauri drives the dev server on a fixed port and watches src-tauri itself.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte()],

  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url))
    }
  },

  // Tauri expects a fixed port and fails if it is not available.
  clearScreen: false,
  server: {
    port: 5183,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 5184 } : undefined,
    watch: {
      // The Rust side has its own watcher; watching it here only burns CPU.
      ignored: ['**/src-tauri/**']
    }
  },

  // The WebView2 runtime we target is evergreen Chromium, so we can emit modern
  // output and skip legacy transforms entirely.
  build: {
    target: 'chrome120',
    // Vite 8 minifies with oxc; `true` selects it. Naming esbuild explicitly
    // would pull in a separate optional dependency for no benefit.
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
    chunkSizeWarningLimit: 900
  },

  test: {
    environment: 'jsdom',
    globals: true,
    include: ['tests/**/*.test.ts', 'src/**/*.test.ts'],
    setupFiles: ['./tests/setup.ts']
  }
});
