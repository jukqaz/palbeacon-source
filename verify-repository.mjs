import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

export function isPrivatePath(path) {
  return /^(?:assets|docs\/archive|docs\/data|\.tmp|\.tools|artifacts|datasets)\//.test(path)
    || /(?:^|\/)(?:\.env(?:\..*)?|\.dev\.vars(?:\..*)?)$/.test(path) && !path.endsWith('.example')
    || /^cloudflare\/wrangler[^/]*\.toml$/.test(path)
    || /\.(?:sav|pak|ucas|utoc|pfx|p12|pem|key)$/i.test(path);
}

export function containsCredential(text) {
  return /\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{50,})\b/.test(text)
    || /\b(?:AKIA|ASIA)[A-Z0-9]{16}\b/.test(text)
    || /\b(?:xox[baprs])-[A-Za-z0-9-]{20,}\b/.test(text)
    || /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----\s+[A-Za-z0-9+/=\s]{80,}-----END/.test(text);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = fileURLToPath(new URL('./', import.meta.url));
  const files = execFileSync('git', ['ls-files', '-z'], { cwd: root }).toString().split('\0').filter(Boolean);
  for (const path of files) {
    assert.ok(!isPrivatePath(path), `Local-only file must not be tracked: ${path}`);
    assert.ok(!containsCredential(readFileSync(resolve(root, path), 'utf8')), `Credential-shaped content: ${path}`);
  }
  console.log(`Public repository boundary passed: ${files.length} tracked files.`);
}
