import assert from "node:assert/strict";
import { existsSync, readFileSync, realpathSync } from "node:fs";
import { createRequire } from "node:module";
import { resolve } from "node:path";
import test from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { isPrivatePath } from "../../verify-repository.mjs";

const root = fileURLToPath(new URL("../../", import.meta.url));
const uiRequire = createRequire(resolve(root, "apps/palbeacon-ui/package.json"));
const desktopRequire = createRequire(resolve(root, "apps/palbeacon-desktop/package.json"));
const driverRequire = createRequire(desktopRequire.resolve("webdriverio"));
// Resolve package metadata without asking an ESM-only package for a CJS entry.
const utilityManifest = driverRequire.resolve
  .paths("@wdio/utils")
  .map((directory) => resolve(directory, "@wdio/utils/package.json"))
  .find(existsSync);
assert.ok(utilityManifest, "WDIO utility package must be installed");
const utilityRequire = createRequire(realpathSync(utilityManifest));
const kitRequire = createRequire(uiRequire.resolve("@sveltejs/kit/package.json"));

test("both Rust lockfiles reject the vulnerable TLS handshake release range", () => {
  for (const path of ["Cargo.lock", "apps/palbeacon-desktop/src-tauri/Cargo.lock"]) {
    const lock = readFileSync(resolve(root, path), "utf8");
    const versions = [...lock.matchAll(/name = "rustls"\r?\nversion = "(\d+)\.(\d+)\.(\d+)"/g)];
    assert.ok(versions.length > 0, path);
    for (const [, major, minor, patch] of versions) {
      assert.ok(
        Number(major) > 0 || Number(minor) > 23 || (Number(minor) === 23 && Number(patch) >= 45),
        `${path} must not restore RUSTSEC-2026-0285`,
      );
    }
  }
});

test("SvelteKit uses the patched cookie API and rejects delimiter injection", () => {
  const cookie = kitRequire("cookie");
  assert.equal(
    cookie.serialize("session", "ok", { path: "/", httpOnly: true }),
    "session=ok; Path=/; HttpOnly",
  );
  assert.equal(cookie.parse("session=ok").session, "ok");
  assert.throws(() => cookie.serialize("session;injected", "ok"), TypeError);
  assert.throws(() => cookie.serialize("session", "ok", { path: "/;injected" }), TypeError);
  assert.throws(
    () => cookie.serialize("session", "ok", { domain: "example.com;injected" }),
    TypeError,
  );
});

test("WDIO retains the official Puppeteer browser management API without extract-zip", async () => {
  const browsers = await import(pathToFileURL(utilityRequire.resolve("@puppeteer/browsers")).href);
  for (const name of [
    "install",
    "getInstalledBrowsers",
    "computeExecutablePath",
    "detectBrowserPlatform",
    "resolveBuildId",
  ]) {
    assert.equal(typeof browsers[name], "function", name);
  }
  assert.match(
    browsers.computeExecutablePath({
      cacheDir: resolve(root, ".tmp/security-browser-cache"),
      browser: browsers.Browser.CHROME,
      platform: browsers.BrowserPlatform.WIN64,
      buildId: "152.0.0.0",
    }),
    /chrome\.exe$/,
  );
});

test("active lockfile cannot restore retired vulnerable tool chains", () => {
  const lock = readFileSync(resolve(root, "pnpm-lock.yaml"), "utf8");
  for (const name of ["extract-zip", "mocha", "serialize-javascript", "js-yaml"]) {
    assert.doesNotMatch(lock, new RegExp(`^  ${name}@`, "m"), name);
  }
  const workspace = readFileSync(resolve(root, "pnpm-workspace.yaml"), "utf8");
  assert.match(workspace, /^minimumReleaseAge: 1440$/m);
  assert.match(workspace, /^minimumReleaseAgeStrict: true$/m);
  assert.match(workspace, /^strictPeerDependencies: true$/m);
});

test("private design references stay excluded, not an installable dependency graph", () => {
  const prototype = resolve(
    root,
    "docs/archive/legacy-2026-08-24/payload/docs/design/palbeacon-atlas-matrix-2026-08-24/prototype",
  );
  for (const name of [
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "pnpm-workspace.yaml",
    ".npmrc",
  ]) {
    assert.equal(existsSync(resolve(prototype, name)), false, name);
  }
  assert.equal(isPrivatePath("docs/archive/prototype/implementation.png"), true);
});

test("CI audits all dependency severities without an advisory ignore list", () => {
  const pkg = JSON.parse(readFileSync(resolve(root, "package.json"), "utf8"));
  assert.equal(pkg.scripts["quality:security"], "pnpm audit --audit-level low");
  const workflow = readFileSync(
    resolve(root, ".github/workflows/source-snapshot.yml"),
    "utf8",
  );
  assert.match(workflow, /run: pnpm quality:security/);
  const deployment = JSON.parse(readFileSync(resolve(root, "cloudflare/package.json"), "utf8"));
  assert.match(deployment.scripts["web:verify"], /^pnpm --dir \.\. quality:security &&/);
});
