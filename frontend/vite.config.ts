import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import { defineConfig } from 'vitest/config';

// Port the backend must listen on while the frontend development server is in
// use. See docs/development/frontend.md.
const BACKEND_PORT = 4780;

export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  build: {
    // The Rust runtime embeds this directory into the executable.
    outDir: '../crates/eario-runtime/frontend-dist',
    emptyOutDir: true,
    // One bundle served from the local machine: splitting it would gain nothing.
    chunkSizeWarningLimit: 800,
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: `http://127.0.0.1:${BACKEND_PORT}`, changeOrigin: true },
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
  },
});
