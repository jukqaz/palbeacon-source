import { expect, test, type TestInfo } from '@playwright/test';

type UserAction = (label: string, action: () => Promise<unknown>) => Promise<void>;

const auditJourney = async (
  testInfo: TestInfo,
  journeyId: string,
  maxActions: number,
  run: (act: UserAction) => Promise<void>,
): Promise<void> => {
  const actions: string[] = [];
  let completed = false;
  const act: UserAction = async (label, action) => {
    actions.push(label);
    expect(
      actions.length,
      `${journeyId} exceeded its ${maxActions}-action budget`,
    ).toBeLessThanOrEqual(maxActions);
    await test.step(`${actions.length.toString()}. ${label}`, action);
  };

  try {
    await run(act);
    completed = true;
  } finally {
    await testInfo.attach('usability-metrics.json', {
      body: Buffer.from(
        JSON.stringify(
          {
            schema: 'palbeacon.usability-metrics.v1',
            journey_id: journeyId,
            project: testInfo.project.name,
            completed,
            action_count: actions.length,
            max_actions: maxActions,
            actions,
          },
          null,
          2,
        ),
      ),
      contentType: 'application/json',
    });
  }
};

const inProjects = (testInfo: TestInfo, projects: readonly string[]): boolean =>
  projects.includes(testInfo.project.name);

test('UQ-SEARCH-01 opens the exact catalog detail within three actions', async ({
  page,
}, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome', 'mobile-chrome']), 'Desktop and mobile task.');
  await page.goto('/');

  await auditJourney(testInfo, 'UQ-SEARCH-01', 3, async (act) => {
    const search = page.getByRole('main').getByRole('searchbox', { name: '통합 검색' });
    await expect(search).toBeVisible();
    await act('한글 이름 입력', () => search.fill('도로롱'));
    await act('검색 실행', () =>
      page.getByRole('main').getByRole('button', { name: '찾기' }).click(),
    );
    await expect(page).toHaveURL(/\/search\/?\?q=/u);
    const result = page.getByRole('link', { name: /도로롱.*팰.*상세 보기/ }).first();
    await expect(result).toBeVisible();
    await act('검색 결과 상세 열기', () => result.click());
    await expect(page).toHaveURL(/\/pals\/?\?q=.*&id=/u);
    await expect(page.getByRole('complementary', { name: '선택한 팰 상세' })).toBeVisible();
  });
});

test('UQ-PALS-01 restores filters and focus after reviewing a Pal', async ({ page }, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome', 'mobile-chrome']), 'Desktop and mobile task.');
  await page.goto('/pals/');
  const compact = (page.viewportSize()?.width ?? 0) < 1024;

  await auditJourney(testInfo, 'UQ-PALS-01', 5, async (act) => {
    if (compact) {
      await act('속성 필터 열기', () =>
        page.getByRole('button', { name: '속성', exact: true }).click(),
      );
    }
    const water = page.getByRole('checkbox', { name: '물' });
    await act('물 속성 선택', () => water.click());
    if (compact) {
      await expect(page.getByRole('button', { name: '속성 1', exact: true })).toBeVisible();
    }
    const search = page.getByRole('searchbox', { name: '팰 도감 검색' });
    await act('한글 팰 이름 입력', () => search.fill('검구리'));
    const result = page.getByRole('button', { name: /검구리.*팰/ }).first();
    await expect(result).toBeVisible();
    await act('팰 상세 열기', () => result.click());
    await expect(page.getByRole('complementary', { name: '선택한 팰 상세' })).toBeVisible();

    if (compact) {
      await act('팰 목록으로 돌아가기', () =>
        page.getByRole('button', { name: '팰 목록' }).click(),
      );
      await expect(search).toHaveValue('검구리');
      await expect(page.getByRole('button', { name: '속성 1', exact: true })).toBeVisible();
      await expect(result).toBeFocused();
    } else {
      await expect(result).toHaveAttribute('aria-pressed', 'true');
    }
  });
});

