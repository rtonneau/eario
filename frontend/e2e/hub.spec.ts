import type { Locator, Page } from '@playwright/test';

import { expect, test } from './app';
import { expectAccessible } from './axe';

/** Longest wait for a dashboard to reach a state: its process has to start. */
const STATE_TIMEOUT_MS = 30_000;

/** The card of one dashboard in the hub's list. */
function card(page: Page, name: string): Locator {
  return page
    .getByRole('listitem')
    .filter({ has: page.getByRole('heading', { level: 3, name, exact: true }) });
}

/** Waits until the card of `name` shows `state`. */
async function expectState(page: Page, name: string, state: string): Promise<void> {
  await expect(card(page, name).getByText(state, { exact: true })).toBeVisible({
    timeout: STATE_TIMEOUT_MS,
  });
}

async function start(page: Page, name: string): Promise<void> {
  await card(page, name).getByRole('button', { name: `Start ${name}` }).click();
  await expectState(page, name, 'Running');
}

test.describe('hub', () => {
  // Each test installs both examples and starts dashboard processes, which
  // can take longer than Playwright's default 30 s on a loaded runner.
  test.describe.configure({ timeout: 120_000 });

  test('lists the installed dashboards, stopped', async ({ page, launchHub }) => {
    await launchHub();

    await expect(page.getByRole('list', { name: 'Installed dashboards' })).toBeVisible();
    for (const name of ['Data Explorer', 'Signal Lab']) {
      await expect(card(page, name)).toBeVisible();
      await expectState(page, name, 'Stopped');
      await expect(card(page, name).getByRole('button', { name: `Start ${name}` })).toBeEnabled();
      await expect(card(page, name).getByRole('button', { name: `Open ${name}` })).toBeDisabled();
    }
    await expect(page.getByRole('button', { name: 'Stop all' })).toBeDisabled();
    await expectAccessible(page, 'hub with stopped dashboards');
  });

  test('starts a dashboard, opens it in a new tab, then stops it', async ({ page, launchHub }) => {
    await launchHub();

    await start(page, 'Data Explorer');
    await expect(page.getByRole('status')).toHaveText('Data Explorer is running.');
    await expectAccessible(page, 'hub with a running dashboard');
    // The other dashboard is untouched.
    await expectState(page, 'Signal Lab', 'Stopped');

    const [tab] = await Promise.all([
      page.context().waitForEvent('page'),
      card(page, 'Data Explorer').getByRole('button', { name: 'Open Data Explorer' }).click(),
    ]);
    await expect(tab.getByRole('heading', { level: 1, name: 'Data Explorer' })).toBeVisible();
    await expectAccessible(tab, 'dashboard opened from the hub');
    await tab.close();

    await expectAccessible(page, 'hub after opening a dashboard');
    await card(page, 'Data Explorer').getByRole('button', { name: 'Stop Data Explorer' }).click();
    await expectState(page, 'Data Explorer', 'Stopped');
    await expect(page.getByRole('status')).toHaveText('Data Explorer is stopped.');
    await expectAccessible(page, 'hub after stopping a dashboard');
  });

  test('stops every running dashboard with Stop all', async ({ page, launchHub }) => {
    await launchHub();
    await start(page, 'Data Explorer');
    await start(page, 'Signal Lab');
    const stopAll = page.getByRole('button', { name: 'Stop all' });
    await expect(stopAll).toBeEnabled();
    await expectAccessible(page, 'hub with two running dashboards');

    await stopAll.click();

    await expectState(page, 'Data Explorer', 'Stopped');
    await expectState(page, 'Signal Lab', 'Stopped');
    await expect(stopAll).toBeDisabled();
    await expectAccessible(page, 'hub after Stop all');
  });

  test('uninstalls a stopped dashboard after a confirmation', async ({ page, launchHub }) => {
    await launchHub();
    await start(page, 'Signal Lab');
    // A running dashboard cannot be uninstalled.
    await expect(
      card(page, 'Signal Lab').getByRole('button', { name: 'Uninstall Signal Lab' }),
    ).toBeDisabled();
    await card(page, 'Signal Lab').getByRole('button', { name: 'Stop Signal Lab' }).click();
    await expectState(page, 'Signal Lab', 'Stopped');

    // Declining keeps it.
    page.once('dialog', (dialog) => void dialog.dismiss());
    await card(page, 'Signal Lab').getByRole('button', { name: 'Uninstall Signal Lab' }).click();
    await expect(card(page, 'Signal Lab')).toBeVisible();

    page.once('dialog', (dialog) => void dialog.accept());
    await card(page, 'Signal Lab').getByRole('button', { name: 'Uninstall Signal Lab' }).click();

    await expect(card(page, 'Signal Lab')).toHaveCount(0);
    await expect(page.getByRole('status')).toHaveText('Signal Lab uninstalled.');
    await expect(card(page, 'Data Explorer')).toBeVisible();
    await expectAccessible(page, 'hub after uninstalling a dashboard');
  });
});
