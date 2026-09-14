import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

test('browser and usability gates build a fresh deployable artifact first', () => {
  const packageJson = JSON.parse(
    readFileSync(new URL('../../apps/palbeacon-ui/package.json', import.meta.url), 'utf8'),
  );
  for (const name of ['test:e2e', 'test:usability']) {
    assert.match(packageJson.scripts[name], /^pnpm build && node scripts\/run-e2e\.mjs/);
  }
});

test('release interaction runner serves production without dev compiler warmup', () => {
  const source = readFileSync(
    new URL('../../apps/palbeacon-ui/scripts/run-e2e.mjs', import.meta.url),
    'utf8',
  );
  assert.match(source, /import \{ preview \} from 'vite'/);
  assert.match(source, /existsSync\(new URL\('\.\.\/build\/index\.html'/);
  assert.match(source, /args: \[playwrightCli, 'test', \.\.\.process\.argv\.slice\(2\)\]/);
  assert.doesNotMatch(source, /createServer|depsOptimizer|waitForClientDependencies/);
});

test('CI isolates heavy browser journeys without relaxing assertions', () => {
  const config = readFileSync(
    new URL('../../apps/palbeacon-ui/playwright.config.ts', import.meta.url),
    'utf8',
  );
  assert.match(config, /workers: process\.env\['CI'\] \? 1 : 4/);
  assert.match(config, /retries: 0/);
  assert.match(config, /timeout: 60_000/);
});

test('CI checks real PWA installation and retains the full browser evidence', () => {
  const workflow = readFileSync(
    new URL('../../.github/workflows/palbeacon-svelte-tauri.yml', import.meta.url),
    'utf8',
  );
  const runner = readFileSync(
    new URL('../../apps/palbeacon-ui/scripts/run-pwa-e2e.mjs', import.meta.url),
    'utf8',
  );
  assert.match(workflow, /run: pnpm --filter @palbeacon\/ui test:pwa/);
  assert.match(runner, /--output=test-results\/pwa/);
});
