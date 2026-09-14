import AxeBuilder from '@axe-core/playwright';
import { expect, test, type TestInfo } from '@playwright/test';

const coreRoutes = [
  '/',
  '/fan-content-notice/',
  '/search/',
  '/pals/',
  '/items/',
  '/technology/',
  '/map/',
  '/plan/',
] as const;

const attachViolations = async (
  violations: Awaited<ReturnType<AxeBuilder['analyze']>>['violations'],
  testInfo: TestInfo,
): Promise<void> => {
  if (violations.length === 0) return;
  await testInfo.attach('axe-violations.json', {
    body: Buffer.from(JSON.stringify(violations, null, 2)),
    contentType: 'application/json',
  });
};

for (const route of coreRoutes) {
  test(`${route} satisfies automated WCAG A/AA checks`, async ({ page }, testInfo) => {
    test.skip(
      !['desktop-chrome', 'mobile-chrome'].includes(testInfo.project.name),
      'Run the accessibility matrix once per desktop and mobile layout.',
    );

    await page.goto(route, { waitUntil: 'networkidle' });

    const results = await new AxeBuilder({ page })
      .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
      .analyze();
    await attachViolations(results.violations, testInfo);

    expect(results.violations, `${route} has automated accessibility violations`).toEqual([]);
  });
}
