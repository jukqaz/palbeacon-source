import { defineConfig, devices } from '@playwright/test';

const baseUrl = process.env['PALBEACON_E2E_BASE_URL'] ?? 'http://127.0.0.1:5173';

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  // Follow Playwright's CI guidance: a shared runner must not run four
  // data-heavy browsers plus their offline installers against one server.
  workers: process.env['CI'] ? 1 : 4,
  timeout: 60_000,
  retries: 0,
  reporter: 'list',
  use: {
    baseURL: baseUrl,
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
  },
  projects: [
    {
      name: 'desktop-chrome',
      use: {
        ...devices['Desktop Chrome'],
        channel: 'chrome',
        viewport: { width: 1440, height: 900 },
      },
    },
    {
      name: 'mobile-chrome',
      use: {
        ...devices['Desktop Chrome'],
        channel: 'chrome',
        viewport: { width: 390, height: 844 },
      },
    },
    {
      name: 'compact-chrome',
      use: {
        ...devices['Desktop Chrome'],
        channel: 'chrome',
        viewport: { width: 320, height: 720 },
      },
    },
    {
      name: 'tablet-chrome',
      use: {
        ...devices['Desktop Chrome'],
        channel: 'chrome',
        viewport: { width: 768, height: 1024 },
      },
    },
    {
      name: 'desktop-webkit',
      use: { ...devices['Desktop Safari'], viewport: { width: 1440, height: 900 } },
    },
  ],
});