test('UQ-ITEMS-01 finds an item and opens its verified relations', async ({ page }, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome', 'mobile-chrome']), 'Desktop and mobile task.');
  await page.goto('/items/');
  const compact = (page.viewportSize()?.width ?? 0) < 1024;

  await auditJourney(testInfo, 'UQ-ITEMS-01', 4, async (act) => {
    const search = page.getByRole('searchbox', { name: '아이템 도감 검색' });
    await act('한글 아이템 이름 입력', () => search.fill('가죽'));
    const result = page.getByRole('button', { name: /^가죽, 재료, 일반/ }).first();
    await expect(result).toBeVisible();
    await act('아이템 상세 열기', () => result.click());
    const detail = page.getByRole('complementary', { name: '선택한 아이템 상세' });
    await expect(detail.getByRole('heading', { name: '가죽' })).toBeVisible();
    await expect(detail.getByText(/상점|드롭|정보/u).first()).toBeVisible();
    if (compact) {
      await act('아이템 목록으로 돌아가기', () =>
        page.getByRole('button', { name: '아이템 목록' }).click(),
      );
      await expect(search).toHaveValue('가죽');
      await expect(result).toBeFocused();
    }
  });
});

test('UQ-TECHNOLOGY-01 finds a technology and opens its requirements', async ({
  page,
}, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome', 'mobile-chrome']), 'Desktop and mobile task.');
  await page.goto('/technology/');

  await auditJourney(testInfo, 'UQ-TECHNOLOGY-01', 3, async (act) => {
    await act('기술·해금 항목 입력', () =>
      page.getByRole('searchbox', { name: '기술·해금 항목 검색' }).fill('배합 목장'),
    );
    const result = page.getByRole('button', { name: /배합 목장/ }).first();
    await expect(result).toBeVisible();
    await act('기술 상세 열기', () => result.click());
    const detail = page.getByRole('complementary', { name: '선택한 기술 상세' });
    await expect(detail).toBeVisible();
    await expect(detail.getByText(/레벨|포인트/u).first()).toBeVisible();
  });
});

test('UQ-BUILDINGS-01 finds a building and opens exact materials', async ({ page }, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome', 'mobile-chrome']), 'Desktop and mobile task.');
  await page.goto('/buildings/');

  await auditJourney(testInfo, 'UQ-BUILDINGS-01', 4, async (act) => {
    await act('건축물 이름 입력', () =>
      page.getByRole('searchbox', { name: '건축물 검색' }).fill('가축 목장'),
    );
    await act('팰 시설 분류 선택', () =>
      page
        .getByRole('navigation', { name: '건축물 대분류' })
        .getByRole('button', { name: '팰 시설', exact: true })
        .click(),
    );
    const result = page.getByRole('button', { name: /가축 목장, 팰 시설/ });
    await expect(result).toBeVisible();
    await act('건축물 상세 열기', () => result.click());
    const detail = page.getByRole('complementary', { name: '선택한 건축물 상세' });
    await expect(detail.getByRole('heading', { name: '필요 재료' })).toBeVisible();
    await act('같은 건축물 재료 계산 열기', () =>
      detail.getByRole('link', { name: '재료 계산' }).click(),
    );
    await expect(page).toHaveURL(/\/plan\/materials\/?\?id=/u);
    await expect(page.getByRole('combobox', { name: '건축물' })).toHaveValue('가축 목장');
  });
});

test('UQ-MAP-01 finds a Korean map location without exposing its raw ID', async ({
  page,
}, testInfo) => {
  test.skip(
    !inProjects(testInfo, ['desktop-chrome', 'mobile-chrome', 'compact-chrome', 'tablet-chrome']),
    'Chrome viewport task.',
  );
  await page.goto('/map/');

  await auditJourney(testInfo, 'UQ-MAP-01', 3, async (act) => {
    const viewportWidth = page.viewportSize()?.width ?? 0;
    if (viewportWidth <= 820) {
      await act('지도 필터 열기', () => page.getByRole('button', { name: /^필터 \d+$/ }).click());
    }
    const controls = page.locator('#map-filter-panel');
    await act('한글 위치 입력', () =>
      controls.getByRole('searchbox', { name: '위치 검색' }).fill('라이바오'),
    );
    const result = controls.getByRole('button', { name: /라이바오/ });
    await expect(result).toBeInViewport();
    await act('위치 결과 열기', () => result.click());
    const detail = page.getByRole('complementary', { name: '선택한 지도 위치 상세' });
    await expect(detail.getByRole('heading', { name: '라이바오' })).toBeVisible();
    await expect(detail.getByText('GrassPanda_Electric')).toHaveCount(0);
    if (viewportWidth < 720) {
      await expect(page.getByRole('dialog', { name: '지도 필터' })).toHaveCount(0);
      await expect(detail).toBeFocused();
      await expect(detail).toBeInViewport();
    }
  });
});

