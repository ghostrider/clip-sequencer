import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// `npm run dev` serves the UI with hot reload and proxies the app's endpoints
// (seq-app must be running, UI port 8090 by default).
const app = process.env.SEQ_APP ?? 'http://127.0.0.1:8090';

export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: {
      '/ws': { target: app.replace('http', 'ws'), ws: true },
      '/thumbs': app,
    },
  },
  build: { outDir: 'dist', emptyOutDir: true, chunkSizeWarningLimit: 1000 },
});
