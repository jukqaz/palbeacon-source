import { expect, test, type Page } from '@playwright/test';

const expectNoHorizontalOverflow = async (page: Page): Promise<void> => {
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );
  expect(overflow).toBe(false);
};

test('turns an exact item recipe and technology unlock into readable actions', async ({ page }) => {
  await page.goto('/items/?q=AI%20코어&id=AIcore');

  const inspector = page.getByRole('complementary', { name: '선택한 아이템 상세' });
  await expect(inspector).toBeVisible({ timeout: 15_000 });
  await inspector.getByRole('tab', { name: '제작 1' }).click();
  await expect(inspector.getByRole('heading', { name: '만드는 법' })).toBeVisible();
  await expect(inspector.getByText('컴퓨터')).toBeVisible();
  await expect(inspector.getByText('× 5')).toBeVisible();
  await inspector.getByRole('tab', { name: '기술 1' }).click();
  await expect(inspector.getByRole('heading', { name: '해금 기술' })).toBeVisible();
  const technologyHref = await inspector
    .getByRole('link', { name: /AI 코어.*Lv\. 67 · 4 PT/ })
    .getAttribute('href');
  const technologyUrl = new URL(technologyHref ?? '', 'http://127.0.0.1');
  expect(technologyUrl.pathname).toBe('/technology/');
  expect(technologyUrl.searchParams.get('q')).toBe('AI 코어');
  expect(technologyUrl.searchParams.get('id')).toBe('AIcore');
  await expect(inspector.locator('code')).toBeHidden();
  await expectNoHorizontalOverflow(page);
});

test('shows a canonical Pal drop without leaking internal method identifiers', async ({ page }) => {
  await page.goto('/items/?q=기술서&id=TechnologyBook_G2');

  const inspector = page.getByRole('complementary', { name: '선택한 아이템 상세' });
  await expect(inspector).toBeVisible({ timeout: 15_000 });
  await inspector.getByRole('tab', { name: /드롭 \d+/ }).click();
  await expect(inspector.getByRole('heading', { name: '얻는 법' })).toBeVisible();
  const dropLink = inspector.getByRole('link', { name: /제노그리프/ }).first();
  await expect(dropLink).toBeVisible();
  const dropHref = await dropLink.getAttribute('href');
  const dropUrl = new URL(dropHref ?? '', 'http://127.0.0.1');
  expect(dropUrl.pathname).toBe('/pals/');
  expect(dropUrl.searchParams.get('q')).toBe('제노그리프');
  expect(dropUrl.searchParams.get('id')).toBe('BlackGriffon');
  await expect(inspector.getByText('10%').first()).toBeVisible();
  await expect(inspector.getByText('1개').first()).toBeVisible();
  await expect(inspector).not.toContainText('PalDrop_');
  await expectNoHorizontalOverflow(page);
});
