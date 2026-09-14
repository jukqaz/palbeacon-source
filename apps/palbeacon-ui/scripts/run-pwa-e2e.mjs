import { fileURLToPath } from 'node:url';
import { runChildWithCleanup } from './run-child-with-cleanup.mjs';
import { preview } from 'vite';

const appRoot = fileURLToPath(new URL('../', import.meta.url));
const playwrightCli = fileURLToPath(
  new URL('../node_modules/@playwright/test/cli.js', import.meta.url),
);
const baseUrl = 'http://127.0.0.1:5174';
process.chdir(appRoot);

const server = await preview({
  preview: {
    host: '127.0.0.1',
    port: 5174,
    strictPort: true,
  },
});

process.exitCode = await runChildWithCleanup({
  args: [
    playwrightCli,
    'test',
    'e2e/pwa.spec.ts',
    '--workers=1',
    '--project=desktop-chrome',
    '--output=test-results/pwa',
  ],
  cwd: appRoot,
  env: {
    ...process.env,
    PALBEACON_E2E_BASE_URL: baseUrl,
    PALBEACON_PWA_E2E: 'true',
  },
  stop: () =>
    new Promise((resolve, reject) => {
      server.httpServer.close((error) => {
        if (error) reject(error);
        else resolve();
      });
    }),
});
