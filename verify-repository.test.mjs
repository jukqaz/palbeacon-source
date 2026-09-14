import assert from 'node:assert/strict';
import test from 'node:test';
import { containsCredential, isPrivatePath } from './verify-repository.mjs';

test('game resources, private evidence, saves, certificates and bindings stay local', () => {
  for (const path of ['assets/game/map.json', 'docs/archive/log.md', 'docs/data/catalog.json',
    '.tmp/report.json', '.tools/bin.exe', '.env', 'cloudflare/.dev.vars',
    'cloudflare/wrangler.toml', 'Level.sav', 'cert.pfx', 'server.key']) {
    assert.equal(isPrivatePath(path), true, path);
  }
  for (const path of ['apps/palbeacon-ui/src/app.css', 'Cargo.lock', 'cloudflare/.env.example']) {
    assert.equal(isPrivatePath(path), false, path);
  }
});

test('credential checks reject token material but allow parser constants', () => {
  assert.equal(containsCredential(['ghp', '_', 'A'.repeat(36)].join('')), true);
  assert.equal(containsCredential('AK' + 'IA' + 'A'.repeat(16)), true);
  assert.equal(containsCredential('-----BEGIN PRIVATE KEY-----\n' + 'A'.repeat(80) + '\n-----END'), true);
  assert.equal(containsCredential('b"-----BEGIN PRIVATE KEY-----".as_slice()'), false);
  assert.equal(containsCredential('process.env.GITHUB_TOKEN'), false);
});
