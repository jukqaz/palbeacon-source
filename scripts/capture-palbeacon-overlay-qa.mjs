import { mkdir } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "../apps/palbeacon-ui/node_modules/@playwright/test/index.mjs";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const outputRoot = resolve(repositoryRoot, "docs/design/palbeacon-overlay-workshop-2026-08-25");
const baseUrl = process.env["PALBEACON_QA_BASE_URL"] ?? "http://127.0.0.1:5173";

const runtimeStatus = {
  core_connected: true,
  game_running: true,
  overlay_running: true,
  position_live: true,
  map_ready: true,
  live_position_ready: true,
  map_alignment_verified: true,
  alignment_verified: true,
  source_mode: "approved_map_pack",
  freshness: "fresh",
  supported_client_build_id: "24575825",
  map_build_id: "24575825",
  message_ko: "현재 위치가 지도에 표시됩니다.",
};

const control = {
  schema: "palbeacon.overlay_control.v2",
  version: 9,
  updated_at_unix_ms: 1_787_000_000_000,
  settings: {
    enabled: true,
    rotation_mode: "north_up",
    input_mode: "locked",
    display_mode: "mini_map",
    opacity: 0.92,
    diameter_px: 320,
    zoom: 1,
    fps_profile: "sixty",
    poi_filters: {
      fast_travel: true,
      boss: true,
      wanted: false,
      dungeon: true,
      enabled_layer_ids: ["map-unlock", "poi", "tower"],
      selected_pal_ids: [],
      night_only: false,
    },
    hotkey_bindings: {
      overlay_visibility: {
        modifiers: { control: false, alt: false, shift: false, windows: false },
        virtual_key: 0x77,
      },
      rotation_toggle: {
        modifiers: { control: true, alt: false, shift: false, windows: false },
        virtual_key: 0x78,
      },
      temporary_interaction: null,
      interaction_lock: null,
    },
  },
};

const scenarios = [
  { id: "windows-1440x900", viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 },
  { id: "windows-1024x720", viewport: { width: 1024, height: 720 }, deviceScaleFactor: 1 },
  {
    id: "windows-1024x720-text-200",
    viewport: { width: 1024, height: 720 },
    deviceScaleFactor: 1,
    rootFontPercent: 200,
  },
  { id: "windows-768x1024", viewport: { width: 768, height: 1024 }, deviceScaleFactor: 1 },
  { id: "windows-390x844", viewport: { width: 390, height: 844 }, deviceScaleFactor: 1 },
  {
    id: "windows-1440x900-150-percent",
    viewport: { width: 960, height: 600 },
    deviceScaleFactor: 1.5,
  },
];

await mkdir(outputRoot, { recursive: true });
const browser = await chromium.launch({ channel: "chrome", headless: true });
try {
  for (const scenario of scenarios) {
    const context = await browser.newContext({
      viewport: scenario.viewport,
      deviceScaleFactor: scenario.deviceScaleFactor,
      reducedMotion: "no-preference",
    });
    const page = await context.newPage();
    const pageErrors = [];
    page.on("pageerror", (error) => pageErrors.push(error.message));
    await page.addInitScript(
      ({ runtimeStatus, control }) => {
        window.__PALBEACON_QA_SAVES__ = [];
        window.__TAURI_INTERNALS__ = {
          invoke: async (command, args = {}) => {
            if (command === "runtime_context") {
              return { platform: "windows", shell: "tauri", appVersion: "0.1.0" };
            }
            if (command === "native_read" && args.operation === "runtime_status") {
              return structuredClone(runtimeStatus);
            }
            if (command === "native_read" && args.operation === "overlay_control") {
              return structuredClone(control);
            }
            if (command === "native_update_overlay_control") {
              window.__PALBEACON_QA_SAVES__.push(structuredClone(args));
              return {
                ...structuredClone(control),
                version: Number(args.expectedVersion) + 1,
                settings: structuredClone(args.settings),
              };
            }
            throw new Error(`Unexpected native QA command: ${command}`);
          },
        };
      },
      { runtimeStatus, control },
    );

    await page.goto(`${baseUrl}/connection/overlay/`, { waitUntil: "networkidle" });
    if (scenario.rootFontPercent) {
      await page.evaluate((percent) => {
        document.documentElement.style.fontSize = `${percent}%`;
      }, scenario.rootFontPercent);
    }
    await page.getByRole("heading", { name: "게임 오버레이" }).waitFor();
    const overflow = await page.evaluate(() => ({
      document: document.documentElement.scrollWidth - document.documentElement.clientWidth,
      body: document.body.scrollWidth - document.body.clientWidth,
    }));
    if (overflow.document > 1 || overflow.body > 1) {
      throw new Error(`${scenario.id}: horizontal overflow ${JSON.stringify(overflow)}`);
    }
    const visibleText = await page.locator("main").innerText();
    for (const diagnostic of ["Core", "빌드", "검증", "좌표"]) {
      if (visibleText.includes(diagnostic)) {
        throw new Error(`${scenario.id}: user-facing diagnostic text ${diagnostic}`);
      }
    }

    const northUpTransforms = await page.evaluate(() => ({
      map: getComputedStyle(document.querySelector(".map-raster")).transform,
      player: getComputedStyle(document.querySelector(".player")).transform,
    }));
    await page.screenshot({
      path: resolve(outputRoot, `${scenario.id}-north-up.png`),
      animations: "disabled",
    });

    await page.getByRole("radio", { name: "진행 방향" }).click();
    await page.waitForTimeout(250);
    const headingUpTransforms = await page.evaluate(() => ({
      map: getComputedStyle(document.querySelector(".map-raster")).transform,
      player: getComputedStyle(document.querySelector(".player")).transform,
    }));
    if (
      northUpTransforms.map === headingUpTransforms.map ||
      northUpTransforms.player === headingUpTransforms.player
    ) {
      throw new Error(`${scenario.id}: map and pointer direction modes are not visually distinct`);
    }
    const saveButton = page.getByRole("button", { name: "적용" });
    if (!(await saveButton.isEnabled())) {
      throw new Error(`${scenario.id}: direction change did not enable save`);
    }
    await saveButton.click();
    await page.waitForFunction(() => window.__PALBEACON_QA_SAVES__?.length === 1);
    const saved = await page.evaluate(() => window.__PALBEACON_QA_SAVES__[0]);
    if (saved.expectedVersion !== 9 || saved.settings.rotation_mode !== "heading_up") {
      throw new Error(`${scenario.id}: saved overlay settings are not current`);
    }
    const savedToast = page
      .locator("[data-sonner-toast]")
      .filter({ hasText: "오버레이 설정을 적용했습니다." });
    await savedToast.waitFor({ state: "visible" });
    await savedToast.locator("[data-close-button]").click();
    await savedToast.waitFor({ state: "hidden" });

    await page.evaluate(() => {
      if (document.activeElement instanceof HTMLElement) document.activeElement.blur();
      document.documentElement.scrollTop = 0;
      document.body.scrollTop = 0;
      window.scrollTo(0, 0);
    });
    await page.waitForFunction(() => window.scrollY === 0);

    await page.screenshot({
      path: resolve(outputRoot, `${scenario.id}.png`),
      animations: "disabled",
    });
    if (pageErrors.length > 0) {
      throw new Error(`${scenario.id}: page errors: ${pageErrors.join("; ")}`);
    }
    await context.close();
  }
} finally {
  await browser.close();
}

console.log(`Captured ${scenarios.length} PalBeacon overlay QA screens in ${outputRoot}`);
