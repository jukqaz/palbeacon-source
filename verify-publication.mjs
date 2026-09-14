import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
const manifest = JSON.parse(readFileSync(new URL('./PUBLICATION.json', import.meta.url)));
for (const file of manifest.files) {
  assert.ok(!file.path.startsWith('assets/') && !file.path.startsWith('docs/'));
  const bytes = readFileSync(new URL(file.path, import.meta.url));
  assert.equal(createHash('sha256').update(bytes).digest('hex'), file.sha256, file.path);
}
console.log('Verified ' + manifest.files.length + ' source files; game data and original history excluded.');
