import { expect, test } from '@playwright/test';

const isResizeObserverDeliveryNotice = (message: string): boolean =>
  message === 'ResizeObserver loop completed with undelivered notifications.' ||
  message === 'ResizeObserver loop limit exceeded';

const webMenuRoutes: ReadonlyArray<{ path: string; heading: string | RegExp }> = [
  { path: '/', heading: '통합 검색' },
  { path: '/fan-content-notice/', heading: '팬 프로젝트 이용 고지' },
  { path: '/search/', heading: '통합 검색' },
  { path: '/map/', heading: '탐험 지도' },
  { path: '/pals/', heading: '팰 도감' },
  { path: '/items/', heading: '아이템 도감' },
  { path: '/skills/active/', heading: '액티브 스킬' },
  { path: '/skills/passive/', heading: '패시브 스킬' },
  { path: '/buildings/', heading: '건축물 도감' },
  { path: '/technology/', heading: '기술 도감' },
  { path: '/plan/', heading: '팀 구성' },
  { path: '/plan/breeding/', heading: '교배 계산' },
  { path: '/plan/compare/', heading: '팰 비교' },
  { path: '/plan/materials/', heading: '재료 계산' },
];

test('renders every Web menu route without page overflow or runtime errors', async ({ page }) => {
  test.setTimeout(120_000);
  const runtimeErrors: string[] = [];
  page.on('pageerror', (error) => {
    if (!isResizeObserverDeliveryNotice(error.message)) runtimeErrors.push(error.message);
  });
  page.on('console', (message) => {
    if (message.type() === 'error' && !isResizeObserverDeliveryNotice(message.text())) {
      runtimeErrors.push(message.text());
    }
  });
  const inspectRoute = async (index: number): Promise<void> => {
    if (index >= webMenuRoutes.length) return;
    const route = webMenuRoutes[index];
    if (!route) return;
    await page.goto(route.path, { waitUntil: 'domcontentloaded' });
    await expect(page.getByRole('heading', { name: route.heading, level: 1 })).toBeVisible({
      timeout: 15_000,
    });
    // A visible heading can precede late module and image requests. Let the
    // current route settle before navigating again so WebKit does not report
    // deliberately aborted imports as runtime failures.
    await page.waitForLoadState('networkidle');
    const hasPageOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    );
    expect(hasPageOverflow, `${route.path} must not overflow the viewport`).toBe(false);
    await inspectRoute(index + 1);
  };

  await inspectRoute(0);

  expect(runtimeErrors).toEqual([]);
});

test('web shell omits Windows-only navigation', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  const shell = page.getByRole('banner');
  const catalogLink = shell.getByRole('link', { name: '도감', exact: true });
  const mapLink = shell.getByRole('link', { name: '지도', exact: true });
  await expect(catalogLink).toBeVisible();
  await expect(mapLink).toBeVisible();
  await expect(shell.getByRole('link', { name: '계획', exact: true })).toBeVisible();
  await expect(shell.getByRole('link', { name: '위키', exact: true })).toHaveCount(0);
  await expect(shell.getByRole('link', { name: '내 게임', exact: true })).toHaveCount(0);
  await expect(shell.getByRole('link', { name: '설정', exact: true })).toHaveCount(0);
  await expect(page.getByRole('navigation', { name: 'PC 메뉴' })).toHaveCount(0);

  await mapLink.click();
  await expect(page).toHaveURL(/\/map\/?$/u);
  await expect(page.getByRole('heading', { name: '탐험 지도' })).toBeVisible();

  await shell.getByRole('link', { name: '도감', exact: true }).click();
  await expect(page).toHaveURL(/\/pals\/?$/u);
  await expect(page.getByRole('heading', { name: '팰 도감' })).toBeVisible();
});

test('shell keeps one common search entry at every responsive width', async ({ page }) => {
  await page.goto('/', { waitUntil: 'domcontentloaded' });
  await expect(page.getByRole('searchbox', { name: '통합 검색' })).toBeVisible();
  await expect(page.locator('label[for="home-search"]')).toHaveCSS('clip-path', 'inset(50%)');
  await expect(page.getByRole('link', { name: '검색', exact: true })).toHaveCount(0);

  await page.goto('/pals/', { waitUntil: 'domcontentloaded' });
  await expect(page.getByRole('searchbox', { name: '팰 도감 검색' })).toBeVisible();
  await expect(page.getByRole('link', { name: '검색', exact: true })).toBeVisible();
  await expect(page.locator('.global-search')).toHaveCount(0);

  await page.getByRole('link', { name: '검색', exact: true }).click();
  await expect(page).toHaveURL(/\/search\/?$/u);
  await expect(page.getByRole('searchbox', { name: '통합 검색' })).toBeVisible();
  await expect(page.getByRole('link', { name: '검색', exact: true })).toHaveCount(0);
  await expect(page.locator('.global-search')).toHaveCount(0);
});

