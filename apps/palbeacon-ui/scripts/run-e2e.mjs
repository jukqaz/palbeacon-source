import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { runChildWithCleanup } from './run-child-with-cleanup.mjs';
import { preview } from 'vite';

const appRoot = fileURLToPath(new URL('../', import.meta.url));
const playwrightCli = fileURLToPath(
  new URL('../node_modules/@playwright/test/cli.js', import.meta.url),
);
const e2ePort = Number.parseInt(process.env.PALBEACON_E2E_PORT ?? '5173', 10);
if (!Number.isSafeInteger(e2ePort) || e2ePort < 1 || e2ePort > 65_535) {
  throw new Error('PALBEACON_E2E_PORT must be a valid TCP port');
}
const baseUrl = `http://127.0.0.1:${e2ePort}`;
if (!existsSync(new URL('../build/index.html', import.meta.url))) {
  throw new Error('Build the verified Web release before running browser tests.');
}
process.chdir(appRoot);
const server = await preview({
  preview: {
    host: '127.0.0.1',
    port: e2ePort,
    strictPort: true,
  },
});

// Exercise the deployable artifact: dev-server module compilation is not a
// product interaction and must not consume the unchanged assertion budgets.
process.exitCode = await runChildWithCleanup({
  args: [playwrightCli, 'test', ...process.argv.slice(2)],
  cwd: appRoot,
  env: { ...process.env, PALBEACON_E2E_BASE_URL: baseUrl },
  stop: () =>
    new Promise((resolve, reject) => {
      server.httpServer.close((error) => {
        if (error) reject(error);
        else resolve();
      });
    }),
});
