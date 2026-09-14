import { mkdir, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

const baseUrl = process.env.PALBEACON_PREVIEW_URL ?? 'http://127.0.0.1:4222';
const outputDirectory = fileURLToPath(
  new URL('../../../docs/design/palbeacon-flow-integration-2026-08-31/screens/', import.meta.url),
);

const exactCatalogPath = (path, name, id) => {
  const parameters = new URLSearchParams({ q: name, id });
  return `${path}?${parameters.toString()}`;
};

const scenarios = [
  {
    id: 'buildings-1440',
    path: exactCatalogPath('/buildings/', '가축 목장', 'MonsterFarm'),
    viewport: { width: 1440, height: 900 },
    ready: { role: 'complementary', name: '선택한 건축물 상세' },
  },
  {
    id: 'buildings-390',
    path: exactCatalogPath('/buildings/', '가축 목장', 'MonsterFarm'),
    viewport: { width: 390, height: 844 },
    ready: { role: 'complementary', name: '선택한 건축물 상세' },
  },
  {
    id: 'materials-768',
    path: '/plan/materials/?id=MonsterFarm',
    viewport: { width: 768, height: 1024 },
    ready: { role: 'heading', name: '가축 목장', level: 3 },
  },
  {
    id: 'materials-320-text200',
    path: '/plan/materials/?id=MonsterFarm',
    viewport: { width: 320, height: 720 },
    ready: { role: 'heading', name: '가축 목장', level: 3 },
    textScale: 2,
  },
  {
    id: 'pal-breeding-action-1440',
    path: exactCatalogPath('/pals/', '도로롱', 'SheepBall'),
    viewport: { width: 1440, height: 900 },
    ready: { role: 'link', name: '교배 계획' },
  },
  {
    id: 'workspace-home-390',
    path: '/',
    viewport: { width: 390, height: 844 },
    ready: { role: 'complementary', name: '내 목록' },
    workspace: true,
  },
];

await mkdir(outputDirectory, { recursive: true });
const browser = await chromium.launch({ channel: 'chrome', headless: true });
const results = [];

try {
  for (const scenario of scenarios) {
    const context = await browser.newContext({
      viewport: scenario.viewport,
      reducedMotion: 'reduce',
      locale: 'ko-KR',
    });
    const page = await context.newPage();
    if (scenario.workspace) {
      await page.addInitScript(() => {
        const entry = {
          kind: 'pal',
          id: 'SheepBall',
          name_ko: '도로롱',
          href: '/pals/?q=%EB%8F%84%EB%A1%B1&id=SheepBall',
          image_path: '/generated/game/pals/T_SheepBall_icon_normal.webp',
        };
        window.localStorage.setItem(
          'palbeacon.workspace.v1',
          JSON.stringify({ recent: [entry], saved: [entry] }),
        );
      });
    }

    await page.goto(`${baseUrl}${scenario.path}`, { waitUntil: 'networkidle' });
    if (scenario.textScale) {
      await page.addStyleTag({
        content: `:root { font-size: ${scenario.textScale.toString()}00% !important; }`,
      });
    }
    await page
      .getByRole(scenario.ready.role, {
        name: scenario.ready.name,
        level: scenario.ready.level,
      })
      .waitFor({ state: 'visible' });

    const horizontalOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    );
    if (horizontalOverflow) throw new Error(`${scenario.id}: horizontal page overflow`);
    if (scenario.workspace) {
      const duplicateRecent = await page.getByRole('heading', { name: '최근 본 항목' }).count();
      if (duplicateRecent > 0) throw new Error(`${scenario.id}: saved entry repeats as recent`);
    }

    const screenshotPath = `${outputDirectory}${scenario.id}.png`;
    await page.screenshot({ path: screenshotPath, fullPage: true });
    results.push({
      id: scenario.id,
      path: scenario.path,
      viewport: scenario.viewport,
      text_scale: scenario.textScale ?? 1,
      horizontal_overflow: horizontalOverflow,
      screenshot: `screens/${scenario.id}.png`,
    });
    await context.close();
  }
} finally {
  await browser.close();
}

await writeFile(
  fileURLToPath(
    new URL(
      '../../../docs/design/palbeacon-flow-integration-2026-08-31/audit.json',
      import.meta.url,
    ),
  ),
  `${JSON.stringify({ base_url: baseUrl, scenarios: results }, null, 2)}\n`,
  'utf8',
);

console.log(`Captured ${results.length.toString()} PalBeacon flow QA scenarios.`);
