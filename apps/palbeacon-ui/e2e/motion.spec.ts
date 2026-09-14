import { expect, test, type TestInfo } from '@playwright/test';

const durationInMilliseconds = (value: string): number => {
  const first = value.split(',')[0]?.trim() ?? '0s';
  return first.endsWith('ms') ? Number.parseFloat(first) : Number.parseFloat(first) * 1000;
};

const isPrimaryViewport = (testInfo: TestInfo) =>
  ['desktop-chrome', 'mobile-chrome'].includes(testInfo.project.name);

test('uses short purposeful motion and honors reduced motion', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop-chrome', 'Run the motion contract once.');

  await page.goto('/');
  const routeStage = page.getByTestId('route-stage');
  const normalRouteMotion = await routeStage.evaluate((element) => {
    const style = getComputedStyle(element);
    return { name: style.animationName, duration: style.animationDuration };
  });
  expect(normalRouteMotion.name).not.toBe('none');
  expect(durationInMilliseconds(normalRouteMotion.duration)).toBeGreaterThanOrEqual(120);
  expect(durationInMilliseconds(normalRouteMotion.duration)).toBeLessThanOrEqual(300);

  const primaryLink = page.getByRole('link', { name: '도감', exact: true });
  const transitionDuration = await primaryLink.evaluate(
    (element) => getComputedStyle(element).transitionDuration,
  );
  expect(durationInMilliseconds(transitionDuration)).toBeGreaterThan(0);

  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/plan/materials/');
  const reducedMotion = await page.getByTestId('tool-mode-stage').evaluate((element) => {
    const style = getComputedStyle(element);
    return {
      animation: style.animationDuration,
      transition: style.transitionDuration,
    };
  });
  expect(durationInMilliseconds(reducedMotion.animation)).toBeLessThanOrEqual(1);
  expect(durationInMilliseconds(reducedMotion.transition)).toBeLessThanOrEqual(1);
});

test('each public menu exposes a clear usable primary interaction', async ({ page }, testInfo) => {
  test.skip(!isPrimaryViewport(testInfo), 'Audit desktop and mobile menu contracts.');
  test.setTimeout(90_000);
  const runtimeErrors: string[] = [];
  page.on('pageerror', (error) => runtimeErrors.push(error.message));
  page.on('console', (message) => {
    if (message.type() === 'error') runtimeErrors.push(message.text());
  });

  await page.goto('/');
  const homeSearch = page.getByRole('main').getByRole('searchbox', { name: '통합 검색' });
  await homeSearch.fill('도로롱');
  await page.getByRole('main').getByRole('button', { name: '찾기' }).click();
  await expect(page).toHaveURL(/\/search\/?\?q=/u);

  const search = page.getByRole('main').getByRole('searchbox', { name: '통합 검색' });
  await search.fill('도로롱');
  const searchResult = page.getByRole('link', { name: /도로롱.*팰.*상세 보기/ }).first();
  await expect(searchResult).toBeVisible();
  await searchResult.click();
  await expect(page.getByRole('complementary', { name: '선택한 팰 상세' })).toBeVisible();

  await page.goto('/pals/');
  const catalogSearch = page.getByRole('searchbox', { name: '팰 도감 검색' });
  await catalogSearch.fill('도로롱');
  const catalogResult = page.getByRole('button', { name: /도로롱.*팰/ }).first();
  await catalogResult.click();
  if ((page.viewportSize()?.width ?? 0) < 1024) {
    await expect(page.getByRole('complementary', { name: '선택한 팰 상세' })).toBeVisible();
  } else {
    await expect(catalogResult).toHaveAttribute('aria-pressed', 'true');
  }

  await page.goto('/map/');
  if ((page.viewportSize()?.width ?? 0) <= 820) {
    await page.getByRole('button', { name: /^필터 \d+$/ }).click();
  }
  await page.getByRole('searchbox', { name: '위치 검색' }).fill('라이바오');
  const mapResult = page.locator('#map-filter-panel').getByRole('button', { name: /라이바오/ });
  await mapResult.click();
  await expect(
    page.getByRole('complementary', { name: '선택한 지도 위치 상세' }).getByRole('heading', {
      name: '라이바오',
    }),
  ).toBeVisible();

  await page.goto('/plan/');
  await page.goto('/plan/materials/');
  await expect(page.getByRole('heading', { name: '재료 계산' })).toBeVisible();

  expect(runtimeErrors).toEqual([]);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    ),
  ).toBe(false);
});
