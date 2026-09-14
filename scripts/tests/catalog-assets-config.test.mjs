import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const root = new URL('../../', import.meta.url);
const worker = readFileSync(new URL('crates/pal-catalog-worker/src/main.rs', root), 'utf8');
const assets = {
  'pals.json': 'PALS_SHA256',
  'active_skills.json': 'ACTIVE_SKILLS_SHA256',
  'passive_skills.json': 'PASSIVE_SKILLS_SHA256',
  'l10n/ko/pals.json': 'KO_PALS_SHA256',
  'l10n/ko/active_skills.json': 'KO_ACTIVE_SKILLS_SHA256',
  'l10n/ko/passive_skills.json': 'KO_PASSIVE_SKILLS_SHA256',
  'l10n/ko/ui_taxonomy.v1.json': 'KO_UI_TAXONOMY_SHA256',
  'items.v1.json': 'ITEMS_V1_SHA256',
  'world.v1.json': 'WORLD_V1_SHA256',
};

for (const [path, constant] of Object.entries(assets)) {
  test(`catalog checkout preserves the authenticated bytes of ${path}`, () => {
    const expected = worker.match(new RegExp(`const ${constant}: &str =\\s*"([a-f0-9]{64})";`));
    assert.ok(expected, `${constant} must remain pinned by the production reader`);
    const bytes = readFileSync(new URL(`assets/save-parser/${path}`, root));
    assert.equal(createHash('sha256').update(bytes).digest('hex'), expected[1]);
  });
}
