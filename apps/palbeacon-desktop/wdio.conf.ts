import { existsSync } from "node:fs";
import { delimiter, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { SevereServiceError } from "webdriverio";
import {
  attachWebViewCapabilities,
  prepareWebViewCapabilities,
  startWebViewSession,
} from "./e2e/webview-session.mjs";

const appRoot = dirname(fileURLToPath(import.meta.url));
const defaultBinary = resolve(
  appRoot,
  "src-tauri/target/x86_64-pc-windows-msvc/release/palbeacon-desktop.exe",
);
const appBinaryPath = process.env.PALBEACON_TAURI_BINARY
  ? resolve(appRoot, process.env.PALBEACON_TAURI_BINARY)
  : defaultBinary;
const defaultTauriDriver = resolve(appRoot, "../../.tools/tauri-driver/bin/tauri-driver.exe");
const tauriDriverPath = process.env.TAURI_DRIVER_PATH
  ? resolve(appRoot, process.env.TAURI_DRIVER_PATH)
  : defaultTauriDriver;
let ownedSession: Awaited<ReturnType<typeof startWebViewSession>> | undefined;

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: [resolve(appRoot, "e2e/**/*.e2e.ts")],
  maxInstances: 1,
  services: [
    [
      "@wdio/tauri-service",
      {
        appBinaryPath,
        driverProvider: "external",
        tauriDriverPath,
        autoInstallTauriDriver: false,
        autoDownloadEdgeDriver: true,
        startTimeout: 60_000,
        logLevel: "info",
      },
    ],
  ],
  capabilities: [{ browserName: "tauri" }],
  logLevel: "info",
  outputDir: resolve(appRoot, "test-results/wdio"),
  bail: 1,
  waitforTimeout: 15_000,
  connectionRetryTimeout: 90_000,
  connectionRetryCount: 2,
  framework: "jasmine",
  reporters: ["spec"],
  onPrepare(_config, capabilities) {
    if (!existsSync(appBinaryPath)) {
      throw new SevereServiceError(
        `PalBeacon Tauri binary not found at ${appBinaryPath}. Run pnpm desktop:build first.`,
      );
    }
    if (!existsSync(tauriDriverPath)) {
      throw new SevereServiceError(
        `tauri-driver not found at ${tauriDriverPath}. Install the locked 2.0.6 tool before running E2E.`,
      );
    }
    process.env.PATH = `${dirname(tauriDriverPath)}${delimiter}${process.env.PATH ?? ""}`;
    // Preparation failure stops the launcher before any worker can create a session.
    return startWebViewSession(appBinaryPath)
      .then(async (session) => {
        try {
          // Capabilities are serialized to workers; late launcher environment changes
          // are not a reliable cross-process transport on the Windows CI runner.
          prepareWebViewCapabilities(capabilities, session.address);
          ownedSession = session;
        } catch (error) {
          await session.stop();
          throw error;
        }
      })
      .catch((error) => {
        // WDIO logs ordinary onPrepare exceptions and continues. Its official fatal
        // error type stops before a worker can hide the original startup failure.
        throw new SevereServiceError(`Native app preparation failed: ${error.message}`);
      });
  },
  beforeSession(_config, capabilities) {
    attachWebViewCapabilities(capabilities);
  },
  async onComplete() {
    await ownedSession?.stop();
    ownedSession = undefined;
  },
  jasmineOpts: {
    defaultTimeoutInterval: 90_000,
    random: false,
    oneFailurePerSpec: true,
  },
};