test('building page mirrors the game category menu and opens exact material details', async ({
  page,
}) => {
  await page.goto('/buildings/');
  await expect(page.getByRole('heading', { name: '건축물 도감' })).toBeVisible();
  const categories = page.getByRole('navigation', { name: '건축물 대분류' });
  await expect(categories.getByRole('button', { name: '생산', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(page.getByRole('heading', { name: '제작·수리' })).toBeVisible();
  await expect(page.getByRole('heading', { name: '스피어' })).toBeVisible();
  await expect(page.getByText('Product_Repair')).toHaveCount(0);

  const search = page.getByRole('searchbox', { name: '건축물 검색' });
  await search.fill('가축 목장');
  await categories.getByRole('button', { name: '팰 시설', exact: true }).click();
  const card = page.getByRole('button', { name: /가축 목장, 팰 시설/ });
  await expect(card).toBeVisible();
  await card.click();
  const inspector = page.getByRole('complementary', { name: '선택한 건축물 상세' });
  await expect(inspector.getByRole('heading', { name: '가축 목장' })).toBeVisible();
  await expect(inspector.getByRole('heading', { name: '필요 재료' })).toBeVisible();
  await expect(inspector.getByText('목재')).toBeVisible();
  await expect(inspector.getByText('MonsterFarm')).toHaveCount(0);
});

test.describe('compact building layout', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('keeps categories reachable and restores the selected building slot', async ({ page }) => {
    await page.goto('/buildings/');
    const categories = page.getByRole('navigation', { name: '건축물 대분류' });
    await categories.getByRole('button', { name: '팰 시설', exact: true }).click();
    const search = page.getByRole('searchbox', { name: '건축물 검색' });
    await search.fill('가축 목장');
    const card = page.getByRole('button', { name: /가축 목장, 팰 시설/ });
    await card.focus();
    await card.click();

    await expect(search).toBeHidden();
    await expect(page.getByRole('complementary', { name: '선택한 건축물 상세' })).toBeVisible();
    await page.getByRole('button', { name: '건축물 목록' }).click();
    await expect(search).toBeVisible();
    await expect(card).toBeFocused();
  });
});

test('compact building layout remains usable with 200% text', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'compact-chrome', 'Run once in the smallest viewport.');
  await page.goto('/buildings/');
  await page.addStyleTag({ content: ':root { font-size: 200% !important; }' });

  await expect(page.getByRole('heading', { name: '건축물 도감' })).toBeVisible();
  await expect(page.getByRole('searchbox', { name: '건축물 검색' })).toBeVisible();
  const categories = page.getByRole('navigation', { name: '건축물 대분류' });
  await expect(categories.getByRole('button', { name: '생산', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: /^수리대, 생산/ })).toBeVisible();

  const hasPageOverflow = await page.evaluate(
    () => document.documentElement.scrollWidth > document.documentElement.clientWidth,
  );
  expect(hasPageOverflow, 'building catalog must not overflow the compact viewport').toBe(false);
});

test('technology page supports exact-build search and selection', async ({ page }) => {
  await page.goto('/technology/');
  await expect(page.getByRole('heading', { name: '기술 도감' })).toBeVisible();
  await expect(page.getByRole('button', { name: '일반 기술', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: '고대 기술', exact: true })).toBeVisible();
  await expect(page.locator('.lane-columns')).toContainText('일반 기술');
  await expect(page.locator('.lane-columns')).toContainText('고대 기술');
  await expect(page.locator('.level-band').first()).toHaveClass(/lane-all/u);
  await expect(page.getByText('포인트 합계')).toHaveCount(0);

  const search = page.getByRole('searchbox', { name: '기술·해금 항목 검색' });
  await search.fill('배합 목장');
  await expect(page.getByRole('button', { name: /배합 목장/ }).first()).toBeVisible();
  await page
    .getByRole('button', { name: /배합 목장/ })
    .first()
    .click();
  const inspector = page.getByRole('complementary', { name: '선택한 기술 상세' });
  await expect(inspector).toBeVisible();
  await expect(inspector.getByText('티어', { exact: true })).toHaveCount(0);
  await expect(inspector.getByText('Technology_Farm')).toHaveCount(0);
});

