import { defineConfig, devices } from '@playwright/test';

// Browser end-to-end tests: each test starts a real example executable (built
// beforehand, see e2e/app.ts) and drives it in Chromium. Run them with
// `cargo xtask e2e` from the repository root.
export default defineConfig({
  testDir: './e2e',
  // Each test starts its own server, so tests are independent.
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  // A failure must be investigated, not retried away.
  retries: 0,
  reporter: process.env.CI
    ? [['list'], ['html', { open: 'never', outputFolder: 'playwright-report' }]]
    : [['list']],
  use: {
    trace: 'retain-on-failure',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
