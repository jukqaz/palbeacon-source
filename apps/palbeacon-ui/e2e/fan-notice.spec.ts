import { expect, test } from '@playwright/test';

test('opens the fan notice from the shared shell and remains readable with larger text', async ({
  page,
}) => {
  await page.goto('/', { waitUntil: 'networkidle' });
  await page.getByRole('link', { name: '비공식 팬 프로젝트 이용 고지' }).click();
  await expect(page).toHaveURL(/\/fan-content-notice\/?$/u);
  await expect(
    page.getByRole('heading', { name: '팬 프로젝트 이용 고지', level: 1 }),
  ).toBeVisible();
  await expect(
    page.getByText('팰비콘은 비공식·비상업 팰월드 팬 프로젝트입니다.', { exact: false }),
  ).toBeVisible();
  await expect(
    page.getByRole('link', { name: 'Pocketpair 공식 2차 창작 가이드라인' }),
  ).toHaveAttribute('href', 'https://www.pocketpair.jp/en/guidelines-derivativework-en/');
  await page.evaluate(() => {
    document.documentElement.style.fontSize = '200%';
  });
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= document.documentElement.clientWidth + 1,
    ),
  ).toBe(true);
});