test.describe('compact technology layout', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('keeps search, lanes and cards reachable', async ({ page }) => {
    await page.goto('/technology/');
    await expect(page.getByRole('searchbox')).toBeVisible();
    await page.getByRole('button', { name: '고대 기술', exact: true }).click();
    await expect(page.locator('.lane-columns')).toContainText('고대 기술');
    await expect(page.locator('.lane-columns')).not.toContainText('일반 기술');
    await expect(page.locator('[data-testid="ancient-lane"]').first()).toBeVisible();
    await expect(page.locator('body')).not.toHaveCSS('overflow-x', 'scroll');
  });

  test('steps between levels without precision horizontal scrolling', async ({ page }) => {
    await page.goto('/technology/');
    const levelTen = await page.getByRole('button', { name: '레벨 10', exact: true }).boundingBox();
    const levelEleven = await page
      .getByRole('button', { name: '레벨 11', exact: true })
      .boundingBox();
    if (!levelTen || !levelEleven) throw new Error('Adjacent level buttons must be visible.');
    expect(Math.abs(levelTen.y - levelEleven.y)).toBeLessThan(3);
    const selectedVisibility = await page.locator('.level-scroll').evaluate((scroll) => {
      const active = scroll.querySelector('[aria-pressed="true"]');
      if (!active) return false;
      const scrollRect = scroll.getBoundingClientRect();
      const activeRect = active.getBoundingClientRect();
      return activeRect.left >= scrollRect.left && activeRect.right <= scrollRect.right;
    });
    expect(selectedVisibility).toBe(true);
    await expect(page.getByRole('button', { name: '다음 기술 레벨' })).toBeVisible();
    await page.getByRole('button', { name: '다음 기술 레벨' }).click();
    await expect(page.getByRole('button', { name: '레벨 11', exact: true })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
    await expect(page.locator('.level-band').first()).toHaveAttribute('id', 'technology-level-11');
  });

  test('opens detail as a sequential screen and restores the selected card', async ({ page }) => {
    await page.goto('/technology/');
    const search = page.getByRole('searchbox', { name: '기술·해금 항목 검색' });
    await search.fill('배합 목장');
    const card = page.getByRole('button', { name: /배합 목장/ }).first();
    await card.focus();
    await card.click();

    await expect(search).toBeHidden();
    await expect(page.getByRole('complementary', { name: '선택한 기술 상세' })).toBeVisible();
    await page.getByRole('button', { name: '기술 목록' }).click();
    await expect(search).toBeVisible();
    await expect(card).toBeFocused();
  });
});

test('global search opens the exact technology detail', async ({ page }) => {
  await page.goto('/search/?q=배합%20목장');
  await expect(page.getByRole('heading', { name: '통합 검색' })).toBeVisible();
  await expect(page.getByText('배합 목장').first()).toBeVisible();
  await page
    .getByRole('link', { name: /배합 목장, 기술.*상세 보기/ })
    .first()
    .click();
  await expect(page).toHaveURL(/\/technology\/?\?q=.*&id=/u);
  await expect(page.getByRole('complementary', { name: '선택한 기술 상세' })).toBeVisible();
});

test('global search detail links remain usable with 200% text', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'compact-chrome', 'Run once in the smallest viewport.');
  await page.goto('/search/?q=%EB%B0%B0%ED%95%A9%20%EB%AA%A9%EC%9E%A5');
  await page.addStyleTag({ content: ':root { font-size: 200% !important; }' });

  const result = page.getByRole('link', { name: /배합 목장, 기술.*상세 보기/ }).first();
  await expect(result).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    ),
  ).toBe(false);
  await result.click();
  await expect(page.getByRole('complementary', { name: '선택한 기술 상세' })).toBeVisible();
});

test('item archive exposes exact rarity 5 records as legendary', async ({ page }) => {
  await page.goto('/items/');
  await page.getByRole('searchbox', { name: '아이템 도감 검색' }).fill('등급 5');
  await expect(page.getByText('부패 독 여과막')).toBeVisible();
  await expect(page.getByText('방폭 섬유')).toBeVisible();
  await expect(
    page.getByRole('heading', { name: '아이템 도감' }).locator('..').getByText('2개'),
  ).toBeVisible();
  await expect(page.getByText('등급 5', { exact: true })).toHaveCount(0);
});

