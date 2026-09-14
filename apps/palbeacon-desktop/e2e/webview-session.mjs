import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdirSync, mkdtempSync } from "node:fs";
import { rm } from "node:fs/promises";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve, sep } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const profileRoot = resolve(tmpdir(), "palbeacon-native-webview-tests");

function assertLoopbackAddress(address) {
  if (!/^127\.0\.0\.1:[1-9]\d{0,4}$/.test(address) || Number(address.split(":")[1]) > 65535) {
    throw new Error("Native WebView2 automation must use a loopback address.");
  }
}

export function prepareWebViewCapabilities(capabilities, address) {
  assertLoopbackAddress(address);
  if (!Array.isArray(capabilities) || capabilities.length !== 1) {
    throw new Error("The owned native app requires exactly one WebDriver capability.");
  }
  capabilities[0]["ms:edgeOptions"] = { debuggerAddress: address };
}

export function attachWebViewCapabilities(capabilities) {
  const address = capabilities["ms:edgeOptions"]?.debuggerAddress ?? "";
  // The official Tauri proxy forwards native capabilities when tauri:options is absent.
  // Keep its managed, version-matched EdgeDriver, but use Microsoft's attach API.
  delete capabilities["tauri:options"];
  capabilities.browserName = "webview2";
  capabilities["wdio:enforceWebDriverClassic"] = true;
  capabilities["ms:edgeChromium"] = true;
  // WDIO logs beforeSession exceptions but continues. Invalid attach options must
  // fail session creation rather than letting EdgeDriver launch another app/browser.
  capabilities["ms:edgeOptions"] = { debuggerAddress: "" };
  assertLoopbackAddress(address);
  capabilities["ms:edgeOptions"] = { debuggerAddress: address };
}

export async function waitForWebView(child, address, timeoutMs = 20_000) {
  assertLoopbackAddress(address);
  const deadline = Date.now() + timeoutMs;
  let lastObservation = "no response";
  while (Date.now() < deadline) {
    if (child.exitCode !== null || child.signalCode !== null) {
      throw new Error("PalBeacon exited before its WebView2 became ready.");
    }
    try {
      const response = await fetch(`http://${address}/json/list`, {
        signal: AbortSignal.timeout(Math.min(1000, Math.max(1, deadline - Date.now()))),
      });
      if (response.ok) {
        const targets = await response.json();
        lastObservation = JSON.stringify(
          Array.isArray(targets)
            ? targets.map(({ type, url }) => ({ type, url }))
            : { invalidTargetList: true },
        );
        if (
          Array.isArray(targets) &&
          targets.some(
            (target) =>
              target.type === "page" &&
              /^https?:\/\/tauri\.localhost\//.test(target.url) &&
              typeof target.webSocketDebuggerUrl === "string",
          )
        ) {
          return;
        }
      } else {
        lastObservation = `HTTP ${response.status}`;
      }
    } catch (error) {
      lastObservation = `${error.name}: ${error.cause?.code ?? error.message}`;
      // The owned process is still starting; readiness remains bounded by the deadline.
    }
    await delay(Math.min(100, Math.max(1, deadline - Date.now())));
  }
  throw new Error(
    `PalBeacon WebView2 did not become ready within the startup deadline; last observation: ${lastObservation}`,
  );
}

export async function startWebViewSession(binaryPath) {
  if (process.platform !== "win32") {
    throw new Error("The native PalBeacon interaction suite requires Windows.");
  }
  const reservation = createServer();
  reservation.listen(0, "127.0.0.1");
  await once(reservation, "listening");
  const { port } = reservation.address();
  await new Promise((done, reject) =>
    reservation.close((error) => (error ? reject(error) : done())),
  );

  mkdirSync(profileRoot, { recursive: true });
  const profile = mkdtempSync(join(profileRoot, "session-"));
  const address = `127.0.0.1:${port}`;
  // Test-child environment only: no registry, production config, or security-policy changes.
  const child = spawn(binaryPath, [], {
    cwd: dirname(binaryPath),
    windowsHide: true,
    stdio: "ignore",
    env: {
      ...process.env,
      WEBVIEW2_USER_DATA_FOLDER: profile,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port} --remote-debugging-address=127.0.0.1`,
    },
  });
  let stopped = false;
  const stop = async () => {
    if (stopped) return;
    stopped = true;
    if (child.pid && child.exitCode === null && child.signalCode === null) {
      const exited = once(child, "exit");
      child.kill();
      await Promise.race([exited, delay(5000)]);
    }
    // Only this invocation's newly created profile can be removed; never user app data.
    if (
      dirname(resolve(profile)) !== profileRoot ||
      !resolve(profile).startsWith(`${profileRoot}${sep}`)
    ) {
      throw new Error("Refusing to remove a profile outside the owned test directory.");
    }
    await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  };
  try {
    await once(child, "spawn");
    await waitForWebView(child, address);
    return { address, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}
