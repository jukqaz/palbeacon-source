import { expect, test } from '@playwright/test';

test.skip(
  process.env['PALBEACON_PWA_E2E'] !== 'true',
  'Production service worker and static 404 checks run only against the built preview.',
);

test('installs the production PWA and serves a precached page offline', async ({
  context,
  page,
}) => {
  // Match the public Worker's route boundary, not Vite's permissive preview.
  // A PC/error-page request during installation would otherwise reject addAll.
  const forbiddenPrecacheRequests: string[] = [];
  await context.route(/\/(?:connection(?:\/[^?]*)?|settings|404)\/?$/, async (route) => {
    forbiddenPrecacheRequests.push(new URL(route.request().url()).pathname);
    await route.fulfill({ status: 404, contentType: 'text/plain', body: 'Not found' });
  });

  const manifestResponse = await page.request.get('/manifest.webmanifest');
  expect(manifestResponse.ok()).toBe(true);
  expect(await manifestResponse.json()).toMatchObject({
    name: 'PalBeacon',
    display: 'standalone',
    start_url: '/',
  });

  await page.goto('/');
  await expect(page.locator('link[rel="manifest"]')).toHaveAttribute(
    'href',
    '/manifest.webmanifest',
  );
  await expect(page.locator('[data-palbeacon-hydrated="true"]')).toBeAttached();

  await page.evaluate(async () => {
    await navigator.serviceWorker.ready;
  });
  await page.reload();
  await expect
    .poll(() => page.evaluate(() => navigator.serviceWorker.controller !== null))
    .toBe(true);
  await expect
    .poll(() =>
      page.evaluate(async () =>
        (await caches.keys()).some((key) => key.startsWith('palbeacon-precache-')),
      ),
    )
    .toBe(true);
  const precacheFootprint = await page.evaluate(async () => {
    const cacheName = (await caches.keys()).find((key) => key.startsWith('palbeacon-precache-'));
    if (!cacheName) return { entries: 0, bytes: 0, paths: [] as string[] };
    const cache = await caches.open(cacheName);
    const requests = await cache.keys();
    const responses = await Promise.all(requests.map((request) => cache.match(request)));
    const sizes = await Promise.all(
      responses.map(async (response) => (response ? (await response.arrayBuffer()).byteLength : 0)),
    );
    return {
      entries: requests.length,
      bytes: sizes.reduce((sum, size) => sum + size, 0),
      paths: requests.map((request) => new URL(request.url).pathname),
    };
  });
  expect(precacheFootprint.entries).toBeLessThan(500);
  expect(precacheFootprint.bytes).toBeLessThan(14 * 1024 * 1024);
  expect(forbiddenPrecacheRequests).toEqual([]);
  expect(
    precacheFootprint.paths.filter((path) =>
      /^\/(?:connection(?:\/|$)|settings(?:\/|$)|404(?:\/|$))/.test(path),
    ),
  ).toEqual([]);
  expect(precacheFootprint.paths).toEqual(
    expect.arrayContaining([
      '/generated/game/pals/T_PinkRabbit_Grass_icon_normal.webp',
      '/generated/game/elements/T_Icon_element_s_04.webp',
      '/generated/game/work-suitability/Handcraft.png',
    ]),
  );

  await context.setOffline(true);
  await page.goto('/pals');
  await expect(page.getByRole('heading', { name: '팰 도감' })).toBeVisible();
  await expect(page.getByText('연결 없이 저장된 데이터를 보고 있습니다.')).toBeVisible();
  await expect(page.getByRole('button', { name: '연결 확인' })).toBeVisible();
  const offlineCatalogImages = page.getByRole('main').locator('img');
  await expect(offlineCatalogImages.first()).toBeVisible();
  const offlineCatalogImageCount = await offlineCatalogImages.count();
  expect(offlineCatalogImageCount).toBeGreaterThan(1);
  await expect
    .poll(() =>
      offlineCatalogImages
        .first()
        .evaluate((element) => (element as HTMLImageElement).naturalWidth),
    )
    .toBeGreaterThan(0);
  const offlineCatalogImagePaths = await offlineCatalogImages.evaluateAll((images) =>
    images.map((image) => new URL((image as HTMLImageElement).src).pathname),
  );
  for (const imagePath of offlineCatalogImagePaths) {
    expect(precacheFootprint.paths).toContain(imagePath);
  }
  await context.setOffline(false);
});

test('returns a real 404 status for an unknown path', async ({ page }) => {
  const response = await page.goto('/definitely-not-a-palbeacon-route');
  expect(response?.status()).toBe(404);
});
