import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';

const publicApiOrigin = 'https://palbeacon.jukqaz.xyz';
const apiProxy = {
  '/api': {
    target: publicApiOrigin,
    changeOrigin: true,
    secure: true,
  },
};

export default defineConfig({
  plugins: [sveltekit()],
  optimizeDeps: {
    include: [
      '@hugeicons/core-free-icons',
      '@hugeicons/svelte',
      '@panzoom/panzoom/dist/panzoom.es.js',
      '@tanstack/svelte-virtual',
      '@tauri-apps/api/core',
      '@tauri-apps/plugin-autostart',
      '@tauri-apps/plugin-dialog',
      'bits-ui',
      'svelte-sonner',
      'valibot',
    ],
  },
  resolve: {
    conditions: ['browser'],
  },
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    proxy: apiProxy,
  },
  preview: {
    host: '127.0.0.1',
    port: 4173,
    strictPort: true,
    proxy: apiProxy,
  },
  test: {
    coverage: {
      provider: 'v8',
      reporter: ['text', 'json-summary', 'lcov'],
      reportsDirectory: './coverage/unit',
      thresholds: {
        statements: 75,
        branches: 58,
        functions: 70,
        lines: 77,
      },
      include: ['src/lib/**/*.{ts,svelte}'],
      exclude: [
        'src/lib/generated/**',
        'src/lib/**/*.d.ts',
        'src/lib/**/runtime-schema.ts',
        'src/test/**',
      ],
    },
    environment: 'jsdom',
    include: ['src/**/*.test.ts'],
    setupFiles: ['./src/test/setup.ts'],
    css: true,
  },
});