test('UQ-MATERIALS-01 selects a target, recalculates totals and opens an ingredient', async ({
  page,
}, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome', 'mobile-chrome']), 'Desktop and mobile task.');
  await page.goto('/plan/materials/');

  await auditJourney(testInfo, 'UQ-MATERIALS-01', 3, async (act) => {
    await act('목표 건축물 선택', () =>
      page.getByRole('combobox', { name: '건축물' }).fill('배합 목장'),
    );
    await act('목표 수량 변경', () => page.getByRole('spinbutton', { name: '수량' }).fill('3'));
    await expect(page.getByRole('heading', { name: '배합 목장', level: 3 })).toBeVisible();
    await expect(page.getByText('× 3', { exact: true })).toBeVisible();
    const ingredient = page.getByRole('list', { name: '필요 재료' }).getByRole('link').first();
    const ingredientName = (await ingredient.locator('strong').textContent())?.trim() ?? '';
    expect(ingredientName).not.toBe('');
    await act('필요 아이템 상세 열기', () => ingredient.click());
    await expect(page).toHaveURL(/\/items\/?\?q=.*&id=/u);
    const detail = page.getByRole('complementary', { name: '선택한 아이템 상세' });
    await expect(detail).toBeVisible();
    await expect(detail.getByRole('heading', { name: ingredientName })).toBeVisible();
  });
});

test('UQ-BREEDING-01 calculates an exact child from two parents', async ({ page }, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome', 'mobile-chrome']), 'Desktop and mobile task.');
  await page.goto('/plan/breeding/');

  await auditJourney(testInfo, 'UQ-BREEDING-01', 2, async (act) => {
    await act('첫 번째 부모 선택', () =>
      page.getByRole('combobox', { name: '첫 번째 부모 팰' }).selectOption('SheepBall'),
    );
    await act('두 번째 부모 선택', () =>
      page.getByRole('combobox', { name: '두 번째 부모 팰' }).selectOption('ChickenPal'),
    );
    await expect(page.getByRole('heading', { name: '차코리' })).toBeVisible();
    await expect(page.getByText('일반 교배', { exact: true })).toBeVisible();
  });
});

test('UQ-COMPARE-01 switches exact Pal comparison criteria in one workspace', async ({
  page,
}, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome', 'mobile-chrome']), 'Desktop and mobile task.');
  await page.goto('/plan/compare/');

  await auditJourney(testInfo, 'UQ-COMPARE-01', 2, async (act) => {
    await expect(page.getByRole('combobox', { name: '작업 종류' })).toBeVisible();
    await act('이동 기준으로 전환', () =>
      page.getByRole('button', { name: '이동', exact: true }).click(),
    );
    await expect(page.getByText('탑승 질주').first()).toBeVisible();
    await expect(page.getByText('스태미나').first()).toBeVisible();
    await act('작업 기준으로 돌아가기', () =>
      page.getByRole('button', { name: '작업', exact: true }).click(),
    );
    await expect(page.getByRole('combobox', { name: '작업 종류' })).toBeVisible();
  });
});