test('pal graphical index keeps exact filters across comparison modes', async ({ page }) => {
  await page.goto('/pals/');
  await expect(page.getByRole('button', { name: '그리드 보기' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );

  if ((page.viewportSize()?.width ?? 0) < 1024) {
    await page.getByRole('button', { name: '속성', exact: true }).click();
  }
  await page.getByRole('checkbox', { name: '물' }).click();
  await page.getByRole('searchbox', { name: '팰 도감 검색' }).fill('검구리');
  await expect(page.locator('.pal-card')).toHaveCount(1);
  await expect(page.getByRole('button', { name: /검구리.*팰/ })).toBeVisible();
  await expect(page.getByText('KendoFrog_Dark')).toHaveCount(0);

  await page.getByRole('button', { name: '목록 보기' }).click();
  await expect(page.getByRole('button', { name: '목록 보기' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(page.getByRole('searchbox', { name: '팰 도감 검색' })).toHaveValue('검구리');
  await expect(page.locator('.pal-row')).toHaveCount(1);
  await expect(
    page.getByRole('button', {
      name: (page.viewportSize()?.width ?? 0) < 1024 ? '속성 1' : '속성 1 ×',
      exact: true,
    }),
  ).toBeVisible();
});

test('pal detail prioritizes exact play facts and grouped acquisition data', async ({ page }) => {
  await page.goto('/pals/');
  await page.getByRole('searchbox', { name: '팰 도감 검색' }).fill('가시공주');
  await page
    .getByRole('button', { name: /가시공주.*팰/ })
    .first()
    .click();

  const inspector = page.getByRole('complementary', { name: '선택한 팰 상세' });
  await expect(inspector.getByText('주요 작업')).toBeVisible();
  await expect(inspector.getByText(/Lv\. \d/u).first()).toBeVisible();
  await expect(inspector.getByRole('link', { name: '지도에서 보기' })).toHaveCount(0);

  await inspector.getByRole('tab', { name: /작업 \d/u }).click();
  await expect(inspector.locator('.work-grid li').first()).toBeVisible();

  await inspector.getByRole('tab', { name: /스킬 \d/u }).click();
  await expect(inspector.locator('.skill-level').first()).toContainText('Lv.');
  await expect(inspector.locator('.skill-meta').first()).toContainText('위력');

  await inspector.getByRole('tab', { name: '획득 4' }).click();
  await expect(inspector.getByRole('heading', { name: '일반 드롭' })).toBeVisible();
  await expect(inspector.getByRole('heading', { name: '보스 드롭' })).toBeVisible();
  await expect(inspector.getByText('확정').first()).toBeVisible();
  await expect(inspector.getByText('100%')).toHaveCount(0);
});

test.describe('compact unified catalog', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('keeps the four primary destinations recognizable and clear of content', async ({
    page,
  }) => {
    await page.goto('/pals/');
    const primary = page.locator('header .primary');
    const links = primary.getByRole('link');
    await expect(links).toHaveCount(4);

    await Promise.all(
      ['홈', '지도', '도감', '계획'].map(async (label) => {
        const link = primary.getByRole('link', { name: label, exact: true });
        await expect(link).toBeVisible();
        await expect(link.locator('.primary-icon svg')).toHaveCount(1);
        const box = await link.boundingBox();
        if (!box) throw new Error(`${label} 하단 탐색 항목을 측정할 수 없습니다.`);
        expect(box.height).toBeGreaterThanOrEqual(64);
        expect(box.width).toBeGreaterThanOrEqual(44);
      }),
    );

    await expect(primary.getByRole('link', { name: '도감', exact: true })).toHaveAttribute(
      'aria-current',
      'page',
    );
    const spacing = await page.evaluate(() => {
      const nav = document.querySelector<HTMLElement>('header .primary');
      const content = document.querySelector<HTMLElement>('#main-content');
      return {
        navHeight: nav?.getBoundingClientRect().height ?? 0,
        contentPadding: Number.parseFloat(getComputedStyle(content!).paddingBottom),
      };
    });
    expect(spacing.contentPadding).toBeGreaterThanOrEqual(spacing.navHeight - 1);

    await primary.getByRole('link', { name: '지도', exact: true }).click();
    await expect(page.getByRole('heading', { name: '탐험 지도' })).toBeVisible();
    await expect(primary.getByRole('link', { name: '지도', exact: true })).toHaveAttribute(
      'aria-current',
      'page',
    );
  });

  test('keeps pal results, selection and navigation reachable', async ({ page }) => {
    await page.goto('/pals/');
    await expect(page.getByRole('heading', { name: '팰 도감' })).toBeVisible();
    await page.getByRole('searchbox', { name: '팰 도감 검색' }).fill('도로롱');
    const result = page.getByRole('button', { name: /도로롱.*팰/ }).first();
    await expect(result).toBeVisible();
    await result.click();
    const inspector = page.getByRole('complementary', { name: '선택한 팰 상세' });
    await expect(inspector).toBeVisible();
    await inspector.getByRole('tab', { name: '스킬 8' }).click();
    await expect(inspector.locator('.skill-list li')).toHaveCount(8);
    await inspector.getByRole('button', { name: '목록' }).click();
    await expect(result).toBeVisible();
    await expect(result).toBeFocused();
    await expect(page.locator('html')).not.toHaveCSS('overflow-x', 'scroll');
  });

  test('starts with a compact result page and expands in mobile-sized batches', async ({
    page,
  }) => {
    await page.goto('/items/');
    const more = page.getByRole('button', { name: /다음 24개 보기/ });
    await expect(more).toBeVisible();
    await more.click();
    await expect(page.getByRole('button', { name: /다음 24개 보기/ })).toBeVisible();
    await expect(page.locator('.item-card')).toHaveCount(48);
  });

  test('keeps the actual passive effect visible without internal rank or fake media', async ({
    page,
  }) => {
    await page.goto('/skills/passive/');
    const firstResult = page.locator('.record-card').first();
    await expect(firstResult.locator('.card-description')).toBeVisible();
    await expect(firstResult.locator('.card-metrics')).toHaveCount(0);
    await expect(firstResult.getByText(/효과 등급/)).toHaveCount(0);
    await expect(firstResult.locator('.image-fallback')).toHaveCount(0);
  });

  test('keeps every catalog category directly reachable without a compact dialog', async ({
    page,
  }) => {
    await page.goto('/pals/');
    await expect(page.locator('[data-palbeacon-hydrated="true"]')).toBeAttached();
    const catalogNavigation = page.getByRole('navigation', { name: '도감 세부 메뉴' });
    await expect(catalogNavigation).toBeVisible();
    await expect(catalogNavigation.getByRole('link')).toHaveCount(5);
    await expect(catalogNavigation.getByRole('link', { name: '팰', exact: true })).toHaveAttribute(
      'aria-current',
      'page',
    );
    await expect(page.getByRole('button', { name: '현재 메뉴 열기' })).toHaveCount(0);
  });

  test('keeps one clear mobile entry for every planning purpose', async ({ page }) => {
    await page.goto('/plan/');
    await expect(page.locator('[data-palbeacon-hydrated="true"]')).toBeAttached();
    const menu = page.getByRole('navigation', { name: '계획 세부 메뉴' });
    await expect(menu.getByRole('link')).toHaveCount(4);
    await expect(menu).toHaveCSS('display', 'flex');
    await expect(menu.getByRole('link', { name: '팀 구성' })).toBeVisible();
    await expect(menu.getByRole('link', { name: '팰 비교', exact: true })).toBeVisible();

    const teamBox = await menu.getByRole('link', { name: '팀 구성' }).boundingBox();
    const breedingBox = await menu.getByRole('link', { name: '교배', exact: true }).boundingBox();
    if (!teamBox || !breedingBox) throw new Error('Planning menu rows must be measurable.');
    expect(Math.abs(breedingBox.y - teamBox.y)).toBeLessThan(3);
  });

  test('keeps active and passive skills under one catalog destination', async ({ page }) => {
    await page.goto('/skills/passive/');
    const catalogNavigation = page.getByRole('navigation', { name: '도감 세부 메뉴' });
    await expect(
      catalogNavigation.getByRole('link', { name: '스킬', exact: true }),
    ).toHaveAttribute('aria-current', 'page');
    const skillTypes = page.getByRole('navigation', { name: '스킬 종류' });
    await expect(skillTypes.getByRole('link', { name: '패시브', exact: true })).toHaveAttribute(
      'aria-current',
      'page',
    );
    await skillTypes.getByRole('link', { name: '액티브', exact: true }).click();
    await expect(page).toHaveURL(/\/skills\/active\/?$/u);
  });
});

test('shell separates mobile, tablet, and desktop layout contracts', async ({ page }) => {
  const expectNoPageOverflow = async () => {
    const hasPageOverflow = await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    );
    expect(hasPageOverflow).toBe(false);
  };

  await page.setViewportSize({ width: 320, height: 720 });
  await page.goto('/items/');
  await expect(page.getByRole('heading', { name: '아이템 도감' })).toBeVisible();
  await expect(page.getByRole('navigation', { name: '도감 세부 메뉴' })).toBeVisible();
  await expect(page.getByRole('button', { name: '현재 메뉴 열기' })).toHaveCount(0);
  await expect(page.locator('.item-row').first().locator('.row-metrics')).toBeHidden();
  await expectNoPageOverflow();

  await page.setViewportSize({ width: 768, height: 1024 });
  await page.goto('/skills/active/');
  await expect(page.getByRole('heading', { name: '액티브 스킬' })).toBeVisible();
  await expect(page.getByRole('complementary', { name: '현재 분류 메뉴' })).toHaveCount(0);
  await expect(page.getByRole('navigation', { name: '도감 세부 메뉴' })).toBeVisible();
  await expect(page.getByRole('button', { name: '현재 메뉴 열기' })).toHaveCount(0);
  await expectNoPageOverflow();

  await page.setViewportSize({ width: 901, height: 1024 });
  await page.goto('/pals/');
  await expect(page.getByRole('complementary', { name: '현재 분류 메뉴' })).toHaveCount(0);
  await expect(page.getByRole('navigation', { name: '도감 세부 메뉴' })).toBeVisible();
  await expect(page.getByRole('button', { name: '현재 메뉴 열기' })).toHaveCount(0);
  await page.getByRole('searchbox', { name: '팰 도감 검색' }).fill('가시공주');
  await expect(page.getByRole('button', { name: /가시공주.*팰/ }).getByText('No. 060')).toHaveCount(
    1,
  );
  await expectNoPageOverflow();

  await page.setViewportSize({ width: 1024, height: 720 });
  await page.goto('/skills/active/');
  await expect(page.getByRole('navigation', { name: '도감 세부 메뉴' })).toBeVisible();
  await expect(page.getByRole('complementary', { name: '현재 분류 메뉴' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '현재 메뉴 열기' })).toHaveCount(0);
  await expectNoPageOverflow();
});

test('compact catalog remains usable with 200% text', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'compact-chrome', 'Run once in the smallest viewport.');
  await page.goto('/pals/');
  await page.addStyleTag({ content: ':root { font-size: 200% !important; }' });

  await expect(page.getByRole('heading', { name: '팰 도감' })).toBeVisible();
  await expect(page.getByRole('searchbox', { name: '팰 도감 검색' })).toBeVisible();
  await expect(page.getByRole('navigation', { name: '도감 세부 메뉴' })).toBeVisible();
  await expect(page.getByRole('button', { name: '현재 메뉴 열기' })).toHaveCount(0);
  const primary = page.locator('header .primary');
  await expect(primary.locator('.primary-icon svg')).toHaveCount(4);
  await expect(primary.getByRole('link', { name: '홈', exact: true })).toBeVisible();
  await expect(primary.getByRole('link', { name: '계획', exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: '팰 도감' })).toHaveCSS(
    'writing-mode',
    'horizontal-tb',
  );
  const cards = page.locator('.pal-card');
  await expect(cards.first()).toBeVisible();
  await expect(cards.nth(1)).toBeVisible();
  const [firstCard, secondCard] = await Promise.all([
    cards.first().boundingBox(),
    cards.nth(1).boundingBox(),
  ]);
  expect(firstCard).not.toBeNull();
  expect(secondCard).not.toBeNull();
  expect(secondCard?.y ?? 0).toBeGreaterThan(firstCard?.y ?? 0);
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    ),
  ).toBe(false);
});

