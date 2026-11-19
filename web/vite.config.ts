import { resolve } from 'node:path';
import { defineConfig } from 'vite';
import preact from '@preact/preset-vite';

const pages = ['pay', 'receipt'];

const gateway = process.env.GATEWAY_URL ?? 'http://127.0.0.1:8080';

export default defineConfig({
  root: resolve(import.meta.dirname),
  plugins: [preact()],
  base: '/static/',
  build: {
    target: 'es2022',
    outDir: 'dist',
    emptyOutDir: true,
    manifest: 'manifest.json',
    modulePreload: { polyfill: false },
    rollupOptions: {
      input: {
        ...Object.fromEntries(pages.map(p => [p, resolve(import.meta.dirname, `${p}.html`)])),
        gateway: resolve(import.meta.dirname, 'src/gateway/index.ts'),
      },
      output: {
        entryFileNames: chunk => (chunk.name === 'gateway' ? 'gateway.js' : 'assets/[name]-[hash].js'),
      },
    },
  },
  server: {
    proxy: {
      '/pay': gateway,
      '/embed': gateway,
      '/v1': gateway,
      '/console/api': gateway,
      '/receipt': gateway,
    },
  },
});