test('UQ-WORKSPACE-01 keeps a saved Pal and recent context across routes', async ({
  page,
}, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome', 'mobile-chrome']), 'Desktop and mobile task.');
  await page.addInitScript(() => window.localStorage.clear());
  await page.goto('/pals/');

  await auditJourney(testInfo, 'UQ-WORKSPACE-01', 5, async (act) => {
    const search = page.getByRole('searchbox', { name: '팰 도감 검색' });
    await act('한글 팰 이름 입력', () => search.fill('도로롱'));
    const result = page.getByRole('button', { name: /도로롱.*팰/ }).first();
    await expect(result).toBeVisible();
    await act('팰 상세 열기', () => result.click());
    await act('내 목록에 저장', () => page.getByRole('button', { name: '도로롱 저장' }).click());
    await expect(page.getByRole('complementary', { name: '내 목록' })).toContainText('도로롱');
    await act('홈으로 이동', () => page.getByRole('link', { name: 'PalBeacon 홈' }).click());
    await expect(page.getByRole('heading', { name: '최근 본 항목' })).toHaveCount(0);
    const tray = page.getByRole('complementary', { name: '내 목록' });
    await expect(tray).toContainText('도로롱');
    await act('저장한 팰 다시 열기', () => tray.getByRole('link', { name: /도로롱/ }).click());
    await expect(page.getByRole('complementary', { name: '선택한 팰 상세' })).toBeVisible();
  });
});

test('UQ-PLAN-FILE-01 exports and restores a material plan', async ({ page }, testInfo) => {
  test.skip(!inProjects(testInfo, ['desktop-chrome']), 'Desktop file workflow.');
  await page.addInitScript(() => window.localStorage.clear());
  await page.goto('/plan/materials/');

  await auditJourney(testInfo, 'UQ-PLAN-FILE-01', 4, async (act) => {
    const quantity = page.getByRole('spinbutton', { name: '수량' });
    await act('수량을 3으로 변경', () => quantity.fill('3'));
    let planPath = '';
    await act('계획 파일 저장', async () => {
      const downloadPromise = page.waitForEvent('download');
      await page.getByRole('button', { name: '계획 내보내기' }).click();
      planPath = (await (await downloadPromise).path()) ?? '';
      expect(planPath).not.toBe('');
    });
    await act('수량을 1로 변경', () => quantity.fill('1'));
    await expect(page.getByText('× 1', { exact: true })).toBeVisible();
    await act('계획 파일 불러오기', () => page.getByLabel('계획 가져오기').setInputFiles(planPath));
    await expect(quantity).toHaveValue('3');
    await expect(page.getByText('× 3', { exact: true })).toBeVisible();
  });
});

test('UQ-NAV-01 keeps compact primary destinations large and separate', async ({
  page,
}, testInfo) => {
  test.skip(!inProjects(testInfo, ['compact-chrome', 'mobile-chrome']), 'Compact navigation task.');
  await page.goto('/');

  const navigation = page.getByRole('navigation', { name: '주요 메뉴' });
  const destinations = ['홈', '지도', '도감', '계획'] as const;
  type TargetBox = { destination: string; x: number; y: number; width: number; height: number };
  const boxes = (
    await Promise.all(
      destinations.map(async (destination): Promise<TargetBox | null> => {
        const link = navigation.getByRole('link', { name: destination, exact: true });
        await expect(link).toBeVisible();
        const box = await link.boundingBox();
        expect(box, `${destination} target must be measurable`).not.toBeNull();
        if (!box) return null;
        expect(
          box.width,
          `${destination} target must be at least 44 CSS px wide`,
        ).toBeGreaterThanOrEqual(44);
        expect(
          box.height,
          `${destination} target must be at least 44 CSS px high`,
        ).toBeGreaterThanOrEqual(44);
        return {
          destination,
          x: box.x,
          y: box.y,
          width: box.width,
          height: box.height,
        };
      }),
    )
  ).filter((box): box is TargetBox => box !== null);
  for (let index = 1; index < boxes.length; index += 1) {
    const previous = boxes[index - 1];
    const current = boxes[index];
    if (!previous || !current) continue;
    expect(
      current.x,
      `${current.destination} must not overlap ${previous.destination}`,
    ).toBeGreaterThanOrEqual(previous.x + previous.width - 1);
  }

  await testInfo.attach('usability-metrics.json', {
    body: Buffer.from(
      JSON.stringify(
        {
          schema: 'palbeacon.usability-metrics.v1',
          journey_id: 'UQ-NAV-01',
          project: testInfo.project.name,
          completed: true,
          action_count: 0,
          max_actions: 1,
          target_count: boxes.length,
        },
        null,
        2,
      ),
    ),
    contentType: 'application/json',
  });
});