test('compact item archive keeps filters and core values reachable with 200% text', async ({
  page,
}, testInfo) => {
  test.skip(testInfo.project.name !== 'compact-chrome', 'Run once in the smallest viewport.');
  await page.goto('/items/');
  await page.addStyleTag({ content: ':root { font-size: 200% !important; }' });

  await expect(page.getByRole('heading', { name: '아이템 도감' })).toBeVisible();
  await expect(page.getByRole('searchbox', { name: '아이템 도감 검색' })).toBeVisible();
  await expect(page.getByRole('button', { name: '필터' })).toBeVisible();
  await expect(page.getByRole('button', { name: '그리드 보기' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(page.locator('.item-card').first()).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    ),
  ).toBe(false);
});

test('compact technology archive remains usable with 200% text', async ({ page }, testInfo) => {
  test.skip(testInfo.project.name !== 'compact-chrome', 'Run once in the smallest viewport.');
  await page.goto('/technology/');
  await page.addStyleTag({ content: ':root { font-size: 200% !important; }' });

  await expect(page.getByRole('heading', { name: '기술 도감' })).toBeVisible();
  await expect(page.getByRole('searchbox', { name: '기술·해금 항목 검색' })).toBeVisible();
  await expect(page.getByRole('button', { name: '고대 기술', exact: true })).toBeVisible();
  await expect(page.locator('.technology-card').first()).toBeVisible();
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    ),
  ).toBe(false);
});

