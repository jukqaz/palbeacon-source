import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { createServer } from "node:http";
import { fileURLToPath } from "node:url";
import test from "node:test";
import {
  attachWebViewCapabilities,
  prepareWebViewCapabilities,
  waitForWebView,
} from "../../apps/palbeacon-desktop/e2e/webview-session.mjs";

const configUrl = new URL("../../apps/palbeacon-desktop/wdio.conf.ts", import.meta.url);
const missingBinary = fileURLToPath(
  new URL("./absent-tauri-binary-for-config-test.exe", import.meta.url),
);

function verifyConfig(appBinary, expectedError) {
  assert.equal(existsSync(missingBinary), false);
  const result = spawnSync(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `
    import assert from 'node:assert/strict';
    const previousPath = process.env.PATH;
    const { config } = await import(${JSON.stringify(configUrl.href)});
    assert.equal(process.env.PATH, previousPath, 'import must not mutate the environment');
    assert.equal(config.framework, 'jasmine');
    assert.equal(config.jasmineOpts.defaultTimeoutInterval, 90000);
    assert.equal(config.jasmineOpts.random, false);
    assert.equal(config.jasmineOpts.oneFailurePerSpec, true);
    assert.deepEqual(config.reporters, ['spec']);
    assert.match(config.outputDir, /test-results[\\\\/]wdio$/);
    assert.equal(config.logLevel, 'info');
    assert.throws(() => config.onPrepare(), (error) => {
      assert.equal(error.constructor.name, 'SevereServiceError');
      assert.match(error.message, ${expectedError});
      return true;
    });
  `,
    ],
    {
      encoding: "utf8",
      env: {
        ...process.env,
        PALBEACON_TAURI_BINARY: appBinary,
        TAURI_DRIVER_PATH: missingBinary,
      },
    },
  );
  assert.equal(result.status, 0, result.stderr || result.stdout);
}

test("static analysis can import Windows configuration before building the app", () => {
  verifyConfig(missingBinary, "/PalBeacon Tauri binary not found/");
});

test("native E2E still rejects a missing driver at preparation time", () => {
  verifyConfig(process.execPath, "/tauri-driver not found/");
});

test("native attach keeps the managed driver and uses the existing app through classic WebDriver", () => {
  const capabilities = {
    browserName: "tauri",
    "tauri:options": { application: "unused.exe" },
    hostname: "127.0.0.1",
    port: 4444,
  };
  prepareWebViewCapabilities([capabilities], "127.0.0.1:53210");
  // Reproduce launcher-to-worker transport without inheriting any environment.
  const workerCapabilities = JSON.parse(JSON.stringify(capabilities));
  attachWebViewCapabilities(workerCapabilities);
  assert.deepEqual(workerCapabilities, {
    browserName: "webview2",
    hostname: "127.0.0.1",
    port: 4444,
    "wdio:enforceWebDriverClassic": true,
    "ms:edgeChromium": true,
    "ms:edgeOptions": { debuggerAddress: "127.0.0.1:53210" },
  });
});

test("native attach rejects non-loopback and invalid debugging endpoints", () => {
  for (const address of ["0.0.0.0:9222", "example.com:9222", "127.0.0.1:0", "127.0.0.1:65536"]) {
    const capabilities = {
      "tauri:options": { application: "must-not-launch.exe" },
      "ms:edgeOptions": { debuggerAddress: address },
    };
    assert.throws(() => attachWebViewCapabilities(capabilities), /loopback/);
    assert.equal(capabilities["tauri:options"], undefined);
    assert.deepEqual(capabilities["ms:edgeOptions"], { debuggerAddress: "" });
  }
});

test("native preparation rejects a configuration that could share the owned app", () => {
  for (const capabilities of [[], [{}, {}], {}]) {
    assert.throws(() => prepareWebViewCapabilities(capabilities, "127.0.0.1:53210"), /exactly one/);
  }
});

const runningChild = { exitCode: null, signalCode: null };

async function withTargets(targets, operation) {
  const server = createServer((_request, response) => {
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify(targets));
  });
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  try {
    await operation(`127.0.0.1:${server.address().port}`);
  } finally {
    await new Promise((done) => server.close(done));
  }
}

test("native readiness requires the real packaged app origin, not an empty browser", async () => {
  await withTargets(
    [{ type: "page", url: "http://tauri.localhost/", webSocketDebuggerUrl: "ws://127.0.0.1/test" }],
    (address) => waitForWebView(runningChild, address),
  );
  for (const targets of [
    [],
    [{ type: "page", url: "about:blank", webSocketDebuggerUrl: "ws://127.0.0.1/test" }],
    [
      {
        type: "page",
        url: "https://tauri.localhost.example/",
        webSocketDebuggerUrl: "ws://127.0.0.1/test",
      },
    ],
    [{ type: "page", url: "http://tauri.localhost/" }],
  ]) {
    await withTargets(targets, (address) =>
      assert.rejects(waitForWebView(runningChild, address, 40), /startup deadline/),
    );
  }
});

test("native startup fails immediately when the owned process exits", async () => {
  await assert.rejects(
    waitForWebView({ exitCode: 1, signalCode: null }, "127.0.0.1:9222"),
    /exited before/,
  );
});
