import { expect, fixture, test } from './app';
import { expectAccessible } from './axe';

test.describe('data-explorer', () => {
  test('imports a file through the file picker', async ({ page, launch }) => {
    await launch('data-explorer');
    await expect(page.getByText('No file loaded yet.')).toBeVisible();
    await expectAccessible(page, 'no file loaded');

    await page
      .getByLabel('Choose a CSV, TSV or XLSX file, or drop it here')
      .setInputFiles(fixture('csv/measurements.csv'));

    await expect(page.getByText('Loaded: measurements.csv')).toBeVisible();
    await expect(page.getByText('Computed over 40 rows.')).toBeVisible();
    await expectAccessible(page, 'file imported');
  });

  test('draws the scatter plot in a canvas', async ({ page, launch }) => {
    await launch('data-explorer', [fixture('csv/measurements.csv')]);

    // Two of the 40 rows have a missing value and are not drawn.
    const chart = page.getByRole('img', {
      name: 'Scatter plot of counts against energy_keV, 38 points.',
    });
    await expect(chart).toBeVisible();
    await expect(page.getByText('2 rows skipped for missing values.')).toBeVisible();
    const canvas = chart.locator('canvas').first();
    await expect(canvas).toBeVisible();
    const box = await canvas.boundingBox();
    expect(box?.width ?? 0).toBeGreaterThan(100);
    expect(box?.height ?? 0).toBeGreaterThan(100);
    await expectAccessible(page, 'chart drawn');
  });

  test('filters rows and shows the result everywhere', async ({ page, launch }) => {
    await launch('data-explorer', [fixture('csv/measurements.csv')]);
    await expect(page.getByText('Computed over 40 rows.')).toBeVisible();

    await page.getByLabel('Column', { exact: true }).selectOption('energy_keV');
    await page.getByLabel('Condition').selectOption('ge');
    await page.getByLabel('Value').fill('1000');
    await page.getByRole('button', { name: 'Apply' }).click();

    await expect(page.getByRole('status').filter({ hasText: 'Showing rows where' })).toBeVisible();
    const statistics = page.getByText(/^Computed over \d+ rows\.$/);
    await expect(statistics).not.toHaveText('Computed over 40 rows.');
    await expectAccessible(page, 'filter applied');

    await page.getByRole('button', { name: 'Clear' }).click();
    await expect(page.getByText('No filter: all rows are shown.')).toBeVisible();
    await expect(statistics).toHaveText('Computed over 40 rows.');
  });

  test('pages through the table', async ({ page, launch }) => {
    await launch('data-explorer', [fixture('csv/groups.csv')]);
    await page.getByRole('navigation', { name: 'Pages' }).getByRole('button', { name: 'Rows' }).click();

    const pager = page.getByRole('status').filter({ hasText: /^Rows / });
    await expect(pager).toHaveText('Rows 1–50 of 144');
    await expect(page.getByRole('button', { name: 'Previous' })).toBeDisabled();
    await expectAccessible(page, 'table, first page');

    await page.getByRole('button', { name: 'Next' }).click();
    await expect(pager).toHaveText('Rows 51–100 of 144');
    await expect(page.getByRole('row').nth(1).getByRole('cell').first()).toHaveText('51');
    await expectAccessible(page, 'table, second page');

    await page.getByRole('button', { name: 'Previous' }).click();
    await expect(pager).toHaveText('Rows 1–50 of 144');
  });
});