test('item archive uses a filter rail on desktop and sequential detail below 1024px', async ({
  page,
}) => {
  await page.goto('/items/');
  const width = page.viewportSize()?.width ?? 0;

  if (width < 1024) {
    await page.getByRole('button', { name: '필터' }).click();
    await page.getByRole('checkbox', { name: '재료' }).check();
    await page.getByRole('button', { name: '결과 보기' }).click();
  } else {
    await expect(page.getByRole('complementary', { name: '아이템 필터' })).toBeVisible();
    await page.getByRole('checkbox', { name: '재료' }).check();
  }

  await page.getByRole('searchbox', { name: '아이템 도감 검색' }).fill('가죽');
  const result = page.getByRole('button', { name: /^가죽, 재료, 일반/ }).first();
  await expect(result).toBeVisible();
  await result.click();
  const inspector = page.getByRole('complementary', { name: '선택한 아이템 상세' });
  await expect(inspector).toBeVisible();

  if (width < 1024) {
    await expect(result).toBeHidden();
    await inspector.getByRole('button', { name: /목록/ }).click();
    await expect(result).toBeVisible();
    await expect(result).toBeFocused();
  } else {
    const box = await inspector.boundingBox();
    expect(box?.width ?? 0).toBeGreaterThanOrEqual(300);
    expect(box?.width ?? 999).toBeLessThanOrEqual(380);
  }

  await page.getByRole('button', { name: '목록 보기' }).click();
  await expect(page.getByRole('button', { name: '목록 보기' })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(page.getByRole('searchbox', { name: '아이템 도감 검색' })).toHaveValue('가죽');
  await expect(page.locator('.item-row').first()).toBeVisible();

  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    ),
  ).toBe(false);
});

