import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const dependabot = readFileSync(new URL("../../.github/dependabot.yml", import.meta.url), "utf8");
const entries = dependabot.split(/^  - package-ecosystem:/m).slice(1);

test("final maintenance stops routine PRs without disabling security updates", () => {
  assert.equal(entries.length, 4);
  for (const entry of entries) {
    assert.match(entry, /interval: monthly/);
    assert.match(entry, /open-pull-requests-limit: 0/);
    assert.match(entry, /applies-to: security-updates\s+patterns:\s+- "\*"/);
    assert.doesNotMatch(entry, /^\s*ignore:/m);
  }
});
