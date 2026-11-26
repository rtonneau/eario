import AxeBuilder from '@axe-core/playwright';
import { type Page, expect } from '@playwright/test';

/** WCAG 2.1 levels A and AA: every rule axe has for them. */
const WCAG_TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'];

/**
 * Runs axe on the page as it is now, in the light and the dark theme, and
 * fails on any violation.
 *
 * The theme is set on the root element directly, as the theme selector does
 * through `App.svelte`, so the user's saved preference is never written.
 */
export async function expectAccessible(page: Page, state: string): Promise<void> {
  const original = await page.evaluate(() => document.documentElement.dataset.theme ?? null);
  for (const theme of ['light', 'dark']) {
    await page.evaluate((value) => {
      document.documentElement.dataset.theme = value;
    }, theme);
    const results = await new AxeBuilder({ page }).withTags(WCAG_TAGS).analyze();
    const violations = results.violations.map((violation) => ({
      rule: violation.id,
      impact: violation.impact,
      help: violation.helpUrl,
      targets: violation.nodes.map((node) => node.target.join(' ')),
    }));
    expect(violations, `accessibility violations: ${state}, ${theme} theme`).toEqual([]);
  }
  await page.evaluate((value) => {
    if (value === null) {
      delete document.documentElement.dataset.theme;
    } else {
      document.documentElement.dataset.theme = value;
    }
  }, original);
}