test('pal archive uses sequential compact screens and a bounded desktop inspector', async ({
  page,
}) => {
  const inspectAt = async (width: number, height: number) => {
    await page.setViewportSize({ width, height });
    await page.goto('/pals/');
    await page.getByRole('searchbox', { name: '팰 도감 검색' }).fill('도로롱');
    const result = page.getByRole('button', { name: /도로롱.*팰/ }).first();
    await expect(result).toBeVisible();
    await expect(result.getByText(/SheepBall/)).toHaveCount(0);
    await result.click();
    const inspector = page.getByRole('complementary', { name: '선택한 팰 상세' });
    await expect(inspector).toBeVisible();
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    );
    expect(overflow).toBe(false);
    const inspectorBox = await inspector.boundingBox();
    if (width < 1024) {
      await expect(result).toBeHidden();
      await inspector.getByRole('button', { name: '목록' }).click();
      await expect(result).toBeVisible();
      await expect(result).toBeFocused();
      return { index: await page.locator('.pal-index').boundingBox(), inspector: inspectorBox };
    }
    return { index: await page.locator('.pal-index').boundingBox(), inspector: inspectorBox };
  };

  const mobile = await inspectAt(320, 720);
  if (!mobile.index || !mobile.inspector) throw new Error('Mobile panes must be measurable.');
  expect(Math.abs(mobile.inspector.width - mobile.index.width)).toBeLessThan(3);

  const tablet = await inspectAt(768, 1024);
  if (!tablet.index || !tablet.inspector) throw new Error('Tablet panes must be measurable.');
  expect(Math.abs(tablet.inspector.width - tablet.index.width)).toBeLessThan(3);

  const desktop = await inspectAt(1440, 900);
  if (!desktop.index || !desktop.inspector) throw new Error('Desktop panes must be measurable.');
  expect(desktop.inspector.y).toBeGreaterThanOrEqual(desktop.index.y + desktop.index.height - 3);
  expect(Math.abs(desktop.inspector.width - desktop.index.width)).toBeLessThan(3);
  expect(desktop.inspector.height).toBeLessThanOrEqual(226);
});

test('planning tools switch between model results and exact material totals', async ({ page }) => {
  await page.goto('/plan/');
  await expect(page.getByRole('heading', { name: '팀 구성', level: 1 })).toBeVisible();
  await expect(page.getByRole('button', { name: '공격 우선', exact: true })).toBeVisible();
  await page.goto('/plan/materials/');
  await expect(page.getByRole('heading', { name: '재료 계산' })).toBeVisible();
  const buildingPicker = page.getByRole('combobox', { name: '건축물' });
  const buildingPickerBox = await buildingPicker.boundingBox();
  expect(buildingPickerBox?.width ?? 0).toBeGreaterThanOrEqual(240);
  await page.getByRole('spinbutton', { name: '수량' }).fill('3');
  await expect(page.getByText('× 3', { exact: true })).toBeVisible();
});

test('planning menu gives each tool one stable route', async ({ page }) => {
  await page.goto('/plan/');
  await expect(page.getByRole('heading', { name: '팀 구성', level: 1 })).toBeVisible();
  await page.goto('/plan/compare/');
  await expect(page.getByRole('heading', { name: '팰 비교', level: 1 })).toBeVisible();
  await expect(page.getByRole('combobox', { name: '작업 종류' })).toBeVisible();
  await page.getByRole('button', { name: '이동', exact: true }).click();
  await expect(page.getByText('탑승 질주').first()).toBeVisible();
});

test('breeding calculator preserves exact forward result and special rules', async ({ page }) => {
  await page.goto('/plan/breeding/');
  await expect(page.getByRole('heading', { name: '교배 계산' })).toBeVisible();
  await page.getByRole('combobox', { name: '첫 번째 부모 팰' }).selectOption('SheepBall');
  await page.getByRole('combobox', { name: '두 번째 부모 팰' }).selectOption('ChickenPal');
  await expect(page.getByRole('heading', { name: '차코리' })).toBeVisible();
  await expect(page.getByText('Ganesha', { exact: true })).toHaveCount(0);
  await expect(page.getByText('일반 교배', { exact: true })).toBeVisible();
});

