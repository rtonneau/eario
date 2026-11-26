import { expect, fixture, test } from './app';
import { expectAccessible } from './axe';

test.describe('signal-lab', () => {
  test('an input changes the derived view computed in Rust', async ({ page, launch }) => {
    await launch('signal-lab', [fixture('csv/time-series.csv'), fixture('csv/groups.csv')]);

    // The histogram has one point per bin; its bin count is a number input.
    const histogram = page.getByRole('img', { name: /^Line chart of count against .*, 20 points\.$/ });
    await expect(histogram).toBeVisible();
    await expectAccessible(page, 'signal analysis');

    const bins = page.getByLabel('Histogram bins');
    await bins.fill('8');
    await bins.press('Enter');
    await expect(
      page.getByRole('img', { name: /^Line chart of count against .*, 8 points\.$/ }),
    ).toBeVisible();
    await expectAccessible(page, 'histogram recomputed');
  });

  test('a select on another page recomputes its table', async ({ page, launch }) => {
    await launch('signal-lab', [fixture('csv/time-series.csv'), fixture('csv/groups.csv')]);
    await page
      .getByRole('navigation', { name: 'Pages' })
      .getByRole('button', { name: 'Group summary' })
      .click();

    const table = page.getByRole('table');
    await expect(table).toBeVisible();
    const before = await table.innerText();
    await expectAccessible(page, 'group summary');

    const category = page.getByLabel('Category');
    const options = await category.locator('option').allInnerTexts();
    expect(options.length).toBeGreaterThan(1);
    const current = await category.inputValue();
    const other = options.find((option) => option !== current);
    expect(other).toBeDefined();
    await category.selectOption(other ?? '');

    await expect.poll(() => table.innerText()).not.toBe(before);
    await expectAccessible(page, 'group summary recomputed');
  });
});
