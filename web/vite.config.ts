import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import license from 'rollup-plugin-license';

// `npm run dev` serves the UI with hot reload and proxies the app's endpoints
// (seq-app must be running, UI port 8090 by default).
const app = process.env.SEQ_APP ?? 'http://127.0.0.1:8090';

export default defineConfig({
  plugins: [
    svelte(),
    // Third-party notices for everything bundled into web/dist (code and fonts), M11 Phase D.
    // scripts/third-party-licenses.mjs merges this file into THIRD_PARTY_LICENSES.md.
    {
      ...license({
        thirdParty: {
          includePrivate: false,
          output: {
            file: 'dist/third-party-licenses.json',
            template: (deps) =>
              JSON.stringify(
                deps.map((d) => ({
                  name: d.name,
                  version: d.version,
                  license: d.license,
                  repository: typeof d.repository === 'string' ? d.repository : d.repository?.url,
                  licenseText: d.licenseText,
                  noticeText: d.noticeText,
                })),
                null,
                1,
              ),
          },
        },
      }),
      apply: 'build',
    },
  ],
  server: {
    proxy: {
      '/ws': { target: app.replace('http', 'ws'), ws: true },
      '/thumbs': app,
      '/api': app,
    },
  },
  build: { outDir: 'dist', emptyOutDir: true, chunkSizeWarningLimit: 1000 },
});
