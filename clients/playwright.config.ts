import { defineConfig, devices } from '@playwright/test';

/**
 * Playwright config. Defaults are tuned for the testbed ui-smoke scenario:
 *   - baseURL is taken from CLIENTS_URL (in-cluster Service) or falls back to the local
 *     `pnpm preview` server.
 *   - Single worker so the test output is deterministic in the kubectl Job logs.
 *
 * Run locally:
 *   pnpm test:e2e                        # uses webServer below
 *   CLIENTS_URL=http://creda-clients pnpm test:e2e   # in-cluster
 */
const baseURL = process.env.CLIENTS_URL ?? 'http://127.0.0.1:4173';
const useLocalServer = !process.env.CLIENTS_URL;

export default defineConfig({
  testDir: './e2e',
  timeout: 30_000,
  expect: { timeout: 5_000 },
  fullyParallel: false,
  workers: 1,
  forbidOnly: !!process.env.CI,
  retries: 0,
  reporter: process.env.CI ? [['line'], ['junit', { outputFile: 'test-results/junit.xml' }]] : 'list',
  use: {
    baseURL,
    headless: true,
    actionTimeout: 5_000,
    navigationTimeout: 10_000,
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
  ...(useLocalServer
    ? {
        webServer: {
          command: 'pnpm build && pnpm preview',
          url: baseURL,
          timeout: 120_000,
          reuseExistingServer: !process.env.CI,
        },
      }
    : {}),
});
