import { expect, test, type Page } from '@playwright/test';

const hasHorizontalOverflow = async (page: Page) =>
  page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
  );

test('public map filters remain searchable and usable at every supported viewport', async ({
  page,
}, testInfo) => {
  await page.goto('/map/', { waitUntil: 'networkidle' });

  const responsive = ['compact-chrome', 'mobile-chrome', 'tablet-chrome'].includes(
    testInfo.project.name,
  );
  const zoomIn = page.getByRole('button', { name: '지도 확대' });
  const zoomOut = page.getByRole('button', { name: '지도 축소' });
  const controlBoxes = await Promise.all([zoomIn.boundingBox(), zoomOut.boundingBox()]);
  for (const box of controlBoxes) {
    expect(box).not.toBeNull();
    expect(box?.width).toBeGreaterThanOrEqual(44);
    expect(box?.height).toBeGreaterThanOrEqual(44);
  }
  await zoomIn.hover();
  await expect(page.getByRole('tooltip').filter({ hasText: '지도 확대' })).toBeVisible();
  await zoomIn.click();
  await zoomIn.click();
  await zoomIn.click();
  await zoomIn.click();
  await zoomIn.click();
  await zoomIn.click();
  await zoomIn.click();
  await expect(page.getByText('800%', { exact: true })).toBeVisible();
  await expect(zoomIn).toBeDisabled();
  await expect(page.locator('svg[role="region"] > g')).toHaveAttribute(
    'style',
    /scale\(8\) translate\(-/,
  );
  await page.getByRole('button', { name: '전체', exact: true }).click();
  await expect(page.getByText('100%', { exact: true })).toBeVisible();

  const filterPanel = page.locator('#map-filter-panel');

  if (responsive) {
    const trigger = page.getByRole('button', { name: /^필터 \d+$/ });
    await expect(trigger).toBeVisible();
    const triggerBox = await trigger.boundingBox();
    expect(triggerBox).not.toBeNull();
    expect(triggerBox?.height).toBeGreaterThanOrEqual(44);
    await trigger.click();
    await expect(filterPanel).toBeVisible();
    await expect(filterPanel.getByRole('button', { name: '닫기' })).toBeFocused();
  } else {
    await expect(filterPanel).toBeVisible();
  }

  await filterPanel.getByRole('button', { name: '모두 표시' }).click();
  await expect(filterPanel.getByText('36/36종')).toBeVisible();
  expect(await page.locator('[data-poi-marker]').count()).toBeGreaterThan(0);

  await filterPanel.getByRole('button', { name: '모두 숨기기' }).click();
  await filterPanel.getByRole('searchbox', { name: '위치 검색' }).fill('순수한 석영');
  await expect(filterPanel.getByRole('button', { name: /순수한 석영/ })).toBeInViewport();
  await expect(page.locator('[data-poi-marker]').first()).toBeVisible();
  await expect(page.locator('[data-poi-marker][tabindex="0"]')).toHaveCount(1);
  await expect(
    filterPanel.locator('.filter-groups button').filter({ hasText: '순수한 석영' }),
  ).toHaveAttribute('aria-pressed', 'false');
  expect(await hasHorizontalOverflow(page)).toBe(false);

  if (testInfo.project.name === 'compact-chrome') {
    await page.addStyleTag({ content: ':root { font-size: 200% !important; }' });
    await expect(filterPanel.getByRole('button', { name: '닫기' })).toBeVisible();
    expect(await hasHorizontalOverflow(page)).toBe(false);
  }

  if (responsive) {
    const close = filterPanel.getByRole('button', { name: '닫기' });
    await close.click();
    await expect(page.getByRole('button', { name: /^필터 \d+$/ })).toBeFocused();
  }
});

test('selected map location stays visible through 800 percent zoom', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'desktop-chrome', 'Desktop camera geometry check.');
  await page.goto('/map/', { waitUntil: 'networkidle' });

  const filterPanel = page.locator('#map-filter-panel');
  await filterPanel.getByRole('searchbox', { name: '위치 검색' }).fill('라이바오');
  await filterPanel.getByRole('button', { name: /라이바오/ }).click();
  const selectedMarker = page.locator('[data-poi-marker].selected');
  const map = page.locator('svg[role="region"]');
  await expect(selectedMarker).toBeVisible();

  const zoomIn = page.getByRole('button', { name: '지도 확대' });
  await zoomIn.click({ clickCount: 7 });
  await expect(page.getByText('800%', { exact: true })).toBeVisible();
  await expect
    .poll(async () => {
      const markerBox = await selectedMarker.boundingBox();
      const mapBox = await map.boundingBox();
      if (!markerBox || !mapBox) return false;
      const markerCenter = {
        x: markerBox.x + markerBox.width / 2,
        y: markerBox.y + markerBox.height / 2,
      };
      return (
        markerCenter.x >= mapBox.x &&
        markerCenter.x <= mapBox.x + mapBox.width &&
        markerCenter.y >= mapBox.y &&
        markerCenter.y <= mapBox.y + mapBox.height
      );
    })
    .toBe(true);
});