test('removed legacy routes stay absent instead of redirecting', async ({ page }) => {
  const removedPaths = ['/shops/', '/plan/team/', '/plan/travel/', '/wiki/', '/plan/assistant/'];
  const verifyAbsent = async (index: number): Promise<void> => {
    const path = removedPaths[index];
    if (!path) return;
    await page.goto(path);
    await expect(page).toHaveURL(new RegExp(`${path.replaceAll('/', '\\/')}?$`, 'u'));
    await expect(page.getByRole('heading', { name: '페이지를 찾을 수 없습니다.' })).toBeVisible();
    await verifyAbsent(index + 1);
  };

  await verifyAbsent(0);
});

test('web direct links hide Windows-only screens behind not-found', async ({ page }) => {
  await page.goto('/connection/overlay/');
  await expect(page.getByRole('heading', { name: '페이지를 찾을 수 없습니다.' })).toBeVisible();
  await expect(page.getByText('WINDOWS APP ONLY')).toHaveCount(0);
  await expect(page.getByText('오버레이')).toHaveCount(0);
  await expect(page.getByText('/connection/overlay/')).toHaveCount(0);
  await expect(page.getByText('오버레이 설정을 적용하지 못했습니다.')).toHaveCount(0);
  await expect(page.getByRole('complementary', { name: '현재 분류 메뉴' })).toHaveCount(0);
  await expect(page.getByRole('button', { name: '현재 메뉴 열기' })).toHaveCount(0);
});

test('unknown paths render a real not-found recovery screen', async ({ page }) => {
  await page.goto('/not-a-real-route/');
  await expect(page.getByRole('heading', { name: '페이지를 찾을 수 없습니다.' })).toBeVisible();
  await expect(page.getByText('MIGRATION QUEUE')).toHaveCount(0);
  await expect(page.getByRole('link', { name: '홈으로 돌아가기' })).toBeVisible();
});

test('map keeps exact POIs searchable and hides unverified supplemental coordinates', async ({
  page,
}) => {
  await page.goto('/map/');
  await expect(page.getByRole('heading', { name: '탐험 지도' })).toBeVisible();
  await expect(page.getByText(/지도에 [\d,]+개 표시 중/)).toBeVisible();
  const renderedMarkers = page.locator('[data-poi-marker]');
  const renderedMarkerCount = await renderedMarkers.count();
  expect(renderedMarkerCount).toBeGreaterThan(0);
  expect(renderedMarkerCount).toBeLessThan(432);
  await expect(page.locator('[data-poi-cluster]')).toHaveCount(0);

  const controls = page.locator('#map-filter-panel');
  if ((page.viewportSize()?.width ?? 0) <= 820) {
    await page.getByRole('button', { name: /^필터 \d+$/ }).click();
  }
  await controls.getByRole('searchbox', { name: '위치 검색' }).fill('라이바오');
  await controls.getByRole('button', { name: /라이바오/ }).click();
  const inspector = page.getByRole('complementary', { name: '선택한 지도 위치 상세' });
  await expect(inspector.getByRole('heading', { name: '라이바오' })).toBeVisible();
  await expect(inspector.getByText('GrassPanda_Electric')).toHaveCount(0);
  await expect(inspector.getByText('게임 데이터 검증 완료')).toHaveCount(0);

  await expect(page.getByText(/exact-build 검증 아님/)).toHaveCount(0);
  await expect(page.getByRole('switch', { name: /외부 보조 레이어/ })).toHaveCount(0);
  await expect(page.getByRole('combobox', { name: '한 번에 한 레이어' })).toHaveCount(0);
  await expect(page.getByText(/PalMods \+ PalDB map data/)).toHaveCount(0);

  if ((page.viewportSize()?.width ?? 0) < 720) {
    await page.getByRole('button', { name: /^필터 \d+$/ }).click();
  }
  await controls.getByRole('button', { name: '지도 검색어 지우기' }).click();
  const wanted = controls.getByRole('button', { name: /지명수배/ });
  await expect(wanted).toHaveAttribute('aria-pressed', 'true');
  await wanted.click();
  await expect(wanted).toHaveAttribute('aria-pressed', 'false');
  await wanted.click();
  await expect(wanted).toHaveAttribute('aria-pressed', 'true');
});

test.describe('compact map layout', () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test('keeps region, controls and exact markers reachable', async ({ page }) => {
    await page.goto('/map/');
    await expect(page.getByRole('region', { name: '팰파고스 제도 지도' })).toBeVisible();
    await page.getByRole('tab', { name: '세계수' }).click();
    await expect(page.getByRole('region', { name: '세계수 지도' })).toBeVisible();
    await expect(page.getByText(/현재 위치/)).toHaveCount(0);
    await expect(page.locator('html')).not.toHaveCSS('overflow-x', 'scroll');
  });
});
